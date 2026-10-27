#!/usr/bin/env python3
"""pdf-audit §9: the measurement behind the refusal of PDF (issue #57).

This is the instrument for [docs/pdf-audit.md](../docs/pdf-audit.md#9-the-measurement-method-corpus-and-the-rule-that-is-fixed-in-advance).
It is not part of the build, it is not a dependency of the crate, and it never
touches a file the project ships. It exists so that the numbers in the audit can
be reproduced, and so that a future attempt starts from a runnable script rather
than from a paragraph.

    python3 -m venv /tmp/v && /tmp/v/bin/pip install pikepdf PyMuPDF
    /tmp/v/bin/python .github/scripts/pdf-measure.py --root /tmp/corpus --out /tmp/pdf.jsonl

The corpus is **not** fetched here. `--root` holds a tree that
`.github/scripts/measure-fetch.py` produced from pinned codeload archives, together
with a per-file manifest; `--manifest` makes this script read exactly the files that
manifest names, at the byte length it records, instead of globbing whatever is on
disk:

    python3 .github/scripts/measure-fetch.py --root /tmp/corpus --manifest /tmp/corpus/manifest.json
    /tmp/v/bin/python .github/scripts/pdf-measure.py --root /tmp/corpus \
        --manifest /tmp/corpus/manifest.json --out /tmp/pdf.jsonl

Pinned at the full commits the audit quotes: pdf.js
`606bd6a4e138bbbc22fad10872d2ffa93365b6b1`, qpdf
`4eba95899886e851cc41d76886483b347612f2a8`, veraPDF-corpus
`bb75f4f0073d9350dfd058c0162a367e6fadf25e`. A commit pins the exact source revision; a codeload tarball at
that commit does **not** have to (GitHub regenerates it on demand), which is why the
manifest's per-file hashes — not the archive hash — are what a re-run is checked
against.

`pikepdf` ships libqpdf and `PyMuPDF` ships MuPDF, so `qpdf` and `mutool` do not
have to be installed: the same libraries, driven in-process. The audit records the
versions it ran with.
"""

import argparse
import glob
import hashlib
import json
import os
import random
import sys
import zlib
from collections import Counter

import pikepdf

CORPORA = {
    "pdfjs": ("pdf.js", "test/pdfs"),
    "qpdf": ("qpdf", "qpdf/qtest"),
    "verapdf": ("veraPDF-corpus", "PDF_A-1b"),
}

#: Refusal reasons of `docs/guarantees.md` §4.11, applied in the order they appear
#: in the contract. The first one that fires is the reason recorded.
SIGNATURES = ((b"/ByteRange", "signed"), (b"/Linearized", "linearized"),
              (b"/Hint", "hint_stream"))


def qpdf_check(path, scratch):
    """qpdf's own verdict: exit 0 means no errors and no warnings."""
    try:
        pikepdf.Job(["--check", path, scratch]).run()
        return 0
    except Exception as e:                  # noqa: BLE001 - qpdf reports this way
        return getattr(e, "exit_code", 2)


def iter_streams(pdf):
    """(key, stream) for every stream. `Pdf.objects` yields Object instances whose
    `.objgen` is only defined for indirect objects, so the key falls back to the
    position in the object list, which qpdf preserves."""
    for idx, o in enumerate(pdf.objects):
        try:
            if not isinstance(o, pikepdf.Stream):
                continue
        except Exception:                   # noqa: BLE001
            continue
        try:
            key = str(o.objgen)
        except Exception:                   # noqa: BLE001
            key = "idx%d" % idx
        yield key, o


def rewritable_streams(pdf):
    """The streams §4.11 lets a handler touch, paired with the reason it cannot be.

    A generator yields `(stream, None)` for a stream inside the slice and
    `(None, reason)` for one that puts the whole file outside it — never `return`,
    which would end the iteration and silently declare the rest of the file empty.
    A one-element `/Filter` array counts as "the single name `FlateDecode`", which
    is what the contract says; two or more names are a chain, and a chain has to be
    re-encoded end to end.
    """
    for _key, s in iter_streams(pdf):
        t = s.get("/Type")
        if t is not None and str(t) in ("/ObjStm", "/XRef"):
            continue
        if "/F" in s:
            yield None, "stream_external"
            continue
        f = s.get("/Filter")
        if f is None:
            continue
        if isinstance(f, pikepdf.Array):
            if len(f) != 1:
                yield None, "filter_chain"
                continue
            f = f[0]
        if str(f) != "/FlateDecode":
            continue
        yield s, None


def zlib_stream_ok(raw):
    if len(raw) < 6:
        return False, "stream_too_short"
    cmf, flg = raw[0], raw[1]
    if (cmf & 0x0F) != 8:
        return False, "not_deflate"
    if (cmf >> 4) > 7:
        return False, "cinfo_too_large"
    if flg & 0x20:
        return False, "preset_dictionary"
    if ((cmf << 8) | flg) % 31 != 0:
        return False, "bad_check_bits"
    d = zlib.decompressobj()
    try:
        d.decompress(raw)
        d.flush()
    except zlib.error:
        return False, "not_inflatable"
    if not d.eof:
        return False, "truncated"
    return True, "ok"


def classify(path, scratch):
    data = open(path, "rb").read()
    hdr = data.find(b"%PDF-")
    if hdr < 0 or hdr > 1024:
        return "refused", "no_header", {}
    if data.count(b"%%EOF") != 1:
        return "refused", "incremental_update", {}
    if qpdf_check(path, scratch) != 0:
        return "refused", "damaged", {}
    try:
        pdf = pikepdf.open(path, attempt_recovery=False)
    except Exception:                       # noqa: BLE001
        return "refused", "unreadable", {}
    if pdf.is_encrypted or "/Encrypt" in pdf.trailer:
        return "refused", "encrypted", {}
    if "/Prev" in pdf.trailer:
        return "refused", "incremental_update", {}
    for token, reason in SIGNATURES:
        if token in data:
            return "refused", reason, {}

    n = 0
    for s, why in rewritable_streams(pdf):
        if why:
            return "refused", why, {}
        raw = s.read_raw_bytes()
        declared = s.get("/Length")
        if declared is None or int(declared) != len(raw):
            return "refused", "length_mismatch", {}
        ok, why = zlib_stream_ok(raw)
        if not ok:
            return "refused", why, {}
        n += 1
    if n == 0:
        return "already_optimal", "no_rewritable_flate", {"rewritable": 0}
    return "accepted", "ok", {"rewritable": n}


def faithful(path):
    """What §4.11 permits, measured without writing a file: for every rewritable
    stream, the smaller of the original payload and the payload re-deflated at 9."""
    pdf = pikepdf.open(path, attempt_recovery=False)
    total = after = n = 0
    for s, why in rewritable_streams(pdf):
        if why:
            continue
        raw = s.read_raw_bytes()
        try:
            plain = zlib.decompress(raw)
        except zlib.error:
            continue
        total += len(raw)
        after += min(len(raw), len(zlib.compress(plain, 9)))
        n += 1
    return n, total, after


def qpdf_jobs(path, outdir, tag):
    """The two jobs of the pre-registered table: the permitted transform, and the
    forbidden repack that prices the invariant."""
    allowed = os.path.join(outdir, tag + ".pdf")
    repack = os.path.join(outdir, tag + ".repack.pdf")
    pikepdf.Job(["--recompress-flate", "--compression-level=9",
                 "--object-streams=preserve", path, allowed]).run()
    pikepdf.Job(["--recompress-flate", "--compression-level=9",
                 "--object-streams=generate", path, repack]).run()
    return allowed, repack


def payloads(path):
    """Multiset of sha256 over the inflated payload of every stream. A multiset
    because qpdf renumbers freely; what must not change is the set of payloads.
    `/Type /XRef` streams are skipped: their payload is a table of offsets."""
    pdf = pikepdf.open(path, attempt_recovery=False)
    c = Counter()
    for _key, s in iter_streams(pdf):
        t = s.get("/Type")
        if t is not None and str(t) == "/XRef":
            continue
        raw = s.read_raw_bytes()
        f = s.get("/Filter")
        try:
            content = zlib.decompress(raw) if (f is not None and str(f) == "/FlateDecode") else raw
        except zlib.error:
            content = b"\x00uninflatable"
        c[hashlib.sha256(content).hexdigest()] += 1
    return c


def render_digests(path, pages=6, dpi=100):
    """Page images, hashed. MuPDF, the engine behind `mutool draw`."""
    import pymupdf
    doc = pymupdf.open(path)
    out = [hashlib.sha256(doc[i].get_pixmap(dpi=dpi).samples).hexdigest()
           for i in range(min(len(doc), pages))]
    n = len(doc)
    doc.close()
    return out, n


def from_manifest(path):
    """(corpus, path, row) for each PDF, in original per-corpus path order.

    The original instrument walked ``sorted(glob.glob(...))``. Manifests are
    ordered by the fetcher, but sorting here makes the contract explicit and
    prevents a valid-yet-reordered manifest from changing accepted-row order or
    the seeded render sample.
    """
    with open(path) as f:
        manifest = json.load(f)
    out = []
    for corpus in manifest["corpora"]:
        if corpus["format"] != "pdf":
            continue
        for row in sorted(corpus["entries"], key=lambda item: item["path"]):
            out.append((corpus["name"], row["path"], row))
    return manifest, out


def select_render_sample(accepted_rows, sample_size=140, seed=57):
    """The audit's seeded sample, isolated so order/selection is regression-tested."""
    random.seed(seed)
    return random.sample(accepted_rows, min(sample_size, len(accepted_rows)))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", default="/tmp/corpus", help="directory holding the checkouts")
    ap.add_argument("--manifest", default="", help="per-file manifest from measure-fetch.py")
    ap.add_argument("--out", default="/tmp/pdf.jsonl")
    ap.add_argument("--render-out", default="", help="JSONL records for the exact render sample")
    ap.add_argument("--render-sample", type=int, default=140,
                    help="files rendered and compared; 0 disables the check")
    args = ap.parse_args()

    if args.manifest:
        manifest, named = from_manifest(args.manifest)
        print("manifest digest %s, %d PDF files" % (manifest["manifest_digest"], len(named)))
        work = [(corpus, None, [(path, row)]) for corpus, path, row in named]
    else:
        work = []
        for corpus, (repo, sub) in CORPORA.items():
            root = os.path.join(args.root, repo)
            if not os.path.isdir(root):
                print("missing checkout: %s (see the docstring)" % root, file=sys.stderr)
                continue
            for path in sorted(glob.glob(os.path.join(root, sub, "**", "*.pdf"), recursive=True)):
                work.append((corpus, path, [(path, None)]))

    scratch = "/tmp/pdf-measure-check.out"
    outdir = "/tmp/pdf-measure-out"
    render_path = args.render_out or (args.out + ".render-sample.jsonl")
    os.makedirs(outdir, exist_ok=True)
    rows, n = [], 0
    for corpus, _single, batch in work:
        for path, recorded in batch:
            path = path if os.path.isabs(path) else os.path.join(args.root, path)
            if recorded is not None and os.path.getsize(path) != recorded["bytes"]:
                raise SystemExit(
                    "%s is %d bytes, the manifest says %d — run measure-fetch.py --verify"
                    % (path, os.path.getsize(path), recorded["bytes"])
                )
            n += 1
            size = os.path.getsize(path)
            tag = "%s_%05d" % (corpus, n)
            status, reason, info = classify(path, scratch)
            row = {"corpus": corpus, "path": path, "bytes": size,
                   "status": status, "reason": reason, **info}
            if recorded is not None:
                row["manifest_sha256"] = recorded["sha256"]
                row["manifest_path"] = recorded["path"]
            if status == "accepted":
                nstr, total, after = faithful(path)
                cand, repack = qpdf_jobs(path, outdir, tag)
                row.update(streams=nstr, flate_bytes=total, flate_after=after,
                           faithful_pct=100.0 * (total - after) / size if size else 0.0,
                           bytes_allowed=os.path.getsize(cand),
                           bytes_repack=os.path.getsize(repack),
                           qpdf_pct=100.0 * (size - os.path.getsize(cand)) / size if size else 0.0,
                           repack_pct=100.0 * (size - os.path.getsize(repack)) / size if size else 0.0,
                           payloads_equal=payloads(path) == payloads(cand),
                           check_ok=qpdf_check(cand, scratch) == 0)
            rows.append(row)
            if n % 200 == 0:
                print(n, flush=True)

    with open(args.out, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")

    acc = [r for r in rows if r["status"] == "accepted"]
    print("\nfiles %d" % len(rows), Counter(r["status"] for r in rows))
    print("refusals", Counter(r["reason"] for r in rows if r["status"] == "refused").most_common())
    if acc:
        for label, key in (("flate2 level 9", "faithful_pct"),
                           ("qpdf --recompress-flate", "qpdf_pct"),
                           ("forbidden repack", "repack_pct")):
            p = sorted(r[key] for r in acc)
            print("%-22s median %6.2f  q1 %6.2f  q3 %6.2f  share>0 %3.0f%%"
                  % (label, p[len(p) // 2], p[len(p) // 4], p[3 * len(p) // 4],
                     100 * sum(1 for x in p if x > 0) / len(p)))
        print("qpdf --check passed: %d/%d" % (sum(1 for r in acc if r["check_ok"]), len(acc)))

    render_rows = []
    if args.render_sample and acc:
        render_seed = 57
        sample = select_render_sample(acc, args.render_sample, render_seed)
        compared = pages = bad = errors = 0
        for order, r in enumerate(sample, start=1):
            cand = os.path.join(outdir, "render.pdf")
            record = dict(
                sample_order=order,
                sample_seed=render_seed,
                sample_size=len(sample),
                corpus=r["corpus"],
                path=r["path"],
                relative_path=r.get("manifest_path") or os.path.relpath(r["path"], args.root),
                bytes=r["bytes"],
                manifest_sha256=r.get("manifest_sha256"),
                status=r["status"],
                dpi=100,
                page_cap=6,
                rendered=False,
                render_error=None,
            )
            try:
                pikepdf.Job(["--recompress-flate", "--compression-level=9",
                             "--object-streams=preserve", r["path"], cand]).run()
                a, na = render_digests(r["path"])
                b, nb = render_digests(cand)
            except Exception as error:               # noqa: BLE001
                record["render_error"] = "%s: %s" % (type(error).__name__, error)
                render_rows.append(record)
                errors += 1
                print("render error:", r["path"], record["render_error"])
                continue
            record.update(
                rendered=True,
                original_pages=na,
                candidate_pages=nb,
                original_page_digests=a,
                candidate_page_digests=b,
                compared_pages=len(a),
                identical=(a == b and na == nb),
            )
            compared += 1
            pages += len(a)
            if not record["identical"]:
                bad += 1
                print("render difference:", r["path"])
            render_rows.append(record)
        print("rendered %d files, %d pages, %d differences" % (compared, pages, bad))
        if errors:
            print("render errors %d/%d sampled files" % (errors, len(sample)))

    os.makedirs(os.path.dirname(os.path.abspath(render_path)), exist_ok=True)
    with open(render_path, "w", encoding="utf-8") as handle:
        for record in render_rows:
            handle.write(json.dumps(record, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
