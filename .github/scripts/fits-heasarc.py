#!/usr/bin/env python3
"""#71: the HEASARC gate — `fitsverify` and `funpack` on candidates our writer made.

`docs/fits-audit.md` §8.3 and `docs/guarantees.md` §4.12 both say the FITS
handler may not be released before the HEASARC tools have accepted its output.
The `fits oracle (astropy)` job reads candidates back with CFITSIO through
`astropy`, which is a second implementation for the arrays but is not that gate.
This script is the gate.

Everything is **standard library only**. That is deliberate: a FITS parser
written here shares no code with the crate under test and none with the tools
under test, so "the header was copied verbatim" is checked by a third reader
rather than by the writer's own opinion. The one thing this script does *not*
do is decode tiles: `funpack` reconstructs the image, and the comparison is then
between two uncompressed FITS files.

What it proves, and what it cannot
----------------------------------
* `fitsverify -l` exits with **the number of errors plus warnings** (read from
  `utilities/fitsverify.c`, which prints that sentence itself), so any non-zero
  status is a failure. The allowlist of tolerated warnings is **empty**; a
  warning that turns out to be intrinsic to the format is a contract question
  for the owner, and the full output is in the evidence.
* `funpack` is the reference *reader* of the compressed convention. If it can
  reconstruct the array and the header, a reader written by someone else
  understood the file.
* The header comparison is **literal**: every card `funpack` reconstructs is
  compared with the card the original file had. A difference is not smoothed
  away here — it fails the gate and the complete diff goes into the evidence,
  because §8.3's condition is "a header that `diff`s clean against the
  original", and a gate that normalises before comparing is not that condition.
* Arrays are compared **as bytes**. Floats are compared as bytes too, on
  purpose: `NaN != NaN` is not a difference of content, and `+0.0 == -0.0`, so a
  value comparison cannot show that a NaN payload or the sign of a zero
  survived. The fixtures include several distinct NaN payloads and both zeros.

Exit status: 0 only when every fixture, every control and the toolchain
sanity check passed.
"""

from __future__ import annotations

import argparse
import gzip
import difflib
import hashlib
import json
import math
import os
import platform
import re
import shutil
from collections import Counter
import struct
import subprocess
import traceback
import zlib
import sys

RECORD = 2880
CARD = 80

# External fixture pin: the exact Astropy test-corpus archive and selected file.
ASTROPY_COMMIT = "dbbf2acd6dbb177db184bff319d2a2950dd178bb"
ASTROPY_ARCHIVE_SHA256 = "3c6b762797ea8f292d5c35fbb3ebe82b7a28139783b5c6e870a38e576d62a46f"
ASTROPY_DOUBLE_EXT_SHA256 = "7ab105b7695dbebd7da0bf430f58527b0da8223ea990460d22593c543f757bfc"
ASTROPY_DOUBLE_EXT_BYTES = 11520

#: Card widths by BITPIX, in bytes per sample.
SAMPLE_BYTES = {8: 1, 16: 2, 32: 4, 64: 8, -32: 4, -64: 8}


# ---------------------------------------------------------------------------
# A FITS writer, a FITS reader, and nothing else
# ---------------------------------------------------------------------------


def _card(head: str) -> bytes:
    """One 80-byte card from a pre-formatted head, space padded."""
    if len(head) > CARD:
        raise ValueError("card head is longer than 80 columns: %r" % head)
    return head.ljust(CARD).encode("ascii")


def string_card(keyword: str, value: str, comment: str = "") -> bytes:
    """A character-string card: quotes immediately around the value."""
    head = "%-8s= '%-8s'" % (keyword, value) if len(value) <= 8 else "%-8s= '%-20s'" % (keyword, value)
    if comment:
        head += " / " + comment
    return _card(head)


def int_card(keyword: str, value: int, comment: str = "") -> bytes:
    head = "%-8s= %20d" % (keyword, value)
    if comment:
        head += " / " + comment
    return _card(head)


def float_card(keyword: str, value: float) -> bytes:
    return _card("%-8s= %20.12G" % (keyword, value))


def bool_card(keyword: str, value: bool, comment: str = "") -> bytes:
    head = "%-8s= %20s" % (keyword, "T" if value else "F")
    if comment:
        head += " / " + comment
    return _card(head)


def raw_card(text: str) -> bytes:
    """A card whose value is written exactly as given, e.g. `-32`."""
    keyword, value = text.split("=", 1)
    return _card("%-8s= %20s" % (keyword.strip(), value.strip()))


def commentary_card(keyword: str, text: str) -> bytes:
    """A COMMENT/HISTORY card: no `=`, the text starts in column 9."""
    return _card(("%-8s%s" % (keyword, text))[:CARD])


def end_card() -> bytes:
    return _card("END")


def end_card_text() -> str:
    return end_card().decode("ascii")


def header_bytes(cards) -> bytes:
    """Cards plus `END`, padded to a whole number of 2880-byte records."""
    out = b"".join(cards) + end_card()
    return out + b" " * (-len(out) % RECORD)


def pad_data(data: bytes) -> bytes:
    return data + b"\0" * (-len(data) % RECORD)


def image_hdu(bitpix, axes, data, extra=(), primary=False, xtension="IMAGE", comments=None) -> bytes:
    """A primary array or an image extension, uncompressed."""
    comments = comments or {}
    cards = []
    if primary:
        cards += [
            bool_card("SIMPLE", True, comments.get("SIMPLE", "")),
            int_card("BITPIX", bitpix, comments.get("BITPIX", "")),
            int_card("NAXIS", len(axes), comments.get("NAXIS", "")),
        ]
    else:
        cards += [
            string_card("XTENSION", xtension, comments.get("XTENSION", "")),
            int_card("BITPIX", bitpix, comments.get("BITPIX", "")),
            int_card("NAXIS", len(axes), comments.get("NAXIS", "")),
        ]
    for index, axis in enumerate(axes, start=1):
        keyword = "NAXIS%d" % index
        cards.append(int_card(keyword, axis, comments.get(keyword, "")))
    if primary:
        cards.append(bool_card("EXTEND", True, comments.get("EXTEND", "")))
    else:
        cards.append(int_card("PCOUNT", 0, comments.get("PCOUNT", "")))
        cards.append(int_card("GCOUNT", 1, comments.get("GCOUNT", "")))
    cards.extend(extra)
    return header_bytes(cards) + pad_data(data)


def nonstandard_hdu(name, payload, extra=()) -> bytes:
    """An `XTENSION` this program does not know, which must be copied verbatim."""
    cards = [
        string_card("XTENSION", name),
        int_card("BITPIX", 8),
        int_card("NAXIS", 1),
        int_card("NAXIS1", len(payload)),
        int_card("PCOUNT", 0),
        int_card("GCOUNT", 1),
        *extra,
    ]
    return header_bytes(cards) + pad_data(payload)


def bin_table_hdu(rows, width, extra=(), tform="1PB(1024)", column="DATA") -> bytes:
    """A binary table: `rows` rows of `width` bytes, as raw bytes."""
    cards = [
        string_card("XTENSION", "BINTABLE"),
        int_card("BITPIX", 8),
        int_card("NAXIS", 2),
        int_card("NAXIS1", width),
        int_card("NAXIS2", rows),
        int_card("PCOUNT", 0),
        int_card("GCOUNT", 1),
        int_card("TFIELDS", 1),
        string_card("TTYPE1", column),
        _card("%-8s= '%s'" % ("TFORM1", tform)),
        *extra,
    ]
    return header_bytes(cards) + pad_data(b"\0" * (rows * width))


def empty_primary() -> bytes:
    """The HDU fpack puts in front of a compressed primary array."""
    cards = [
        bool_card("SIMPLE", True),
        int_card("BITPIX", 8),
        int_card("NAXIS", 0),
        bool_card("EXTEND", True),
    ]
    return header_bytes(cards)


def simple_false_hdu(bitpix, axes, data):
    """A deliberately nonconforming primary used for fail-closed preservation."""
    cards = [
        bool_card("SIMPLE", False),
        int_card("BITPIX", bitpix),
        int_card("NAXIS", len(axes)),
    ]
    cards.extend(int_card("NAXIS%d" % i, axis) for i, axis in enumerate(axes, 1))
    cards.append(bool_card("EXTEND", True))
    return header_bytes(cards) + pad_data(data)


def random_groups_hdu():
    """A small, structurally sized random-groups primary (which tzim must refuse)."""
    axes = [0, 64]
    bitpix = 16
    pcount = 2
    gcount = 4
    sample_bytes = abs(bitpix) // 8
    byte_count = (math.prod(axes[1:]) + pcount) * gcount * sample_bytes
    cards = [
        bool_card("SIMPLE", True),
        int_card("BITPIX", bitpix),
        int_card("NAXIS", len(axes)),
        int_card("NAXIS1", axes[0]),
        int_card("NAXIS2", axes[1]),
        bool_card("GROUPS", True),
        int_card("PCOUNT", pcount),
        int_card("GCOUNT", gcount),
        bool_card("EXTEND", True),
    ]
    return header_bytes(cards) + pad_data(b"\0" * byte_count)


def binary_table_with_theap():
    """A valid non-target BINTABLE with a non-default heap start."""
    cards = [
        string_card("XTENSION", "BINTABLE"),
        int_card("BITPIX", 8),
        int_card("NAXIS", 2),
        int_card("NAXIS1", 8),
        int_card("NAXIS2", 2),
        int_card("PCOUNT", 8),
        int_card("GCOUNT", 1),
        int_card("TFIELDS", 1),
        string_card("TTYPE1", "PAYLOAD"),
        _card("%-8s= '%s'" % ("TFORM1", "8A")),
        int_card("THEAP", 16),
    ]
    data = bytes(range(16)) + b"HEAPDATA"
    return header_bytes(cards) + pad_data(data)


# --- the reader ------------------------------------------------------------


class Hdu:
    """One HDU, parsed: its cards in order and the bytes it occupied."""

    def __init__(self, cards, data, raw, start, end_card=None):
        self.cards = cards          # list of str, 80 columns each, END excluded
        self.end_card = end_card or end_card_text()
        self.data = data            # bytes, unpadded
        self.raw = raw              # header + data + padding
        self.start = start          # offset in the file

    def value(self, keyword):
        for card in self.cards:
            if card[:8].strip() == keyword:
                return card[10:30].strip()
        return None

    def int(self, keyword):
        value = self.value(keyword)
        if value is None:
            return None
        try:
            return int(value)
        except ValueError:
            return None

    def text(self, keyword):
        value = self.value(keyword)
        if value is None or not value.startswith("'"):
            return None
        characters = []
        index = 1
        while index < len(value):
            if value[index] == "'":
                if index + 1 < len(value) and value[index + 1] == "'":
                    characters.append("'")
                    index += 2
                    continue
                return "".join(characters).strip()
            characters.append(value[index])
            index += 1
        return None

    def kind(self):
        if (self.value("SIMPLE") or "").upper() == "T":
            return "PRIMARY"
        return self.text("XTENSION") or "?"

    def bitpix(self):
        return self.int("BITPIX") or 0

    def axes(self):
        n = self.int("NAXIS") or 0
        return [self.int("NAXIS%d" % i) or 0 for i in range(1, n + 1)]

    def __repr__(self):
        return "<Hdu %s at %d, %d cards, %d data bytes>" % (
            self.kind(), self.start, len(self.cards), len(self.data)
        )


def parse(raw: bytes):
    """Every HDU in `raw`, with a bounded scan and no guessing."""
    if len(raw) < RECORD:
        raise ValueError("shorter than one 2880-byte record")
    out = []
    offset = 0
    while offset < len(raw):
        start = offset
        cards = []
        raw_end_card = None
        end_at = None
        while end_at is None:
            if offset + RECORD > len(raw):
                raise ValueError("a header record runs past the end of the file")
            for within in range(0, RECORD, CARD):
                # Latin-1 is a one-to-one mapping for all 256 byte values, so a
                # card diff remains byte-literal even for malformed non-ASCII input.
                text = raw[offset + within:offset + within + CARD].decode("latin-1")
                if text[:8].strip() == "END":
                    raw_end_card = text
                    end_at = offset + RECORD
                    offset += within + CARD
                    break
                cards.append(text)
            if end_at is None:
                offset += RECORD
        probe = Hdu(cards, b"", b"", start)
        naxis = probe.int("NAXIS")
        bitpix = probe.int("BITPIX")
        if naxis is None or bitpix is None:
            raise ValueError("HDU at %d has no BITPIX or NAXIS" % start)
        if bitpix not in SAMPLE_BYTES:
            raise ValueError("HDU at %d has unsupported BITPIX %d" % (start, bitpix))
        axes = probe.axes()
        count = 1
        for axis in axes:
            count *= axis
        pcount_text = probe.value("PCOUNT")
        pcount = probe.int("PCOUNT")
        if pcount_text is not None and pcount is None:
            raise ValueError("HDU at %d has a non-integer PCOUNT" % start)
        if pcount is None:
            pcount = 0
        if pcount < 0:
            raise ValueError("HDU at %d has a negative PCOUNT" % start)
        gcount_text = probe.value("GCOUNT")
        gcount = probe.int("GCOUNT")
        if gcount_text is not None and gcount is None:
            raise ValueError("HDU at %d has a non-integer GCOUNT" % start)
        if gcount is None:
            gcount = 1
        if gcount <= 0:
            raise ValueError("HDU at %d has a non-positive GCOUNT" % start)
        random_groups = probe.value("GROUPS") == "T"
        if random_groups:
            if probe.value("SIMPLE") != "T" or naxis < 2 or not axes or axes[0] != 0:
                raise ValueError("HDU at %d has malformed random-groups axes" % start)
            group_samples = 1
            for axis in axes[1:]:
                group_samples *= axis
            count = group_samples
        length = 0 if naxis == 0 else SAMPLE_BYTES[bitpix] * (count + pcount) * gcount
        data_start = end_at
        data_end = data_start + length
        padded = data_end + (-length % RECORD)
        if padded > len(raw):
            raise ValueError("HDU at %d runs past the end of the file" % start)
        out.append(Hdu(cards, raw[data_start:data_end], raw[start:padded], start, raw_end_card))
        offset = padded
    if offset != len(raw):
        raise ValueError("trailing bytes after the last HDU")
    return out


def cards_of(hdu):
    """`(keyword, value)` for every card, in file order, comments dropped."""
    return [(card[:8].strip(), card[10:30].rstrip()) for card in hdu.cards]


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


def int16_pattern(rows, columns):
    """Compressible int16 samples: the byte pattern repeats every four samples."""
    samples = [((i // 4) % 251) - 125 for i in range(rows * columns)]
    return struct.pack(">%dh" % len(samples), *samples)


def float32_with_specials(rows, columns):
    """Float32 with `+0`, `-0`, `inf`, and four distinct NaN payloads.

    A value comparison cannot see the difference between these; a byte
    comparison can, which is the whole point of this fixture.
    """
    specials = [
        0x00000000,  # +0.0
        0x80000000,  # -0.0
        0x7F800000,  # +inf
        0xFF800000,  # -inf
        0x7FC00001,  # quiet NaN, payload 1
        0x7FC00002,  # quiet NaN, payload 2
        0xFFC00001,  # negative quiet NaN, payload 1
        0xFFC00002,  # negative quiet NaN, payload 2
    ]
    words = []
    for index in range(rows * columns):
        if index % 8 == 0:
            words.append(struct.pack(">I", specials[(index // 8) % len(specials)]))
        else:
            words.append(struct.pack(">f", ((index // 4) % 97) / 7.0))
    return b"".join(words)


def float64_pattern(rows, columns):
    return struct.pack(
        ">%dd" % (rows * columns), *[((i // 4) % 89) / 3.0 for i in range(rows * columns)]
    )


def integer_pattern_bitpix(bitpix, samples):
    """Deterministic, compressible FITS storage bytes for every integer BITPIX."""
    if bitpix not in (8, 16, 32, 64):
        raise ValueError("integer pattern requires BITPIX 8, 16, 32 or 64")
    if samples < 0:
        raise ValueError("sample count cannot be negative")
    period = 251 * 4
    if bitpix == 8:
        block = bytes(((index // 4) % 251) for index in range(period))
    else:
        code = {16: "h", 32: "i", 64: "q"}[bitpix]
        values = [((index // 4) % 251) - 125 for index in range(period)]
        block = struct.pack(">%d%s" % (period, code), *values)
    width = bitpix // 8
    return block * (samples // period) + block[: (samples % period) * width]


def float32_residual_specials(samples):
    """IEEE-754 float32 storage with zeros, infinities, NaNs and subnormals."""
    special_words = (
        0x00000000,  # +0.0
        0x80000000,  # -0.0
        0x7F800000,  # +infinity
        0xFF800000,  # -infinity
        0x7FC00001,  # quiet NaN, payload 1
        0x7FC00002,  # quiet NaN, payload 2
        0xFFC00001,  # negative quiet NaN, payload 1
        0xFFC00002,  # negative quiet NaN, payload 2
        0x00000001,  # smallest positive subnormal
        0x80000001,  # smallest negative subnormal
    )
    out = bytearray()
    for index in range(samples):
        if index % 10 == 0:
            out.extend(struct.pack(">I", special_words[(index // 10) % len(special_words)]))
        else:
            out.extend(struct.pack(">f", ((index // 4) % 97) / 7.0))
    return bytes(out)


def float64_residual_specials(samples):
    """IEEE-754 float64 storage with bit-distinct specials and subnormals."""
    special_words = (
        0x0000000000000000,  # +0.0
        0x8000000000000000,  # -0.0
        0x7FF0000000000000,  # +infinity
        0xFFF0000000000000,  # -infinity
        0x7FF8000000000001,  # quiet NaN, payload 1
        0x7FF8000000000002,  # quiet NaN, payload 2
        0xFFF8000000000001,  # negative quiet NaN, payload 1
        0xFFF8000000000002,  # negative quiet NaN, payload 2
        0x0000000000000001,  # smallest positive subnormal
        0x8000000000000001,  # smallest negative subnormal
    )
    out = bytearray()
    for index in range(samples):
        if index % 10 == 0:
            out.extend(struct.pack(">Q", special_words[(index // 10) % len(special_words)]))
        else:
            out.extend(struct.pack(">d", ((index // 4) % 89) / 3.0))
    return bytes(out)


def float_specials_present(data, bitpix):
    """Presence checks over storage words, with no float conversion/arithmetic."""
    if bitpix == -32:
        words = {
            "positive_zero": [bytes.fromhex("00000000")],
            "negative_zero": [bytes.fromhex("80000000")],
            "positive_infinity": [bytes.fromhex("7f800000")],
            "negative_infinity": [bytes.fromhex("ff800000")],
            "distinct_nan_payloads": [
                bytes.fromhex("7fc00001"),
                bytes.fromhex("7fc00002"),
                bytes.fromhex("ffc00001"),
                bytes.fromhex("ffc00002"),
            ],
            "positive_subnormal": [bytes.fromhex("00000001")],
            "negative_subnormal": [bytes.fromhex("80000001")],
        }
    elif bitpix == -64:
        words = {
            "positive_zero": [bytes.fromhex("0000000000000000")],
            "negative_zero": [bytes.fromhex("8000000000000000")],
            "positive_infinity": [bytes.fromhex("7ff0000000000000")],
            "negative_infinity": [bytes.fromhex("fff0000000000000")],
            "distinct_nan_payloads": [
                bytes.fromhex("7ff8000000000001"),
                bytes.fromhex("7ff8000000000002"),
                bytes.fromhex("fff8000000000001"),
                bytes.fromhex("fff8000000000002"),
            ],
            "positive_subnormal": [bytes.fromhex("0000000000000001")],
            "negative_subnormal": [bytes.fromhex("8000000000000001")],
        }
    else:
        return {}
    return {name: all(word in data for word in values) for name, values in words.items()}


def build_fixtures(root):
    """Every fixture, with the contract's expectation for it.

    `expect` is read from `docs/guarantees.md` §4.12 and the owner's explicit
    decision about the strict whole-file size gate, rather than inferred from
    whichever behavior the current binary happens to exhibit.
    """
    os.makedirs(root, exist_ok=True)
    cases = []

    def write(name, blob):
        path = os.path.join(root, name)
        with open(path, "wb") as handle:
            handle.write(blob)
        return path

    # 1. Integer primary array: the ordinary case.
    write(
        "int16_primary.fits",
        image_hdu(
            16,
            [120, 256],
            int16_pattern(120, 256),
            extra=[
                string_card("OBJECT", "M31"),
                string_card("BUNIT", "COUNTS"),
                commentary_card("HISTORY", " first history line"),
                commentary_card("HISTORY", " second history line"),
            ],
            primary=True,
            comments={
                "SIMPLE": "original primary logical comment",
                "BITPIX": "original primary BITPIX comment",
                "NAXIS": "original primary NAXIS comment",
                "NAXIS1": "original primary width comment",
                "NAXIS2": "original primary height comment",
                "EXTEND": "original EXTEND comment",
            },
        ),
    )
    cases.append(
        dict(
            name="int16_primary.fits",
            expect="rewritten",
            images=1,
            hdu_delta=1,
            why="§4.12: an eligible primary array becomes a ZIMAGE table behind an empty primary",
        )
    )

    # 2. Float primary array with NaN payloads and both zeros.
    write(
        "float32_specials.fits",
        image_hdu(-32, [64, 128], float32_with_specials(64, 128), primary=True),
    )
    cases.append(
        dict(
            name="float32_specials.fits",
            expect="rewritten",
            images=1,
            hdu_delta=1,
            why="GZIP_2 must shuffle bytes and give them back; NaN payloads and signed zeros",
        )
    )

    # 3. Float image extension behind an empty primary: no empty primary is added.
    write(
        "float64_extension.fits",
        empty_primary()
        + image_hdu(
            -64,
            [48, 96],
            float64_pattern(48, 96),
            comments={
                "XTENSION": "original XTENSION comment",
                "BITPIX": "original extension BITPIX comment",
                "NAXIS": "original extension NAXIS comment",
                "NAXIS1": "original extension width comment",
                "NAXIS2": "original extension height comment",
                "PCOUNT": "original PCOUNT comment",
                "GCOUNT": "original GCOUNT comment",
            },
        ),
    )
    cases.append(
        dict(
            name="float64_extension.fits",
            expect="rewritten",
            images=1,
            hdu_delta=0,
            why="§4.12: every eligible image HDU is rewritten; the HDU count only changes for a primary",
        )
    )

    # 4. Positive multi-HDU traversal: only registered extension types. The
    # original is required to pass fitsverify before it can diagnose tzim.
    write(
        "multi_hdu.fits",
        image_hdu(16, [64, 128], int16_pattern(64, 128), primary=True)
        + image_hdu(-32, [32, 64], float32_with_specials(32, 64), extra=[string_card("EXTNAME", "SCI")])
        + bin_table_hdu(4, 8),
    )
    cases.append(
        dict(
            name="multi_hdu.fits",
            expect="mixed",
            images=2,
            hdu_delta=1,
            verbatim=["BINTABLE"],
            diagnostics="registered",
            why="two registered image HDUs and a registered BINTABLE; original and candidate must both pass fitsverify",
        )
    )

    # 4b. Preservation-only HDRLET case. Keep an empty primary first so the
    # unregistered extension is encountered before any compressed image table;
    # compare the exact input diagnostic baseline without conflating it with the
    # separately tested registered multi-HDU traversal path.
    write(
        "hdrlet_preservation.fits",
        empty_primary()
        + nonstandard_hdu("HDRLET", b"\x01\x02\x03\x04" * 200, extra=[string_card("EXTNAME", "STWCS")])
        + image_hdu(-32, [256, 256], float32_with_specials(256, 256), extra=[string_card("EXTNAME", "SCI")])
        + bin_table_hdu(4, 8),
    )
    cases.append(
        dict(
            name="hdrlet_preservation.fits",
            expect="mixed",
            images=1,
            hdu_delta=0,
            verbatim=["HDRLET", "BINTABLE"],
            diagnostics="compare_original",
            skip_funpack=True,
            why="HDRLET is not registered in fitsverify; preserve it exactly and compare the per-file diagnostic baseline without making it the first compressed-HDU traversal control",
        )
    )

    # 5. A file whose candidate would grow: §4.12 says keep the smaller bytes.
    write("tiny.fits", image_hdu(16, [8, 8], int16_pattern(8, 8), primary=True))
    cases.append(
        dict(
            name="tiny.fits",
            expect="untouched",
            status="already_optimal",
            why="§4.12: a candidate that is not strictly smaller is not written",
        )
    )

    # 6. A scaled image: §4.12 refuses rather than losing BSCALE/BZERO.
    write(
        "scaled.fits",
        image_hdu(
            16,
            [32, 64],
            int16_pattern(32, 64),
            extra=[int_card("BSCALE", 2), int_card("BZERO", 0)],
            primary=True,
        ),
    )
    cases.append(
        dict(
            name="scaled.fits",
            expect="untouched",
            status="skipped",
            why="§4.12: BSCALE/BZERO that are not the identity are refused",
        )
    )

    # 7. An HDU carrying a checksum: refused, because no mirrored checksum can be right.
    write(
        "checksummed.fits",
        image_hdu(
            16,
            [64, 128],
            int16_pattern(64, 128),
            extra=[string_card("CHECKSUM", "ABCDEFGH"), string_card("DATASUM", "1234")],
            primary=True,
        ),
    )
    cases.append(
        dict(
            name="checksummed.fits",
            expect="untouched",
            status="skipped",
            why="§4.12: CHECKSUM/DATASUM are refused, not recomputed",
        )
    )

    # 8. A Tier 1 file: a ZIMAGE table whose tiles are already compressed.
    write(
        "rice.fits",
        empty_primary()
        + bin_table_hdu(
            2,
            16,
            extra=[
                bool_card("ZIMAGE", True),
                string_card("ZCMPTYPE", "RICE_1"),
                int_card("ZBITPIX", 16),
                int_card("ZNAXIS", 0),
            ],
            tform="1PB(1024)",
            column="COMPRESSED_DATA",
        ),
    )
    cases.append(
        dict(
            name="rice.fits",
            expect="untouched",
            status="skipped",
            why="§4.12: a codec this build was not asked to write is refused",
        )
    )

    # 9. `zerowidth`: an image whose first axis is zero. The contract puts
    #    "no axis has length zero" in the accepted subset, so the *file* is
    #    skipped — this is not a per-HDU refusal, and it is not "corrupt".
    write("zerowidth.fits", image_hdu(16, [0, 64], b"", primary=True))
    cases.append(
        dict(
            name="zerowidth.fits",
            expect="untouched",
            status="skipped",
            why="§4.12 accepted subset: no axis has length zero, so the file is skipped",
        )
    )

    # 10. `double_ext`: two image extensions after a primary array. The contract
    #     rewrites each eligible image HDU, so this is *not* a refusal of the
    #     file: all three arrays are rewritten and the table is untouched.
    write(
        "double_ext.fits",
        image_hdu(16, [64, 128], int16_pattern(64, 128), primary=True)
        + image_hdu(-32, [32, 64], float32_with_specials(32, 64), extra=[string_card("EXTNAME", "SCI")])
        + image_hdu(16, [16, 32], int16_pattern(16, 32), extra=[string_card("EXTNAME", "ERR")]),
    )
    cases.append(
        dict(
            name="double_ext.fits",
            expect="rewritten",
            images=3,
            hdu_delta=1,
            why="§4.12: every eligible image HDU is rewritten; two extensions are two HDUs",
        )
    )

    # 11. A zero-width *column* in a binary table. A BINTABLE is not an image,
    #     so the contract copies it verbatim; it does not refuse the file.
    zero_table = header_bytes(
        [
            string_card("XTENSION", "BINTABLE"),
            int_card("BITPIX", 8),
            int_card("NAXIS", 2),
            int_card("NAXIS1", 8),
            int_card("NAXIS2", 2),
            int_card("PCOUNT", 0),
            int_card("GCOUNT", 1),
            int_card("TFIELDS", 2),
            string_card("TTYPE1", "EMPTY"),
            _card("%-8s= '%s'" % ("TFORM1", "0A")),
            string_card("TTYPE2", "DATA"),
            _card("%-8s= '%s'" % ("TFORM2", "8A")),
        ]
    ) + pad_data(b"\0" * 16)
    write(
        "zero_width_column.fits",
        empty_primary() + image_hdu(16, [32, 64], int16_pattern(32, 64), extra=[string_card("EXTNAME", "SCI")]) + zero_table,
    )
    cases.append(
        dict(
            name="zero_width_column.fits",
            expect="untouched",
            status="already_optimal",
            verbatim=["BINTABLE"],
            input_case=(
                "HDU 0: empty primary (BITPIX=8, NAXIS=0); HDU 1: IMAGE extension "
                "(BITPIX=16, NAXIS=2, NAXIS1=32, NAXIS2=64, EXTNAME='SCI'); HDU 2: "
                "BINTABLE (BITPIX=8, NAXIS=2, NAXIS1=8, NAXIS2=2, TFIELDS=2, "
                "TFORM1='0A' [zero-width], TFORM2='8A', PCOUNT=0, GCOUNT=1; 16 data bytes)."
            ),
            contract_expectation=(
                "§4.12's strict whole-file size gate applies after the per-HDU candidate is "
                "formed: if the complete candidate is not strictly smaller, the original "
                "remains unchanged and the status is already_optimal. The discarded candidate "
                "size is not needed to assert that externally visible contract."
            ),
            why="§4.12: unchanged + already_optimal when the strict whole-file candidate is not smaller",
        )
    )

    # Residual #78 matrix rows: these are additive to the fixtures already
    # independently checked in run 40551071072. In residual scope, the gate runs
    # only these exact missing rows and their controls, not the confirmed fixtures.
    residual_matrix = [
        (
            "residual_bitpix8_naxis1_primary.fits",
            8,
            [32768],
            "PRIMARY",
            integer_pattern_bitpix(8, 32768),
        ),
        (
            "residual_bitpix32_naxis3_image.fits",
            32,
            [64, 32, 16],
            "IMAGE",
            integer_pattern_bitpix(32, 64 * 32 * 16),
        ),
        (
            "residual_bitpix64_naxis4_primary.fits",
            64,
            [64, 8, 4, 2],
            "PRIMARY",
            integer_pattern_bitpix(64, 64 * 8 * 4 * 2),
        ),
        (
            "residual_float32_naxis1_image.fits",
            -32,
            [16384],
            "IMAGE",
            float32_residual_specials(16384),
        ),
        (
            "residual_float64_naxis3_primary.fits",
            -64,
            [64, 8, 4],
            "PRIMARY",
            float64_residual_specials(64 * 8 * 4),
        ),
        (
            "residual_bitpix16_naxis999_primary.fits",
            16,
            [1048576] + [1] * 998,
            "PRIMARY",
            integer_pattern_bitpix(16, 1048576),
        ),
    ]
    for name, bitpix, axes, role, data in residual_matrix:
        image = image_hdu(bitpix, axes, data, primary=(role == "PRIMARY"))
        blob = image if role == "PRIMARY" else empty_primary() + image
        write(name, blob)
        cases.append(
            dict(
                name=name,
                expect="rewritten",
                images=1,
                hdu_delta=1 if role == "PRIMARY" else 0,
                residual_only=True,
                matrix=dict(
                    role=role,
                    bitpix=bitpix,
                    naxis=len(axes),
                    floating_storage=(
                        [
                            "positive_zero",
                            "negative_zero",
                            "positive_infinity",
                            "negative_infinity",
                            "distinct_nan_payloads",
                            "positive_subnormal",
                            "negative_subnormal",
                        ]
                        if bitpix < 0
                        else []
                    ),
                ),
                input_case=(
                    "Deterministic, generated issue #78 residual row: %s image, BITPIX=%d, "
                    "NAXIS=%d; not a corpus measurement input."
                    % (role, bitpix, len(axes))
                ),
                why=(
                    "issue #78 residual actual-binary matrix: all six supported BITPIX values, "
                    "rank 1/3/4/999, and both primary and IMAGE-extension rewrites"
                ),
            )
        )

    residual_untouched = [
        (
            "residual_simple_false.fits",
            simple_false_hdu(16, [128, 64], int16_pattern(64, 128)),
            "SIMPLE = F is refused and must remain byte-identical",
        ),
        (
            "residual_random_groups.fits",
            random_groups_hdu(),
            "GROUPS = T random-groups primary is refused and must remain byte-identical",
        ),
        (
            "residual_pcount_image.fits",
            image_hdu(
                16,
                [128, 64],
                int16_pattern(64, 128) + b"\0\0",
                extra=[int_card("PCOUNT", 1)],
                primary=True,
            ),
            "nonzero PCOUNT on an ordinary image is refused and preserved",
        ),
        (
            "residual_gcount_image.fits",
            image_hdu(
                16,
                [128, 64],
                int16_pattern(64, 128) * 2,
                extra=[int_card("GCOUNT", 2)],
                primary=True,
            ),
            "non-identity GCOUNT on an ordinary image is refused and preserved",
        ),
        (
            "residual_bzero_scaled.fits",
            image_hdu(
                16,
                [64, 128],
                int16_pattern(64, 128),
                extra=[int_card("BSCALE", 1), int_card("BZERO", 100)],
                primary=True,
            ),
            "non-identity BZERO is refused independently of BSCALE",
        ),
    ]
    for name, blob, why in residual_untouched:
        write(name, blob)
        cases.append(
            dict(
                name=name,
                expect="untouched",
                status="skipped",
                residual_only=True,
                input_case="Deterministic issue #78 refusal fixture; not a corpus measurement input.",
                why=why,
            )
        )

    for codec in ("PLIO_1", "HCOMPRESS_1"):
        name = "residual_unsupported_%s.fits" % codec.lower()
        blob = empty_primary() + tiles_fixture(
            codec, [struct.pack(">16h", *([0] * 16))], per_sample=2
        )
        write(name, blob)
        cases.append(
            dict(
                name=name,
                expect="untouched",
                status="skipped",
                residual_only=True,
                input_case=(
                    "Deterministic ZIMAGE refusal fixture with registered but unsupported "
                    "%s codec; its payload is not claimed to be a real %s stream."
                    % (codec, codec)
                ),
                why="unsupported codec %s is refused without rewriting the source" % codec,
            )
        )

    theap_name = "residual_theap_non_target.fits"
    theap_blob = (
        empty_primary()
        + image_hdu(
            16,
            [64, 128],
            int16_pattern(64, 128),
            extra=[string_card("EXTNAME", "SCI")],
        )
        + binary_table_with_theap()
    )
    write(theap_name, theap_blob)
    cases.append(
        dict(
            name=theap_name,
            expect="mixed",
            images=1,
            hdu_delta=0,
            verbatim=["BINTABLE"],
            diagnostics="registered",
            residual_only=True,
            input_case=(
                "Eligible IMAGE extension followed by a valid non-target BINTABLE whose "
                "THEAP is 16 bytes after its header and whose 8-byte heap is explicit."
            ),
            why="a non-target BINTABLE with THEAP must remain byte-identical across rewrite",
        )
    )

    return cases


def add_external_double_ext(root, source_path):
    """Add the pinned real input and an image derived with its literal EXTNAMX cards."""
    if not source_path or not os.path.isfile(source_path):
        raise FileNotFoundError("the external Astropy double_ext.fits input is missing")
    raw = open(source_path, "rb").read()
    destination = os.path.join(root, "external_double_ext.fits")
    with open(destination, "wb") as handle:
        handle.write(raw)

    external_hdus = parse(raw)
    compressed = next((hdu for hdu in external_hdus if hdu.value("ZIMAGE") == "T"), None)
    if compressed is None:
        raise ValueError("the pinned external Astropy fixture has no ZIMAGE HDU")
    extnamx_cards = [card for card in compressed.cards if card[:8].strip() == "EXTNAMX"]
    extname_cards = [card for card in compressed.cards if card[:8].strip() == "EXTNAME"]
    if len(extnamx_cards) != 2 or len(extname_cards) < 2:
        raise ValueError("the pinned external HDU no longer has its expected duplicate name cards")
    if [card[10:30].strip() for card in extnamx_cards] != ["'COMPRESSED_IMAGE'", "'ccd00   '"]:
        raise ValueError("the pinned external fixture's literal EXTNAMX values changed")
    # In Astropy's compressed extension, the last EXTNAME names the source
    # image; the first names the compressed-table extension. Build one small,
    # uncompressed image per external EXTNAMX card from deterministic data.
    # A single card per rewrite keeps CFITSIO's duplicate-keyword warning
    # allowlist empty; the byte-pinned real input below separately proves that
    # both duplicate external cards survive the no-rewrite path. This exercises
    # the actual writer without relying on funpack's failure to decompress the
    # RICE_1 source file.
    external_source_extname = extname_cards[-1]
    if "'ccd00" not in external_source_extname[10:30]:
        raise ValueError("the pinned external source-image EXTNAME card changed")
    # EXTNAME is generated normally; the coverage target is the exact external
    # EXTNAMX card, not Astropy's comment formatting on an EXTNAME card.
    source_extname = string_card("EXTNAME", "ccd00")
    rewrite_cases = []
    for label, external_card in zip(("compressed_image", "ccd00"), extnamx_cards):
        rewrite_name = "external_extnamx_%s.fits" % label
        rewrite_path = os.path.join(root, rewrite_name)
        rewrite_bytes = (
            empty_primary()
            + image_hdu(
                16,
                [64, 128],
                int16_pattern(64, 128),
                extra=[source_extname, external_card.encode("ascii")],
            )
        )
        with open(rewrite_path, "wb") as handle:
            handle.write(rewrite_bytes)
        rewrite_cases.append((rewrite_name, external_card))

    external_case = dict(
        name="external_double_ext.fits",
        expect="untouched",
        status="skipped",
        required_card_counts={"EXTNAMX": 2},
        expected_input_bytes=ASTROPY_DOUBLE_EXT_BYTES,
        expected_input_sha256=ASTROPY_DOUBLE_EXT_SHA256,
        input_case=(
            "External astropy/astropy FITS test input at commit %s, "
            "astropy/io/fits/tests/data/double_ext.fits; selected-file SHA-256 %s, %d bytes."
            % (ASTROPY_COMMIT, ASTROPY_DOUBLE_EXT_SHA256, ASTROPY_DOUBLE_EXT_BYTES)
        ),
        contract_expectation=(
            "The pinned external file is already a RICE_1 ZIMAGE table, outside this build's "
            "Tier 2 rewrite path. The contract says an unsupported RICE_1 codec is refused as "
            "skipped; tzim must preserve the exact file, and both external EXTNAMX cards must "
            "remain byte-identical. Two separate rewrite cases send each literal card through the "
            "actual writer and acceptance path, one card per input so CFITSIO reports no duplicate-keyword warning."
        ),
        why=(
            "external real-file coverage for the Astropy double_ext.fits EXTNAMX cards; "
            "the RICE_1 compressed input is outside the writer's accepted rewrite subset"
        ),
    )
    rewrite_fixture_cases = []
    for rewrite_name, external_card in rewrite_cases:
        label = external_card[10:30].strip().strip("'").strip()
        rewrite_case = dict(
            name=rewrite_name,
            expect="rewritten",
            images=1,
            hdu_delta=0,
            required_card_counts={"EXTNAMX": 1},
            input_case=(
                "Uncompressed synthetic IMAGE data with EXTNAME and one literal EXTNAMX card "
                "(%s) copied byte-for-byte from pinned external Astropy file %s (commit %s, "
                "SHA-256 %s). The pixel array is deterministic harness data; it is not claimed "
                "to be a decompression of the RICE_1 source file."
                % (label, "astropy/io/fits/tests/data/double_ext.fits", ASTROPY_COMMIT,
                   ASTROPY_DOUBLE_EXT_SHA256)
            ),
            contract_expectation=(
                "The actual rewritten candidate must contain this single external EXTNAMX card "
                "byte-for-byte and pass the shared fitsverify/funpack/array/literal-header "
                "acceptance path with no warnings. The two-card external file is checked separately "
                "on its unchanged path, avoiding a duplicate-keyword warning allowlist."
            ),
            why="external Astropy EXTNAMX %s literal card exercised through the writer's rewrite path" % label,
        )
        rewrite_fixture_cases.append(rewrite_case)
    return [external_case, *rewrite_fixture_cases]


# ---------------------------------------------------------------------------
# Running the binary and the tools
# ---------------------------------------------------------------------------


def run(argv, cwd=None, env=None):
    """`(exit code, combined output)`, never raising on a non-zero exit."""
    done = subprocess.run(
        argv, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False
    )
    return done.returncode, done.stdout.decode("utf-8", "replace")


def tool_identity(name, extra_args=()):
    """Run a tool and record what it is, **without assuming any exit code**.

    `fitsverify` exits with the number of errors plus warnings, and its usage
    text exits 0 in the version read from `utilities/fitsverify.c` — but the
    point of this function is that neither of those is assumed: the code is
    recorded, and only the output is required to name the tool.
    """
    argv = [name] + list(extra_args)
    code, output = run(argv)
    first = output.splitlines()[0] if output.strip() else ""
    return dict(
        argv=argv,
        exit=code,
        recognised=name in first or name in output,
        first_lines="\n".join(output.splitlines()[:4]),
        output=output,
    )


def fitsverify(path, scratch=None):
    """`(exit code, output)` from `fitsverify -l`.

    The path is absolute and no working directory is imposed: the earlier
    version passed a basename and ran the tool in the scratch directory, which
    would have reported `file not found` as a verification failure.
    """
    del scratch                                  # kept for the call sites' symmetry
    return run(["fitsverify", "-l", os.path.abspath(path)])


def fitsverify_diagnostics(output):
    """The case-specific error/warning markers and final counts from fitsverify.

    For the nonstandard HDRLET preservation fixture, compare the candidate with
    that same input's actual external-tool baseline. This is not a global
    allowlist: a different diagnostic or count is a new finding.
    """
    lines = output.splitlines()
    markers = [
        line.strip()
        for line in lines
        if line.lstrip().startswith(("*** Error:", "*** Warning:"))
    ]
    summary = [
        line.strip()
        for line in lines
        if line.strip().startswith("**** Verification found ")
    ]
    return dict(markers=markers, summary=summary)


def external_diagnostic_comparison(original_exit, original, candidate_exit, candidate):
    """Whether a candidate added any diagnostic beyond its own input baseline.

    This is deliberately per-file, not a global allowlist. An inherited HDRLET
    error may remain or disappear; a different/new marker, a larger summary
    count, or an exit/count inconsistency is a new finding.
    """
    def counts(diagnostics):
        for line in diagnostics.get("summary", []):
            match = re.search(
                r"Verification found (\d+) warning\(s\) and (\d+) error\(s\)", line
            )
            if match:
                return dict(warnings=int(match.group(1)), errors=int(match.group(2)))
        return None

    original_counts = counts(original)
    candidate_counts = counts(candidate)
    new_markers = list(
        (Counter(candidate.get("markers", [])) - Counter(original.get("markers", []))).elements()
    )
    counts_consistent = (
        original_counts is not None
        and candidate_counts is not None
        and original_exit == original_counts["warnings"] + original_counts["errors"]
        and candidate_exit == candidate_counts["warnings"] + candidate_counts["errors"]
    )
    no_new_diagnostics = (
        counts_consistent
        and not new_markers
        and candidate_counts["warnings"] <= original_counts["warnings"]
        and candidate_counts["errors"] <= original_counts["errors"]
        and candidate_exit <= original_exit
    )
    return dict(
        no_new_diagnostics=no_new_diagnostics,
        exact_match=(original == candidate and original_exit == candidate_exit),
        new_markers=new_markers,
        original_counts=original_counts,
        candidate_counts=candidate_counts,
        exit_count_consistent=counts_consistent,
    )


def funpack(path, scratch):
    """Unpack a compressed candidate with `funpack` and return what came out.

    `funpack` reconstructs `<name>.fits` from `<name>.fits.fz`; the candidate is
    copied under that name so that its own file name is not the one funpack
    writes.
    """
    work = os.path.join(scratch, "funpack")
    os.makedirs(work, exist_ok=True)
    for stale in os.listdir(work):
        os.unlink(os.path.join(work, stale))
    packed = os.path.join(work, "candidate.fits.fz")
    shutil.copyfile(path, packed)
    # `-C` disables checksum updates during reconstruction. The gate compares
    # the full original header literally; leaving CFITSIO's default update on
    # would synthesize CHECKSUM/DATASUM cards and make the reader itself the
    # source of an avoidable diff. `-O` is not "overwrite"; it names an output.
    unpacked_name = os.path.join(work, "candidate.fits")
    if os.path.exists(unpacked_name):
        os.unlink(unpacked_name)
    code, output = run(["funpack", "-C", os.path.basename(packed)], cwd=work)
    unpacked = os.path.join(work, "candidate.fits")
    if not os.path.exists(unpacked):
        others = [n for n in os.listdir(work) if n.endswith(".fits")]
        unpacked = os.path.join(work, others[0]) if others else None
    return code, output, unpacked


# ---------------------------------------------------------------------------
# The checks
# ---------------------------------------------------------------------------


def map_candidate(before, candidate):
    """`{original index: candidate index}` for every HDU of the original file.

    The one shift the contract allows is an empty primary in front of a
    compressed primary array, so the mapping is computed rather than assumed:
    a candidate HDU that is an empty primary where the original had a primary
    array is skipped, and every other HDU is matched in order.
    """
    mapping = {}
    cursor = 0
    for index, original in enumerate(before):
        if (
            cursor + 1 < len(candidate)
            and candidate[cursor].kind() == "PRIMARY"
            and not candidate[cursor].axes()
            and original.kind() == "PRIMARY"
            and original.axes()
        ):
            cursor += 1
        if cursor >= len(candidate):
            break
        mapping[index] = cursor
        cursor += 1
    return mapping, cursor


def hdu_layout(hdus):
    """Offsets that let the tool error be matched to the fixture's exact HDUs."""
    rows = []
    for index, hdu in enumerate(hdus):
        data_padded = ((len(hdu.data) + RECORD - 1) // RECORD) * RECORD
        header_bytes = len(hdu.raw) - data_padded
        rows.append(dict(
            index=index,
            kind=hdu.kind(),
            start=hdu.start,
            header_bytes=header_bytes,
            data_bytes=len(hdu.data),
            padded_data_bytes=data_padded,
            next_start=hdu.start + len(hdu.raw),
        ))
    return rows


def source_image_matrix(hdus):
    """Compact actual input identity for image roles, BITPIX and NAXIS rank."""
    rows = []
    for index, hdu in enumerate(hdus):
        if hdu.kind() in ("PRIMARY", "IMAGE") and hdu.axes():
            rows.append(
                dict(
                    hdu=index,
                    role=hdu.kind(),
                    bitpix=hdu.int("BITPIX"),
                    naxis=hdu.int("NAXIS"),
                    axes=hdu.axes(),
                )
            )
    return rows


def candidate_image_matrix(hdus):
    """Compressed-image identity from the generated ZIMAGE headers."""
    rows = []
    for index, hdu in enumerate(hdus):
        if hdu.value("ZIMAGE") != "T":
            continue
        role = "PRIMARY" if hdu.value("ZSIMPLE") == "T" else hdu.text("ZTENSION")
        naxis = hdu.int("ZNAXIS")
        axes = [hdu.int("ZNAXIS%d" % axis) for axis in range(1, (naxis or 0) + 1)]
        rows.append(
            dict(
                hdu=index,
                role=role,
                bitpix=hdu.int("ZBITPIX"),
                naxis=naxis,
                axes=axes,
                codec=hdu.text("ZCMPTYPE"),
            )
        )
    return rows


def literal_card_diff(original_hdu, reconstructed_hdu):
    """A sequence diff of complete 80-column cards, including comments and order.

    The HDU reader keeps the bytes of each card as text and excludes only the
    FITS record padding. `END` is added explicitly. This deliberately does not
    key by keyword or compare only parsed values: either would hide card reordering,
    comment changes, spacing changes, or duplicate-card movement.
    """
    before = list(original_hdu.cards) + [original_hdu.end_card]
    after = list(reconstructed_hdu.cards) + [reconstructed_hdu.end_card]
    matcher = difflib.SequenceMatcher(a=before, b=after, autojunk=False)
    diffs = []

    def keyword(card):
        return card[:8].strip() if card else None

    def add(kind, original_index, reconstructed_index, original, reconstructed):
        old_key, new_key = keyword(original), keyword(reconstructed)
        if old_key == new_key:
            key = old_key or "<blank>"
        elif old_key is None:
            key = new_key or "<blank>"
        elif new_key is None:
            key = old_key or "<blank>"
        else:
            key = "%s -> %s" % (old_key, new_key)
        diffs.append(dict(
            keyword=key,
            original_keyword=old_key,
            reconstructed_keyword=new_key,
            original_index=original_index,
            reconstructed_index=reconstructed_index,
            original=original,
            reconstructed=reconstructed,
            kind=kind,
        ))

    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag == "equal":
            continue
        if tag == "replace":
            paired = min(i2 - i1, j2 - j1)
            for offset in range(paired):
                add("changed", i1 + offset + 1, j1 + offset + 1,
                    before[i1 + offset], after[j1 + offset])
            for offset in range(paired, i2 - i1):
                add("dropped", i1 + offset + 1, None, before[i1 + offset], None)
            for offset in range(paired, j2 - j1):
                add("invented", None, j1 + offset + 1, None, after[j1 + offset])
        elif tag == "delete":
            for index in range(i1, i2):
                add("dropped", index + 1, None, before[index], None)
        elif tag == "insert":
            for index in range(j1, j2):
                add("invented", None, index + 1, None, after[index])
    return diffs


def classify_diff(diff):
    """Whether §4.12 permits this difference — for the report, never to excuse it."""
    keyword = diff["keyword"]
    keywords = {diff.get("original_keyword"), diff.get("reconstructed_keyword"), keyword}
    structural = {
        "SIMPLE", "XTENSION", "BITPIX", "NAXIS", "PCOUNT", "GCOUNT", "EXTEND",
        "TFIELDS", "TTYPE1", "TFORM1", "NAXIS1", "NAXIS2",
    }
    if any(k and (k.startswith("NAXIS") or k.startswith("Z") or k.startswith("TTYPE") or k.startswith("TFORM"))
           for k in keywords):
        return "structural card differs (still a literal round-trip failure)"
    if keywords & structural:
        return "structural card differs (still a literal round-trip failure)"
    if diff["kind"] == "invented":
        return "present only in funpack-reconstructed header (producer not isolated)"
    if diff["kind"] == "dropped":
        return "absent from funpack-reconstructed header (producer not isolated; §4.12 says no card is dropped)"
    return "literal card changed in funpack-reconstructed header (producer not isolated)"


def compare_arrays(original_hdu, reconstructed_hdu):
    """Byte comparison of two uncompressed image arrays, with the shape checked first."""
    problems = []
    if original_hdu.bitpix() != reconstructed_hdu.bitpix():
        problems.append(
            "BITPIX %d -> %d" % (original_hdu.bitpix(), reconstructed_hdu.bitpix())
        )
    if original_hdu.axes() != reconstructed_hdu.axes():
        problems.append("axes %s -> %s" % (original_hdu.axes(), reconstructed_hdu.axes()))
    if not problems and original_hdu.data != reconstructed_hdu.data:
        first = next(
            (i for i in range(min(len(original_hdu.data), len(reconstructed_hdu.data)))
             if original_hdu.data[i] != reconstructed_hdu.data[i]),
            None,
        )
        problems.append(
            "array bytes differ (%d -> %d bytes, first difference at %s)"
            % (len(original_hdu.data), len(reconstructed_hdu.data), first)
        )
    return problems


# --- the control that has to go red ---------------------------------------


def shuffle(block, per_sample):
    """The `GZIP_2` permutation: byte *b* of every sample, then byte *b + 1*.

    `shuffle` and `unshuffle` are inverses of each other and are **not** the same
    function — applying one twice does not return the input unless there is a
    single sample per byte, which is why the fixture builder gets its own.
    """
    count = len(block) // per_sample
    out = bytearray(len(block))
    for byte in range(per_sample):
        for sample in range(count):
            out[byte * count + sample] = block[sample * per_sample + byte]
    return bytes(out)


def unshuffle(block, per_sample):
    """The inverse of the `GZIP_2` permutation."""
    count = len(block) // per_sample
    out = bytearray(len(block))
    for byte in range(per_sample):
        for sample in range(count):
            out[sample * per_sample + byte] = block[byte * count + sample]
    return bytes(out)


def inflate_tiles(path):
    """The concatenated tiles of every ZIMAGE table, as the array they stand for.

    This is the same comparison the crate's own verifier makes — inflate every
    tile, concatenate in row order, compare with the original array bytes — done
    here with the standard library's gzip, which checks the member's CRC-32 and
    `ISIZE` on the way out. It is what the corruption control asks.
    """
    out = bytearray()
    with open(path, "rb") as handle:
        hdus = parse(handle.read())
    for hdu in hdus:
        if not hdu.value("ZIMAGE"):
            continue
        rows = hdu.int("NAXIS2") or 0
        heap_len = hdu.int("PCOUNT") or 0
        descriptor_bytes = 8 * rows
        if descriptor_bytes + heap_len != len(hdu.data):
            raise ValueError("ZIMAGE row descriptors and PCOUNT do not match the table data length")
        heap = hdu.data[descriptor_bytes:descriptor_bytes + heap_len]
        floating = (hdu.int("ZBITPIX") or 0) < 0
        per_sample = abs(hdu.int("ZBITPIX") or 8) // 8
        for row in range(rows):
            length, offset = struct.unpack(">II", hdu.data[row * 8:row * 8 + 8])
            if length == 0 or offset > heap_len or length > heap_len - offset:
                raise ValueError(
                    "ZIMAGE descriptor %d lies outside its heap (count=%d, offset=%d, heap=%d)"
                    % (row, length, offset, heap_len)
                )
            tile = gzip.decompress(heap[offset:offset + length])
            out += unshuffle(tile, per_sample) if floating else tile
    return bytes(out)


def corrupt_tile(candidate_path, out_path):
    """Flip one bit inside the first tile of the first ZIMAGE table.

    The tile is found through the table's own row descriptor, so this is exactly
    the corruption a broken encoder would produce: the file still parses, and
    only the compressed payload is wrong.
    """
    raw = open(candidate_path, "rb").read()
    hdus = parse(raw)
    for hdu in hdus:
        if hdu.value("ZIMAGE") and hdu.text("XTENSION") in (None, "BINTABLE"):
            rows = hdu.int("NAXIS2") or 0
            if rows == 0:
                continue
            length, offset = struct.unpack(">II", hdu.data[0:8])
            heap_start = hdu.start + len(hdu.raw) - (-(-len(hdu.data) // RECORD)) * RECORD
            # The heap begins after the row descriptors; offsets are relative to it.
            heap = hdu.start + (len(hdu.raw) - (-(-len(hdu.data) // RECORD)) * RECORD) + 8 * rows
            at = heap + offset + min(10, max(0, length - 1))
            blob = bytearray(raw)
            blob[at] ^= 0x40
            open(out_path, "wb").write(bytes(blob))
            return dict(tile_length=length, tile_offset=offset, corrupted_at=at)
    raise RuntimeError("no ZIMAGE table with a tile to corrupt in %s" % candidate_path)


def broken_structure(candidate_path, out_path):
    """Make `NAXIS1` disagree with the column width of a ZIMAGE table.

    `fitsverify` lists "Sum of table column widths is inconsistent with NAXIS1"
    as an *error condition*, so this is a file it must reject; a verifier that
    accepts it is not verifying. The data is untouched, which also makes the
    point that a structural defect is not a content defect.
    """
    raw = bytearray(open(candidate_path, "rb").read())
    hdus = parse(bytes(raw))
    for hdu in hdus:
        if hdu.value("ZIMAGE"):
            head = hdu.start
            for index in range(0, len(hdu.raw), CARD):
                card = bytes(raw[head + index:head + index + CARD])
                if card[:8].strip() == b"NAXIS1":
                    raw[head + index:head + index + CARD] = (
                        "%-8s= %20d" % ("NAXIS1", 16)
                    ).encode("ascii").ljust(CARD)
                    open(out_path, "wb").write(bytes(raw))
                    return dict(card=card.decode("ascii").rstrip(), replacement="NAXIS1 = 16 (TFORM1 says 8)")
    raise RuntimeError("no ZIMAGE table to break in %s" % candidate_path)


def _header_size(hdu):
    """Number of bytes occupied by the parsed cards through the END record."""
    return ((len(hdu.cards) + 1) * CARD + RECORD - 1) // RECORD * RECORD


def _replace_header_card(header, keyword, replacement):
    """Replace one card in a header bytearray, retaining an exact 80-byte card."""
    if len(replacement) != CARD:
        raise ValueError("replacement card must be exactly 80 bytes")
    for offset in range(0, len(header), CARD):
        if bytes(header[offset:offset + 8]).strip() == keyword.encode("ascii"):
            previous = bytes(header[offset:offset + CARD])
            header[offset:offset + CARD] = replacement
            return previous
    raise ValueError("no %s card in ZIMAGE table" % keyword)


def mutate_zimage_candidate(
    candidate_path,
    out_path,
    *,
    descriptor_mutator=None,
    payload_mutator=None,
    compressed_member_mutator=None,
    card_replacements=None,
):
    """Create one controlled ZIMAGE mutation while preserving all other HDUs.

    Pixel mutators see the unshuffled FITS storage bytes. Compressed-member
    mutators see the exact gzip bytes. Descriptor and card mutations are applied
    without repairing the defect, so the shared external acceptance path sees
    exactly the damage under test.
    """
    with open(candidate_path, "rb") as handle:
        raw = handle.read()
    hdus = parse(raw)
    chunks = []
    changed = False
    details = {}
    for hdu in hdus:
        if hdu.value("ZIMAGE") != "T" or changed:
            chunks.append(hdu.raw)
            continue
        rows = hdu.int("NAXIS2") or 0
        pcount = hdu.int("PCOUNT") or 0
        descriptor_bytes = bytearray(hdu.data[:8 * rows])
        heap = hdu.data[8 * rows:8 * rows + pcount]
        if len(descriptor_bytes) != 8 * rows or len(heap) != pcount:
            raise ValueError("ZIMAGE row descriptors or heap are truncated")
        if descriptor_mutator is not None:
            details["descriptor"] = descriptor_mutator(descriptor_bytes, heap, rows)
        if payload_mutator is not None or compressed_member_mutator is not None:
            new_members = []
            floating = (hdu.int("ZBITPIX") or 0) < 0
            per_sample = abs(hdu.int("ZBITPIX") or 8) // 8
            for row in range(rows):
                length, offset = struct.unpack(">II", descriptor_bytes[row * 8:row * 8 + 8])
                member = heap[offset:offset + length]
                if offset + length > len(heap) or len(member) != length:
                    raise ValueError("source ZIMAGE descriptor lies outside its heap")
                if row == 0 and compressed_member_mutator is not None:
                    member, detail = compressed_member_mutator(member)
                    details["compressed_member"] = detail
                elif row == 0 and payload_mutator is not None:
                    shuffled = gzip.decompress(member)
                    payload = unshuffle(shuffled, per_sample) if floating else shuffled
                    payload, detail = payload_mutator(payload, hdu.int("ZBITPIX"))
                    shuffled = shuffle(payload, per_sample) if floating else payload
                    member = gzip.compress(shuffled, mtime=0)
                    details["pixel_payload"] = detail
                new_members.append(member)
            descriptors = bytearray()
            new_heap = bytearray()
            for member in new_members:
                descriptors.extend(struct.pack(">II", len(member), len(new_heap)))
                new_heap.extend(member)
            descriptor_bytes = descriptors
            heap = bytes(new_heap)
        header_size = _header_size(hdu)
        header = bytearray(hdu.raw[:header_size])
        for keyword, replacement in (card_replacements or {}).items():
            previous = _replace_header_card(header, keyword, replacement)
            details.setdefault("cards", {})[keyword] = dict(
                original=previous.decode("ascii", "replace").rstrip(),
                replacement=replacement.decode("ascii", "replace").rstrip(),
            )
        if descriptor_mutator is None:
            _replace_header_card(header, "PCOUNT", int_card("PCOUNT", len(heap)))
            max_length = max(
                (struct.unpack(">II", descriptor_bytes[row * 8:row * 8 + 8])[0]
                 for row in range(rows)),
                default=0,
            )
            old_tform = hdu.text("TFORM1") or ""
            match = re.search(r"1PB\((\d+)\)", old_tform)
            old_limit = int(match.group(1)) if match else 0
            if max_length > old_limit:
                _replace_header_card(
                    header,
                    "TFORM1",
                    _card("%-8s= '%s'" % ("TFORM1", "1PB(%d)" % max_length)),
                )
        data = bytes(descriptor_bytes) + bytes(heap)
        chunks.append(bytes(header) + pad_data(data))
        changed = True
    if not changed:
        raise RuntimeError("no ZIMAGE table to mutate in %s" % candidate_path)
    result = b"".join(chunks)
    with open(out_path, "wb") as handle:
        handle.write(result)
    details["candidate_bytes"] = len(result)
    details["candidate_sha256"] = hashlib.sha256(result).hexdigest()
    return details


def _mutate_float_word(payload, bitpix, old_word, new_word):
    width = abs(bitpix) // 8
    if bitpix not in (-32, -64):
        raise ValueError("float storage mutation requires BITPIX -32 or -64")
    if len(old_word) != width or len(new_word) != width:
        raise ValueError("float words must match BITPIX sample width")
    offset = payload.find(old_word)
    if offset < 0:
        raise ValueError("requested floating-point storage word was not found")
    changed = payload[:offset] + new_word + payload[offset + width:]
    return changed, dict(bitpix=bitpix, byte_offset=offset, before=old_word.hex(), after=new_word.hex())


def _damage_gzip_crc(member):
    if len(member) < 8:
        raise ValueError("gzip member is shorter than its trailer")
    damaged = bytearray(member)
    crc_at = len(damaged) - 8
    damaged[crc_at] ^= 1
    return bytes(damaged), dict(crc32_byte=crc_at, bit_mask=1)


# ---------------------------------------------------------------------------
# The gate
# ---------------------------------------------------------------------------


def tzim_run(binary, path):
    """`(exit code, file record)` for one file, from the binary's own JSON."""
    code, output = run([binary, "--fits-tile", "--json", os.path.basename(path)], cwd=os.path.dirname(path))
    record = None
    for line in output.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            parsed = json.loads(line)
        except json.JSONDecodeError:
            continue
        if parsed.get("type") == "file":
            record = parsed
    return code, record, output


def controlled_candidate_refusal(
    original_path,
    candidate_path,
    evidence,
    evidence_tag,
    stage,
    error,
    traceback_text="",
    reconstructed_path=None,
):
    """Turn an unexpected reader/checker exception into a failed result + evidence.

    Candidate bytes and the precise exception are retained; the traceback is
    stored as evidence rather than escaping as an unhandled job-level traceback.
    """
    tag = re.sub(r"[^A-Za-z0-9_.-]+", "_", str(evidence_tag)).strip("._")[:100] or "candidate"
    detail = dict(
        accepted=False,
        stage=stage,
        reason="%s: %s" % (type(error).__name__, error),
        exception_type=type(error).__name__,
        exception=str(error),
        traceback=traceback_text,
        original_path=os.path.abspath(original_path) if original_path else None,
        candidate_path=os.path.abspath(candidate_path) if candidate_path else None,
        evidence_files=[],
    )
    for label, path, suffix in (
        ("original", original_path, ".original.fits"),
        ("candidate", candidate_path, ".candidate.fits"),
        ("funpack reconstruction", reconstructed_path, ".funpack.fits"),
    ):
        if not path or not os.path.isfile(path):
            continue
        try:
            with open(path, "rb") as handle:
                content = handle.read()
            detail[label.replace(" ", "_") + "_bytes"] = len(content)
            detail[label.replace(" ", "_") + "_sha256"] = hashlib.sha256(content).hexdigest()
            name = "controlled-refusal-%s%s" % (tag, suffix)
            evidence.write_bytes(name, content)
            detail["evidence_files"].append(name)
        except Exception as evidence_error:  # keep the gate controlled if storage itself fails
            detail.setdefault("evidence_write_errors", []).append(
                "%s: %s: %s" % (label, type(evidence_error).__name__, evidence_error)
            )
    text_name = "controlled-refusal-%s.txt" % tag
    json_name = "controlled-refusal-%s.json" % tag
    detail["evidence_files"].extend([text_name, json_name])
    try:
        evidence.write_json(json_name, detail)
        evidence.write_text(
            text_name,
            "Controlled gate refusal\n"
            "stage: %s\nreason: %s\ntraceback retained below:\n%s"
            % (stage, detail["reason"], traceback_text),
        )
    except Exception as evidence_error:
        detail.setdefault("evidence_write_errors", []).append(
            "refusal record: %s: %s" % (type(evidence_error).__name__, evidence_error)
        )
        print("::error::could not write controlled-refusal evidence: %s" % evidence_error)
    return dict(
        accepted=False,
        candidate_sha256=detail.get("candidate_sha256"),
        problems=["controlled refusal at %s: %s" % (stage, detail["reason"])],
        controlled_failure=detail,
    )


def safe_inflate_tiles(path, evidence, evidence_tag):
    """Inflate untrusted candidate tiles without allowing parser exceptions to escape."""
    try:
        return inflate_tiles(path), None
    except Exception as error:
        refusal = controlled_candidate_refusal(
            None,
            path,
            evidence,
            evidence_tag,
            "candidate tile inflation",
            error,
            traceback.format_exc(),
        )
        return None, refusal


def _check_candidate_impl(original_path, candidate_path, case, scratch, evidence, evidence_tag):
    """Implementation for the guarded candidate acceptance entry point."""
    with open(original_path, "rb") as handle:
        original_bytes = handle.read()
    with open(candidate_path, "rb") as handle:
        candidate_bytes = handle.read()
    try:
        before = parse(original_bytes)
    except Exception as error:
        return controlled_candidate_refusal(
            original_path,
            candidate_path,
            evidence,
            evidence_tag,
            "parse original FITS input",
            error,
            traceback.format_exc(),
        )
    try:
        candidate = parse(candidate_bytes)
    except Exception as error:
        return controlled_candidate_refusal(
            original_path,
            candidate_path,
            evidence,
            evidence_tag,
            "parse tzim candidate",
            error,
            traceback.format_exc(),
        )
    diagnostic_mode = case.get("diagnostics")
    result = dict(
        candidate_sha256=hashlib.sha256(candidate_bytes).hexdigest(),
        bytes=dict(before=len(original_bytes), after=len(candidate_bytes)),
        source_matrix=source_image_matrix(before),
        candidate_matrix=candidate_image_matrix(candidate),
        problems=[],
    )
    required_card_checks = {}
    for keyword, expected_count in case.get("required_card_counts", {}).items():
        source_cards = [
            card for hdu in before for card in hdu.cards if card[:8].strip() == keyword
        ]
        candidate_cards = [
            card for hdu in candidate for card in hdu.cards if card[:8].strip() == keyword
        ]
        unchanged = source_cards == candidate_cards
        required_card_checks[keyword] = dict(
            expected_count=expected_count,
            source_count=len(source_cards),
            candidate_count=len(candidate_cards),
            source_cards=source_cards,
            candidate_cards=candidate_cards,
            unchanged=unchanged,
        )
        if len(source_cards) != expected_count or len(candidate_cards) != expected_count:
            result["problems"].append(
                "%s occurs %d times in the source and %d times in the rewritten candidate; expected %d"
                % (keyword, len(source_cards), len(candidate_cards), expected_count)
            )
        if not unchanged:
            result["problems"].append(
                "%s cards changed in the rewritten candidate" % keyword
            )
    if required_card_checks:
        result["required_card_checks"] = required_card_checks

    original_verify_code = None
    original_diagnostics = None
    if diagnostic_mode:
        result["original_hdus"] = hdu_layout(before)
        original_verify_code, original_verify_output = fitsverify(original_path, scratch)
        original_diagnostics = fitsverify_diagnostics(original_verify_output)
        result["original_fitsverify_exit"] = original_verify_code
        result["original_fitsverify_diagnostics"] = original_diagnostics
        evidence.write_text(
            "fitsverify-original-%s.txt" % evidence_tag, original_verify_output
        )
        if diagnostic_mode == "registered" and original_verify_code != 0:
            result["problems"].append(
                "registered positive fixture original fitsverify exit is %d, expected 0"
                % original_verify_code
            )
        if diagnostic_mode == "compare_original":
            inherited = any(
                "Unregistered XTENSION" in marker and "HDRLET" in marker
                for marker in original_diagnostics["markers"]
            )
            if original_verify_code == 0 or not inherited:
                result["problems"].append(
                    "HDRLET original did not reproduce the expected unregistered-XTENSION baseline"
                )

    verify_code, verify_output = fitsverify(candidate_path, scratch)
    result["fitsverify_exit"] = verify_code
    evidence.write_text("fitsverify-%s.txt" % evidence_tag, verify_output)
    if diagnostic_mode == "compare_original":
        candidate_diagnostics = fitsverify_diagnostics(verify_output)
        comparison = external_diagnostic_comparison(
            original_verify_code,
            original_diagnostics,
            verify_code,
            candidate_diagnostics,
        )
        result["candidate_fitsverify_diagnostics"] = candidate_diagnostics
        result["external_diagnostic_comparison"] = dict(
            **comparison,
            original_exit=original_verify_code,
            candidate_exit=verify_code,
            original=original_diagnostics,
            candidate=candidate_diagnostics,
        )
        evidence.write_json(
            "fitsverify-diagnostic-comparison-%s.json" % evidence_tag,
            result["external_diagnostic_comparison"],
        )
        if not comparison["no_new_diagnostics"]:
            result["problems"].append(
                "candidate introduced a new fitsverify error/warning or an inconsistent diagnostic count versus the exact HDRLET input baseline"
            )
    elif verify_code != 0:
        result["problems"].append(
            "fitsverify exited %d (errors + warnings); its output is in the evidence" % verify_code
        )

    if diagnostic_mode:
        result["candidate_hdus"] = hdu_layout(candidate)
    original_images = [h for h in before if h.kind() in ("PRIMARY", "IMAGE") and h.axes()]
    candidate_images = [
        h for h in candidate if h.kind() in ("PRIMARY", "IMAGE") and h.axes()
    ]
    result["hdus"] = dict(before=len(before), after=len(candidate))
    if len(original_images) != case.get("images", len(original_images)):
        result["problems"].append(
            "the fixture itself has %d images, the case says %d"
            % (len(original_images), case.get("images"))
        )
    if candidate_images:
        result["problems"].append(
            "%d image HDUs are still uncompressed after the rewrite" % len(candidate_images)
        )
    mapping, matched = map_candidate(before, candidate)
    if matched != len(candidate):
        result["problems"].append(
            "the candidate has %d HDUs the original does not account for" % (len(candidate) - matched)
        )
    for index, original_hdu in enumerate(before):
        if original_hdu.kind() in ("PRIMARY", "IMAGE"):
            continue
        other = candidate[mapping.get(index, -1)] if index in mapping else None
        if other is None:
            result["problems"].append("HDU %d (%s) is missing" % (index, original_hdu.kind()))
        elif other.raw != original_hdu.raw:
            result["problems"].append(
                "HDU %d (%s) was not copied verbatim: %d -> %d bytes"
                % (index, original_hdu.kind(), len(original_hdu.raw), len(other.raw))
            )
    tables = [h for h in candidate if h.value("ZIMAGE")]
    if len(tables) != len(original_images):
        result["problems"].append(
            "%d ZIMAGE tables for %d original images" % (len(tables), len(original_images))
        )
    if "hdu_delta" in case:
        delta = len(candidate) - len(before)
        if delta != case["hdu_delta"]:
            result["problems"].append(
                "HDU count changed by %+d, the contract says %+d" % (delta, case["hdu_delta"])
            )

    if case.get("skip_funpack"):
        result["funpack"] = "not run: HDRLET case is a byte-preservation and external-baseline test"
        result["accepted"] = not result["problems"]
        return result

    # Every array assertion below is source-vs-candidate->funpack -C. The fpack
    # control remains a separate integer-only reader/writer sanity check.
    unpack_code, unpack_output, unpacked = funpack(candidate_path, scratch)
    evidence.write_text("funpack-%s.txt" % evidence_tag, unpack_output)
    result["funpack_exit"] = unpack_code
    result["funpack_output"] = unpack_output
    if unpack_code != 0 or not unpacked:
        tail = " / ".join(line.strip() for line in unpack_output.splitlines()[-3:] if line.strip())
        result["problems"].append(
            "funpack -C exited %d and produced no file%s" % (unpack_code, (": " + tail) if tail else "")
        )
        result["accepted"] = False
        return result
    try:
        with open(unpacked, "rb") as handle:
            rebuilt_bytes = handle.read()
        rebuilt = parse(rebuilt_bytes)
    except Exception as error:
        refusal = controlled_candidate_refusal(
            original_path,
            candidate_path,
            evidence,
            evidence_tag,
            "parse funpack -C reconstruction",
            error,
            traceback.format_exc(),
            reconstructed_path=unpacked,
        )
        refusal["controlled_failure"]["funpack_exit"] = unpack_code
        refusal["controlled_failure"]["funpack_output"] = unpack_output
        tag = re.sub(r"[^A-Za-z0-9_.-]+", "_", str(evidence_tag)).strip("._")[:100] or "candidate"
        evidence.write_json(
            "controlled-refusal-%s.json" % tag,
            refusal["controlled_failure"],
        )
        return refusal
    if rebuilt and rebuilt[0].kind() == "PRIMARY" and not rebuilt[0].axes() and before[0].axes():
        rebuilt = rebuilt[1:]
    if len(rebuilt) != len(before):
        result["problems"].append(
            "funpack -C produced %d HDUs for an original of %d" % (len(rebuilt), len(before))
        )

    diffs = {}
    array_checks = []
    for index, (a, b) in enumerate(zip(before, rebuilt)):
        label = "%d-%s" % (index, a.kind())
        if a.kind() in ("PRIMARY", "IMAGE"):
            problems = compare_arrays(a, b)
            array_checks.append(dict(hdu=index, kind=a.kind(), matches=(not problems), problems=problems))
            result["problems"].extend("%s: %s" % (label, problem) for problem in problems)
        elif cards_of(a) != cards_of(b) or a.data != b.data:
            result["problems"].append(
                "%s: funpack -C did not reconstruct the copied HDU (its cards are in the diff)" % label
            )
        cards = literal_card_diff(a, b)
        if cards:
            diffs[label] = [dict(diff, classification=classify_diff(diff)) for diff in cards]
    result["array_checks"] = array_checks
    result["header_diff"] = diffs
    evidence.write_json("header-diff-%s.json" % evidence_tag, diffs)
    if diffs:
        text = "\n".join(
            "%s: %s card; original index %s, reconstructed index %s; %s\n"
            "  original: %r\n  reconstructed: %r\n  classification: %s"
            % (
                label,
                diff["kind"],
                diff["original_index"],
                diff["reconstructed_index"],
                diff["keyword"],
                diff["original"],
                diff["reconstructed"],
                diff["classification"],
            )
            for label, items in diffs.items()
            for diff in items
        ) + "\n"
        evidence.write_text("header-diff-%s.txt" % evidence_tag, text)
        result["problems"].append(
            "the reconstructed header is not identical (see header-diff-%s.txt)" % evidence_tag
        )
    else:
        evidence.write_text(
            "header-diff-%s.txt" % evidence_tag,
            "No literal 80-column card differences, including END, comments, spacing, or order.\n",
        )
    result["accepted"] = not result["problems"]
    return result


def check_candidate(original_path, candidate_path, case, scratch, evidence, evidence_tag):
    """Controlled, fail-closed entry point for every candidate acceptance check."""
    try:
        return _check_candidate_impl(
            original_path, candidate_path, case, scratch, evidence, evidence_tag
        )
    except Exception as error:
        return controlled_candidate_refusal(
            original_path,
            candidate_path,
            evidence,
            evidence_tag,
            "candidate acceptance check",
            error,
            traceback.format_exc(),
        )


def check_fixture(binary, case, root, scratch, evidence):
    """Run tzim on one input, then use the shared candidate acceptance path."""
    path = os.path.join(root, case["name"])
    original_bytes = _read_bytes(path)
    before = parse(original_bytes)
    kept = os.path.join(scratch, "original-" + case["name"])
    with open(kept, "wb") as handle:
        handle.write(original_bytes)
    evidence.write_bytes("input-" + case["name"], original_bytes)
    input_sha = hashlib.sha256(original_bytes).hexdigest()
    result = dict(
        name=case["name"],
        why=case["why"],
        expect=case["expect"],
        input_case=case.get("input_case"),
        contract_expectation=case.get("contract_expectation"),
        input_sha256=input_sha,
        original=kept,
        source_matrix=source_image_matrix(before),
        problems=[],
    )
    expected_matrix = case.get("matrix")
    if expected_matrix and expected_matrix.get("floating_storage"):
        source_image = next(
            (
                hdu
                for hdu in before
                if hdu.kind() == expected_matrix["role"]
                and hdu.int("BITPIX") == expected_matrix["bitpix"]
                and hdu.int("NAXIS") == expected_matrix["naxis"]
            ),
            None,
        )
        result["floating_storage"] = (
            float_specials_present(source_image.data, expected_matrix["bitpix"])
            if source_image is not None
            else {}
        )
        missing_specials = [
            name
            for name in expected_matrix["floating_storage"]
            if not result["floating_storage"].get(name, False)
        ]
        if missing_specials:
            result["problems"].append(
                "source float fixture is missing storage patterns: %s"
                % ", ".join(missing_specials)
            )
    expected_input_bytes = case.get("expected_input_bytes")
    expected_input_sha256 = case.get("expected_input_sha256")
    if expected_input_bytes is not None and len(original_bytes) != expected_input_bytes:
        result["problems"].append(
            "external fixture is %d bytes, expected %d"
            % (len(original_bytes), expected_input_bytes)
        )
        return result
    if expected_input_sha256 and input_sha != expected_input_sha256:
        result["problems"].append(
            "external fixture SHA-256 is %s, expected %s"
            % (input_sha, expected_input_sha256)
        )
        return result

    code, record, output = tzim_run(binary, path)
    result["tzim_exit"] = code
    result["tzim"] = record
    evidence.write_json("tzim-%s.json" % case["name"], record)
    candidate_bytes = _read_bytes(path)
    evidence.write_bytes("candidate-" + case["name"], candidate_bytes)
    result["candidate_sha256"] = hashlib.sha256(candidate_bytes).hexdigest()
    result["bytes"] = dict(before=len(original_bytes), after=len(candidate_bytes))
    if code != 0:
        result["problems"].append("tzim exited %d: %s" % (code, output.strip()[:400]))
        return result

    status = (record or {}).get("status")
    if case["expect"] == "untouched":
        expected_status = case.get("status")
        if expected_status and status != expected_status:
            result["problems"].append("status is %r, expected %r" % (status, expected_status))
        if candidate_bytes != original_bytes:
            result["problems"].append("a file that must not be rewritten was rewritten")
        required = case.get("required_card_counts", {})
        card_record = {}
        after = parse(candidate_bytes)
        for keyword, expected_count in required.items():
            source_cards = [
                card for hdu in before for card in hdu.cards
                if card[:8].strip() == keyword
            ]
            candidate_cards = [
                card for hdu in after for card in hdu.cards
                if card[:8].strip() == keyword
            ]
            same = source_cards == candidate_cards
            card_record[keyword] = dict(
                expected_count=expected_count,
                source_count=len(source_cards),
                candidate_count=len(candidate_cards),
                cards=[card for card in candidate_cards],
                unchanged=same,
            )
            if len(source_cards) != expected_count or len(candidate_cards) != expected_count:
                result["problems"].append(
                    "%s occurs %d times in the external input and %d times in the candidate; expected %d"
                    % (keyword, len(source_cards), len(candidate_cards), expected_count)
                )
            if not same:
                result["problems"].append("%s cards changed across the no-rewrite path" % keyword)
        if card_record:
            result["external_card_checks"] = card_record
        result["candidate_accepted"] = None
        return result

    if candidate_bytes == original_bytes:
        said = ""
        if record:
            said = " (tzim said: %s%s)" % (
                record.get("status", "?"),
                (": " + record["note"]) if record.get("note") else "",
            )
        result["problems"].append("the file did not change, so nothing was written%s" % said)
        return result
    strict_size = len(candidate_bytes) < len(original_bytes)
    result["strict_size"] = dict(
        input_bytes=len(original_bytes),
        candidate_bytes=len(candidate_bytes),
        improved=strict_size,
    )
    if not strict_size:
        result["problems"].append(
            "candidate is %d bytes, not strictly smaller than input %d bytes"
            % (len(candidate_bytes), len(original_bytes))
        )
    if status != "optimized":
        result["problems"].append("status is %r, expected 'optimized'" % status)

    acceptance = check_candidate(kept, path, case, scratch, evidence, case["name"])
    result["candidate_acceptance"] = acceptance["accepted"]
    result["acceptance"] = {
        "accepted": acceptance["accepted"],
        "problems": acceptance["problems"],
        "fitsverify_exit": acceptance.get("fitsverify_exit"),
        "funpack_exit": acceptance.get("funpack_exit"),
        "array_checks": acceptance.get("array_checks", []),
    }
    result.update({key: value for key, value in acceptance.items() if key != "problems"})
    result["problems"].extend(acceptance["problems"])
    return result


def select_fixtures(cases, scope):
    """Choose a full run or only additive rows missing from the reviewed handoff."""
    if scope == "full":
        return list(cases)
    if scope == "residual":
        return [case for case in cases if case.get("residual_only")]
    raise ValueError("unknown FITS evidence scope %r" % scope)


def acceptance_matrix_summary(cases, fixture_results, scope):
    """Bind each new matrix row to its raw input, writer result and candidate."""
    by_name = {result.get("name"): result for result in fixture_results}
    rows = []
    for case in cases:
        expected = case.get("matrix")
        if not expected:
            continue
        result = by_name.get(case["name"], {})
        identity = (expected["role"], expected["bitpix"], expected["naxis"])
        source_matches = any(
            (row.get("role"), row.get("bitpix"), row.get("naxis")) == identity
            for row in result.get("source_matrix", [])
        )
        candidate_matches = any(
            (row.get("role"), row.get("bitpix"), row.get("naxis")) == identity
            for row in result.get("candidate_matrix", [])
        )
        identity_bound = all(
            result.get(field)
            for field in ("input_sha256", "candidate_sha256", "binary_sha256")
        ) and all(
            isinstance(value, int) and value > 0
            for value in ((result.get("bytes") or {}).get("before"),
                          (result.get("bytes") or {}).get("after"))
        )
        row_ok = (
            identity_bound
            and source_matches
            and candidate_matches
            and result.get("candidate_acceptance") is True
            and result.get("strict_size", {}).get("improved") is True
            and not result.get("problems")
        )
        rows.append(
            dict(
                fixture=case["name"],
                expected=expected,
                input_bytes=(result.get("bytes") or {}).get("before"),
                input_sha256=result.get("input_sha256"),
                candidate_bytes=(result.get("bytes") or {}).get("after"),
                candidate_sha256=result.get("candidate_sha256"),
                binary_sha256=result.get("binary_sha256"),
                identity_bound=identity_bound,
                tzim_exit=result.get("tzim_exit"),
                tzim_status=(result.get("tzim") or {}).get("status"),
                source_matrix_match=source_matches,
                candidate_matrix_match=candidate_matches,
                strict_size=(result.get("strict_size") or {}).get("improved"),
                independent_acceptance=result.get("candidate_acceptance"),
                accepted=row_ok,
                problems=result.get("problems", []),
            )
        )
    expected_bitpix = sorted({row["expected"]["bitpix"] for row in rows})
    expected_naxis = sorted({row["expected"]["naxis"] for row in rows})
    expected_roles = sorted({row["expected"]["role"] for row in rows})
    accepted_rows = [row for row in rows if row["accepted"]]
    covered_bitpix = sorted({row["expected"]["bitpix"] for row in accepted_rows})
    covered_naxis = sorted({row["expected"]["naxis"] for row in accepted_rows})
    covered_roles = sorted({row["expected"]["role"] for row in accepted_rows})
    missing = [row["fixture"] for row in rows if not row["accepted"]]
    return dict(
        schema_version=1,
        scope=scope,
        source="new issue #78 residual rows; previously confirmed fixtures are not replayed in residual scope",
        inherited_run_id="40551071072",
        inherited_run_sha="0232d92b3171b4b7af5cdb4910f852a66308e7b4",
        inherited_run_attempt=1,
        inherited_run_context_only=True,
        inherited_run_replayed=False,
        expected=dict(bitpix=expected_bitpix, naxis=expected_naxis, roles=expected_roles),
        observed=dict(bitpix=covered_bitpix, naxis=covered_naxis, roles=covered_roles),
        missing=missing,
        rows=rows,
        complete=bool(rows) and not missing,
    )


def corpus_manifest(root, cases, scope="full"):
    """Hash the exact synthetic and pinned external input fixtures for this run."""
    fixtures = []
    for case in cases:
        path = os.path.join(root, case["name"])
        with open(path, "rb") as handle:
            raw = handle.read()
        fixtures.append(
            dict(
                name=case["name"],
                bytes=len(raw),
                sha256=hashlib.sha256(raw).hexdigest(),
                expectation=case.get("expect"),
                status=case.get("status"),
                residual_only=bool(case.get("residual_only")),
                matrix=case.get("matrix"),
                input_case=case.get("input_case"),
                why=case.get("why"),
            )
        )
    return dict(
        schema_version=2,
        scope=scope,
        requested_sha=os.environ.get("REQUESTED_SHA", os.environ.get("GITHUB_SHA")),
        checked_out_sha=os.environ.get("CHECKED_OUT_SHA"),
        run_id=os.environ.get("GITHUB_RUN_ID"),
        run_attempt=os.environ.get("GITHUB_RUN_ATTEMPT"),
        source="deterministic fixtures generated by fits-heasarc.py",
        prior_run=dict(
            run_id="40551071072",
            sha="0232d92b3171b4b7af5cdb4910f852a66308e7b4",
            attempt=1,
            evidence_source="requester-reported exact-SHA handoff; this job does not download or re-transfer it",
            context_only=True,
            replayed=False,
            current_fixture_scope=(
                "additive #78 rows only" if scope == "residual" else "current full fixture set at this SHA"
            ),
            note=(
                "this job does not replay the inherited workflow run; full scope, if selected, "
                "runs the current script's fixture set at the current SHA"
            ),
        ),
        external_astropy=dict(
            repository="astropy/astropy",
            commit=ASTROPY_COMMIT,
            archive_sha256=ASTROPY_ARCHIVE_SHA256,
            selected_path="astropy/io/fits/tests/data/double_ext.fits",
            selected_sha256=ASTROPY_DOUBLE_EXT_SHA256,
            selected_bytes=ASTROPY_DOUBLE_EXT_BYTES,
            included=any(case["name"] == "external_double_ext.fits" for case in cases),
        ),
        fixtures=fixtures,
    )


class Evidence:
    """A directory of files that outlive the job, with a checksum over them."""

    def __init__(self, root):
        self.root = root
        os.makedirs(root, exist_ok=True)
        self.written = []

    def _path(self, name):
        return os.path.join(self.root, name)

    def write_text(self, name, text):
        with open(self._path(name), "w", encoding="utf-8") as handle:
            handle.write(text)
        self.written.append(name)

    def write_bytes(self, name, content):
        with open(self._path(name), "wb") as handle:
            handle.write(content)
        self.written.append(name)

    def read_text(self, name):
        with open(self._path(name), encoding="utf-8") as handle:
            return handle.read()

    def write_json(self, name, payload):
        with open(self._path(name), "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2, sort_keys=True, default=str)
            handle.write("\n")
        self.written.append(name)

    def seal(self, extra=None):
        """Write `checksums.txt`, and say what it covers."""
        digest = hashlib.sha256()
        lines = []
        for name in sorted(self.written):
            blob = open(self._path(name), "rb").read()
            digest.update(name.encode() + b"\0" + blob)
            lines.append("%s  %s" % (hashlib.sha256(blob).hexdigest(), name))
        if extra:
            lines.append("")
            lines.extend(extra)
        lines.append("manifest digest (sha256 over name and bytes, in this order): %s" % digest.hexdigest())
        with open(self._path("checksums.txt"), "w", encoding="utf-8") as handle:
            handle.write("\n".join(lines) + "\n")
        return digest.hexdigest()


def render_diagnostics(summary, evidence):
    """Human-readable, artifact-backed diagnostics that CI can relay verbatim."""
    lines = ["# FITS gate diagnostics", ""]
    if os.environ.get("GITHUB_RUN_ID") and os.environ.get("GITHUB_REPOSITORY"):
        lines.append("Run: %s/%s/actions/runs/%s" % (
            os.environ.get("GITHUB_SERVER_URL", "https://github.com"),
            os.environ["GITHUB_REPOSITORY"], os.environ["GITHUB_RUN_ID"]))
        lines.append("")

    scope = (summary.get("run") or {}).get("scope", "full")
    lines.append("Evidence scope: `%s`." % scope)
    binary = summary.get("binary") or {}
    if binary:
        lines.append(
            "Executed binary: `%s`, %s bytes, SHA-256 `%s`."
            % (binary.get("path", "not recorded"), binary.get("bytes", "not recorded"),
               binary.get("sha256", "not recorded"))
        )
    lines.append("")

    lines += ["## Literal 80-column header diffs", ""]
    lines.append(
        "The comparison is original HDU versus the bytes produced by `tzim` then `funpack -C`. "
        "It includes every 80-byte card through END, card order, comments, spacing, CHECKSUM, "
        "DATASUM, EXTEND and ZEXTEND; nothing is normalized or excluded. These diffs diagnose "
        "the complete round trip and do not by themselves assign a difference to one program. "
        "The raw source, tzim candidate, full JSON/text diff, and per-tool output are retained in "
        "the checksummed artifact."
    )
    lines.append("")
    diffs = [f for f in summary["fixtures"] if f.get("header_diff")]
    if not diffs:
        lines.append("No literal card differences were found in the fixtures that completed `funpack -C`.")
        lines.append("")
    for fixture in diffs:
        lines.append("### " + fixture["name"])
        lines.append("")
        lines.extend(evidence.read_text("header-diff-%s.txt" % fixture["name"]).rstrip().splitlines())
        lines.append("")

    if scope == "residual":
        run = summary.get("run") or {}
        lines.append("## Exact run and issue #78 residual evidence")
        lines.append("")
        lines.append(
            "Requested SHA `%s`; checked-out SHA `%s`; run `%s`, attempt `%s`."
            % (run.get("requested_sha", "not recorded"),
               run.get("checked_out_sha", "not recorded"),
               run.get("run_id", "not recorded"),
               run.get("run_attempt", "not recorded"))
        )
        lines.append(
            "Run `40551071072` at SHA `0232d92b3171b4b7af5cdb4910f852a66308e7b4` is context only; "
            "it is neither downloaded nor replayed by this gate. Only the additive missing rows "
            "selected in `corpus-manifest.json` run at this SHA."
        )
        lines.append("")
        matrix = summary.get("acceptance_matrix") or {}
        lines.append("Acceptance matrix: `%s`; complete: **%s**; observed coverage: `%s`." % (
            matrix.get("scope", scope), matrix.get("complete", False),
            json.dumps(matrix.get("observed", {}), sort_keys=True)))
        lines.append("")
        lines.append("| fixture | input SHA-256 | candidate SHA-256 | binary SHA-256 | strict-size | independent acceptance | result |")
        lines.append("| --- | --- | --- | --- | --- | --- | --- |")
        for row in matrix.get("rows", []):
            lines.append(
                "| %(fixture)s | `%(input_sha256)s` | `%(candidate_sha256)s` | `%(binary_sha256)s` | "
                "%(strict_size)s | %(independent_acceptance)s | %(accepted)s |" % {
                    "fixture": row.get("fixture", "?"),
                    "input_sha256": row.get("input_sha256") or "not recorded",
                    "candidate_sha256": row.get("candidate_sha256") or "not recorded",
                    "binary_sha256": row.get("binary_sha256") or "not recorded",
                    "strict_size": row.get("strict_size"),
                    "independent_acceptance": row.get("independent_acceptance"),
                    "accepted": row.get("accepted"),
                }
            )
        lines.append("")
        lines.append("## Residual controls")
        lines.append("")
        for control in summary.get("controls", []):
            lines.append(
                "- `%s` (%s): expected %s; detected **%s**."
                % (control.get("name"), control.get("control_type", "unclassified"),
                   control.get("expected", "not recorded"), control.get("detected", False))
            )
            for row in control.get("rows", []):
                lines.append(
                    "  - `%s`: ok=%s; binary SHA-256 `%s`; input SHA-256 `%s`; output SHA-256 `%s`."
                    % (row.get("fixture", row.get("name", "row")), row.get("ok", row.get("detected", False)),
                       row.get("binary_sha256", "not recorded"), row.get("before_sha256", "not recorded"),
                       row.get("after_sha256", row.get("candidate_sha256", "not recorded")))
                )
        lines.append("")
        lines.append(
            "The per-file inputs, rewritten candidates, independent-reader reconstructions, literal card diffs, "
            "control JSON, second-run outputs, and SHA-256 sidecars are retained in the sealed artifact. "
            "`fitsverify` and independent CFITSIO `funpack -C` reads occur in CI only; they are not "
            "runtime reads for every accepted user file. FITS remains opt-in Tier 2; this evidence is "
            "not full-format admission or a universal stability guarantee."
        )
        return "\n".join(lines).rstrip() + "\n"

    for fixture_name, title in (
        ("multi_hdu.fits", "registered multi-HDU positive traversal"),
        ("hdrlet_preservation.fits", "HDRLET preservation-only baseline comparison"),
    ):
        fixture = next((f for f in summary["fixtures"] if f["name"] == fixture_name), None)
        lines.append("## " + title)
        lines.append("")
        if not fixture:
            lines.append("The fixture did not run.")
            lines.append("")
            continue
        lines.append(
            "Original fitsverify exit %s; candidate exit %s; funpack -C exit %s."
            % (fixture.get("original_fitsverify_exit", "not run"),
               fixture.get("fitsverify_exit", "not run"),
               fixture.get("funpack_exit", "not run"))
        )
        if fixture_name == "multi_hdu.fits":
            lines.append(
                "This fixture uses only registered IMAGE and BINTABLE extensions; the original must "
                "pass fitsverify before the candidate is interpreted."
            )
        else:
            comparison = fixture.get("external_diagnostic_comparison", {})
            lines.append(
                "The original's unregistered HDRLET diagnostic is an explicit per-file baseline; "
                "candidate added no new marker or warning/error count: %s (exact match: %s)."
                % (comparison.get("no_new_diagnostics", "not compared"),
                   comparison.get("exact_match", "not compared"))
            )
            lines.append(
                "The inherited unregistered-XTENSION error may remain or disappear; a different/new "
                "candidate diagnostic fails this case. This is not a global warning/error allowlist. "
                "HDRLET raw bytes must match exactly."
            )
        for label in ("original", "candidate"):
            layout = fixture.get(label + "_hdus") or []
            lines.append("")
            lines.append("%s HDU layout (byte offsets):" % label.capitalize())
            for row in layout:
                lines.append(
                    "- HDU %(index)s %(kind)s: start=%(start)s, header=%(header_bytes)s, "
                    "data=%(data_bytes)s, padded-data=%(padded_data_bytes)s, next=%(next_start)s" % row
                )
        for name, caption in (
            ("fitsverify-original-%s.txt" % fixture_name, "fitsverify -l on original input"),
            ("fitsverify-%s.txt" % fixture_name, "fitsverify -l on tzim candidate"),
            ("funpack-%s.txt" % fixture_name, "funpack -C on tzim candidate"),
        ):
            try:
                text = evidence.read_text(name)
            except FileNotFoundError:
                continue
            lines += ["", "### " + caption, "", "````text"]
            lines.extend(text.rstrip().splitlines())
            lines += ["````", ""]
        lines.append("")

    zero = next((f for f in summary["fixtures"] if f["name"] == "zero_width_column.fits"), None)
    lines.append("## zero_width_column.fits — owner-confirmed strict whole-file size result")
    lines.append("")
    if zero:
        lines.append("**Input:** " + (zero.get("input_case") or "description not recorded"))
        lines.append("")
        lines.append("Input SHA-256: `%s`; output SHA-256: `%s`." % (
            zero.get("input_sha256", "not recorded"), zero.get("candidate_sha256", "not recorded")))
        lines.append("")
        lines.append("**Contract:** " +
                     (zero.get("contract_expectation") or zero.get("why", "not recorded")))
        lines.append("")
        record = zero.get("tzim") or {}
        lines.append("**Observed:** status `%s`, note `%s`; bytes %s → %s."
                     % (record.get("status", "?"), record.get("note"),
                        zero.get("bytes", {}).get("before", "?"),
                        zero.get("bytes", {}).get("after", "?")))
        lines.append("")
        lines.append(
            "Expected result is unchanged + `already_optimal`: the strict whole-file size gate "
            "applies after candidate formation. The discarded candidate size is not needed; no "
            "zero-width behavior was expanded."
        )
    else:
        lines.append("The fixture did not run.")
    external = next(
        (f for f in summary["fixtures"] if f["name"] == "external_double_ext.fits"), None
    )
    lines += ["", "## External Astropy `double_ext.fits` — real-file EXTNAMX coverage", ""]
    if external:
        lines.append("Source identity: %s; input SHA-256 `%s`; output SHA-256 `%s`." % (
            external.get("input_case"), external.get("input_sha256", "not recorded"),
            external.get("candidate_sha256", "not recorded")))
        lines.append("")
        lines.append("The byte-pinned external input is already a RICE_1 compressed image outside this build's rewrite subset. The contract expects `skipped`, unchanged bytes, and both literal EXTNAMX cards preserved. Each card is also exercised separately through a rewritten synthetic harness image; the pixel arrays are explicitly synthetic, not claimed RICE_1 decompressions. Single-card inputs avoid a duplicate-keyword warning, so no warning allowlist is introduced.")
        lines.append("")
        lines.append("External no-op card check: `%s`." % json.dumps(
            external.get("external_card_checks", {}), sort_keys=True))
        lines.append("")
    else:
        lines.append("The external fixture did not run.")
        lines.append("")
    external_rewrites = [
        f for f in summary["fixtures"] if f["name"].startswith("external_extnamx_")
    ]
    lines.append("### External-card-derived rewrite paths")
    lines.append("")
    if external_rewrites:
        for external_rewrite in external_rewrites:
            lines.append("#### %s" % external_rewrite["name"])
            lines.append("")
            lines.append("Input: %s" % external_rewrite.get("input_case", "description not recorded"))
            lines.append("")
            lines.append("Candidate SHA-256 `%s`; accepted by the shared candidate path: `%s`." % (
                external_rewrite.get("candidate_sha256", "not recorded"),
                external_rewrite.get("candidate_acceptance", "not reached")))
            lines.append("")
            lines.append("Exact-card check and acceptance problems: `%s`; `%s`." % (
                json.dumps(external_rewrite.get("required_card_checks", {}), sort_keys=True),
                json.dumps(external_rewrite.get("acceptance", {}).get("problems", []), sort_keys=True)))
            lines.append("")
    else:
        lines.append("No external-card-derived rewrite fixture ran.")
        lines.append("")
    controls = summary.get("controls", [])
    control_counts = Counter(control.get("control_type", "unclassified") for control in controls)
    lines += ["## Controls", "", "Counts: `%s`." % json.dumps(control_counts, sort_keys=True), ""]
    for control in controls:
        role = control.get("control_type", "unclassified")
        lines.append("- `%s` (%s): expected %s; detected **%s**; record `%s`." % (
            control.get("name"), role, control.get("expected", "not recorded"),
            control.get("detected", False), json.dumps(control, sort_keys=True)))
    lines.append("")
    lines.append(
        "The corrupt-tile control runs both the intact and altered candidates through the same "
        "main acceptance helper used by normal fixtures. It passes only when the intact candidate "
        "is accepted and the corrupted candidate is rejected by `funpack -C` or the byte-exact "
        "array comparison; fitsverify may accept structurally valid FITS with changed image data. "
        "The separate structural control must make fitsverify nonzero. The positive fpack control "
        "is `fpack → funpack -C`: its output must pass fitsverify and its full literal header diff "
        "must be empty. The fpack reference is integer-only; float arrays are compared only as "
        "original versus `tzim → funpack -C`. The additional #78 controls run every "
        "successful residual rewrite a second time, preserve malformed inputs and their "
        "directory contents on failure, and send NaN-payload, signed-zero, gzip, descriptor "
        "and card mutations through the same independent acceptance path."
    )
    lines.append("")
    lines.append("Full control-tool output and round-trip diffs are retained in the checksummed evidence.")
    return "\n".join(lines).rstrip() + "\n"

def _read_bytes(path):
    with open(path, "rb") as handle:
        return handle.read()


def _first_source_image(path):
    hdus = parse(_read_bytes(path))
    return next(hdu for hdu in hdus if hdu.kind() in ("PRIMARY", "IMAGE") and hdu.axes())


def run_idempotence_control(binary, cases, fixture_results, root, scratch, evidence, binary_sha256):
    """Run every successful residual rewrite a second time and retain both byte streams."""
    by_name = {result.get("name"): result for result in fixture_results}
    rows = []
    for case in cases:
        if case.get("expect") not in ("rewritten", "mixed"):
            continue
        result = by_name.get(case["name"], {})
        if (result.get("tzim") or {}).get("status") != "optimized":
            continue
        path = os.path.join(root, case["name"])
        before = _read_bytes(path)
        before_sha = hashlib.sha256(before).hexdigest()
        code, record, output = tzim_run(binary, path)
        after = _read_bytes(path)
        after_sha = hashlib.sha256(after).hexdigest()
        status = (record or {}).get("status")
        unchanged = before == after
        ok = code == 0 and status == "already_optimal" and unchanged
        row = dict(
            fixture=case["name"],
            binary_sha256=binary_sha256,
            second_run_exit=code,
            second_run_status=status,
            before_bytes=len(before),
            after_bytes=len(after),
            before_sha256=before_sha,
            after_sha256=after_sha,
            unchanged=unchanged,
            output=output,
            ok=ok,
        )
        rows.append(row)
        result["idempotence"] = {key: value for key, value in row.items() if key != "output"}
        evidence.write_bytes("idempotence-input-" + case["name"], before)
        evidence.write_bytes("idempotence-output-" + case["name"], after)
        evidence.write_json("idempotence-" + case["name"] + ".json", row)
        evidence.write_text("idempotence-" + case["name"] + ".txt", output)
    detected = bool(rows) and all(row["ok"] for row in rows)
    control = dict(
        name="second_run_idempotence",
        control_type="preservation",
        expected=(
            "for each successfully rewritten candidate, a second --fits-tile run returns "
            "already_optimal and preserves exact bytes"
        ),
        binary_sha256=binary_sha256,
        rows=rows,
        detected=detected,
    )
    evidence.write_json("control-idempotence.json", control)
    return control


def run_failure_preservation_control(binary, scratch, evidence, binary_sha256):
    """Exercise actual parse failures and prove the original path is not replaced."""
    control_root = os.path.join(scratch, "failure-preservation")
    os.makedirs(control_root, exist_ok=True)
    valid = image_hdu(16, [64, 128], int16_pattern(64, 128), primary=True)
    malformed = [
        ("trailing_bytes", valid + b"TRAILING", "bytes after the last HDU"),
        ("truncated_record", valid[:-1], "final FITS record truncated by one byte"),
    ]
    rows = []
    for name, raw, defect in malformed:
        case_dir = os.path.join(control_root, name)
        os.makedirs(case_dir, exist_ok=True)
        path = os.path.join(case_dir, name + ".fits")
        with open(path, "wb") as handle:
            handle.write(raw)
        before = _read_bytes(path)
        before_sha = hashlib.sha256(before).hexdigest()
        before_names = sorted(os.listdir(case_dir))
        try:
            code, record, output = tzim_run(binary, path)
            after = _read_bytes(path)
            after_sha = hashlib.sha256(after).hexdigest()
            after_names = sorted(os.listdir(case_dir))
            status = (record or {}).get("status")
            unchanged = before == after
            no_temporary_files = before_names == after_names
            ok = code != 0 and status == "failed" and unchanged and no_temporary_files
            row = dict(
                fixture=name + ".fits",
                defect=defect,
                binary_sha256=binary_sha256,
                exit=code,
                status=status,
                before_bytes=len(before),
                after_bytes=len(after),
                before_sha256=before_sha,
                after_sha256=after_sha,
                unchanged=unchanged,
                no_temporary_files=no_temporary_files,
                output=output,
                ok=ok,
            )
            evidence.write_bytes("failure-preservation-input-" + name + ".fits", before)
            evidence.write_bytes("failure-preservation-output-" + name + ".fits", after)
        except Exception as error:
            row = dict(
                fixture=name + ".fits",
                defect=defect,
                binary_sha256=binary_sha256,
                ok=False,
                exception="%s: %s" % (type(error).__name__, error),
                traceback=traceback.format_exc(),
            )
            evidence.write_bytes("failure-preservation-input-" + name + ".fits", before)
            evidence.write_bytes("failure-preservation-output-" + name + ".fits", _read_bytes(path))
        rows.append(row)
        evidence.write_json("failure-preservation-" + name + ".json", row)
        evidence.write_text("failure-preservation-" + name + ".txt", row.get("output", row.get("traceback", "")))
    control = dict(
        name="failure_preservation",
        control_type="preservation",
        expected=(
            "malformed FITS input returns failed/nonzero, leaves the original SHA unchanged, "
            "and leaves no temporary siblings"
        ),
        binary_sha256=binary_sha256,
        rows=rows,
        detected=bool(rows) and all(row["ok"] for row in rows),
    )
    evidence.write_json("control-failure-preservation.json", control)
    return control


def run_residual_mutation_controls(binary, cases, fixture_results, root, scratch, evidence, binary_sha256):
    """Mutate actual candidates, then require the shared independent acceptance path to reject them."""
    cases_by_name = {case["name"]: case for case in cases}
    results_by_name = {result.get("name"): result for result in fixture_results}
    float_name = "residual_float32_naxis1_image.fits"
    integer_name = "residual_bitpix8_naxis1_primary.fits"
    if float_name not in cases_by_name or integer_name not in cases_by_name:
        missing = [name for name in (float_name, integer_name) if name not in cases_by_name]
        control = dict(
            name="candidate_mutations",
            control_type="negative",
            expected="all mutation rows run through actual candidates and the shared acceptance path",
            detected=False,
            missing_fixtures=missing,
        )
        evidence.write_json("control-candidate-mutations.json", control)
        return control

    mutations = [
        (
            "nan_payload",
            float_name,
            dict(
                payload_mutator=lambda payload, bitpix: _mutate_float_word(
                    payload, bitpix, bytes.fromhex("7fc00001"), bytes.fromhex("7fc00002")
                )
            ),
            "external funpack/byte-array comparison rejects changed NaN payload bytes",
        ),
        (
            "signed_zero",
            float_name,
            dict(
                payload_mutator=lambda payload, bitpix: _mutate_float_word(
                    payload, bitpix, bytes.fromhex("00000000"), bytes.fromhex("80000000")
                )
            ),
            "external funpack/byte-array comparison rejects +0 to -0 storage mutation",
        ),
        (
            "gzip_crc",
            float_name,
            dict(compressed_member_mutator=_damage_gzip_crc),
            "gzip CRC damage is rejected by tile inflation or the external reader",
        ),
        (
            "descriptor_zero_length",
            integer_name,
            dict(
                descriptor_mutator=lambda descriptors, heap, rows: (
                    descriptors.__setitem__(slice(0, 4), struct.pack(">I", 0))
                    or dict(length=0, offset=struct.unpack(">I", descriptors[4:8])[0])
                )
            ),
            "a zero-length tile descriptor is rejected rather than treated as an empty image",
        ),
        (
            "descriptor_negative_count",
            integer_name,
            dict(
                descriptor_mutator=lambda descriptors, heap, rows: (
                    descriptors.__setitem__(slice(0, 4), struct.pack(">i", -1))
                    or dict(count=-1, offset=struct.unpack(">I", descriptors[4:8])[0])
                )
            ),
            "negative signed P count is rejected by the same candidate acceptance path",
        ),
        (
            "descriptor_negative_offset",
            integer_name,
            dict(
                descriptor_mutator=lambda descriptors, heap, rows: (
                    descriptors.__setitem__(slice(4, 8), struct.pack(">i", -1))
                    or dict(count=struct.unpack(">I", descriptors[:4])[0], offset=-1)
                )
            ),
            "a negative signed P heap offset is rejected by the same candidate path",
        ),
        (
            "descriptor_offset_past_heap",
            integer_name,
            dict(
                descriptor_mutator=lambda descriptors, heap, rows: (
                    descriptors.__setitem__(slice(4, 8), struct.pack(">I", len(heap) + 1))
                    or dict(count=struct.unpack(">I", descriptors[:4])[0], offset=len(heap) + 1)
                )
            ),
            "a descriptor offset one byte beyond the heap is rejected",
        ),
        (
            "descriptor_length_past_heap",
            integer_name,
            dict(
                descriptor_mutator=lambda descriptors, heap, rows: (
                    descriptors.__setitem__(slice(0, 4), struct.pack(">I", len(heap) + 1))
                    or dict(length=len(heap) + 1, offset=0)
                )
            ),
            "a descriptor count extending past the heap is rejected",
        ),
        (
            "card_tform1",
            integer_name,
            dict(
                card_replacements={
                    "TFORM1": _card("%-8s= '%s'" % ("TFORM1", "1QB(1024)"))
                }
            ),
            "fitsverify rejects a damaged P/Q descriptor TFORM1 card",
        ),
    ]
    rows = []
    for name, fixture_name, options, expected in mutations:
        fixture = cases_by_name[fixture_name]
        result = results_by_name.get(fixture_name, {})
        original_path = result.get("original")
        candidate_path = os.path.join(root, fixture_name)
        tag = "mutation-" + name
        mutation_root = os.path.join(scratch, tag)
        os.makedirs(mutation_root, exist_ok=True)
        mutated_path = os.path.join(mutation_root, tag + ".fits")
        try:
            details = mutate_zimage_candidate(candidate_path, mutated_path, **options)
            candidate_bytes = _read_bytes(mutated_path)
            evidence.write_bytes(tag + ".candidate.fits", candidate_bytes)
            source_image = _first_source_image(original_path)
            inflated, local_refusal = safe_inflate_tiles(
                mutated_path, evidence, tag + "-local-inflate"
            )
            if name in ("nan_payload", "signed_zero"):
                local_detected = local_refusal is None and inflated != source_image.data
                local_record = dict(
                    local_inflation_failure=local_refusal,
                    tile_bytes_equal_source=(inflated == source_image.data)
                    if local_refusal is None
                    else None,
                )
            elif name == "card_tform1":
                local_detected = False
                local_record = dict(
                    local_inflation_failure=local_refusal,
                    local_tile_comparison="not a card-structure oracle",
                )
            else:
                local_detected = local_refusal is not None
                local_record = dict(local_inflation_failure=local_refusal)
            acceptance = check_candidate(
                original_path,
                mutated_path,
                fixture,
                mutation_root,
                evidence,
                tag,
            )
            array_mismatch = any(
                not check.get("matches") for check in acceptance.get("array_checks", [])
            )
            external_signals = dict(
                fitsverify_rejected=acceptance.get("fitsverify_exit") not in (None, 0),
                funpack_rejected=acceptance.get("funpack_exit") not in (None, 0),
                byte_array_mismatch=array_mismatch,
                controlled_failure=acceptance.get("controlled_failure") is not None,
            )
            gate_rejected = acceptance.get("accepted") is False
            if name == "card_tform1":
                detected = gate_rejected and external_signals["fitsverify_rejected"]
            else:
                detected = gate_rejected and local_detected and any(external_signals.values())
            row = dict(
                name=name,
                fixture=fixture_name,
                binary_sha256=binary_sha256,
                expected=expected,
                candidate_sha256=hashlib.sha256(candidate_bytes).hexdigest(),
                candidate_bytes=len(candidate_bytes),
                mutation=details,
                local_oracle=local_record,
                external_signals=external_signals,
                candidate_acceptance=acceptance.get("accepted"),
                acceptance_problems=acceptance.get("problems", []),
                fitsverify_exit=acceptance.get("fitsverify_exit"),
                funpack_exit=acceptance.get("funpack_exit"),
                detected=detected,
            )
        except Exception as error:
            row = dict(
                name=name,
                fixture=fixture_name,
                binary_sha256=binary_sha256,
                expected=expected,
                detected=False,
                exception="%s: %s" % (type(error).__name__, error),
                traceback=traceback.format_exc(),
            )
        rows.append(row)
        evidence.write_json(tag + ".json", row)
    control = dict(
        name="candidate_mutations",
        control_type="negative",
        expected=(
            "NaN payload, signed-zero, gzip CRC, zero/negative/out-of-range descriptor count/offset/length "
            "and TFORM1 "
            "mutations are rejected by the same fail-closed acceptance path as normal candidates"
        ),
        binary_sha256=binary_sha256,
        rows=rows,
        detected=bool(rows) and all(row.get("detected") for row in rows),
    )
    evidence.write_json("control-candidate-mutations.json", control)
    return control


def run_residual_controls(binary, cases, fixture_results, root, scratch, evidence, binary_sha256):
    """Residual-only actual-binary controls; no previously confirmed fixture is replayed."""
    controls = [
        run_idempotence_control(binary, cases, fixture_results, root, scratch, evidence, binary_sha256),
        run_failure_preservation_control(binary, scratch, evidence, binary_sha256),
        run_residual_mutation_controls(
            binary, cases, fixture_results, root, scratch, evidence, binary_sha256
        ),
    ]
    return controls


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="the tzim binary built with --features fits")
    parser.add_argument(
        "--external-double-ext",
        help="byte-pinned external Astropy double_ext.fits input for EXTNAMX coverage",
    )
    parser.add_argument("--work", default="fits-heasarc-work")
    parser.add_argument("--evidence", default="fits-heasarc-evidence")
    parser.add_argument(
        "--scope",
        choices=("full", "residual"),
        default=os.environ.get("FITS_HEASARC_SCOPE", "full"),
        help="run the full gate or only the additive #78 rows missing from the reviewed handoff",
    )
    parser.add_argument(
        "--self-check",
        action="store_true",
        help="exercise the parser, the diff and both negative controls without tools",
    )
    args = parser.parse_args()

    evidence = Evidence(args.evidence)
    root = os.path.join(args.work, "fixtures")
    all_cases = build_fixtures(root)
    cases = select_fixtures(all_cases, args.scope)
    scratch = os.path.join(args.work, "scratch")
    os.makedirs(scratch, exist_ok=True)

    summary = dict(
        run=dict(
            requested_sha=os.environ.get("REQUESTED_SHA", os.environ.get("GITHUB_SHA")),
            checked_out_sha=os.environ.get("CHECKED_OUT_SHA"),
            run_id=os.environ.get("GITHUB_RUN_ID"),
            run_attempt=os.environ.get("GITHUB_RUN_ATTEMPT"),
            workflow=os.environ.get("GITHUB_WORKFLOW"),
            scope=args.scope,
        ),
        environment=dict(
            python=sys.version.split()[0],
            platform=platform.platform(),
            machine=platform.machine(),
        ),
        fixtures=[],
        controls=[],
        tools={},
        ok=False,
    )

    if args.self_check:
        evidence.write_json("corpus-manifest.json", corpus_manifest(root, cases, args.scope))
        return self_check(root, all_cases, scratch, evidence, summary)

    if not args.binary:
        parser.error("--binary is required unless --self-check is given")
    if args.scope == "full":
        if not args.external_double_ext:
            parser.error("--external-double-ext is required for the full real-file EXTNAMX control")
        cases.extend(add_external_double_ext(root, args.external_double_ext))
    evidence.write_json("corpus-manifest.json", corpus_manifest(root, cases, args.scope))

    # Every candidate is produced by running the binary from the fixture's own
    # directory, so a relative path is resolved **once**, here: `cwd=` would
    # otherwise look for `target/release/tzim` inside the scratch directory, which
    # is how the first real run of this gate died with FileNotFoundError.
    args.binary = os.path.realpath(os.path.abspath(args.binary))
    if not os.access(args.binary, os.X_OK):
        print("::error::%s is not an executable file" % args.binary)
        return 2
    binary_bytes = _read_bytes(args.binary)
    binary_identity = dict(
        path=args.binary,
        bytes=len(binary_bytes),
        sha256=hashlib.sha256(binary_bytes).hexdigest(),
        scope=args.scope,
    )
    summary["binary"] = binary_identity
    summary["verification_scope"] = dict(
        accepted_subset=(
            "only whole-file FITS inputs whose complete candidate is verified and strictly smaller; "
            "only unscaled, non-GROUPS image HDUs with supported BITPIX, valid dimensions, "
            "identity PCOUNT/GCOUNT, no CHECKSUM/DATASUM, and GZIP_1/GZIP_2 output"
        ),
        runtime=(
            "repository V3 structure/header/table verification; candidate gzip inflation with CRC-32/ISIZE; "
            "byte-exact pixel-storage comparison; literal source-card preservation rules; exact non-target "
            "HDU preservation; strict whole-file size gate; atomic replacement only after success"
        ),
        ci_only=(
            "on the exact checked-out SHA, fitsverify and independent CFITSIO funpack -C read every candidate "
            "that tzim rewrites in the selected scope, including altered-candidate controls; compare "
            "pixel-storage bytes, literal headers, and byte-exact non-target HDUs. Explicit refusal rows "
            "are checked for their declared status and unchanged source bytes, not independently decompressed"
        ),
        limitation=(
            "the CI matrix is representative candidate evidence, not an independent runtime read-back on every "
            "accepted user file; this opt-in feature is not a full-format admission or a universal stability guarantee"
        ),
    )
    evidence.write_json("tzim-binary-identity.json", binary_identity)
    evidence.write_json("verification-scope.json", summary["verification_scope"])

    # The tools first: a missing tool must not look like a rejected candidate.
    for name in ("fitsverify", "funpack", "fpack"):
        if shutil.which(name) is None:
            print("::error::%s is not on PATH; the gate cannot run" % name)
            return 2
        identity = tool_identity(name)
        summary["tools"][name] = {k: identity[k] for k in ("argv", "exit", "recognised", "first_lines")}
        evidence.write_text("%s-identity.txt" % name, identity["output"])
        if not identity["recognised"]:
            print("::error::%s did not identify itself; its output is in the evidence" % name)
            return 2
    tzim_code, tzim_version = run([args.binary, "--version"])
    summary["tools"]["tzim"] = dict(
        argv=[args.binary, "--version"],
        exit=tzim_code,
        first_lines=tzim_version.strip(),
        binary_sha256=binary_identity["sha256"],
        binary_bytes=binary_identity["bytes"],
    )
    evidence.write_text("tzim-identity.txt", tzim_version)

    failures = 0
    for case in cases:
        fixture_path = os.path.join(root, case["name"])
        try:
            result = check_fixture(args.binary, case, root, scratch, evidence)
        except Exception as error:
            result = controlled_candidate_refusal(
                None,
                fixture_path,
                evidence,
                "fixture-" + case["name"],
                "fixture/candidate processing",
                error,
                traceback.format_exc(),
            )
            result.update(
                name=case["name"],
                why=case.get("why"),
                expect=case.get("expect"),
                input_case=case.get("input_case"),
            )
        result["binary_sha256"] = binary_identity["sha256"]
        summary["fixtures"].append(result)
        mark = "ok" if not result["problems"] else "FAIL"
        print("%-40s %-10s %s" % (case["name"], case["expect"], mark))
        for problem in result["problems"]:
            print("    %s" % problem)
        failures += bool(result["problems"])

    matrix = acceptance_matrix_summary(cases, summary["fixtures"], args.scope)
    summary["acceptance_matrix"] = matrix
    evidence.write_json("acceptance-matrix.json", matrix)
    if not matrix["complete"]:
        print("::error::the requested accepted-file matrix is incomplete: %s" % matrix["missing"])
        failures += 1

    residual_controls = run_residual_controls(
        args.binary,
        cases,
        summary["fixtures"],
        root,
        scratch,
        evidence,
        binary_identity["sha256"],
    )
    summary["controls"].extend(residual_controls)
    for control in residual_controls:
        if not control.get("detected"):
            print("::error::residual control failed: %s" % control["name"])
            failures += 1

    if args.scope == "residual":
        summary["ok"] = failures == 0
        evidence.write_json("summary.json", summary)
        diagnostics = render_diagnostics(summary, evidence)
        evidence.write_text("ci-diagnostics.md", diagnostics)
        evidence.write_text("summary.md", render_summary(summary) + "\n" + diagnostics)
        digest = evidence.seal()
        print("%d residual fixture(s), %d residual control(s), %d failure(s)"
              % (len(summary["fixtures"]), len(residual_controls), failures))
        print("exact binary SHA-256: %s" % binary_identity["sha256"])
        print("evidence digest: %s" % digest)
        print("\n=== BEGIN CI diagnostic report (same content as ci-diagnostics.md) ===")
        print(diagnostics.rstrip())
        print("=== END CI diagnostic report ===")
        return 0 if summary["ok"] else 1

    # --- negative controls use the same acceptance path as a normal fixture ---
    # The intact candidate must first pass the complete acceptance helper. Then
    # the bit-flipped candidate is sent through that same helper; a local tile
    # inequality alone is not the gate's acceptance criterion.
    good = next(
        (
            c
            for c in summary["fixtures"]
            if c.get("expect") in ("rewritten", "mixed")
            and (c.get("tzim") or {}).get("status") == "optimized"
            and c.get("candidate_acceptance") is True
            and not c.get("problems")
            and c.get("bytes", {}).get("after", 0) != c.get("bytes", {}).get("before", 0)
        ),
        None,
    )
    if good is None:
        summary["controls"].append(
            dict(name="corrupt_tile", control_type="negative", detected=None,
                 expected="a candidate accepted by the main path becomes rejected after one tile bit is flipped",
                 note="no intact candidate passed the full acceptance path")
        )
        print("!! no intact accepted candidate is available, so the corruption control did not run")
        failures += 1
    else:
        path = os.path.join(root, good["name"])
        corrupt = os.path.join(scratch, "corrupt-" + good["name"])
        detail = corrupt_tile(path, corrupt)
        source = parse(open(good["original"], "rb").read())
        image = next(h for h in source if h.kind() in ("PRIMARY", "IMAGE") and h.axes())
        intact_tiles, intact_failure = safe_inflate_tiles(
            path, evidence, "control-intact-" + good["name"]
        )
        intact = intact_tiles == image.data if intact_failure is None else False
        damaged_tiles, damaged_failure = safe_inflate_tiles(
            corrupt, evidence, "control-corrupt-" + good["name"]
        )
        if damaged_failure is not None:
            damaged = False
            note = "the corrupted tile was refused during controlled inflation: %s" % (
                damaged_failure["problems"][0]
            )
        else:
            damaged = damaged_tiles == image.data
            note = "the corrupted tile inflated and differed"

        case_by_name = {case["name"]: case for case in cases}
        corrupt_acceptance = check_candidate(
            good["original"], corrupt, case_by_name[good["name"]], scratch,
            evidence, "control-corrupt-tile",
        )
        acceptance_rejected = not corrupt_acceptance["accepted"]
        payload_rejected = (
            corrupt_acceptance.get("funpack_exit") not in (None, 0)
            or any(not check.get("matches") for check in corrupt_acceptance.get("array_checks", []))
            or corrupt_acceptance.get("controlled_failure") is not None
        )
        detected = intact and not damaged and acceptance_rejected and payload_rejected
        evidence.write_json(
            "control-corrupt-acceptance.json",
            corrupt_acceptance,
        )
        evidence.write_json(
            "control-corrupt.json",
            dict(
                fixture=good["name"],
                **detail,
                intact_candidate_accepted=good.get("candidate_acceptance"),
                tiles_equal_original=intact,
                intact_tile_inflation_failure=intact_failure,
                corrupted_tiles_equal_original=damaged,
                corrupted_tile_inflation_failure=damaged_failure,
                candidate_acceptance_rejected=acceptance_rejected,
                content_rejection_via_funpack_or_array_check=payload_rejected,
                candidate_fitsverify_exit=corrupt_acceptance.get("fitsverify_exit"),
                candidate_funpack_exit=corrupt_acceptance.get("funpack_exit"),
                acceptance_problems=corrupt_acceptance["problems"],
                detected=detected,
                note=note,
            ),
        )
        summary["controls"].append(
            dict(
                name="corrupt_tile", control_type="negative", fixture=good["name"],
                expected="the normal acceptance path accepts the intact candidate and rejects the tile-corrupted candidate on its funpack/byte comparison",
                intact_candidate_accepted=good.get("candidate_acceptance"),
                candidate_acceptance_rejected=acceptance_rejected,
                content_rejected=payload_rejected,
                intact_tile_inflation_failure=intact_failure,
                corrupted_tile_inflation_failure=damaged_failure,
                controlled_candidate_failure=corrupt_acceptance.get("controlled_failure"),
                detected=detected,
                fitsverify_exit=corrupt_acceptance.get("fitsverify_exit"),
                funpack_exit=corrupt_acceptance.get("funpack_exit"),
                note=note,
                **detail,
            )
        )
        print(
            "control: same acceptance path: intact accepted=%s; corrupted accepted=%s; "
            "content rejection=%s; fitsverify exit=%s; funpack exit=%s"
            % (good.get("candidate_acceptance"), not acceptance_rejected, payload_rejected,
               corrupt_acceptance.get("fitsverify_exit"), corrupt_acceptance.get("funpack_exit"))
        )
        if not detected:
            failures += 1

    # The structural control needs *a* compressed candidate. When the run
    # produced none, the control cannot be demonstrated and that is a failure of
    # the run, not a reason to skip the control silently.
    if good is None:
        summary["controls"].append(
            dict(name="broken_structure", control_type="negative", detected=None,
                 expected="fitsverify rejects NAXIS1 inconsistent with TFORM1",
                 note="no candidate to break")
        )
        print("!! the structural control did not run: there is no candidate")
        summary["ok"] = False
        evidence.write_json("summary.json", summary)
        diagnostics = render_diagnostics(summary, evidence)
        evidence.write_text("ci-diagnostics.md", diagnostics)
        evidence.write_text("summary.md", render_summary(summary) + "\n" + diagnostics)
        print("evidence digest: %s" % evidence.seal())
        print("\n=== BEGIN CI diagnostic report (same content as ci-diagnostics.md) ===")
        print(diagnostics.rstrip())
        print("=== END CI diagnostic report ===")
        return 1
    candidate_path = os.path.join(root, good["name"])
    broken_path = os.path.join(scratch, "broken-structure.fits")
    detail = broken_structure(candidate_path, broken_path)
    verify_code, verify_output = fitsverify(broken_path, scratch)
    evidence.write_text("control-structure-fitsverify.txt", verify_output)
    evidence.write_json(
        "control-structure.json", dict(**detail, fitsverify_exit=verify_code)
    )
    summary["controls"].append(
        dict(name="broken_structure", control_type="negative",
             expected="fitsverify exit must be nonzero for NAXIS1/TFORM1 contradiction",
             detected=(verify_code != 0), fitsverify_exit=verify_code, **detail)
    )
    print("control: NAXIS1 made inconsistent with TFORM1 -> fitsverify exit %d" % verify_code)
    if verify_code == 0:
        print("::error::fitsverify accepted a file whose NAXIS1 contradicts its column width;"
              " the gate is not verifying anything")
        failures += 1

    # --- positive toolchain control: fpack -> funpack -C ----------------------
    sane_src = os.path.join(scratch, "sanity")
    os.makedirs(sane_src, exist_ok=True)
    source_path = os.path.join(scratch, "original-int16_primary.fits")
    sanity_path = os.path.join(sane_src, "sanity.fits")
    shutil.copyfile(source_path, sanity_path)
    sanity_original = open(sanity_path, "rb").read()
    evidence.write_bytes("control-toolchain-input.fits", sanity_original)
    packed = os.path.join(sane_src, "sanity.fits.fz")
    if os.path.exists(packed):
        os.unlink(packed)
    pack_code, pack_output = run(["fpack", "sanity.fits"], cwd=sane_src)
    evidence.write_text("control-fpack.txt", pack_output)
    control = dict(
        name="toolchain",
        control_type="positive_sanity",
        expected="fpack -> funpack -C; fitsverify exit 0 and zero literal 80-column header diff",
        fpack_exit=pack_code,
        fitsverify_exit=None,
        funpack_exit=None,
        header_diff_count=None,
        array_equal=None,
        ok=False,
    )
    if pack_code != 0 or not os.path.exists(packed):
        tail = " / ".join(line.strip() for line in pack_output.splitlines()[-6:] if line.strip())
        control["note"] = "fpack failed: %s" % (tail or "no output")
        evidence.write_text("control-toolchain-fitsverify.txt", "not run: no fpack output\n")
        evidence.write_text("control-toolchain-funpack.txt", "not run: no fpack output\n")
        print("::error::fpack exited %d and produced no reference file: %s"
              % (pack_code, tail or "no output"))
        failures += 1
    else:
        verify_code, verify_output = fitsverify(packed, sane_src)
        control["fitsverify_exit"] = verify_code
        evidence.write_text("control-toolchain-fitsverify.txt", verify_output)
        unpack_code, unpack_output, unpacked = funpack(packed, os.path.join(scratch, "fpack-control"))
        control["funpack_exit"] = unpack_code
        evidence.write_text("control-toolchain-funpack.txt", unpack_output)
        if unpack_code == 0 and unpacked:
            source_hdus = parse(sanity_original)
            rebuilt_hdus = parse(open(unpacked, "rb").read())
            control_diffs = {}
            if len(source_hdus) != len(rebuilt_hdus):
                control_diffs["hdu_count"] = dict(
                    original=len(source_hdus), reconstructed=len(rebuilt_hdus)
                )
            for index, (a, b) in enumerate(zip(source_hdus, rebuilt_hdus)):
                cards = literal_card_diff(a, b)
                if cards:
                    control_diffs["%d-%s" % (index, a.kind())] = cards
            control["header_diff_count"] = sum(len(items) for items in control_diffs.values()
                                                if isinstance(items, list))
            control["header_diff"] = control_diffs
            control["array_equal"] = (
                len(source_hdus) == len(rebuilt_hdus)
                and all(not compare_arrays(a, b) for a, b in zip(source_hdus, rebuilt_hdus))
            )
            evidence.write_json("control-toolchain-header-diff.json", control_diffs)
            if control_diffs:
                evidence.write_text(
                    "control-toolchain-header-diff.txt",
                    json.dumps(control_diffs, indent=2, sort_keys=True) + "\n",
                )
            else:
                evidence.write_text(
                    "control-toolchain-header-diff.txt",
                    "No literal 80-column card differences, including END, comments, spacing, or order.\n",
                )
        else:
            control["note"] = "funpack -C produced no reconstructed FITS file"
        control["ok"] = (
            pack_code == 0
            and verify_code == 0
            and unpack_code == 0
            and bool(unpacked)
            and control["header_diff_count"] == 0
            and control["array_equal"] is True
        )
        if not control["ok"]:
            failures += 1
            print(
                "::error::fpack -> funpack -C positive control failed: fitsverify=%s, funpack=%s, "
                "literal card diff count=%s, integer array equal=%s"
                % (verify_code, unpack_code, control["header_diff_count"], control["array_equal"])
            )
        else:
            print("control: fpack -> funpack -C -> zero literal card diff; fitsverify exit 0")
    evidence.write_text(
        "control-toolchain.txt",
        "fpack exit: %s\nfitsverify exit: %s\nfunpack -C exit: %s\nliteral header diff cards: %s\ninteger array equal: %s\nresult: %s\n"
        % (control["fpack_exit"], control["fitsverify_exit"], control["funpack_exit"],
           control["header_diff_count"], control["array_equal"], control["ok"]),
    )
    summary["controls"].append(control)

    summary["ok"] = failures == 0
    evidence.write_json("summary.json", summary)
    diagnostics = render_diagnostics(summary, evidence)
    evidence.write_text("ci-diagnostics.md", diagnostics)
    evidence.write_text("summary.md", render_summary(summary) + "\n" + diagnostics)
    digest = evidence.seal()
    print()
    for case in summary["fixtures"]:
        for problem in case["problems"][:3]:
            print("::error::%s: %s" % (case["name"], problem))
    negative_count = sum(1 for c in summary["controls"] if c.get("control_type") == "negative")
    positive_count = sum(1 for c in summary["controls"] if c.get("control_type") == "positive_sanity")
    print("%d fixture(s), %d negative control(s), %d positive sanity control(s), %d failure(s)"
          % (len(summary["fixtures"]), negative_count, positive_count, failures))
    print("evidence digest: %s" % digest)
    print("\n=== BEGIN CI diagnostic report (same content as ci-diagnostics.md) ===")
    print(diagnostics.rstrip())
    print("=== END CI diagnostic report ===")
    return 0 if failures == 0 else 1


def self_check(root, cases, scratch, evidence, summary):
    """Everything in this file that can be exercised without a FITS tool.

    It is not a substitute for the gate: it proves the reader, the diff and both
    negative controls behave, so that a failure in CI is a fact about the
    candidate rather than about this script.
    """
    problems = []

    def check(condition, message):
        if not condition:
            problems.append(message)

    for case in cases:
        path = os.path.join(root, case["name"])
        raw = open(path, "rb").read()
        hdus = parse(raw)
        check(b"".join(h.raw for h in hdus) == raw, "%s does not round trip through the reader" % case["name"])
        check(len(raw) % RECORD == 0, "%s is not a whole number of records" % case["name"])
    print("reader: %d fixtures re-emit their own bytes" % len(cases))

    multi = parse(open(os.path.join(root, "multi_hdu.fits"), "rb").read())
    table = next((h for h in multi if h.kind() == "BINTABLE"), None)
    check(table is not None, "multi_hdu fixture has no binary table")
    if table:
        check(table.int("NAXIS1") == 8 and table.text("TFORM1") == "1PB(1024)",
              "multi_hdu's one 1PB column is 8 bytes, so NAXIS1 must be 8")
    print("fixture: multi_hdu BINTABLE NAXIS1 agrees with its one 1PB descriptor")
    extension = parse(open(os.path.join(root, "float64_extension.fits"), "rb").read())
    check(len(extension) == 2 and extension[1].kind() == "IMAGE",
          "a quoted XTENSION value with a comment must stop at its closing quote")
    print("reader: quoted string values end before comments")
    check(not any(h.kind() == "HDRLET" for h in multi),
          "the positive multi_hdu fixture must use only registered extension types")
    hdrlet_case = next((c for c in cases if c["name"] == "hdrlet_preservation.fits"), None)
    check(hdrlet_case is not None and hdrlet_case.get("skip_funpack") is True,
          "HDRLET must be a separate preservation-only case")
    hdrlet_hdus = parse(open(os.path.join(root, "hdrlet_preservation.fits"), "rb").read())
    hdrlet = next((h for h in hdrlet_hdus if h.kind() == "HDRLET"), None)
    check(hdrlet is not None and hdrlet.data == b"\x01\x02\x03\x04" * 200,
          "HDRLET must carry exactly 200 repetitions of 01 02 03 04")
    baseline = dict(
        markers=['*** Error: Unregistered XTENSION value "HDRLET  ".'],
        summary=["**** Verification found 0 warning(s) and 1 error(s). ****"],
    )
    cleared = dict(
        markers=[], summary=["**** Verification found 0 warning(s) and 0 error(s). ****"]
    )
    new_traversal_error = dict(
        markers=["*** Error: Bad HDU? (from CFITSIO error stack:)."],
        summary=["**** Verification found 0 warning(s) and 1 error(s). ****"],
    )
    check(external_diagnostic_comparison(1, baseline, 0, cleared)["no_new_diagnostics"],
          "removing the inherited HDRLET error must not be treated as a regression")
    check(not external_diagnostic_comparison(1, baseline, 1, new_traversal_error)["no_new_diagnostics"],
          "a new traversal error must not be hidden by the inherited HDRLET error")
    zero_case = next((c for c in cases if c["name"] == "zero_width_column.fits"), None)
    check(zero_case is not None and zero_case.get("expect") == "untouched"
          and zero_case.get("status") == "already_optimal",
          "zero-width column must expect unchanged + already_optimal")

    # The specials fixture really does carry four NaN payloads and both zeros.
    specials = parse(open(os.path.join(root, "float32_specials.fits"), "rb").read())[0]
    words = {struct.unpack(">I", specials.data[i:i + 4])[0] for i in range(0, len(specials.data), 4)}
    for payload in (0x00000000, 0x80000000, 0x7FC00001, 0x7FC00002, 0xFFC00001, 0xFFC00002):
        check(payload in words, "the float fixture lost word 0x%08X" % payload)
    print("fixtures: both zeros and four NaN payloads are present")

    # The card diff notices a changed value, and the classifier says which kind.
    base = parse(open(os.path.join(root, "int16_primary.fits"), "rb").read())[0]
    edited = Hdu(
        [c if c[:8].strip() != "BUNIT" else "%-8s= '%-8s'" % ("BUNIT", "JY") for c in base.cards],
        base.data,
        base.raw,
        0,
    )
    diffs = literal_card_diff(base, edited)
    check(len(diffs) == 1, "the diff found %d differences for one changed card" % len(diffs))
    if diffs:
        check("changed" in classify_diff(diffs[0]), "a changed card was not classified as changed")
        check(len(diffs[0].get("original", "")) == CARD,
              "the header diff did not retain the full 80-column original card")

    # A change in a trailing/comment column that the old parsed-value comparison
    # ignored must be visible; card order is also part of the literal sequence.
    object_index = next(i for i, card in enumerate(base.cards) if card[:8].strip() == "OBJECT")
    card = list(base.cards[object_index])
    card[55] = "X"
    changed_comment = list(base.cards)
    changed_comment[object_index] = "".join(card)
    comment_diff = literal_card_diff(base, Hdu(changed_comment, base.data, base.raw, 0))
    check(bool(comment_diff), "the literal diff ignored a change beyond column 30")
    reordered = list(base.cards)
    object_index = next(i for i, card in enumerate(reordered) if card[:8].strip() == "OBJECT")
    bunit_index = next(i for i, card in enumerate(reordered) if card[:8].strip() == "BUNIT")
    reordered[object_index], reordered[bunit_index] = reordered[bunit_index], reordered[object_index]
    order_diff = literal_card_diff(base, Hdu(reordered, base.data, base.raw, 0))
    check(bool(order_diff), "the literal diff ignored a card reordering")
    print("header diff: complete 80-column content, comments and card order are compared")

    # The corruption control flips exactly one bit inside the heap of a table we
    # build ourselves (no tzim is available here), and the byte comparison sees it.
    table = empty_primary() + compressed_table_fixture()
    fake = os.path.join(scratch, "selfcheck-compressed.fits")
    open(fake, "wb").write(table)
    corrupt = os.path.join(scratch, "selfcheck-corrupt.fits")
    detail = corrupt_tile(fake, corrupt)
    a = parse(open(fake, "rb").read())[1]
    b = parse(open(corrupt, "rb").read())[1]
    check(a.raw != b.raw, "the corruption control changed nothing")
    check(detail["tile_length"] > 0, "the corruption control found no tile")
    print("control: corrupting a tile changes the file (offset %d)" % detail["corrupted_at"])

    # The tile reader: a table written here, with two gzip tiles, has to give
    # back the array its tiles stand for — and for GZIP_2 the shuffled form has
    # to come back too, because that is the permutation the fixtures exercise.
    for codec, samples in (("GZIP_1", struct.pack(">8h", *range(8))), ("GZIP_2", struct.pack(">2f", 1.5, -2.25) * 2)):
        per = 2 if codec == "GZIP_1" else 4
        half = len(samples) // 2
        rows = [samples[:half], samples[half:]]
        path = os.path.join(scratch, "selfcheck-tiles-%s.fits" % codec)
        with open(path, "wb") as handle:
            handle.write(tiles_fixture(codec, rows, per))
        got = inflate_tiles(path)
        check(got == samples, "%s tiles did not come back as the array they hold" % codec)
    print("tile reader: GZIP_1 and GZIP_2 rows come back unchanged")

    # The structural control produces a card that contradicts TFORM1, which is a
    # documented `fitsverify` error condition.
    broken = os.path.join(scratch, "selfcheck-broken.fits")
    broken_structure(fake, broken)
    fixed = parse(open(broken, "rb").read())[1]
    check(fixed.int("NAXIS1") == 16, "the structural control did not rewrite NAXIS1")
    check(fixed.text("TFORM1") == "1PB(1024)", "the structural control disturbed TFORM1")
    print("control: NAXIS1 rewritten to 16 while TFORM1 still says 8")

    summary["fixtures"] = [dict(name=c["name"], self_check="parsed") for c in cases]
    summary["ok"] = not problems
    evidence.write_json("self-check.json", dict(problems=problems, ok=not problems))
    for problem in problems:
        print("!! %s" % problem)
    print()
    print("%d self-check problem(s)" % len(problems))
    return 0 if not problems else 1


def compressed_table_fixture():
    """A one-row ZIMAGE table with a real gzip tile, built here.

    Only `--self-check` uses it: it is enough to prove that the corruption and
    structural controls find a tile and a `NAXIS1` card, and it makes no claim
    about our writer.
    """
    tile = gzip.compress(struct.pack(">8h", *range(8)), mtime=0)
    rows = 1
    cards = [
        string_card("XTENSION", "BINTABLE"),
        int_card("BITPIX", 8),
        int_card("NAXIS", 2),
        int_card("NAXIS1", 8),
        int_card("NAXIS2", rows),
        int_card("PCOUNT", len(tile)),
        int_card("GCOUNT", 1),
        int_card("TFIELDS", 1),
        string_card("TTYPE1", "COMPRESSED_DATA"),
        _card("%-8s= '%s'" % ("TFORM1", "1PB(1024)")),
        bool_card("ZIMAGE", True),
        string_card("ZCMPTYPE", "GZIP_1"),
        int_card("ZBITPIX", 16),
        int_card("ZNAXIS", 2),
        int_card("ZNAXIS1", 8),
        int_card("ZNAXIS2", 1),
        int_card("ZTILE1", 8),
        int_card("ZTILE2", 1),
    ]
    return header_bytes(cards) + pad_data(struct.pack(">II", len(tile), 0) + tile)


def tiles_fixture(codec, rows, per_sample):
    """A ZIMAGE table whose tiles are `rows`, gzip members built here.

    Only `--self-check` uses it, to prove the tile reader in this file agrees
    with the convention: `GZIP_2` rows are shuffled before deflating, exactly as
    the writer does, and the reader has to undo it.
    """
    floating = codec == "GZIP_2"
    tiles = [gzip.compress(shuffle(row, per_sample) if floating else row, mtime=0) for row in rows]
    heap = b"".join(tiles)
    descriptors = b"".join(struct.pack(">II", len(t), off) for t, off in
                           zip(tiles, [0] + [len(t) for t in tiles[:-1]]))
    cards = [
        string_card("XTENSION", "BINTABLE"),
        int_card("BITPIX", 8),
        int_card("NAXIS", 2),
        int_card("NAXIS1", 8),
        int_card("NAXIS2", len(rows)),
        int_card("PCOUNT", len(heap)),
        int_card("GCOUNT", 1),
        int_card("TFIELDS", 1),
        string_card("TTYPE1", "COMPRESSED_DATA"),
        _card("%-8s= '%s'" % ("TFORM1", "1PB(%d)" % max(len(t) for t in tiles))),
        bool_card("ZIMAGE", True),
        string_card("ZCMPTYPE", codec),
        int_card("ZBITPIX", -32 if floating else 16),
        int_card("ZNAXIS", 2),
        int_card("ZNAXIS1", len(rows[0]) // per_sample),
        int_card("ZNAXIS2", len(rows)),
        int_card("ZTILE1", len(rows[0]) // per_sample),
        int_card("ZTILE2", 1),
    ]
    return header_bytes(cards) + pad_data(descriptors + heap)


def render_summary(summary):
    run = summary.get("run") or {}
    scope = run.get("scope", "full")
    lines = ["# fitsverify / funpack gate (%s scope)" % scope, ""]
    lines.append(
        "Requested SHA `%s`; checked-out SHA `%s`; run `%s`, attempt `%s`."
        % (run.get("requested_sha", "not recorded"),
           run.get("checked_out_sha", "not recorded"),
           run.get("run_id", "not recorded"), run.get("run_attempt", "not recorded"))
    )
    binary = summary.get("binary") or {}
    if binary:
        lines.append(
            "Executed binary: `%s`, %s bytes, SHA-256 `%s`."
            % (binary.get("path", "not recorded"), binary.get("bytes", "not recorded"),
               binary.get("sha256", "not recorded"))
        )
    env = summary["environment"]
    lines.append("- Python %s on %s (%s)" % (env["python"], env["platform"], env["machine"]))
    for name, tool in summary["tools"].items():
        lines.append("- %s: exit %s, %s" % (name, tool.get("exit"), tool.get("first_lines", "").splitlines()[:1]))
    lines += ["", "| fixture | expectation | tzim | fitsverify | funpack -C | problems |", "| --- | --- | --- | --- | --- | --- |"]
    for fixture in summary["fixtures"]:
        problems = fixture.get("problems") or []
        lines.append(
            "| %s | %s | %s | %s | %s | %s |"
            % (
                fixture.get("name"),
                fixture.get("expect"),
                fixture.get("tzim", {}).get("status") if isinstance(fixture.get("tzim"), dict) else "-",
                fixture.get("fitsverify_exit", "-"),
                fixture.get("funpack_exit", "-"),
                "; ".join(problems) if problems else "none",
            )
        )
    matrix = summary.get("acceptance_matrix") or {}
    if matrix:
        lines += ["", "## Accepted-file matrix", ""]
        lines.append(
            "Complete: **%s**; expected `%s`; observed `%s`; missing `%s`."
            % (matrix.get("complete", False),
               json.dumps(matrix.get("expected", {}), sort_keys=True),
               json.dumps(matrix.get("observed", {}), sort_keys=True),
               json.dumps(matrix.get("missing", []), sort_keys=True))
        )
        lines.append("")
        lines.append("| fixture | input SHA-256 | candidate SHA-256 | binary SHA-256 | strict-size | independent acceptance | identity bound | result |")
        lines.append("| --- | --- | --- | --- | --- | --- | --- | --- |")
        for row in matrix.get("rows", []):
            lines.append(
                "| %(fixture)s | `%(input)s` | `%(candidate)s` | `%(binary)s` | %(size)s | %(independent)s | %(identity)s | %(accepted)s |" % {
                    "fixture": row.get("fixture", "?"),
                    "input": row.get("input_sha256") or "not recorded",
                    "candidate": row.get("candidate_sha256") or "not recorded",
                    "binary": row.get("binary_sha256") or "not recorded",
                    "size": row.get("strict_size"),
                    "independent": row.get("independent_acceptance"),
                    "identity": row.get("identity_bound"),
                    "accepted": row.get("accepted"),
                }
            )
    zero = next((f for f in summary["fixtures"] if f["name"] == "zero_width_column.fits"), None)
    if zero:
        lines += ["", "## zero-width column: unchanged by the strict whole-file gate", ""]
        lines.append("**Input:** " + (zero.get("input_case") or "description not recorded"))
        lines.append("")
        lines.append("Input SHA-256: `%s`; output SHA-256: `%s`." % (
            zero.get("input_sha256", "not recorded"), zero.get("candidate_sha256", "not recorded")))
        lines.append("")
        lines.append("**Contract:** " + (zero.get("contract_expectation") or zero.get("why", "not recorded")))
        lines.append("")
        record = zero.get("tzim") or {}
        lines.append("**Observed:** status `%s`, note `%s`; bytes %s → %s."
                     % (record.get("status", "?"), record.get("note"),
                        zero.get("bytes", {}).get("before", "?"),
                        zero.get("bytes", {}).get("after", "?")))
    controls = summary.get("controls", [])
    control_counts = Counter(control.get("control_type", "unclassified") for control in controls)
    lines += ["", "## Controls", ""]
    lines.append("Counts: `%s`." % json.dumps(control_counts, sort_keys=True))
    for control in controls:
        lines.append("- `%s` (%s; expected: %s): detected **%s**; record `%s`." % (
            control.get("name"), control.get("control_type", "unclassified"),
            control.get("expected", "not recorded"), control.get("detected", False),
            json.dumps(control, sort_keys=True)))
    verification = summary.get("verification_scope") or {}
    if verification:
        lines += ["", "## Verification boundary", ""]
        lines.append("Runtime: " + verification.get("runtime", "not recorded") + ".")
        lines.append("")
        lines.append("CI only: " + verification.get("ci_only", "not recorded") + ".")
        lines.append("")
        lines.append(verification.get("limitation", ""))
    lines += ["", "Overall: **%s**" % ("PASS" if summary["ok"] else "FAIL")]
    return "\n".join(lines) + "\n"

if __name__ == "__main__":
    sys.exit(main())
