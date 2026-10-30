#!/usr/bin/env python3
"""Fetch and freeze the exact 14 full-size #59 survey products from comment 6498702204."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import platform
import shutil
import sys
import tempfile
import urllib.request
from pathlib import Path

PRODUCTS = [
    {
        "id": "tess-s1-c1-ccd1-calibrated",
        "mission": "TESS",
        "source_uri": "https://archive.stsci.edu/missions/tess/ffi/s0001/2018/206/1-1/tess2018206192942-s0001-1-1-0120-s_ffic.fits",
        "retrieval_url": "https://archive.stsci.edu/missions/tess/ffi/s0001/2018/206/1-1/tess2018206192942-s0001-1-1-0120-s_ffic.fits",
        "product": "Sector 1 camera 1 CCD 1 calibrated full-frame image",
        "rights_basis": "MAST Open Data registry: TESS mission data public domain; attribution required for refereed scientific papers.",
    },
    {
        "id": "tess-s1-c1-ccd1-uncalibrated",
        "mission": "TESS",
        "source_uri": "https://archive.stsci.edu/missions/tess/ffi/s0001/2018/206/1-1/tess2018206192942-s0001-1-1-0120-s_ffir.fits",
        "retrieval_url": "https://archive.stsci.edu/missions/tess/ffi/s0001/2018/206/1-1/tess2018206192942-s0001-1-1-0120-s_ffir.fits",
        "product": "Sector 1 camera 1 CCD 1 uncalibrated full-frame image",
        "rights_basis": "MAST Open Data registry: TESS mission data public domain; attribution required for refereed scientific papers.",
    },
    {
        "id": "tess-s1-c4-ccd2-calibrated",
        "mission": "TESS",
        "source_uri": "https://archive.stsci.edu/missions/tess/ffi/s0001/2018/206/4-2/tess2018206192942-s0001-4-2-0120-s_ffic.fits",
        "retrieval_url": "https://archive.stsci.edu/missions/tess/ffi/s0001/2018/206/4-2/tess2018206192942-s0001-4-2-0120-s_ffic.fits",
        "product": "Sector 1 camera 4 CCD 2 calibrated full-frame image",
        "rights_basis": "MAST Open Data registry: TESS mission data public domain; attribution required for refereed scientific papers.",
    },
    {
        "id": "tess-s1-c4-ccd2-uncalibrated",
        "mission": "TESS",
        "source_uri": "https://archive.stsci.edu/missions/tess/ffi/s0001/2018/206/4-2/tess2018206192942-s0001-4-2-0120-s_ffir.fits",
        "retrieval_url": "https://archive.stsci.edu/missions/tess/ffi/s0001/2018/206/4-2/tess2018206192942-s0001-4-2-0120-s_ffir.fits",
        "product": "Sector 1 camera 4 CCD 2 uncalibrated full-frame image",
        "rights_basis": "MAST Open Data registry: TESS mission data public domain; attribution required for refereed scientific papers.",
    },
]

GALEX_KEYS = [
    "s3://stpubdata/galex/GR6/pipe/01-vsn/03329-MISDR1_18916_0459/d/01-main/0001-img/07-try/MISDR1_18916_0459-fd-int.fits.gz",
    "s3://stpubdata/galex/GR6/pipe/01-vsn/03329-MISDR1_18916_0459/d/01-main/0001-img/07-try/MISDR1_18916_0459-nd-int.fits.gz",
    "s3://stpubdata/galex/GR6/pipe/01-vsn/17464-MISWZS03_18917_0284/d/01-main/0001-img/07-try/MISWZS03_18917_0284-fd-int.fits.gz",
    "s3://stpubdata/galex/GR6/pipe/01-vsn/17464-MISWZS03_18917_0284/d/01-main/0001-img/07-try/MISWZS03_18917_0284-nd-int.fits.gz",
]
for index, key in enumerate(GALEX_KEYS, start=1):
    leaf = key.rsplit("/", 1)[-1]
    PRODUCTS.append({
        "id": f"galex-tile-{index:02d}-{leaf.removesuffix('.fits.gz')}",
        "mission": "GALEX",
        "source_uri": key,
        "retrieval_url": "https://stpubdata.s3.amazonaws.com/" + key.split("/", 3)[3],
        "product": f"Complete GALEX MIS tile intensity map band {leaf.rsplit('-', 2)[-2]}; no spatial cutout",
        "rights_basis": "MAST Open Data registry: GALEX mission data public domain; attribution required for refereed scientific papers.",
        "transport": "gzip",
    })

for product_id, filename in (
    ("rosat-rass-c", "rass_c.fits"),
    ("rosat-rass-m", "rass_m.fits"),
    ("rosat-rass-ij", "rass_ij.fits"),
    ("rosat-rass-exp-c", "rass_exp_c.fits"),
    ("rosat-rass-exp-m", "rass_exp_m.fits"),
    ("rosat-rass-exp-ij", "rass_exp_ij.fits"),
):
    url = f"https://heasarc.gsfc.nasa.gov/docs/rosat/survey/sxrb/{filename}"
    PRODUCTS.append({
        "id": product_id,
        "mission": "ROSAT",
        "source_uri": url,
        "retrieval_url": url,
        "product": "Complete 480 x 240 Hammer-Aitoff ROSAT XRT/PSPC all-sky map",
        "rights_basis": "Scientific-use permission with citation to Snowden et al., ApJ 454 (1995), 643–653; not public-domain-labelled. Do not bundle ROSAT bytes.",
    })

assert len(PRODUCTS) == 14


class RedirectRecorder(urllib.request.HTTPRedirectHandler):
    def __init__(self):
        super().__init__()
        self.redirects = []

    def redirect_request(self, request, response, code, message, headers, new_url):
        self.redirects.append({"from": request.full_url, "to": new_url, "status": code, "message": message})
        return super().redirect_request(request, response, code, message, headers, new_url)


def file_digest(path: Path) -> tuple[int, str]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while chunk := stream.read(8 * 1024 * 1024):
            size += len(chunk)
            digest.update(chunk)
    return size, digest.hexdigest()


def stream_download(url: str, destination: Path, user_agent: str) -> dict:
    recorder = RedirectRecorder()
    opener = urllib.request.build_opener(recorder)
    request = urllib.request.Request(url, headers={"User-Agent": user_agent, "Accept-Encoding": "identity"})
    digest = hashlib.sha256()
    size = 0
    destination.parent.mkdir(parents=True, exist_ok=True)
    with opener.open(request, timeout=120) as response, destination.open("wb") as output:
        while True:
            chunk = response.read(8 * 1024 * 1024)
            if not chunk:
                break
            output.write(chunk)
            digest.update(chunk)
            size += len(chunk)
        return {
            "requested_url": url,
            "final_url": response.geturl(),
            "http_status": response.status,
            "response_headers": {
                key: response.headers.get(key)
                for key in ("Content-Type", "Content-Length", "Last-Modified", "ETag", "Accept-Ranges")
            },
            "redirects": recorder.redirects,
            "bytes": size,
            "sha256": digest.hexdigest(),
            "user_agent": user_agent,
        }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", required=True)
    parser.add_argument("--output-manifest", required=True)
    parser.add_argument("--expected-source-sha", required=True)
    args = parser.parse_args()
    work = Path(args.work).resolve()
    downloads = work / "downloads"
    inputs = work / "inputs"
    downloads.mkdir(parents=True, exist_ok=True)
    inputs.mkdir(parents=True, exist_ok=True)
    user_agent = f"tzim-fixed-59/{args.expected_source_sha} Python-urllib/{platform.python_version()}"
    products = []
    usable = []
    for item in PRODUCTS:
        row = dict(item)
        row.update(fetch_status="failed", requested_url=item["retrieval_url"], fetch_tool="Python urllib.request / HTTPS GET", user_agent=user_agent)
        fits_path = inputs / (item["id"] + ".fits")
        # Uncompressed products are downloaded directly to their measurement
        # location. Only GALEX's transport-compressed bytes need a separate
        # temporary archive, which is deleted after the verified decompression.
        raw_path = (
            downloads / (item["id"] + ".fits.gz")
            if item.get("transport") == "gzip"
            else fits_path
        )
        try:
            retrieval = stream_download(item["retrieval_url"], raw_path, user_agent)
            row["retrieval"] = retrieval
            row["source_bytes"] = retrieval["bytes"]
            row["source_sha256"] = retrieval["sha256"]
            if item.get("transport") == "gzip":
                digest = hashlib.sha256()
                decompressed_size = 0
                with gzip.open(raw_path, "rb") as source, fits_path.open("wb") as output:
                    while True:
                        chunk = source.read(8 * 1024 * 1024)
                        if not chunk:
                            break
                        output.write(chunk)
                        digest.update(chunk)
                        decompressed_size += len(chunk)
                row["transport_decompression"] = {
                    "operation": "single gzip transport decompression; FITS payload bytes otherwise unmodified",
                    "tool": f"Python {platform.python_version()} gzip module",
                    "archive_bytes": retrieval["bytes"],
                    "archive_sha256": retrieval["sha256"],
                    "fits_bytes": decompressed_size,
                    "fits_sha256": digest.hexdigest(),
                }
                raw_path.unlink(missing_ok=True)
            else:
                row["transport_decompression"] = {"operation": "none; source is uncompressed FITS"}
            size, sha = file_digest(fits_path)
            row.update(fetch_status="downloaded", measurement_path=f"inputs/{fits_path.name}", measurement_bytes=size, measurement_sha256=sha)
            usable.append({"path": f"inputs/{fits_path.name}", "bytes": size, "sha256": sha, "product_id": item["id"]})
        except Exception as error:
            row["fetch_error"] = f"{type(error).__name__}: {error}"
            row["needed_owner_action"] = f"Make this exact source retrievable or provide an authorized mirror for the same product; do not substitute another product: {item['source_uri']}"
            cleanup_errors = []
            for temporary in (raw_path, fits_path):
                try:
                    temporary.unlink(missing_ok=True)
                except OSError as cleanup_error:
                    cleanup_errors.append(
                        f"{temporary}: {type(cleanup_error).__name__}: {cleanup_error}"
                    )
            if cleanup_errors:
                row["temporary_cleanup_errors"] = cleanup_errors
        products.append(row)
        print(f"{row['fetch_status']}: {item['id']} — {row.get('measurement_bytes', row.get('fetch_error'))}", flush=True)

    # The full manifest preserves all 14 rows, including exact failed URLs/errors.
    manifest = {
        "schema": "tzim-fixed-fits-source-manifest-v1",
        "source_decision_comment": "6498702204",
        "manifest_frozen_before_gain": True,
        "repository_source_sha": args.expected_source_sha,
        "rights_and_scope": {
            "TESS": "MAST public-domain mission data; attribution required in refereed papers.",
            "GALEX": "MAST public-domain mission data; attribution required in refereed papers.",
            "ROSAT": "Scientific use with Snowden et al. ApJ 454 (1995), 643–653 citation; keep source/candidate FITS bytes out of redistributed artifacts.",
        },
        "fetch_environment": {"python": platform.python_version(), "platform": platform.platform(), "user_agent": user_agent},
        "product_count": len(products),
        "products": products,
    }
    target = Path(args.output_manifest)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    full_sha = hashlib.sha256(target.read_bytes()).hexdigest()
    target.with_suffix(target.suffix + ".sha256").write_text(f"{full_sha}  {target.name}\n", encoding="ascii")

    fits_manifest = {
        "manifest_digest": full_sha,
        "schema": "tzim-fixed-fits-audit-inputs-v1",
        "corpora": [{"name": "fixed-mast-rosat-products", "format": "fits", "entries": usable}],
    }
    fits_manifest_path = target.with_name("fits-measure-input-manifest.json")
    fits_manifest_path.write_text(json.dumps(fits_manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    fits_manifest_path.with_suffix(fits_manifest_path.suffix + ".sha256").write_text(
        f"{hashlib.sha256(fits_manifest_path.read_bytes()).hexdigest()}  {fits_manifest_path.name}\n", encoding="ascii"
    )
    failures = [row for row in products if row["fetch_status"] != "downloaded"]
    cleanup_failures = [row for row in products if row.get("temporary_cleanup_errors")]
    print(f"frozen all {len(products)} exact FITS product rows; downloaded {len(usable)}; fetch failures {len(failures)}")
    for row in cleanup_failures:
        print(
            f"temporary cleanup failed for {row['id']}: {row['temporary_cleanup_errors']}",
            file=sys.stderr,
        )
    return 1 if cleanup_failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
