#!/usr/bin/env python3
"""fits-audit §8: the measurement that admitted Tier 2 (issue #59).

This is the instrument for [docs/fits-audit.md](../docs/fits-audit.md#8-the-measurement-and-the-rule-fixed-in-advance).
It is not part of the build and not a dependency of the crate; it exists so the
numbers in the audit can be reproduced.

    python3 -m venv /tmp/v && /tmp/v/bin/pip install numpy astropy
    /tmp/v/bin/python .github/scripts/fits-measure.py --root /tmp/corpus --out /tmp/fits.jsonl

`--root` holds the checkouts; the corpus is fetched, never committed:

    git clone --depth 1 --filter=blob:none --sparse https://github.com/astropy/astropy.git
    git -C astropy sparse-checkout set astropy/io/fits/tests/data
    git clone --depth 1 --filter=blob:none --sparse https://github.com/heasarc/cfitsio.git
    git -C cfitsio sparse-checkout set testprog
    git clone --depth 1 --filter=blob:none --sparse https://github.com/spacetelescope/drizzlepac.git
    git -C drizzlepac sparse-checkout set tests drizzlepac/pars
    git clone --depth 1 --filter=blob:none --sparse https://github.com/astropy/reproject.git
    git -C reproject sparse-checkout set reproject

Pinned at the commits the audit quotes: astropy `dbbf2acd6dbb`, cfitsio
`113781d8aa44`, drizzlepac `635834581ea2`, reproject `9dddd419111a`.

`fpack` and `fitsverify` are not needed and were not available where this ran:
`astropy`'s `CompImageHDU` calls the same CFITSIO tile writer `fpack` calls, and
the round trip is checked by comparing every array bit for bit, which is stronger
than comparing `funpack`'s output. The audit states this substitution rather than
hiding it.

Two things the audit insists on, and this script honours:

* `GZIP_2` shuffles bytes in **decreasing** order of significance (convention §6.2),
  which is what CFITSIO implements;
* the tile payload is a **gzip** member (RFC 1952, CRC-32 and `ISIZE`), not zlib —
  so re-decompressing a tile here uses `gzip`, and a `zlib` prefix check would be
  wrong.
"""

import argparse
import glob
import gzip
import json
import os
from collections import Counter

import numpy as np
from astropy.io import fits

CORPORA = (
    ("astropy", "astropy", "astropy/io/fits/tests/data"),
    ("cfitsio", "cfitsio", ""),
    ("drizzlepac", "drizzlepac", ""),
    ("reproject", "reproject", ""),
)

#: Keywords the compressed form cannot carry back; skipped in the header comparison.
SKIP = {"", "EXTNAME", "SIMPLE", "XTENSION", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2",
        "NAXIS3", "NAXIS4", "PCOUNT", "GCOUNT", "EXTEND", "BSCALE", "BZERO", "TFIELDS",
        "CHECKSUM", "DATASUM", "COMMENT", "HISTORY", "ZIMAGE", "ZCMPTYPE", "ZBITPIX",
        "ZNAXIS", "ZNAXIS1", "ZNAXIS2", "ZNAXIS3", "ZTILE1", "ZTILE2", "ZNAME1",
        "ZVAL1", "THEAP"}


def eligible(hdu):
    """An uncompressed image HDU the handler would turn into a compressed table."""
    if not isinstance(hdu, (fits.PrimaryHDU, fits.ImageHDU)):
        return False
    if hdu.data is None or hdu.header.get("NAXIS", 0) < 1:
        return False
    return not hdu.header.get("GROUPS")


def rebuild(path, out):
    """The whole file, every eligible image HDU compressed the way fpack would.

    A compressed primary array needs an empty `PrimaryHDU` in front of it: a
    `CompImageHDU` is a BINTABLE and a BINTABLE cannot be the first HDU. The mapping
    from original to candidate index is returned rather than assumed."""
    with fits.open(path) as hdul:
        new, mapping = [], []
        for idx, h in enumerate(hdul):
            if eligible(h):
                data = np.asarray(h.data)
                ctype = "GZIP_1" if data.dtype.kind in "iu" else "GZIP_2"
                if idx == 0:
                    new.append(fits.PrimaryHDU())
                new.append(fits.CompImageHDU(data=data, header=h.header.copy(),
                                             compression_type=ctype, quantize_level=0))
                mapping.append((idx, len(new) - 1))
            elif h.data is not None:
                # A lazy copy still points into the source file, which is closed
                # before the write; the data has to be materialised first or the
                # writer silently drops the HDU.
                copy = h.copy()
                copy.data = np.array(h.data)
                new.append(copy)
            else:
                new.append(h.copy())
        fits.HDUList(new).writeto(out, overwrite=True)
        return len(mapping), mapping


def verify(path, out, mapping):
    """Every image array identical bit for bit, and every carried keyword equal."""
    with fits.open(path) as a, fits.open(out) as b:
        if len(b) < len(a):
            return False, "hdu_count"
        for orig, cand in mapping:
            ha, hb = a[orig], b[cand]
            da, db = np.asarray(ha.data), np.asarray(hb.data)
            # `equal_nan` because NaN is not equal to itself, and a science array
            # full of NaN must not look like a difference of content.
            if da.shape != db.shape or not np.array_equal(da, db, equal_nan=da.dtype.kind == "f"):
                return False, "array_%d" % orig
            for key, value in ha.header.items():
                if key in SKIP or key not in hb.header:
                    continue
                try:
                    if hb.header[key] != value:
                        return False, "keyword_%s" % key
                except Exception:               # noqa: BLE001
                    continue
    return True, "ok"


def tiles(path):
    """Every tile payload of every compressed HDU, as raw bytes."""
    out = []
    with fits.open(path, disable_image_compression=True) as hdul:
        for hdu in hdul:
            if not hdu.header.get("ZIMAGE"):
                continue
            names = [c for c in hdu.columns.names if "COMPRESSED_DATA" in c.upper()]
            if names:
                out.extend(bytes(bytearray(row)) for row in hdu.data[names[0]])
    return out


def recompress(payloads, level=9):
    """Tier 1: what re-deflating the existing gzip members would save."""
    old = new = 0
    for tile in payloads:
        old += len(tile)
        try:
            plain = gzip.decompress(tile)
        except OSError:
            return None
        new += min(len(tile), len(gzip.compress(plain, compresslevel=level, mtime=0)))
    return old, new


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", default="/tmp/corpus")
    ap.add_argument("--out", default="/tmp/fits.jsonl")
    args = ap.parse_args()

    rows, seen = [], set()
    for corpus, repo, sub in CORPORA:
        root = os.path.join(args.root, repo)
        if not os.path.isdir(root):
            print("missing checkout:", root)
            continue
        for path in sorted(glob.glob(os.path.join(root, sub, "**", "*.fit*"), recursive=True)):
            if not path.lower().endswith((".fits", ".fit", ".fts")) or path in seen:
                continue
            seen.add(path)
            size = os.path.getsize(path)
            row = {"corpus": corpus, "path": path, "bytes": size}
            try:
                with fits.open(path) as hdul:
                    n = sum(1 for h in hdul if eligible(h))
                    codecs = sorted({str(h.header.get("ZCMPTYPE", "")).strip().strip("'")
                                     for h in hdul if h.header.get("ZIMAGE")})
            except Exception:                   # noqa: BLE001
                rows.append({**row, "status": "refused", "reason": "unreadable"})
                continue
            if n == 0:
                rows.append({**row, "status": "refused",
                             "reason": "other_codec:" + ",".join(codecs) if codecs else "no_image"})
                continue

            out = "/tmp/fits-measure.fits"
            try:
                images, mapping = rebuild(path, out)
            except Exception as e:              # noqa: BLE001
                rows.append({**row, "status": "refused", "reason": "cannot_compress",
                             "err": str(e)[:80]})
                continue
            ok, why = verify(path, out, mapping)
            new_size = os.path.getsize(out)
            result = recompress(tiles(out))
            row.update(status="tier2", images=images, new_size=new_size,
                       pct=100.0 * (size - new_size) / size if size else 0.0,
                       exact=ok, why=why)
            if result:
                old, new = result
                row.update(tiles=len(tiles(out)), tile_bytes=old,
                           tier1_pct=100.0 * (old - new) / new_size if new_size else 0.0)
            rows.append(row)

    with open(args.out, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")

    t2 = [r for r in rows if r["status"] == "tier2"]
    ok = [r for r in t2 if r["exact"]]
    print("files %d" % len(rows), Counter(r["status"] for r in rows))
    print("refusals", Counter(r["reason"] for r in rows if r["status"] == "refused").most_common())
    if ok:
        p = sorted(r["pct"] for r in ok)
        print("tier2 n=%d median %.2f q1 %.2f q3 %.2f share>0 %.0f%% max %.2f"
              % (len(p), p[len(p) // 2], p[len(p) // 4], p[3 * len(p) // 4],
                 100 * sum(1 for x in p if x > 0) / len(p), p[-1]))
        print("share of corpus %.1f%%" % (100 * len(ok) / len(rows)))
        t1 = sorted(r["tier1_pct"] for r in ok if "tier1_pct" in r)
        if t1:
            print("tier1 n=%d median %.2f max %.2f" % (len(t1), t1[len(t1) // 2], t1[-1]))
    print("verification failures:", [(r["why"], os.path.basename(r["path"]))
                                     for r in t2 if not r["exact"]])


if __name__ == "__main__":
    main()
