#!/usr/bin/env python3
"""Regression test: pinned funpack -C must retain a literal terminal blank FITS card."""

from __future__ import annotations

import argparse
import importlib.util
import subprocess
import tempfile
from pathlib import Path

import numpy as np
from astropy.io import fits

ROOT = Path(__file__).resolve().parents[2]
BLANK_CARD = b" " * 80


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def raw_header_cards(path: Path, hdu_index: int) -> list[bytes]:
    with fits.open(path, disable_image_compression=True, memmap=False) as hdus:
        hdu = hdus[hdu_index]
        start = int(hdu._header_offset)
        end = int(hdu._data_offset)
    raw = path.read_bytes()[start:end]
    if len(raw) % 80:
        raise RuntimeError(f"{path.name} HDU {hdu_index} header is not card-aligned")
    cards = [raw[offset : offset + 80] for offset in range(0, len(raw), 80)]
    end_index = next(
        (index for index, card in enumerate(cards) if card[:8] == b"END     "),
        None,
    )
    if end_index is None:
        raise RuntimeError(f"{path.name} HDU {hdu_index} has no literal END card")
    return cards[: end_index + 1]


def insert_terminal_blank_card(path: Path, hdu_index: int) -> None:
    with fits.open(path, disable_image_compression=True, memmap=False) as hdus:
        hdu = hdus[hdu_index]
        start = int(hdu._header_offset)
        data_start = int(hdu._data_offset)

    raw = bytearray(path.read_bytes())
    header = bytearray(raw[start:data_start])
    cards = [header[offset : offset + 80] for offset in range(0, len(header), 80)]
    end_index = next(
        (index for index, card in enumerate(cards) if card[:8] == b"END     "),
        None,
    )
    if end_index is None or end_index + 1 >= len(cards):
        raise RuntimeError("synthetic source has no reserved header card for the regression case")
    if cards[end_index + 1] != BLANK_CARD:
        raise RuntimeError("expected FITS header padding after END before inserting the test card")

    # Move END one literal card later without moving the data unit or changing
    # its padding. This creates the exact source condition that CFITSIO dropped.
    header[end_index * 80 : (end_index + 1) * 80] = BLANK_CARD
    header[(end_index + 1) * 80 : (end_index + 2) * 80] = b"END".ljust(80, b" ")
    raw[start:data_start] = header
    path.write_bytes(raw)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--funpack", required=True, type=Path)
    args = parser.parse_args()

    fits_measure = load_module("cfitsio_blank_test_fits_measure", ROOT / ".github/scripts/fits-measure.py")
    fixed_measure = load_module(
        "cfitsio_blank_test_fixed_measure", ROOT / ".github/scripts/fixed-fits-measure.py"
    )

    with tempfile.TemporaryDirectory(prefix="tzim-cfitsio-terminal-blank-") as temp_dir:
        work = Path(temp_dir)
        source = work / "source.fits"
        candidate = work / "candidate.fits"
        reconstructed = work / "reconstructed.fits"

        header = fits.Header()
        header["OBJECT"] = ("terminal blank regression", "literal-header test")
        image = np.arange(20, dtype=np.int16).reshape(4, 5)
        fits.HDUList([fits.PrimaryHDU(), fits.ImageHDU(data=image, header=header)]).writeto(source)
        insert_terminal_blank_card(source, 1)

        source_cards = raw_header_cards(source, 1)
        if len(source_cards) < 2 or source_cards[-2] != BLANK_CARD:
            raise RuntimeError("the synthetic source does not contain one terminal all-space card")

        image_count, _mapping = fits_measure.rebuild(source, str(candidate))
        if image_count != 1:
            raise RuntimeError(f"expected one compressed image in regression candidate, got {image_count}")
        candidate_cards = raw_header_cards(candidate, 1)
        if len(candidate_cards) < 2 or candidate_cards[-2] != BLANK_CARD:
            raise RuntimeError("Astropy reference candidate did not retain the literal blank card")

        completed = subprocess.run(
            [str(args.funpack), "-C", "-O", str(reconstructed), str(candidate)],
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            raise RuntimeError(
                "patched funpack -C failed on the synthetic regression candidate: "
                f"exit={completed.returncode}; stderr={completed.stderr[-2000:]}"
            )

        comparison = fixed_measure.compare_reference_funpack(source, reconstructed)
        if comparison.get("ok") is not True:
            raise RuntimeError(
                "patched funpack -C did not pass the unchanged literal-header/storage-payload gate: "
                f"{comparison.get('failure_class_counts')}; "
                f"mismatches={comparison.get('mismatch_records', [])[:3]}"
            )
        for check in comparison.get("hdu_checks", []):
            if check.get("header_cards_through_end_equality") is not True:
                raise RuntimeError("regression comparator did not verify every literal header card through END")
            if check.get("storage_payload_equality") is not True:
                raise RuntimeError("regression comparator did not verify exact storage-payload equality")

    print("PASS: funpack -C preserves the terminal literal FITS card; exact header and storage payload match")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
