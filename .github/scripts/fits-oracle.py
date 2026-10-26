#!/usr/bin/env python3
"""FITS tile compression, checked by a reader written by someone else (#70).

`docs/fits-audit.md` §4 gates Tier 2 on `fitsverify` accepting the output and
`funpack` producing a header that diffs clean. Neither tool is installable in
this environment, so this script uses the substitute the audit itself names in
§8.1 and does not hide: **`astropy`, which is CFITSIO underneath** — the same
tile decoder `fpack` and `funpack` call. What it proves is that a second
implementation reads the file and sees the same numbers; what it does not prove
is `fitsverify`'s own structural lint, which has still not run anywhere.

    python3 -m pip install numpy astropy
    python3 .github/scripts/fits-oracle.py --binary target/release/tzim

Every assertion is about the file, never about the handler's opinion of it: the
arrays are compared after `astropy` has decompressed them, the headers a reader
reconstructs are compared with the original cards, and every HDU that was not a
rewrite target is compared byte for byte.

Exit code 0 means every fixture behaved; 1 means at least one did not, and the
reason is printed either way. The script is not part of the build and not a
dependency of the crate.
"""

import argparse
import io
import os
import subprocess
import sys
import tempfile

import numpy as np
import astropy
from astropy.io import fits

RECORD = 2880
CARD = 80


# ---------------------------------------------------------------------------
# A FITS walker, so that "copied verbatim" can be checked on the bytes
# ---------------------------------------------------------------------------


class Hdu:
    """One HDU: its cards, its data, and the bytes it occupied in the file."""

    def __init__(self, cards, data, raw):
        self.cards = cards
        self.data = data
        self.raw = raw

    def card(self, keyword):
        for card in self.cards:
            if card[:8].strip() == keyword:
                return card[10:30].strip()
        return None

    def int(self, keyword):
        value = self.card(keyword)
        return int(value) if value is not None else None

    def text(self, keyword):
        value = self.card(keyword)
        if value is None or not value.startswith("'"):
            return None
        return value.strip("'").strip()


def walk(raw):
    """Every HDU in `raw`, header and data, without trusting any of it."""
    out = []
    offset = 0
    while offset + RECORD <= len(raw):
        start = offset
        cards = []
        end_of_header = None
        while end_of_header is None:
            if offset + RECORD > len(raw):
                raise ValueError("a header record runs past the end of the file")
            for within in range(0, RECORD, CARD):
                card = raw[offset + within:offset + within + CARD].decode(
                    "ascii", "replace"
                )
                cards.append(card)
                if card[:8].strip() == "END":
                    end_of_header = offset + RECORD
                    break
            offset += RECORD
        probe = Hdu(cards, b"", b"")
        naxis = probe.int("NAXIS")
        bitpix = probe.int("BITPIX")
        if naxis is None or bitpix is None:
            raise ValueError("an HDU with no BITPIX or NAXIS")
        length = 1
        for axis in range(1, naxis + 1):
            length *= probe.int("NAXIS%d" % axis)
        pcount = probe.int("PCOUNT") or 0
        gcount = probe.int("GCOUNT") or 1
        data_len = 0 if naxis == 0 else abs(bitpix) // 8 * (length + pcount) * gcount
        data_start = end_of_header
        end = data_start + -(-data_len // RECORD) * RECORD
        if end > len(raw):
            raise ValueError("an HDU runs past the end of the file")
        out.append(Hdu(cards, raw[data_start:data_start + data_len], raw[start:end]))
        offset = end
    if offset != len(raw):
        raise ValueError("trailing bytes after the last HDU")
    return out


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


def pattern(shape, dtype):
    """A repeating pattern: compressible, deterministic, and never NaN."""
    count = int(np.prod(shape))
    return (np.arange(count, dtype=np.int64) % 251).astype(dtype).reshape(shape)


def hdu_bytes(hdu):
    """The bytes astropy writes for one primary HDU."""
    buffer = io.BytesIO()
    fits.HDUList([hdu]).writeto(buffer, overwrite=True, output_verify="silentfix")
    return buffer.getvalue()


def extension_bytes(hdu):
    """The bytes astropy writes for one extension HDU.

    astropy refuses to write an extension on its own, so it is written behind an
    empty primary and that primary — exactly one 2880-byte record, because it has
    no data — is dropped.
    """
    buffer = io.BytesIO()
    fits.HDUList([fits.PrimaryHDU(), hdu]).writeto(
        buffer, overwrite=True, output_verify="silentfix"
    )
    raw = buffer.getvalue()
    assert raw[:RECORD].decode("ascii").count("NAXIS   =                    0") == 1
    return raw[RECORD:]


def raw_hdu(cards, data):
    """Builds one HDU by hand: cards, END, padding, data, padding."""
    body = b"".join(card.ljust(CARD)[:CARD].encode() for card in cards)
    body += b"END".ljust(CARD)
    body += b" " * (-len(body) % RECORD)
    body += data
    body += b"\x00" * (-len(body) % RECORD)
    return body


def nonstandard(name, payload):
    """An extension type this build is not supposed to understand."""
    return raw_hdu(
        [
            "%-8s= '%s'" % ("XTENSION", name),
            "%-8s= %20d" % ("BITPIX", 8),
            "%-8s= %20d" % ("NAXIS", 1),
            "%-8s= %20d" % ("NAXIS1", len(payload)),
            "%-8s= %20d" % ("PCOUNT", 0),
            "%-8s= %20d" % ("GCOUNT", 1),
        ],
        payload,
    )


def build_fixtures(root):
    """Every fixture, and exactly what the handler is expected to do with it.

    `mapping` is what makes the assertions unambiguous: `(original HDU index,
    candidate HDU index)`. A rewritten primary array shifts everything by one,
    because a `BINTABLE` cannot be the first HDU of a file.
    """
    cases = []

    # 1. A primary image with metadata: the common case, rewritten.
    header = fits.Header()
    header["OBJECT"] = ("M31", "what it is")
    header["BUNIT"] = "COUNTS"
    header.add_history("first history line")
    header.add_history("second history line")
    image = pattern((80, 128), ">i2")
    with open(os.path.join(root, "int.fits"), "wb") as handle:
        handle.write(hdu_bytes(fits.PrimaryHDU(image, header=header)))
    cases.append(
        {
            "name": "int.fits",
            "expect": "rewritten",
            "arrays": [(0, 1)],
            "metadata": [(0, 1, ["OBJECT", "BUNIT", "HISTORY"])],
            "tables": [1],
        }
    )

    # 2. Floating point: GZIP_2, and the shuffle that goes with it.
    floats = pattern((48, 64), ">f4") / 7.0
    with open(os.path.join(root, "float.fits"), "wb") as handle:
        handle.write(hdu_bytes(fits.PrimaryHDU(floats)))
    cases.append(
        {
            "name": "float.fits",
            "expect": "rewritten",
            "arrays": [(0, 1)],
            "tables": [1],
        }
    )

    # 3. A multi-HDU file: an unknown extension between two images, plus a table.
    #    Everything that is not an image must come through byte for byte — the
    #    mistake that lost five HST files in the measurement.
    second = pattern((40, 64), ">f8")
    table = raw_hdu(
        [
            "%-8s= '%s'" % ("XTENSION", "BINTABLE"),
            "%-8s= %20d" % ("BITPIX", 8),
            "%-8s= %20d" % ("NAXIS", 2),
            "%-8s= %20d" % ("NAXIS1", 4),
            "%-8s= %20d" % ("NAXIS2", 4),
            "%-8s= %20d" % ("PCOUNT", 0),
            "%-8s= %20d" % ("GCOUNT", 1),
            "%-8s= %20d" % ("TFIELDS", 1),
            "%-8s= '%s'" % ("TTYPE1", "VALUE"),
            "%-8s= '%s'" % ("TFORM1", "1J"),
        ],
        np.arange(4, dtype=">i4").tobytes(),
    )
    multi = (
        hdu_bytes(fits.PrimaryHDU(image))
        + nonstandard("HDRLET", bytes(range(256)) * 16)
        + extension_bytes(fits.ImageHDU(second, name="SCI"))
        + table
    )
    with open(os.path.join(root, "multi.fits"), "wb") as handle:
        handle.write(multi)
    cases.append(
        {
            "name": "multi.fits",
            "expect": "rewritten",
            "arrays": [(0, 1), (2, 3)],
            "metadata": [(2, 3, ["EXTNAME"])],
            "verbatim": [(1, 2), (3, 4)],
            "tables": [1, 3],
        }
    )

    # 4. Already compressed with a codec this build does not have.
    rice = hdu_bytes(fits.PrimaryHDU()) + extension_bytes(
        fits.CompImageHDU(
            data=image.astype(">i4"), compression_type="RICE_1", quantize_level=0
        )
    )
    with open(os.path.join(root, "rice.fits"), "wb") as handle:
        handle.write(rice)
    cases.append(
        {
            "name": "rice.fits",
            "expect": "skipped",
            "reason": "RICE_1",
        }
    )

    # 5. A file the rewrite would grow: the header outweighs the samples.
    with open(os.path.join(root, "tiny.fits"), "wb") as handle:
        handle.write(hdu_bytes(fits.PrimaryHDU(pattern((4, 4), ">i2"))))
    cases.append({"name": "tiny.fits", "expect": "already optimal"})

    # 6. A stale-checksum trap: the HDU carries CHECKSUM/DATASUM, and this build
    #    does not recompute them, so it must be left exactly as it is.
    with open(os.path.join(root, "checksum.fits"), "wb") as handle:
        handle.write(hdu_bytes_checked(fits.PrimaryHDU(image)))
    cases.append({"name": "checksum.fits", "expect": "skipped", "reason": "CHECKSUM"})

    # 7. A scaled image: refused, because the scaling is what a reader applies and
    #    this build cannot check that re-derivation yet. The cards are written by
    #    hand: astropy treats `BSCALE`/`BZERO` as a description of the array it
    #    holds and does not write them back out unchanged.
    scaled = raw_hdu(
        [
            "%-8s= %20s" % ("SIMPLE", "T"),
            "%-8s= %20d" % ("BITPIX", 16),
            "%-8s= %20d" % ("NAXIS", 2),
            "%-8s= %20d" % ("NAXIS1", 64),
            "%-8s= %20d" % ("NAXIS2", 40),
            "%-8s= %20d" % ("BSCALE", 1),
            "%-8s= %20d" % ("BZERO", 32768),
        ],
        pattern((40, 64), ">i2").tobytes(),
    )
    with open(os.path.join(root, "scaled.fits"), "wb") as handle:
        handle.write(scaled)
    cases.append({"name": "scaled.fits", "expect": "skipped", "reason": "BSCALE"})

    return cases


def hdu_bytes_checked(hdu):
    """The bytes astropy writes for one HDU with `CHECKSUM`/`DATASUM` set."""
    buffer = io.BytesIO()
    fits.HDUList([hdu]).writeto(buffer, overwrite=True, checksum=True)
    return buffer.getvalue()


# ---------------------------------------------------------------------------
# The checks
# ---------------------------------------------------------------------------


def run(binary, path):
    done = subprocess.run(
        [binary, "--fits-tile", os.path.basename(path)],
        cwd=os.path.dirname(path),
        capture_output=True,
        text=True,
        check=False,
    )
    return done.returncode, done.stdout + done.stderr


def check_rewritten(path, original, case):
    """Everything a rewritten file has to satisfy, checked by astropy."""
    problems = []
    before = walk(original)
    after = walk(open(path, "rb").read())
    expected_shapes = {}
    for source, target in case.get("arrays", []):
        array = case["expected"][source]
        if array is not None:
            expected_shapes[target] = np.shape(array)
    if len(after) < len(before):
        problems.append("the candidate has fewer HDUs than the original")

    with fits.open(path) as hdul:
        # (a) the arrays, as a second implementation decodes them. Floats are
        #     compared with `equal_nan`, because NaN is not a difference of
        #     content — that mistake produced 136 false failures in the audit.
        for source, target in case.get("arrays", []):
            expected = case["expected"][source]
            if expected is None:
                problems.append("the fixture's HDU %d has no data" % source)
                continue
            try:
                got = np.asarray(hdul[target].data)
            except IndexError:
                problems.append("candidate HDU %d is missing" % target)
                continue
            except Exception as error:  # noqa: BLE001 - a reader's own complaint
                # A candidate whose cards disagree with its tiles is a failure
                # of the candidate, not a crash of this script: a mutation that
                # writes `ZTILE1 = 1` makes astropy raise instead of decoding,
                # and a traceback here would lose the other fixtures' verdicts.
                problems.append(
                    "HDU %d could not be read: %s: %s"
                    % (target, type(error).__name__, error)
                )
                continue
            if got.shape != expected.shape:
                problems.append("shape %s -> %s" % (expected.shape, got.shape))
            elif expected.dtype.kind == "f":
                if not np.array_equal(got, expected, equal_nan=True):
                    problems.append("the float array changed")
            elif got.astype(expected.dtype).tobytes() != expected.tobytes():
                problems.append("the integer array changed")

        # (b) the cards a reader reconstructs from the compressed header
        for source, target, keywords in case.get("metadata", []):
            for keyword in keywords:
                if keyword not in hdul[target].header:
                    problems.append(
                        "%s did not survive into HDU %d" % (keyword, target)
                    )

        # (c) the table itself has to be what the convention says it is. The
        #     cards come from the walker rather than from `hdul[target].header`:
        #     astropy replaces a compressed HDU's own keywords with the image
        #     header it reconstructs from them, so `ZIMAGE` and `ZCMPTYPE` are
        #     not in the object it hands back.
        for target in case.get("tables", []):
            table = after[target]
            if table.card("ZIMAGE") is None:
                problems.append("HDU %d is not a ZIMAGE table" % target)
                continue
            codec = table.text("ZCMPTYPE")
            bitpix = table.int("ZBITPIX")
            want = "GZIP_2" if bitpix < 0 else "GZIP_1"
            if codec != want:
                problems.append("%s for ZBITPIX %d" % (codec, bitpix))
            if table.text("TTYPE1") != "COMPRESSED_DATA":
                problems.append("HDU %d has no COMPRESSED_DATA column" % target)
            if table.int("NAXIS1") != 8:
                problems.append("HDU %d row width is %s" % (target, table.int("NAXIS1")))

            #     The tiling rule this handler promises: `ZTILE1` is the whole
            #     first axis and every other `ZTILEn` is 1, so tile *i* is row
            #     *i*. Nothing else in this file looks at the `Z` cards beyond
            #     their presence, and a mutation of the tiling was the one
            #     mutation that used to pass: a file whose tiles are one row
            #     each but whose cards claim a square tiling decodes to the
            #     right array only if the reader ignores the cards, and a
            #     candidate that says `ZTILE1 = 1` with row-sized tiles is not
            #     caught by comparing arrays at all — astropy raises, which
            #     used to abort this script rather than fail the fixture.
            znaxis = table.int("ZNAXIS")
            #     The shape comes from the original array, not from the
            #     candidate: with the cards and the original in hand there is
            #     nothing left for the file to disagree with, and a candidate
            #     whose `ZTILEn` lies is reported as a tiling problem rather
            #     than as an opaque failure to read it.
            shape = expected_shapes.get(target)
            if shape is None:
                continue
            if znaxis is None or len(shape) != znaxis:
                problems.append(
                    "HDU %d: ZNAXIS %s against an array of rank %d"
                    % (target, znaxis, len(shape))
                )
                continue
            # FITS counts axes fastest-varying last, so `ZNAXIS1` is the last
            # entry of numpy's shape.
            axes = [table.int("ZNAXIS%d" % (n + 1)) for n in range(znaxis)]
            if axes != list(shape[::-1]):
                problems.append(
                    "HDU %d: ZNAXISn %s against the shape %s" % (target, axes, shape)
                )
                continue
            if axes and table.int("ZTILE1") != axes[0]:
                problems.append(
                    "HDU %d: ZTILE1 %s is not the whole first axis %s"
                    % (target, table.int("ZTILE1"), axes[0])
                )
            for n in range(2, znaxis + 1):
                if table.int("ZTILE%d" % n) != 1:
                    problems.append(
                        "HDU %d: ZTILE%d %s is not 1, so the tiling is not the one "
                        "the contract names" % (target, n, table.int("ZTILE%d" % n))
                    )
            rows = table.int("NAXIS2")
            want_rows = int(np.prod(axes[1:])) if len(axes) > 1 else 1
            if rows != want_rows:
                problems.append(
                    "HDU %d: NAXIS2 %s for an array that needs %s tiles"
                    % (target, rows, want_rows)
                )

    # (d) every HDU that was not a rewrite target is byte-identical
    for source, target in case.get("verbatim", []):
        if target >= len(after):
            problems.append("HDU %d is missing from the candidate" % target)
        elif before[source].raw != after[target].raw:
            problems.append("HDU %d was not copied verbatim" % source)

    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", required=True, help="the tzim to test")
    parser.add_argument("--keep", action="store_true", help="keep the fixtures")
    args = parser.parse_args()

    binary = os.path.abspath(args.binary)
    root = tempfile.mkdtemp(prefix="tzim-fits-")
    cases = build_fixtures(root)
    failures = 0

    for case in cases:
        path = os.path.join(root, case["name"])
        original = open(path, "rb").read()
        # The arrays are read back through astropy from the original file, which
        # is the yardstick every candidate is compared with.
        # A copy, not a view: astropy memory-maps the file, and the file is
        # rewritten before these arrays are read.
        with fits.open(path) as hdul:
            case["expected"] = [
                None if hdu.data is None else np.array(hdu.data, copy=True)
                for hdu in hdul
            ]

        code, output = run(binary, path)
        problems = []
        if code != 0:
            problems.append("exit code %d: %s" % (code, output.strip()))

        if case["expect"] == "rewritten":
            after = open(path, "rb").read()
            if after == original:
                problems.append("the file did not change")
            elif len(after) >= len(original):
                problems.append(
                    "the file grew: %d -> %d bytes" % (len(original), len(after))
                )
            problems += check_rewritten(path, original, case)
        else:
            if case["expect"] not in output:
                problems.append("the report never uses %r" % case["expect"])
            if "reason" in case and case["reason"] not in output:
                problems.append("the note never mentions %r" % case["reason"])
            if open(path, "rb").read() != original:
                problems.append("a file that could not be improved was rewritten")

        print("%-14s %-16s %s" % (case["name"], case["expect"], "ok" if not problems else "FAIL"))
        for problem in problems:
            print("    %s" % problem)
        failures += bool(problems)

    if args.keep:
        print("\nfixtures kept in", root)
    else:
        for case in cases:
            os.unlink(os.path.join(root, case["name"]))
        os.rmdir(root)

    print()
    print("%d fixture(s), %d failure(s)" % (len(cases), failures))
    print("read back by astropy %s (CFITSIO underneath); fitsverify has not run"
          % astropy.__version__)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
