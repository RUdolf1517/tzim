#!/usr/bin/env python3
"""#72: fetch the measurement corpora from pinned archives, and write the manifest.

The protocol, agreed before this file was written:

* **a codeload URL at a full commit SHA** — `https://codeload.github.com/<repo>/tar.gz/<sha>`,
  never a branch name and never `--depth 1` of a default branch. (The docstrings of
  `fits-measure.py` and `pdf-measure.py` say `git clone --depth 1 …`, which does not
  contain the commit the audits quote; `git fetch origin <sha>` does not work either,
  because GitHub does not serve arbitrary SHAs as refs. A blobless full clone does,
  and so does codeload at a SHA. This script uses codeload.)
* **the archive's SHA-256, verified before extraction.** The values in `CORPORA`
  below are the **first checksums anyone recorded** for these seven archives. They
  are a *new baseline*, not a recovered historical one: GitHub generates these
  tarballs on demand and does not promise that the bytes are stable, so what is
  verified here is "the same archive as the run that recorded this number", not
  "the archive the original audits used".
* **a per-file manifest**: path, byte length and SHA-256 for every file the
  measurement is allowed to read, with counts and totals per corpus and a digest
  over the whole thing.

`--verify` re-hashes an extracted tree against a manifest and fails when a file
differs, is missing, or has appeared that the manifest does not name. The runner
calls it *after* the measurement, so a run cannot quietly measure a file the
manifest never described.

Usage:
    python3 .github/scripts/measure-fetch.py --root /tmp/corpus --manifest manifest.json
    python3 .github/scripts/measure-fetch.py --root /tmp/corpus --manifest manifest.json --verify
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
import tarfile
import time
import urllib.error
import urllib.request

#: `selected` is a path inside the archive (empty means the whole tree) and
#: `suffixes` are the file names the measurement reads, matched **case-sensitively**
#: — `.Pdf` is not `.pdf`. The counts are what the audits report: 34 + 5 + 123 + 16
#: = 178 FITS files and 1009 + 630 + 569 = 2208 PDF files, and the run fails when
#: they disagree, because a manifest that silently selects a different set is a
#: different measurement. qpdf has eleven `.Pdf` files under `qtest/qpdf` that the
#: case-insensitive count picks up and the audit's 630 does not; they are recorded
#: as exclusions rather than dropped.
CORPORA = (
    dict(
        name="astropy",
        repo="astropy/astropy",
        commit="dbbf2acd6dbb177db184bff319d2a2950dd178bb",
        sha256="3c6b762797ea8f292d5c35fbb3ebe82b7a28139783b5c6e870a38e576d62a46f",
        bytes=6989587,
        format="fits",
        selected="astropy/io/fits/tests/data",
        suffixes=(".fits", ".fit", ".fts"),
        files=34,
    ),
    dict(
        name="cfitsio",
        repo="heasarc/cfitsio",
        commit="113781d8aa442cf8d3c27295d9084674b8b039f2",
        sha256="d93c0de372a264156d2f3e1eab2d4159e132c35c2216f71c0af0009b92a72b5e",
        bytes=2169497,
        format="fits",
        selected="",
        suffixes=(".fits", ".fit", ".fts"),
        files=5,
    ),
    dict(
        name="drizzlepac",
        repo="spacetelescope/drizzlepac",
        commit="635834581ea2b226a3f3028b6ac54efb5e180dfc",
        sha256="c78e1eff17a80ddfbcfc78b1ba26fc542dec14e476cca4faf8af9d496f33386e",
        bytes=14851757,
        format="fits",
        selected="",
        suffixes=(".fits", ".fit", ".fts"),
        files=123,
    ),
    dict(
        name="reproject",
        repo="astropy/reproject",
        commit="9dddd419111a9ca159baaf7d5e2eb5729b447b58",
        sha256="d20377b7480f05869bb6666c01a32fd59b019f373553b13dc3feb0e7d514d222",
        bytes=1672777,
        format="fits",
        selected="",
        suffixes=(".fits", ".fit", ".fts"),
        files=16,
    ),
    dict(
        name="pdfjs",
        repo="mozilla/pdf.js",
        commit="606bd6a4e138bbbc22fad10872d2ffa93365b6b1",
        sha256="33bac2537b4a5e3d1cfd74083c4b3bcca1f0bd1646a8046526a1b1de1b2830f3",
        bytes=105087051,
        format="pdf",
        selected="test/pdfs",
        suffixes=(".pdf",),
        files=1009,
    ),
    dict(
        name="qpdf",
        repo="qpdf/qpdf",
        commit="4eba95899886e851cc41d76886483b347612f2a8",
        sha256="c6de7611d2651ac71eebbabd0132b1dc13e8def0e4e640e791190f903490c799",
        bytes=19383624,
        format="pdf",
        selected="qpdf/qtest",
        suffixes=(".pdf",),
        files=630,
    ),
    dict(
        name="verapdf",
        repo="veraPDF/veraPDF-corpus",
        commit="bb75f4f0073d9350dfd058c0162a367e6fadf25e",
        sha256="26e44331c4cbd5bbc98662e179704b8c13334f8f439cbb8e0a46f21fc8489c1b",
        bytes=140980346,
        format="pdf",
        selected="PDF_A-1b",
        suffixes=(".pdf",),
        files=569,
    ),
)


def archive_url(entry):
    return "https://codeload.github.com/%s/tar.gz/%s" % (entry["repo"], entry["commit"])


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def fetch(entry, root):
    """Download, verify against the baseline, and extract one corpus."""
    src = os.path.join(root, "src")
    os.makedirs(src, exist_ok=True)
    archive = os.path.join(src, "%s.tar.gz" % entry["name"])
    url = archive_url(entry)
    if not os.path.exists(archive):
        print("  fetching %s (%s)" % (url, "%d bytes pinned" % entry["bytes"]))
        for attempt in range(3):
            try:
                with urllib.request.urlopen(url, timeout=300) as response, open(archive, "wb") as out:
                    shutil.copyfileobj(response, out)
                break
            except (urllib.error.URLError, TimeoutError) as error:
                if attempt == 2:
                    raise SystemExit("could not fetch %s: %s" % (url, error))
                time.sleep(2 + attempt * 3)
    size = os.path.getsize(archive)
    if size != entry["bytes"]:
        raise SystemExit(
            "%s is %d bytes, the pinned archive is %d — a different archive is not a newer one"
            % (entry["name"], size, entry["bytes"])
        )
    digest = sha256_of(archive)
    if digest != entry["sha256"]:
        raise SystemExit(
            "%s has sha256 %s, the baseline is %s.\n"
            "Either the upstream repository regenerated its archive or this baseline is wrong;"
            " neither is something to paper over. The run stops here."
            % (entry["name"], digest, entry["sha256"])
        )
    return archive, digest, size


def extract(entry, root, archive):
    """Unpack the selected files, and return exactly those paths.

    Members are written one at a time from `extractfile` rather than through
    `TarFile.extract`, so no path from an archive is ever joined onto the
    destination without being checked, and no `filter=` argument is needed (it
    only exists from Python 3.12).
    """
    dest = os.path.join(root, entry["name"])
    extract_files = not os.path.isdir(dest)
    os.makedirs(dest, exist_ok=True)
    near = []
    with tarfile.open(archive) as tarball:
        for member in tarball.getmembers():
            if not member.isfile():
                continue
            parts = member.name.split("/", 1)
            if len(parts) != 2:
                continue
            relative = parts[1]
            if entry["selected"] and not (
                relative == entry["selected"] or relative.startswith(entry["selected"] + "/")
            ):
                continue
            if not relative.endswith(entry["suffixes"]):
                if relative.lower().endswith(entry["suffixes"]):
                    near.append(os.path.join(dest, relative))
                continue
            if not extract_files:
                continue
            if os.path.isabs(relative) or ".." in relative.split("/"):
                raise SystemExit("archive member escapes the tree: %s" % member.name)
            target = os.path.join(dest, relative)
            os.makedirs(os.path.dirname(target), exist_ok=True)
            source = tarball.extractfile(member)
            with open(target, "wb") as handle:
                shutil.copyfileobj(source, handle)
    files = []
    for dirpath, _, names in os.walk(dest):
        for name in names:
            if name.endswith(entry["suffixes"]):
                files.append(os.path.join(dirpath, name))
            elif name.lower().endswith(entry["suffixes"]):
                near.append(os.path.join(dest, os.path.relpath(os.path.join(dirpath, name), dest)))
    return sorted(files), sorted(near)


def manifest(root, entries, extra=None):
    """The per-file record the run is judged against."""
    out = dict(
        protocol="codeload archive at a full commit sha, sha256 verified, per-file manifest",
        note=(
            "The archive checksums are the first recorded baseline for these URLs; they are not"
            " claimed to be the archives the original audits used."
        ),
        corpora=[],
        generated=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    )
    digest = hashlib.sha256()
    for entry, files, near in entries:
        rows = []
        total = 0
        for path in files:
            size = os.path.getsize(path)
            total += size
            rows.append(
                dict(
                    path=os.path.relpath(path, root),
                    bytes=size,
                    sha256=sha256_of(path),
                )
            )
        for row in rows:
            digest.update(row["path"].encode() + b"\0" + row["sha256"].encode() + b"\0")
        out["corpora"].append(
            dict(
                name=entry["name"],
                repo=entry["repo"],
                commit=entry["commit"],
                archive=archive_url(entry),
                archive_sha256=entry["sha256"],
                archive_bytes=entry["bytes"],
                format=entry["format"],
                selected=entry["selected"],
                files=len(rows),
                expected_files=entry["files"],
                bytes=total,
                entries=rows,
                excluded=[
                    dict(path=os.path.relpath(p, root), why="not selected: the suffix is not lowercase")
                    for p in near
                ],
            )
        )
    out["manifest_digest"] = digest.hexdigest()
    if extra:
        out.update(extra)
    return out


def verify(root, path):
    """Re-hash an extracted tree and compare it with a manifest. Returns problems."""
    with open(path) as handle:
        recorded = json.load(handle)
    problems = []
    for corpus in recorded["corpora"]:
        missing = present = 0
        for row in corpus["entries"]:
            full = os.path.join(root, row["path"])
            if not os.path.exists(full):
                missing += 1
                problems.append("missing: %s" % row["path"])
                continue
            size = os.path.getsize(full)
            if size != row["bytes"]:
                problems.append("%s is %d bytes, the manifest says %d" % (row["path"], size, row["bytes"]))
                continue
            if sha256_of(full) != row["sha256"]:
                problems.append("%s does not hash to the manifest's value" % row["path"])
                continue
            present += 1
        extra = 0
        want = {os.path.join(root, row["path"]) for row in corpus["entries"]}
        dest = os.path.join(root, corpus["name"])
        for dirpath, _, names in os.walk(dest):
            for name in names:
                full = os.path.join(dirpath, name)
                if full not in want:
                    extra += 1
        print(
            "  %-11s %4d verified, %d missing, %d not in the manifest"
            % (corpus["name"], present, missing, extra)
        )
        if extra:
            problems.append("%s has %d files the manifest does not name" % (corpus["name"], extra))
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", required=True, help="where the corpora live")
    parser.add_argument("--manifest", default="measure-manifest.json")
    parser.add_argument("--verify", action="store_true", help="re-hash an existing tree instead of fetching")
    args = parser.parse_args()

    if args.verify:
        problems = verify(args.root, args.manifest)
        for problem in problems[:50]:
            print("!! %s" % problem)
        print("%d problem(s)" % len(problems))
        return 1 if problems else 0

    entries = []
    failures = 0
    for entry in CORPORA:
        print("== %s @ %s" % (entry["name"], entry["commit"]))
        archive, digest, size = fetch(entry, args.root)
        files, near = extract(entry, args.root, archive)
        print(
            "  %s: %d files, %d bytes%s"
            % (
                entry["name"],
                len(files),
                sum(os.path.getsize(f) for f in files),
                "; %d near-miss(es) excluded" % len(near) if near else "",
            )
        )
        for path in near:
            print("    excluded %s (uppercase suffix)" % os.path.relpath(path, args.root))
        if len(files) != entry["files"]:
            print(
                "::error::%s selected %d files, the audit reports %d"
                % (entry["name"], len(files), entry["files"])
            )
            failures += 1
        entries.append((entry, files, near))

    payload = manifest(args.root, entries)
    with open(args.manifest, "w") as handle:
        json.dump(payload, handle, indent=2, sort_keys=True)
        handle.write("\n")
    print(
        "manifest: %d corpora, %d files, %d bytes, digest %s"
        % (
            len(payload["corpora"]),
            sum(c["files"] for c in payload["corpora"]),
            sum(c["bytes"] for c in payload["corpora"]),
            payload["manifest_digest"],
        )
    )
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
