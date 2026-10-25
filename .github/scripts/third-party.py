#!/usr/bin/env python3
"""Compare tzim's candidates with an implementation that shares no code with it.

`verify` proves that a candidate decodes to the same bytes and the same pixels as
the original — but it uses *our* decoder on one side of the PNG/JPEG comparison,
and for the ZIP structure our own reader on both sides. A defect symmetric in the
encoder and the decoder is invisible to it (docs/guarantees.md §2.6). This script
is the missing side: Pillow for images, the standard library's `zlib` for
streams, `zipfile` for archives — none of which was written in this repository.

Three things follow from "a check that cannot fail is not a check":

  * the comparison is at the strongest level each library offers — pixel data,
    mode and dimensions for images, decompressed bytes for streams and every
    entry of an archive — not "it opens";
  * `--self-test` mutates one candidate in memory and requires the comparison to
    notice, so the gate proves itself on every run instead of once;
  * the library versions are printed, because a verdict nobody can reproduce is
    not a verdict.

Usage:
    third-party.py --before DIR --after DIR --run RUN.json [--self-test]
                   [--report FILE]

`RUN.json` is the newline-delimited JSON of a tzim run over `--after`; `--before`
is a copy of the same tree taken before that run.
"""

import argparse
import collections
import gzip
import io
import json
import os
import sys
import zipfile
import zlib

try:
    from PIL import Image, features
except ImportError:  # pragma: no cover - CI installs it
    Image = None

IMAGE_FORMATS = {"png", "jpeg", "tiff", "ico"}
STREAM_FORMATS = {"gzip", "zlib"}
ARCHIVE_FORMATS = {"zip"}


class Mismatch(Exception):
    """The two bytes on either side of the comparison are not equivalent."""


def versions():
    lines = [f"python {sys.version.split()[0]}", f"zlib {zlib.ZLIB_VERSION}"]
    if Image is None:
        lines.append("Pillow: NOT INSTALLED")
    else:
        lines.append(f"Pillow {Image.__version__}")
        lines.append(f"libjpeg-turbo {features.version('jpg')}")
        lines.append(f"libtiff {features.version('libtiff')}")
        lines.append(f"zlib (Pillow) {features.version('zlib')}")
    lines.append("zipfile: Python standard library")
    return lines


# ---------------------------------------------------------------------------
# Comparison, one function per family
# ---------------------------------------------------------------------------


def image_digest(data):
    """Mode, size and every decoded pixel — the strongest thing Pillow offers."""
    if Image is None:
        raise RuntimeError("Pillow is not installed")
    with Image.open(io.BytesIO(data)) as im:
        im.load()
        # `tobytes` is the decoded sample data, without any container framing:
        # two files that differ in compression but agree here carry the same
        # image. Frames of an animation are compared too, when there are any.
        frames = [im.tobytes()]
        for index in range(1, getattr(im, "n_frames", 1)):
            im.seek(index)
            im.load()
            frames.append(im.tobytes())
        return (im.mode, im.size, im.info.get("transparency"), tuple(frames))


def stream_digest(fmt, data):
    if fmt == "gzip":
        return gzip.decompress(data)
    return zlib.decompress(data)


def archive_digest(data):
    """Every entry's name, method and decompressed bytes, in order."""
    with zipfile.ZipFile(io.BytesIO(data)) as zf:
        broken = zf.testzip()
        if broken is not None:
            raise Mismatch(f"entry {broken} fails its CRC")
        return [
            (info.filename, info.compress_type, zf.read(info))
            for info in zf.infolist()
        ]


def digest(fmt, data):
    if fmt in IMAGE_FORMATS:
        return image_digest(data)
    if fmt in STREAM_FORMATS:
        return stream_digest(fmt, data)
    if fmt in ARCHIVE_FORMATS:
        return archive_digest(data)
    raise AssertionError(f"no third-party decoder for {fmt!r}")


def compare(fmt, original, candidate):
    """Raises Mismatch unless the two files carry the same information."""
    left = digest(fmt, original)
    right = digest(fmt, candidate)
    if left == right:
        return
    if fmt in IMAGE_FORMATS:
        mode_a, size_a, transparency_a, frames_a = left
        mode_b, size_b, transparency_b, frames_b = right
        if (mode_a, size_a, transparency_a) != (mode_b, size_b, transparency_b):
            raise Mismatch(
                f"mode/size/transparency changed: "
                f"{(mode_a, size_a, transparency_a)} -> "
                f"{(mode_b, size_b, transparency_b)}"
            )
        for index, (a, b) in enumerate(zip(frames_a, frames_b)):
            if a == b:
                continue
            offset = next(i for i, (x, y) in enumerate(zip(a, b)) if x != y)
            raise Mismatch(
                f"frame {index} differs at byte {offset}: {a[offset]} != {b[offset]} "
                f"({len(a)} bytes)"
            )
        raise Mismatch(f"frame count changed: {len(frames_a)} -> {len(frames_b)}")
    if fmt in STREAM_FORMATS:
        raise Mismatch(
            f"decompressed bytes differ: {len(left)} vs {len(right)}"
        )
    names_a = [name for name, _, _ in left]
    names_b = [name for name, _, _ in right]
    if names_a != names_b:
        raise Mismatch(f"entries changed: {names_a} -> {names_b}")
    for (name, method_a, data_a), (_, method_b, data_b) in zip(left, right):
        if method_a != method_b:
            raise Mismatch(f"entry {name}: method {method_a} -> {method_b}")
        if data_a != data_b:
            raise Mismatch(f"entry {name}: decompressed bytes differ")
    raise Mismatch("archives differ")


# ---------------------------------------------------------------------------
# The run
# ---------------------------------------------------------------------------


def load_records(path):
    records = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if line:
                records.append(json.loads(line))
    return [r for r in records if r.get("type") == "file"]


def relative(root, path):
    """The path of `path` below the corpus root, as both trees see it."""
    for candidate in (path, os.path.abspath(path)):
        try:
            return os.path.relpath(candidate, root)
        except ValueError:  # different drives on Windows
            continue
    return os.path.basename(path)


def check(before_root, after_root, records):
    accepted = [r for r in records if r.get("status") == "optimized"]
    by_format = collections.Counter()
    problems = []
    skipped = []

    for record in accepted:
        fmt = record.get("format")
        if fmt is None:
            skipped.append((record.get("path"), "no format in the record"))
            continue
        if not (fmt in IMAGE_FORMATS or fmt in STREAM_FORMATS or fmt in ARCHIVE_FORMATS):
            skipped.append((record.get("path"), f"no decoder for {fmt}"))
            continue
        before = os.path.join(before_root, relative(after_root, record["path"]))
        after = record["path"]
        if not os.path.exists(before):
            problems.append(f"{after}: the original is missing from --before")
            continue
        with open(before, "rb") as fh:
            original = fh.read()
        with open(after, "rb") as fh:
            candidate = fh.read()
        by_format[fmt] += 1
        try:
            compare(fmt, original, candidate)
        except Mismatch as exc:
            problems.append(f"{after} [{fmt}]: {exc}")
        except Exception as exc:  # a decoder refusing a file we accepted
            problems.append(f"{after} [{fmt}]: third-party decoder failed: {exc}")
    return accepted, by_format, problems, skipped


def self_test(before_root, after_root, accepted):
    """Mutate one candidate and require the comparison to notice."""
    for record in accepted:
        fmt = record.get("format")
        if fmt not in IMAGE_FORMATS and fmt not in STREAM_FORMATS:
            continue
        before = os.path.join(before_root, relative(after_root, record["path"]))
        if not os.path.exists(before):
            continue
        with open(before, "rb") as fh:
            original = fh.read()
        with open(record["path"], "rb") as fh:
            candidate = fh.read()
        mutated = mutate(fmt, candidate)
        if mutated is None:
            continue
        try:
            compare(fmt, original, mutated)
        except Mismatch:
            return record["path"], fmt
        except Exception:
            return record["path"], fmt
        return None
    return None


def mutate(fmt, data):
    """One changed pixel, or one changed byte of a compressed payload."""
    if fmt in STREAM_FORMATS:
        # Flip a bit in the last quarter, away from the header and the trailer.
        if len(data) < 16:
            return None
        out = bytearray(data)
        out[len(out) * 3 // 4] ^= 0x01
        return bytes(out)
    if Image is None:
        return None
    try:
        with Image.open(io.BytesIO(data)) as im:
            im.load()
            if im.mode not in ("RGB", "RGBA", "L", "LA", "P"):
                return None
            fmt = im.format
            frame = im.copy()
        frame.putpixel((0, 0), _neighbour(frame.getpixel((0, 0))))
        buffer = io.BytesIO()
        frame.save(buffer, format=fmt)
        return buffer.getvalue()
    except Exception:
        # A mode this script cannot rewrite is not a reason to fail the run;
        # the self-test simply picks another file.
        return None


def _neighbour(pixel):
    if isinstance(pixel, int):
        return (pixel + 1) % 256
    first = (pixel[0] + 1) % 256
    return (first,) + tuple(pixel[1:])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", required=True, help="tree as it was before the run")
    parser.add_argument("--after", required=True, help="tree as the run left it")
    parser.add_argument("--run", required=True, help="NDJSON output of the run")
    parser.add_argument("--self-test", action="store_true", help="prove the gate can fail")
    parser.add_argument("--report", help="write the markdown report here")
    args = parser.parse_args()

    print("== third-party decoders ==")
    for line in versions():
        print(f"  {line}")
    print()

    records = load_records(args.run)
    accepted, by_format, problems, skipped = check(
        os.path.abspath(args.before), os.path.abspath(args.after), records
    )

    print(f"accepted by tzim: {len(accepted)}")
    for fmt, count in sorted(by_format.items()):
        print(f"  {fmt}: {count}")
    if skipped:
        print(f"skipped by this script: {len(skipped)}")
        for path, why in skipped[:10]:
            print(f"  {os.path.basename(path)}: {why}")

    lines = ["## Third-party verification", ""]
    lines.append("| Format | Files compared |")
    lines.append("| --- | --- |")
    for fmt, count in sorted(by_format.items()):
        lines.append(f"| {fmt} | {count} |")
    lines.append("")
    lines.append("Decoders: " + ", ".join(versions()))
    lines.append("")

    if args.self_test:
        result = self_test(os.path.abspath(args.before), os.path.abspath(args.after), accepted)
        if result is None:
            problems.append(
                "the self-test could not make the comparison fail: the gate proves nothing"
            )
            lines.append("Self-test: **failed to fail**")
        else:
            path, fmt = result
            print(f"self-test: a mutated {fmt} candidate was rejected ({os.path.basename(path)})")
            lines.append(
                f"Self-test: a mutated `{fmt}` candidate (`{os.path.basename(path)}`) was "
                "rejected, so the comparison can fail."
            )
        lines.append("")

    if problems:
        print()
        for problem in problems:
            print(f"::error::{problem}")
        lines.append("### Mismatches")
        lines.append("")
        for problem in problems:
            lines.append(f"- {problem}")
        lines.append("")
    else:
        print("third-party gate: OK")

    if args.report:
        with open(args.report, "w", encoding="utf-8") as fh:
            fh.write("\n".join(lines) + "\n")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
