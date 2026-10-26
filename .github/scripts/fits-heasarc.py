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
import os
import platform
import shutil
import struct
import subprocess
import zlib
import sys

RECORD = 2880
CARD = 80

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


def string_card(keyword: str, value: str) -> bytes:
    """A character-string card: quotes immediately around the value."""
    return _card("%-8s= '%-8s'" % (keyword, value)) if len(value) <= 8 else _card(
        "%-8s= '%-20s'" % (keyword, value)
    )


def int_card(keyword: str, value: int) -> bytes:
    return _card("%-8s= %20d" % (keyword, value))


def float_card(keyword: str, value: float) -> bytes:
    return _card("%-8s= %20.12G" % (keyword, value))


def bool_card(keyword: str, value: bool) -> bytes:
    return _card("%-8s= %20s" % (keyword, "T" if value else "F"))


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


def image_hdu(bitpix, axes, data, extra=(), primary=False, xtension="IMAGE") -> bytes:
    """A primary array or an image extension, uncompressed."""
    cards = []
    if primary:
        cards += [bool_card("SIMPLE", True), int_card("BITPIX", bitpix), int_card("NAXIS", len(axes))]
    else:
        cards += [string_card("XTENSION", xtension), int_card("BITPIX", bitpix), int_card("NAXIS", len(axes))]
    for index, axis in enumerate(axes, start=1):
        cards.append(int_card("NAXIS%d" % index, axis))
    if primary:
        cards.append(bool_card("EXTEND", True))
    else:
        cards.append(int_card("PCOUNT", 0))
        cards.append(int_card("GCOUNT", 1))
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
        return value.strip().strip("'").strip()

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
        count = 1
        for axis in probe.axes():
            count *= axis
        pcount = probe.int("PCOUNT") or 0
        gcount = probe.int("GCOUNT") or 1
        length = 0 if naxis == 0 else SAMPLE_BYTES.get(bitpix, 0) * (count + pcount) * gcount
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


def build_fixtures(root):
    """Every fixture, with the contract's expectation for it.

    `expect` is read from `docs/guarantees.md` §4.12 rather than from what the
    handler happens to do; where this script could not decide what the contract
    says, the field says `undecided` and the gate reports the observed behaviour
    instead of pretending to assert one.
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
        empty_primary() + image_hdu(-64, [48, 96], float64_pattern(48, 96)),
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

    # 4. A multi-HDU file: an unknown extension between two images, and a table.
    write(
        "multi_hdu.fits",
        image_hdu(16, [64, 128], int16_pattern(64, 128), primary=True)
        + nonstandard_hdu("HDRLET", b"\x01\x02\x03\x04" * 200, extra=[string_card("EXTNAME", "STWCS")])
        + image_hdu(-32, [32, 64], float32_with_specials(32, 64), extra=[string_card("EXTNAME", "SCI")])
        + bin_table_hdu(4, 8),
    )
    cases.append(
        dict(
            name="multi_hdu.fits",
            expect="mixed",
            images=2,
            hdu_delta=1,
            verbatim=["HDRLET", "BINTABLE"],
            why="§4.12: every other HDU is copied verbatim, including an unknown extension",
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
            expect="undecided",
            images=1,
            hdu_delta=0,
            verbatim=["BINTABLE"],
            input_case=(
                "HDU 0: empty primary (BITPIX=8, NAXIS=0); HDU 1: IMAGE extension "
                "(BITPIX=16, NAXIS=2, NAXIS1=32, NAXIS2=64, EXTNAME='SCI'); HDU 2: "
                "BINTABLE (BITPIX=8, NAXIS=2, NAXIS1=8, NAXIS2=2, TFIELDS=2, "
                "TFORM1='0A' [zero-width], TFORM2='8A', PCOUNT=0, GCOUNT=1; 16 data bytes)."
            ),
            contract_expectation=(
                "§4.12 has two interacting clauses: an eligible IMAGE HDU may be tiled "
                "and every other HDU is copied verbatim; but the candidate is written "
                "only if the whole file is strictly smaller. The fixture's no-change "
                "result is consistent with no whole-file gain, but the harness has no "
                "candidate-size measurement for the discarded candidate. It therefore "
                "does not assert either a partial rewrite or a zero-width whole-file refusal."
            ),
            why="§4.12 interaction: per-HDU transformation versus strict whole-file size gain",
        )
    )

    return cases


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
    # `-O` is **not** "overwrite" in these tools: it names the output file, so
    # `funpack -O candidate.fits.fz` means "write to candidate.fits.fz and read
    # nothing", which the tool correctly rejects with "no FITS files to
    # uncompress". The output is removed first instead, and the input named alone.
    unpacked_name = os.path.join(work, "candidate.fits")
    if os.path.exists(unpacked_name):
        os.unlink(unpacked_name)
    code, output = run(["funpack", os.path.basename(packed)], cwd=work)
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
        return "structural (allowed to change on a rewritten HDU)"
    if keywords & structural:
        return "structural (allowed to change on a rewritten HDU)"
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
    for hdu in parse(open(path, "rb").read()):
        if not hdu.value("ZIMAGE"):
            continue
        rows = hdu.int("NAXIS2") or 0
        heap_len = hdu.int("PCOUNT") or 0
        heap = hdu.data[8 * rows:8 * rows + heap_len]
        floating = (hdu.int("ZBITPIX") or 0) < 0
        per_sample = abs(hdu.int("ZBITPIX") or 8) // 8
        for row in range(rows):
            length, offset = struct.unpack(">II", hdu.data[row * 8:row * 8 + 8])
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


def check_fixture(binary, case, root, scratch, evidence):
    """Everything the gate asserts about one fixture."""
    path = os.path.join(root, case["name"])
    original = open(path, "rb").read()
    before = parse(original)
    # `tzim` rewrites the file in place, so the fixture as it was has to survive
    # somewhere: the controls compare the candidate's tiles against *this*.
    kept = os.path.join(scratch, "original-" + case["name"])
    with open(kept, "wb") as handle:
        handle.write(original)
    evidence.write_bytes("input-" + case["name"], original)
    result = dict(
        name=case["name"],
        why=case["why"],
        expect=case["expect"],
        input_case=case.get("input_case"),
        contract_expectation=case.get("contract_expectation"),
        input_sha256=hashlib.sha256(original).hexdigest(),
        original=kept,
        problems=[],
    )
    if case["name"] == "multi_hdu.fits":
        result["original_hdus"] = hdu_layout(before)
        original_verify_code, original_verify_output = fitsverify(kept, scratch)
        result["original_fitsverify_exit"] = original_verify_code
        evidence.write_text("fitsverify-original-multi_hdu.fits.txt", original_verify_output)

    code, record, output = tzim_run(binary, path)
    result["tzim_exit"] = code
    result["tzim"] = record
    evidence.write_json("tzim-%s.json" % case["name"], record)
    after = open(path, "rb").read()
    evidence.write_bytes("candidate-" + case["name"], after)
    result["candidate_sha256"] = hashlib.sha256(after).hexdigest()
    result["bytes"] = dict(before=len(original), after=len(after))
    if code != 0:
        result["problems"].append("tzim exited %d: %s" % (code, output.strip()[:400]))
        return result

    status = (record or {}).get("status")
    if case["expect"] == "untouched":
        expected_status = case.get("status")
        if expected_status and status != expected_status:
            result["problems"].append("status is %r, expected %r" % (status, expected_status))
        if after != original:
            result["problems"].append("a file that must not be rewritten was rewritten")
        return result

    verify_code, verify_output = fitsverify(path, scratch)
    result["fitsverify_exit"] = verify_code
    evidence.write_text("fitsverify-%s.txt" % case["name"], verify_output)
    if verify_code != 0:
        result["problems"].append(
            "fitsverify exited %d (errors + warnings); its output is in the evidence" % verify_code
        )

    if after == original:
        if case["expect"] == "undecided":
            result["observed_behavior"] = "unchanged; the discarded candidate size is not exposed"
            result["candidate_hdus"] = hdu_layout(before)
            return result
        # `tzim` reports the decision it made; "nothing was written" without that
        # reason is a riddle for whoever reads the log next.
        said = ""
        if record:
            said = " (tzim said: %s%s)" % (
                record.get("status", "?"),
                (": " + record["note"]) if record.get("note") else "",
            )
        result["problems"].append("the file did not change, so nothing was written%s" % said)
        return result
    if status != "optimized":
        result["problems"].append("status is %r, expected 'optimized'" % status)

    candidate = parse(after)
    if case["name"] == "multi_hdu.fits":
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
    if len(candidate_images) != 0:
        result["problems"].append(
            "%d image HDUs are still uncompressed after the rewrite" % len(candidate_images)
        )
    mapping, matched = map_candidate(before, candidate)
    if matched != len(candidate):
        result["problems"].append(
            "the candidate has %d HDUs the original does not account for" % (len(candidate) - matched)
        )
    for index, original in enumerate(before):
        if original.kind() in ("PRIMARY", "IMAGE"):
            continue
        other = candidate[mapping.get(index, -1)] if index in mapping else None
        if other is None:
            result["problems"].append("HDU %d (%s) is missing" % (index, original.kind()))
        elif other.raw != original.raw:
            result["problems"].append(
                "HDU %d (%s) was not copied verbatim: %d -> %d bytes"
                % (index, original.kind(), len(original.raw), len(other.raw))
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

    # (2) funpack rebuilds the file, and the arrays are compared as bytes.
    unpack_code, unpack_output, unpacked = funpack(path, scratch)
    evidence.write_text("funpack-%s.txt" % case["name"], unpack_output)
    result["funpack_exit"] = unpack_code
    result["funpack_output"] = unpack_output
    if unpack_code != 0 or not unpacked:
        # The exit code alone is not evidence. `funpack` exits 255 for its own
        # errors and what it printed is the only thing that says which.
        tail = " / ".join(line.strip() for line in unpack_output.splitlines()[-3:] if line.strip())
        result["problems"].append(
            "funpack exited %d and produced no file%s" % (unpack_code, (": " + tail) if tail else "")
        )
        return result
    rebuilt = parse(open(unpacked, "rb").read())
    if rebuilt and rebuilt[0].kind() == "PRIMARY" and not rebuilt[0].axes() and before[0].axes():
        rebuilt = rebuilt[1:]          # the empty primary funpack leaves behind
    if len(rebuilt) != len(before):
        result["problems"].append(
            "funpack produced %d HDUs for an original of %d" % (len(rebuilt), len(before))
        )

    diffs = {}
    for index, (a, b) in enumerate(zip(before, rebuilt)):
        label = "%d-%s" % (index, a.kind())
        if a.kind() in ("PRIMARY", "IMAGE"):
            problems = compare_arrays(a, b)
            result["problems"].extend("%s: %s" % (label, p) for p in problems)
        elif cards_of(a) != cards_of(b) or a.data != b.data:
            result["problems"].append(
                "%s: funpack did not reconstruct the copied HDU (its cards are in the diff)" % label
            )
        cards = literal_card_diff(a, b)
        if cards:
            diffs[label] = [
                dict(diff, classification=classify_diff(diff)) for diff in cards
            ]
    if diffs:
        evidence.write_json("header-diff-%s.json" % case["name"], diffs)
        evidence.write_text(
            "header-diff-%s.txt" % case["name"],
            "\n".join(
                "%s: %s card; original index %s, reconstructed index %s; %s\n"
                "  original: %r\n  reconstructed: %r\n  classification: %s"
                % (label, d["kind"], d["original_index"], d["reconstructed_index"],
                   d["keyword"], d["original"], d["reconstructed"], d["classification"])
                for label, items in diffs.items()
                for d in items
            ) + "\n",
        )
        result["header_diff"] = diffs
        result["problems"].append(
            "the reconstructed header is not identical (see header-diff-%s.txt)" % case["name"]
        )
    evidence.write_json("tzim-%s.json" % case["name"], record)
    return result


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
    diffs = [f for f in summary["fixtures"] if f.get("header_diff")]
    lines.append("## Literal header diffs")
    lines.append("")
    lines.append(
        "These compare the original HDU with `funpack`'s reconstructed HDU, as complete 80-column "
        "cards (including comments and spacing); `repr` retains trailing blanks. They prove a literal "
        "round-trip difference, but do not by themselves attribute it to `tzim` or `funpack`. The raw "
        "tzim candidate, JSON diff, and text diff are sealed in the artifact."
    )
    lines.append("")
    if not diffs:
        lines.append("No header differences were found.")
    for fixture in diffs:
        lines.append("### " + fixture["name"])
        lines.append("")
        lines.extend(evidence.read_text("header-diff-%s.txt" % fixture["name"]).rstrip().splitlines())
        lines.append("")

    multi = next((f for f in summary["fixtures"] if f["name"] == "multi_hdu.fits"), None)
    lines.append("## multi_hdu.fits tool diagnosis")
    lines.append("")
    if multi:
        lines.append("Original input fitsverify exit %s; tzim candidate fitsverify exit %s; funpack exit %s." % (
            multi.get("original_fitsverify_exit", "not run"),
            multi.get("fitsverify_exit", "not run"),
            multi.get("funpack_exit", "not run"),
        ))
        for label in ("original", "candidate"):
            layout = multi.get(label + "_hdus") or []
            lines.append("")
            lines.append("%s HDU layout (byte offsets):" % label.capitalize())
            for row in layout:
                lines.append(
                    "- HDU %(index)s %(kind)s: start=%(start)s, header=%(header_bytes)s, "
                    "data=%(data_bytes)s, padded-data=%(padded_data_bytes)s, next=%(next_start)s" % row
                )
        lines.append("")
        lines.append(
            "The reported `^A^B^C^D` bytes are the exact repeating payload written in the `HDRLET` "
            "fixture. The baseline fitsverify output below distinguishes an input/tool limitation from "
            "a problem introduced by the candidate; neither is guessed from the exit code."
        )
        for name, caption in (
            ("fitsverify-original-multi_hdu.fits.txt", "fitsverify -l on original input"),
            ("fitsverify-multi_hdu.fits.txt", "fitsverify -l on tzim candidate"),
            ("funpack-multi_hdu.fits.txt", "funpack on tzim candidate"),
        ):
            lines += ["", "### " + caption, "", "````text"]
            try:
                lines.extend(evidence.read_text(name).rstrip().splitlines())
            except FileNotFoundError:
                lines.append("(no output file was produced)")
            lines += ["````", ""]
    else:
        lines.append("The fixture did not run.")

    zero = next((f for f in summary["fixtures"] if f["name"] == "zero_width_column.fits"), None)
    lines.append("## zero_width_column.fits — contract question, no writer change")
    lines.append("")
    if zero:
        lines.append("**Input:** " + (zero.get("input_case") or "description not recorded"))
        lines.append("")
        lines.append("Input SHA-256: `%s`; candidate SHA-256: `%s`." % (
            zero.get("input_sha256", "not recorded"), zero.get("candidate_sha256", "not recorded")))
        lines.append("")
        lines.append("**§4.12 reading encoded in the gate:** " +
                     (zero.get("contract_expectation") or zero.get("why", "not recorded")))
        lines.append("")
        record = zero.get("tzim") or {}
        lines.append("**Observed:** status `%s`, note `%s`; bytes %s → %s; `fitsverify` exit %s."
                     % (record.get("status", "?"), record.get("note"),
                        zero.get("bytes", {}).get("before", "?"),
                        zero.get("bytes", {}).get("after", "?"),
                        zero.get("fitsverify_exit", "not run")))
        lines.append("")
        lines.append(
            "The precise open point is how the eligible-IMAGE clause interacts with the strict "
            "whole-file-smaller clause when the candidate is discarded: current `already_optimal` "
            "does not expose the discarded candidate size. The prior harness expected `mixed` "
            "unconditionally; that overstates the contract. No writer behavior is changed, no "
            "whole-file refusal is inferred from the zero-width column, and the gate records this "
            "case as undecided pending owner agreement."
        )
    else:
        lines.append("The fixture did not run.")
    lines += ["", "## Controls: two negative controls plus one positive toolchain sanity check", ""]
    for c in summary.get("controls", []):
        role = c.get("control_type", "unclassified")
        lines.append("- `%s` (%s): expected %s; record `%s`." % (
            c.get("name"), role, c.get("expected", "not recorded"),
            json.dumps(c, sort_keys=True)))
    lines.append("")
    lines.append(
        "The corrupted-tile control is an integrity-comparison control, not a test that "
        "fitsverify must reject valid FITS structure with changed image content. Its own "
        "fitsverify exit is reported separately. The structural control must make "
        "fitsverify nonzero; the fpack control is positive and must remain exit 0."
    )
    lines.append("")
    lines.append("Full control-tool output is retained in the checksummed evidence files "
                 "`control-corrupt-fitsverify.txt`, `control-structure-fitsverify.txt`, and "
                 "`control-toolchain.txt`.")
    return "\n".join(lines).rstrip() + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="the tzim binary built with --features fits")
    parser.add_argument("--work", default="fits-heasarc-work")
    parser.add_argument("--evidence", default="fits-heasarc-evidence")
    parser.add_argument(
        "--self-check",
        action="store_true",
        help="exercise the parser, the diff and both negative controls without tools",
    )
    args = parser.parse_args()

    evidence = Evidence(args.evidence)
    root = os.path.join(args.work, "fixtures")
    cases = build_fixtures(root)
    scratch = os.path.join(args.work, "scratch")
    os.makedirs(scratch, exist_ok=True)

    summary = dict(
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
        return self_check(root, cases, scratch, evidence, summary)

    if not args.binary:
        parser.error("--binary is required unless --self-check is given")

    # Every candidate is produced by running the binary from the fixture's own
    # directory, so a relative path is resolved **once**, here: `cwd=` would
    # otherwise look for `target/release/tzim` inside the scratch directory, which
    # is how the first real run of this gate died with FileNotFoundError.
    args.binary = os.path.abspath(args.binary)
    if not os.access(args.binary, os.X_OK):
        print("::error::%s is not an executable file" % args.binary)
        return 2

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
        argv=[args.binary, "--version"], exit=tzim_code, first_lines=tzim_version.strip()
    )
    evidence.write_text("tzim-identity.txt", tzim_version)

    failures = 0
    for case in cases:
        result = check_fixture(args.binary, case, root, scratch, evidence)
        summary["fixtures"].append(result)
        mark = "ok" if not result["problems"] else "FAIL"
        print("%-24s %-10s %s" % (case["name"], case["expect"], mark))
        for problem in result["problems"]:
            print("    %s" % problem)
        failures += bool(result["problems"])

    # --- negative controls: each one has to be *detected* ---------------------
    # A candidate to corrupt: any fixture that was actually rewritten. It does
    # not have to be a flawless one — the control asks whether corruption is
    # *detected*, and choosing only from fixtures without other problems would
    # quietly turn the control off whenever the run had something else to say.
    good = next(
        (
            c
            for c in summary["fixtures"]
            if c.get("expect") in ("rewritten", "mixed")
            and (c.get("tzim") or {}).get("status") == "optimized"
            and c.get("bytes", {}).get("after", 0) != c.get("bytes", {}).get("before", 0)
        ),
        None,
    )
    if good is None:
        summary["controls"].append(
            dict(name="corrupt_tile", control_type="negative", detected=None,
                 expected="the integrity comparison detects the changed tile",
                 note="no accepted candidate to corrupt")
        )
        print("!! no accepted candidate is available, so the corruption control did not run")
        failures += 1
    else:
        path = os.path.join(root, good["name"])
        corrupt = os.path.join(scratch, "corrupt-" + good["name"])
        detail = corrupt_tile(path, corrupt)
        # The control is about the *tiles*: the candidate's own heap, inflated,
        # has to equal the original array before the flip and differ after it.
        source = parse(open(good["original"], "rb").read())
        image = next(h for h in source if h.kind() in ("PRIMARY", "IMAGE") and h.axes())
        # The intact candidate must inflate: if it does not, the run has a real
        # problem and the control must not paper over it with an exception.
        intact = inflate_tiles(path) == image.data
        try:
            damaged = inflate_tiles(corrupt) == image.data
            note = "the corrupted tile inflated and differed"
        except (OSError, EOFError, zlib.error) as error:
            # A flipped bit inside the deflate stream is often past repair: the
            # gzip member's own CRC-32 or its stream makes it fail outright.
            # That is detection too, and the failure is recorded.
            damaged = False
            note = "the corrupted tile no longer inflates: %s" % error
        detected = intact and not damaged
        verify_code, verify_output = fitsverify(corrupt, scratch)
        evidence.write_text("control-corrupt-fitsverify.txt", verify_output)
        evidence.write_json(
            "control-corrupt.json",
            dict(
                fixture=good["name"],
                **detail,
                fitsverify_exit=verify_code,
                note=note,
                tiles_equal_original=intact,
                corrupted_tiles_equal_original=damaged,
                our_detection=detected,
            ),
        )
        summary["controls"].append(
            dict(name="corrupt_tile", control_type="negative", fixture=good["name"],
                 expected="intact tile bytes match source; flipped tile does not",
                 detected=detected, fitsverify_exit=verify_code, note=note, **detail)
        )
        print("control: one bit flipped in a tile -> tiles still match the original: %s;"
              " after the flip: %s; fitsverify exit %d"
              % (intact, damaged, verify_code))
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

    # --- toolchain sanity: a file *fpack* wrote must pass ---------------------
    sane_src = os.path.join(scratch, "sanity")
    os.makedirs(sane_src, exist_ok=True)
    shutil.copyfile(os.path.join(root, "int16_primary.fits"), os.path.join(sane_src, "sanity.fits"))
    packed = os.path.join(sane_src, "sanity.fits.fz")
    if os.path.exists(packed):
        os.unlink(packed)
    # Same `-O` trap as `funpack` above: it names the output, it does not mean
    # "replace". `fpack sanity.fits` writes `sanity.fits.fz` next to it.
    pack_code, pack_output = run(["fpack", "sanity.fits"], cwd=sane_src)
    if pack_code != 0 or not os.path.exists(packed):
        tail = " / ".join(line.strip() for line in pack_output.splitlines()[-6:] if line.strip())
        summary["controls"].append(
            dict(name="toolchain", control_type="positive_sanity", ok=False,
                 expected="fpack output passes fitsverify", note="fpack failed: %s" % (tail or "no output"))
        )
        # The annotation is the only channel that survives when neither the log nor
        # the artifact can be read: put the tool's own words in it.
        print("::error::fpack exited %d and produced no reference file: %s"
              % (pack_code, tail or "no output"))
        evidence.write_text("control-toolchain.txt", pack_output)
        print("::error::fpack could not produce a reference file, so the tools are not trustworthy")
        failures += 1
    else:
        verify_code, verify_output = fitsverify(packed, sane_src)
        evidence.write_text("control-toolchain.txt", verify_output)
        summary["controls"].append(dict(name="toolchain", control_type="positive_sanity",
                                         expected="fpack output passes fitsverify",
                                         fpack=pack_code, fitsverify=verify_code,
                                         ok=(verify_code == 0)))
        print("control: fpack's own output -> fitsverify exit %d" % verify_code)
        if verify_code != 0:
            print("::error::fitsverify rejected fpack's own output; a red gate would say"
                  " nothing about this program")
            failures += 1

    # --- reconstruction differences that need ownership, counted but not excused ---
    # The literal diff is original-vs-funpack output. If it contains only an
    # inserted EXTNAME, keep that pattern separate for the owner, but do not say
    # which program introduced it: the gate does not yet isolate that producer.
    # It remains a failure and is never normalized away.
    escalation = []
    for case in summary["fixtures"]:
        diffs = case.get("header_diff") or {}
        cards = [d for items in diffs.values() for d in items]
        if cards and all(d["keyword"] == "EXTNAME" and d["kind"] == "invented" for d in cards):
            escalation.append(case["name"])
    summary["escalation"] = escalation
    if escalation:
        print(
            "escalation: %d fixture(s) have only an inserted `EXTNAME` in funpack's reconstructed header;"
            " producer not isolated, still a failure: %s" % (len(escalation), ", ".join(escalation))
        )

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
    lines = ["# fitsverify / funpack gate", ""]
    env = summary["environment"]
    lines.append("- python %s on %s (%s)" % (env["python"], env["platform"], env["machine"]))
    for name, tool in summary["tools"].items():
        lines.append("- %s: exit %s, %s" % (name, tool.get("exit"), tool.get("first_lines", "").splitlines()[:1]))
    lines += ["", "| fixture | expectation | tzim | fitsverify | funpack | problems |", "| --- | --- | --- | --- | --- | --- |"]
    for f in summary["fixtures"]:
        problems = f.get("problems") or []
        lines.append(
            "| %s | %s | %s | %s | %s | %s |"
            % (
                f.get("name"),
                f.get("expect"),
                f.get("tzim", {}).get("status") if isinstance(f.get("tzim"), dict) else "-",
                f.get("fitsverify_exit", "-"),
                f.get("funpack_exit", "-"),
                "; ".join(problems) if problems else "none",
            )
        )
    zero = next((f for f in summary["fixtures"] if f["name"] == "zero_width_column.fits"), None)
    if zero:
        lines += ["", "## zero-width column: unresolved contract reading", ""]
        lines.append("**Input:** " + (zero.get("input_case") or "description not recorded"))
        lines.append("")
        lines.append("Input SHA-256: `%s`; candidate SHA-256: `%s`." % (
            zero.get("input_sha256", "not recorded"), zero.get("candidate_sha256", "not recorded")))
        lines.append("")
        lines.append("**Contract reading:** " + (zero.get("contract_expectation") or zero.get("why", "not recorded")))
        lines.append("")
        record = zero.get("tzim") or {}
        lines.append("**Observed:** status `%s`, note `%s`; bytes %s → %s; `fitsverify` exit %s."
                     % (record.get("status", "?"), record.get("note"),
                        zero.get("bytes", {}).get("before", "?"),
                        zero.get("bytes", {}).get("after", "?"),
                        zero.get("fitsverify_exit", "not run")))
        lines.append("")
        lines.append("The `mixed` expectation is not asserted: `already_optimal` does not expose the discarded candidate size. No writer behavior is changed and no whole-file refusal is inferred.")
    lines += ["", "## controls", ""]
    for c in summary["controls"]:
        lines.append("- `%s` (%s; expected: %s): %s" % (
            c.get("name"), c.get("control_type", "unclassified"),
            c.get("expected", "not recorded"), json.dumps(c, sort_keys=True)))
    lines += ["", "Overall: **%s**" % ("PASS" if summary["ok"] else "FAIL")]
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    sys.exit(main())
