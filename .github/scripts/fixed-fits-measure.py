#!/usr/bin/env python3
"""Run frozen #59 FITS products through tzim, fitsverify, funpack -C and exact array/header checks."""

from __future__ import annotations

import argparse
import difflib
import hashlib
import importlib.util
import json
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

astropy = None
try:
    import astropy
    import numpy as np
    from astropy.io import fits
    FITS_IO_IMPORT_ERROR = None
except Exception as error:
    astropy = None
    np = None
    fits = None
    FITS_IO_IMPORT_ERROR = f"{type(error).__name__}: {error}"

fits_measure = None

STRUCTURAL_CARDS = {
    "SIMPLE", "XTENSION", "BITPIX", "NAXIS", "PCOUNT", "GCOUNT", "EXTEND", "GROUPS",
    "TFIELDS", "THEAP", "ZIMAGE", "ZCMPTYPE", "ZBITPIX", "ZNAXIS", "ZTENSION",
}


def structural(keyword: str) -> bool:
    key = keyword.upper()
    return (
        key in STRUCTURAL_CARDS
        or key.startswith("NAXIS")
        or key.startswith("ZNAXIS")
        or key.startswith("ZTILE")
        or key.startswith("ZNAME")
        or key.startswith("ZVAL")
        or key.startswith("TTYPE")
        or key.startswith("TFORM")
        or key.startswith("TUNIT")
        or key.startswith("TSCAL")
        or key.startswith("TZERO")
    )


def load_fits_measure(path: Path):
    spec = importlib.util.spec_from_file_location("tzim_fits_measure", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import existing FITS measurement instrument {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_fixed_products(path: Path) -> list[dict]:
    spec = importlib.util.spec_from_file_location("tzim_fixed_fits_sources", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import the owner-fixed FITS source registry {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.PRODUCTS


def digest(path: Path) -> tuple[int, str]:
    h = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while chunk := stream.read(8 * 1024 * 1024):
            size += len(chunk)
            h.update(chunk)
    return size, h.hexdigest()


def run(argv: list[str], log: list[dict], timeout: int = 1800, cwd: Path | None = None) -> dict:
    try:
        result = subprocess.run(
            argv,
            cwd=str(cwd) if cwd is not None else None,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
        row = {"argv": argv, "returncode": result.returncode, "stdout": result.stdout, "stderr": result.stderr}
        if cwd is not None:
            row["cwd"] = str(cwd)
    except Exception as error:
        row = {"argv": argv, "error": f"{type(error).__name__}: {error}"}
        if cwd is not None:
            row["cwd"] = str(cwd)
    log.append(row)
    return row


def image_bytes(hdu) -> bytes | None:
    if hdu.data is None:
        return None
    array = np.asarray(hdu.data)
    if array.dtype.fields is not None or array.dtype.kind == "O":
        return None
    # FITS images are stored big-endian. Normalize both reader paths to that
    # byte order so signed zero and NaN payload bits remain part of the check.
    dtype = array.dtype.newbyteorder(">")
    normalized = array.astype(dtype, copy=False)
    return normalized.tobytes(order="C")


def user_cards(hdu) -> list[str]:
    """All non-structural cards, including CHECKSUM/DATASUM, in literal order."""
    return [card.image for card in hdu.header.cards if not structural(card.keyword)]


def raw_hdu(path: Path, hdu) -> bytes:
    start = int(hdu._header_offset)
    data_start = int(hdu._data_offset if hdu._data_offset is not None else start + len(hdu.header.tostring().encode("ascii")))
    data_size = int(hdu._data_size or 0)
    padded = ((data_size + 2879) // 2880) * 2880
    end = data_start + padded
    with path.open("rb") as stream:
        stream.seek(start)
        return stream.read(end - start)


def raw_header(path: Path, hdu) -> bytes:
    """Return the original FITS header record bytes without Astropy normalization."""
    start = int(hdu._header_offset)
    header_end = int(
        hdu._data_offset
        if hdu._data_offset is not None
        else start + len(hdu.header.tostring().encode("ascii"))
    )
    with path.open("rb") as stream:
        stream.seek(start)
        return stream.read(header_end - start)


def raw_data(path: Path, hdu) -> bytes:
    data_start = int(hdu._data_offset if hdu._data_offset is not None else hdu._header_offset + len(hdu.header.tostring().encode("ascii")))
    data_size = int(hdu._data_size or 0)
    padded = ((data_size + 2879) // 2880) * 2880
    with path.open("rb") as stream:
        stream.seek(data_start)
        return stream.read(padded)


def first_byte_difference(before: bytes, after: bytes) -> int | None:
    """Return the first unequal byte offset, including a length-only suffix."""
    for offset, (left, right) in enumerate(zip(before, after)):
        if left != right:
            return offset
    return min(len(before), len(after)) if len(before) != len(after) else None


def _card_keyword(card: bytes | None) -> str | None:
    if card is None:
        return None
    return card[:8].decode("ascii", errors="replace").strip(" ")


def _header_cards_and_padding(raw_header: bytes) -> tuple[list[bytes], bytes, bool]:
    """Return literal 80-byte cards through actual END and only the trailing padding.

    No card is decoded, stripped or normalized for equality. In particular, a
    genuinely blank card before END remains an ordinary member of the sequence.
    FITS header record padding starts immediately after the complete END card.
    """
    cards = []
    for start in range(0, len(raw_header), 80):
        card = raw_header[start : start + 80]
        if len(card) != 80:
            return cards, b"", False
        cards.append(card)
        if card[:8] == b"END     ":
            return cards, raw_header[start + 80 :], True
    return cards, b"", False


def _header_card_differences(
    original_cards: list[bytes],
    reconstructed_cards: list[bytes],
    original_header_file_offset: int,
    reconstructed_header_file_offset: int,
    hdu_index: int,
) -> list[dict]:
    """Sequence-align literal cards and retain each exact inserted/dropped/changed card.

    Exact 80-byte equality drives alignment. The comparison does not key by
    keyword or parsed value, so comments, spacing, duplicates, order, CHECKSUM,
    DATASUM, blank cards and END all remain significant.
    """
    matcher = difflib.SequenceMatcher(
        a=original_cards,
        b=reconstructed_cards,
        autojunk=False,
    )
    differences = []

    def add(kind: str, original_index: int | None, reconstructed_index: int | None,
            before: bytes | None, after: bytes | None, original_boundary: int,
            reconstructed_boundary: int) -> None:
        if kind == "changed" and before == after:
            return
        within_card = first_byte_difference(before, after) if kind == "changed" else 0
        if within_card is None:
            raise AssertionError("a changed card must have a differing byte")
        original_header_byte_offset = (
            original_index * 80 + within_card if original_index is not None else original_boundary * 80
        )
        reconstructed_header_byte_offset = (
            reconstructed_index * 80 + within_card
            if reconstructed_index is not None else reconstructed_boundary * 80
        )
        original_keyword = _card_keyword(before)
        reconstructed_keyword = _card_keyword(after)
        if original_keyword == reconstructed_keyword:
            keyword = original_keyword or "<blank>"
        elif original_keyword is None:
            keyword = reconstructed_keyword or "<blank>"
        elif reconstructed_keyword is None:
            keyword = original_keyword or "<blank>"
        else:
            keyword = f"{original_keyword} -> {reconstructed_keyword}"
        card_index = original_index if original_index is not None else reconstructed_index
        differences.append({
            "hdu_index": hdu_index,
            "card_index": card_index,
            "original_card_index": original_index,
            "reconstructed_card_index": reconstructed_index,
            "keyword": keyword,
            "mismatch_kind": "header_card",
            "kind": kind,
            "original_keyword": original_keyword,
            "reconstructed_keyword": reconstructed_keyword,
            "first_differing_byte_offset": within_card,
            "first_differing_header_byte_offset": (
                original_header_byte_offset if original_index is not None
                else reconstructed_header_byte_offset
            ),
            "first_differing_original_header_byte_offset": original_header_byte_offset,
            "first_differing_reconstructed_header_byte_offset": reconstructed_header_byte_offset,
            # For an insertion/deletion, the missing-side file offset denotes
            # the sequence boundary where the card belongs; card-file offsets
            # remain null because that side has no corresponding card.
            "original_file_offset": original_header_file_offset + original_header_byte_offset,
            "reconstructed_file_offset": reconstructed_header_file_offset + reconstructed_header_byte_offset,
            "original_card_file_offset": (
                original_header_file_offset + original_index * 80 if original_index is not None else None
            ),
            "reconstructed_card_file_offset": (
                reconstructed_header_file_offset + reconstructed_index * 80
                if reconstructed_index is not None else None
            ),
            # Keep both exact 80-column text and hex. Hex retains trailing
            # spaces and unexpected bytes without lossy rendering.
            "before_card": before.decode("ascii", errors="replace") if before is not None else None,
            "after_card": after.decode("ascii", errors="replace") if after is not None else None,
            "before_card_hex": before.hex() if before is not None else None,
            "after_card_hex": after.hex() if after is not None else None,
            "before_card_bytes": len(before) if before is not None else 0,
            "after_card_bytes": len(after) if after is not None else 0,
        })

    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag == "equal":
            continue
        if tag == "replace":
            paired = min(i2 - i1, j2 - j1)
            for offset in range(paired):
                add("changed", i1 + offset, j1 + offset,
                    original_cards[i1 + offset], reconstructed_cards[j1 + offset],
                    i1 + offset, j1 + offset)
            for offset in range(paired, i2 - i1):
                add("dropped", i1 + offset, None, original_cards[i1 + offset], None,
                    i1 + offset, j2)
            for offset in range(paired, j2 - j1):
                add("inserted", None, j1 + offset, None, reconstructed_cards[j1 + offset],
                    i2, j1 + offset)
        elif tag == "delete":
            for index in range(i1, i2):
                add("dropped", index, None, original_cards[index], None, index, j1)
        elif tag == "insert":
            for index in range(j1, j2):
                add("inserted", None, index, None, reconstructed_cards[index], i1, index)
    return differences


def _payload_comparison(
    original_path: Path,
    original_hdu,
    reconstructed_path: Path,
    reconstructed_hdu,
    hdu_index: int,
) -> dict:
    original_image = image_bytes(original_hdu)
    reconstructed_image = image_bytes(reconstructed_hdu)
    original_data_offset = int(original_hdu._data_offset or 0)
    reconstructed_data_offset = int(reconstructed_hdu._data_offset or 0)
    original_shape = tuple(np.asarray(original_hdu.data).shape) if original_image is not None else None
    reconstructed_shape = (
        tuple(np.asarray(reconstructed_hdu.data).shape) if reconstructed_image is not None else None
    )
    if original_image is None and reconstructed_image is None:
        original_payload = raw_data(original_path, original_hdu)
        reconstructed_payload = raw_data(reconstructed_path, reconstructed_hdu)
        payload_kind = "non-image-HDU-data-and-padding"
        shape_equal = None
        type_compatible = type(original_hdu).__name__ == type(reconstructed_hdu).__name__
    elif original_image is not None and reconstructed_image is not None:
        original_payload = original_image
        reconstructed_payload = reconstructed_image
        payload_kind = "FITS image storage-array bytes (big-endian, row-major)"
        shape_equal = original_shape == reconstructed_shape
        type_compatible = True
    else:
        original_payload = original_image if original_image is not None else raw_data(original_path, original_hdu)
        reconstructed_payload = (
            reconstructed_image if reconstructed_image is not None else raw_data(reconstructed_path, reconstructed_hdu)
        )
        payload_kind = "image/non-image HDU type mismatch"
        shape_equal = False
        type_compatible = False

    byte_equal = original_payload == reconstructed_payload
    first_difference = (
        None if byte_equal else first_byte_difference(original_payload, reconstructed_payload)
    )
    storage_equal = byte_equal and (shape_equal is not False) and type_compatible
    detail = {
        "hdu_index": hdu_index,
        "payload_kind": payload_kind,
        "original_payload_bytes": len(original_payload),
        "reconstructed_payload_bytes": len(reconstructed_payload),
        "payload_bytes_equal": byte_equal,
        "storage_array_shape_equal": shape_equal,
        "storage_payload_equal": storage_equal,
        "original_shape": list(original_shape) if original_shape is not None else None,
        "reconstructed_shape": list(reconstructed_shape) if reconstructed_shape is not None else None,
        "original_payload_sha256": hashlib.sha256(original_payload).hexdigest(),
        "reconstructed_payload_sha256": hashlib.sha256(reconstructed_payload).hexdigest(),
        "first_differing_byte_offset": first_difference,
        "difference_reason": (
            "storage shape/type differs although payload bytes are identical"
            if not storage_equal and byte_equal else None
        ),
        "original_file_offset": (
            original_data_offset + first_difference
            if first_difference is not None and first_difference < len(original_payload)
            else None
        ),
        "reconstructed_file_offset": (
            reconstructed_data_offset + first_difference
            if first_difference is not None and first_difference < len(reconstructed_payload)
            else None
        ),
        "original_byte_hex": (
            original_payload[first_difference : first_difference + 1].hex()
            if first_difference is not None and first_difference < len(original_payload) else None
        ),
        "reconstructed_byte_hex": (
            reconstructed_payload[first_difference : first_difference + 1].hex()
            if first_difference is not None and first_difference < len(reconstructed_payload) else None
        ),
    }
    return detail


def compare_reference_funpack(original: Path, reconstructed: Path) -> dict:
    """Compare a `funpack -C` reconstruction to the exact uncompressed source.

    Header equality is the sequence of literal 80-byte cards from each HDU
    through its actual END card. Only record padding after END is excluded from
    the gate and reported separately. No blank-card, comment, spacing, order,
    duplicate, CHECKSUM or DATASUM normalization/filtering is applied. Image
    storage-array bytes remain exact; non-image HDUs retain full data-area and
    padding comparison. Diagnostics contain hashes and header cards, never FITS
    data payload bytes.
    """
    with fits.open(original, memmap=False, do_not_scale_image_data=True, uint=False) as before, \
         fits.open(reconstructed, memmap=False, do_not_scale_image_data=True, uint=False) as after:
        checks = []
        mismatch_records = []
        common_hdus = min(len(before), len(after))
        for index in range(common_hdus):
            original_hdu = before[index]
            reconstructed_hdu = after[index]
            original_header = raw_header(original, original_hdu)
            reconstructed_header = raw_header(reconstructed, reconstructed_hdu)
            original_header_offset = int(original_hdu._header_offset or 0)
            reconstructed_header_offset = int(reconstructed_hdu._header_offset or 0)
            original_cards, original_padding, original_end_found = _header_cards_and_padding(original_header)
            reconstructed_cards, reconstructed_padding, reconstructed_end_found = _header_cards_and_padding(
                reconstructed_header
            )
            header_differences = _header_card_differences(
                original_cards,
                reconstructed_cards,
                original_header_offset,
                reconstructed_header_offset,
                index,
            )
            header_cards_equal = (
                original_end_found
                and reconstructed_end_found
                and original_cards == reconstructed_cards
            )
            padding_equal = original_padding == reconstructed_padding
            padding_first_difference = (
                None if padding_equal else first_byte_difference(original_padding, reconstructed_padding)
            )
            original_padding_start = len(original_cards) * 80
            reconstructed_padding_start = len(reconstructed_cards) * 80
            padding_difference = {
                "equal": padding_equal,
                "original_padding_bytes": len(original_padding),
                "reconstructed_padding_bytes": len(reconstructed_padding),
                "original_padding_sha256": hashlib.sha256(original_padding).hexdigest(),
                "reconstructed_padding_sha256": hashlib.sha256(reconstructed_padding).hexdigest(),
                "first_differing_byte_offset": padding_first_difference,
                "original_header_byte_offset": (
                    original_padding_start + padding_first_difference
                    if padding_first_difference is not None else None
                ),
                "reconstructed_header_byte_offset": (
                    reconstructed_padding_start + padding_first_difference
                    if padding_first_difference is not None else None
                ),
                "original_file_offset": (
                    original_header_offset + original_padding_start + padding_first_difference
                    if padding_first_difference is not None
                    and padding_first_difference < len(original_padding) else None
                ),
                "reconstructed_file_offset": (
                    reconstructed_header_offset + reconstructed_padding_start + padding_first_difference
                    if padding_first_difference is not None
                    and padding_first_difference < len(reconstructed_padding) else None
                ),
                "original_byte_hex": (
                    original_padding[padding_first_difference : padding_first_difference + 1].hex()
                    if padding_first_difference is not None
                    and padding_first_difference < len(original_padding) else None
                ),
                "reconstructed_byte_hex": (
                    reconstructed_padding[padding_first_difference : padding_first_difference + 1].hex()
                    if padding_first_difference is not None
                    and padding_first_difference < len(reconstructed_padding) else None
                ),
            }
            card_kind_counts = Counter(item["kind"] for item in header_differences)
            header_card_difference_counts = {
                kind: card_kind_counts.get(kind, 0) for kind in ("inserted", "dropped", "changed")
            }
            keyword_counts: dict[str, dict[str, int]] = {}
            for kind in ("inserted", "dropped", "changed"):
                counts = Counter(
                    item["keyword"] for item in header_differences if item["kind"] == kind
                )
                keyword_counts[kind] = dict(sorted(counts.items()))

            header_integrity_errors = []
            if not original_end_found:
                header_integrity_errors.append({"side": "original", "reason": "actual END card not found"})
            if not reconstructed_end_found:
                header_integrity_errors.append({"side": "reconstructed", "reason": "actual END card not found"})
            for error in header_integrity_errors:
                mismatch_records.append({
                    "hdu_index": index,
                    "card_index": None,
                    "keyword": "END",
                    "mismatch_kind": "header_end",
                    **error,
                    "first_differing_byte_offset": None,
                })

            payload = _payload_comparison(
                original, original_hdu, reconstructed, reconstructed_hdu, index
            )
            hdu_type_equal = type(original_hdu).__name__ == type(reconstructed_hdu).__name__
            payload_equal = payload["storage_payload_equal"]
            padding_only_difference = header_cards_equal and not padding_equal
            if not header_cards_equal and not payload_equal:
                failure_class = "combined"
            elif not header_cards_equal:
                failure_class = "header_only"
            elif not payload_equal:
                failure_class = "payload_only"
            elif not hdu_type_equal:
                failure_class = "hdu_type_only"
            else:
                failure_class = "match"

            first_actual_difference = None
            if header_differences:
                first_actual_difference = dict(header_differences[0])
            elif header_integrity_errors:
                first_actual_difference = {
                    "kind": "header_integrity",
                    "mismatch_kind": "header_end",
                    **header_integrity_errors[0],
                }
            elif not padding_equal:
                first_actual_difference = {
                    "kind": "padding_only",
                    "mismatch_kind": "header_padding",
                    **padding_difference,
                }
            elif not payload_equal:
                first_actual_difference = {
                    "kind": "storage_payload",
                    "mismatch_kind": "storage_payload",
                    "first_differing_byte_offset": payload["first_differing_byte_offset"],
                    "original_file_offset": payload["original_file_offset"],
                    "reconstructed_file_offset": payload["reconstructed_file_offset"],
                    "original_byte_hex": payload["original_byte_hex"],
                    "reconstructed_byte_hex": payload["reconstructed_byte_hex"],
                }

            check = {
                "hdu_index": index,
                "original_hdu_type": type(original_hdu).__name__,
                "reconstructed_hdu_type": type(reconstructed_hdu).__name__,
                "hdu_type_equal": hdu_type_equal,
                "original_header_file_offset": original_header_offset,
                "reconstructed_header_file_offset": reconstructed_header_offset,
                "original_header_record_bytes": len(original_header),
                "reconstructed_header_record_bytes": len(reconstructed_header),
                "original_header_card_count_through_end": len(original_cards),
                "reconstructed_header_card_count_through_end": len(reconstructed_cards),
                "original_end_card_found": original_end_found,
                "reconstructed_end_card_found": reconstructed_end_found,
                "header_cards_through_end_equality": header_cards_equal,
                # Retain the old key as an alias, now explicitly card-through-END equality.
                "header_record_equality": header_cards_equal,
                "header_record_bytes_equality": original_header == reconstructed_header,
                "original_header_cards_through_end_sha256": hashlib.sha256(b"".join(original_cards)).hexdigest(),
                "reconstructed_header_cards_through_end_sha256": hashlib.sha256(
                    b"".join(reconstructed_cards)
                ).hexdigest(),
                "original_header_record_sha256": hashlib.sha256(original_header).hexdigest(),
                "reconstructed_header_record_sha256": hashlib.sha256(reconstructed_header).hexdigest(),
                "header_padding_equality": padding_equal,
                "header_padding_only_difference": padding_only_difference,
                "header_padding_difference": padding_difference,
                "header_integrity_errors": header_integrity_errors,
                "header_card_difference_counts": header_card_difference_counts,
                "header_keyword_difference_counts": keyword_counts,
                "header_card_differences": header_differences,
                "first_header_card_difference": dict(header_differences[0]) if header_differences else None,
                "first_actual_difference": first_actual_difference,
                "storage_payload_equality": payload_equal,
                "storage_payload": payload,
                "failure_class": failure_class,
            }
            checks.append(check)
            mismatch_records.extend(header_differences)
            if not payload_equal:
                mismatch_records.append({
                    "hdu_index": index,
                    "card_index": None,
                    "keyword": "<storage-payload>",
                    "mismatch_kind": "storage_payload",
                    "first_differing_byte_offset": payload["first_differing_byte_offset"],
                    "original_file_offset": payload["original_file_offset"],
                    "reconstructed_file_offset": payload["reconstructed_file_offset"],
                    "original_byte_hex": payload["original_byte_hex"],
                    "reconstructed_byte_hex": payload["reconstructed_byte_hex"],
                    "original_payload_bytes": payload["original_payload_bytes"],
                    "reconstructed_payload_bytes": payload["reconstructed_payload_bytes"],
                    "original_shape": payload["original_shape"],
                    "reconstructed_shape": payload["reconstructed_shape"],
                    "payload_kind": payload["payload_kind"],
                })
            if not hdu_type_equal:
                mismatch_records.append({
                    "hdu_index": index,
                    "card_index": None,
                    "keyword": "<HDU-type>",
                    "mismatch_kind": "hdu_type",
                    "original_hdu_type": type(original_hdu).__name__,
                    "reconstructed_hdu_type": type(reconstructed_hdu).__name__,
                    "first_differing_byte_offset": None,
                })

        for index in range(common_hdus, max(len(before), len(after))):
            original_hdu = before[index] if index < len(before) else None
            reconstructed_hdu = after[index] if index < len(after) else None
            mismatch_records.append({
                "hdu_index": index,
                "card_index": None,
                "keyword": "<HDU-count>",
                "mismatch_kind": "hdu_count",
                "original_hdu_type": type(original_hdu).__name__ if original_hdu is not None else None,
                "reconstructed_hdu_type": (
                    type(reconstructed_hdu).__name__ if reconstructed_hdu is not None else None
                ),
                "first_differing_byte_offset": None,
            })

        failure_counts = dict(sorted(Counter(
            item["failure_class"] for item in checks if item["failure_class"] != "match"
        ).items()))
        hdu_count_equal = len(before) == len(after)
        ok = hdu_count_equal and all(item["failure_class"] == "match" for item in checks)
        if ok:
            reason = "ok"
        elif not hdu_count_equal:
            reason = "hdu_count_mismatch"
        else:
            reason = "header cards through END, storage payload, HDU type or END-card integrity mismatch"
        return {
            "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v2",
            "diagnostics_complete": True,
            "ok": ok,
            "reason": reason,
            "comparison": (
                "literal 80-byte header-card sequences through actual END; record padding excluded from equality "
                "and reported separately; exact storage-array bytes; non-image data bytes and padding"
            ),
            "original_hdus": len(before),
            "reconstructed_hdus": len(after),
            "hdu_count_equal": hdu_count_equal,
            "failure_class_counts": failure_counts,
            "mismatch_records": mismatch_records,
            "hdu_checks": checks,
        }

def is_compressed_image_hdu(hdu) -> bool:
    """Recognize Astropy's logical CompImageHDU view as well as raw ZIMAGE tables.

    Astropy projects a compressed binary table into the original image-header
    view by default, so `hdu.header.get("ZIMAGE")` is absent even though the raw
    table has `ZIMAGE = T`. The class identity is the reliable signal here.
    """
    return isinstance(hdu, fits.CompImageHDU) or bool(hdu.header.get("ZIMAGE", False))


def compare_fits(
    original: Path,
    candidate: Path,
    compare_untransformed_raw: bool = True,
    allow_primary_filler: bool = False,
) -> dict:
    with fits.open(original, memmap=False, do_not_scale_image_data=True, uint=False) as before, \
         fits.open(candidate, memmap=False, do_not_scale_image_data=True, uint=False) as after:
        offset = 0
        offset_probe = None
        if len(before) and len(after) == len(before) + 1 and len(after) >= 2:
            first = after[0]
            second = after[1]
            primary_eligible = bool(fits_measure.eligible(before[0]) and not is_compressed_image_hdu(before[0]))
            has_filler = first.header.get("NAXIS", 0) == 0 and first.data is None
            second_is_compressed = is_compressed_image_hdu(second)
            offset_probe = {
                "primary_eligible": primary_eligible,
                "empty_primary_filler": has_filler,
                "second_hdu_is_compressed_image": second_is_compressed,
                "second_hdu_type": type(second).__name__,
            }
            if primary_eligible and has_filler and (second_is_compressed or allow_primary_filler):
                offset = 1
        if len(after) != len(before) + offset:
            result = {"ok": False, "reason": "hdu_count", "before_hdus": len(before), "after_hdus": len(after), "offset": offset}
            if offset_probe is not None:
                result["primary_offset_probe"] = offset_probe
            return result
        if offset:
            filler = after[0]
            if filler.header.get("SIMPLE") is not True or filler.header.get("NAXIS", 0) != 0 or filler.data is not None:
                return {"ok": False, "reason": "invalid-empty-primary-before-compressed-primary"}

        checks = []
        for index, original_hdu in enumerate(before):
            candidate_index = index + offset
            candidate_hdu = after[candidate_index]
            original_zimage = is_compressed_image_hdu(original_hdu)
            candidate_zimage = is_compressed_image_hdu(candidate_hdu)
            original_eligible = bool(
                fits_measure.eligible(original_hdu)
                and not original_zimage
            )
            transformed = original_eligible and candidate_zimage
            item = {"original_hdu": index, "candidate_hdu": candidate_index, "transformed": transformed}
            if transformed:
                left = image_bytes(original_hdu)
                right = image_bytes(candidate_hdu)
                if left is None or right is None or left != right:
                    item.update(ok=False, reason="image-array-byte-mismatch")
                elif user_cards(original_hdu) != user_cards(candidate_hdu):
                    item.update(ok=False, reason="image-user-header-card-mismatch")
                else:
                    item.update(ok=True, reason="array-bytes-and-user-header-cards-identical",
                                image_sha256=hashlib.sha256(left).hexdigest())
            else:
                # Every HDU not transformed must remain a complete byte-for-byte
                # copy, including non-image HDUs and original compressed tables.
                raw_equal = raw_hdu(original, original_hdu) == raw_hdu(candidate, candidate_hdu)
                left = image_bytes(original_hdu)
                right = image_bytes(candidate_hdu)
                if left is None and right is None:
                    arrays_equal = raw_data(original, original_hdu) == raw_data(candidate, candidate_hdu)
                else:
                    arrays_equal = left is not None and right is not None and left == right
                cards_equal = user_cards(original_hdu) == user_cards(candidate_hdu)
                exact = raw_equal if compare_untransformed_raw else (arrays_equal and cards_equal)
                item.update(ok=exact, reason="untransformed-HDU-byte-identical" if exact else "untransformed-HDU-changed",
                            raw_hdu_equal=raw_equal, arrays_equal=arrays_equal, user_header_cards_equal=cards_equal)
            checks.append(item)
        if offset:
            checks.append({"original_hdu": None, "candidate_hdu": 0, "transformed": False,
                           "ok": True, "reason": "required-empty-primary"})
        return {"ok": all(item.get("ok", False) for item in checks), "hdu_checks": checks,
                "original_hdus": len(before), "candidate_hdus": len(after), "primary_offset": offset}


def sample_tier1(original: Path, fits_measure) -> dict:
    try:
        payloads = fits_measure.tiles(str(original))
        if not payloads:
            return {"status": "not_applicable", "reason": "no existing compressed image tiles"}
        result = fits_measure.recompress(payloads)
        if result is None:
            return {"status": "refused", "reason": "existing tile is not a complete gzip member"}
        old, new = result
        return {
            "status": "measured_comparator",
            "tier": "Tier 1 existing compressed tiles, recompress-only estimate",
            "old_tile_bytes": old,
            "recompressed_tile_bytes": new,
            "gain_pct": 100.0 * (old - new) / old if old else 0.0,
            "parent_product_input_sha256": digest(original)[1],
            "derivation": "read tiles from this published source FITS; gzip-decompress and recompress each at level 9; no new published product and no source modification",
            "tool": "Python gzip, same function as .github/scripts/fits-measure.py",
        }
    except Exception as error:
        return {"status": "failed", "error": f"{type(error).__name__}: {error}"}


def distribution_stats(values: list[float]) -> dict:
    ordered = sorted(values)
    if not ordered:
        return {
            "n": 0,
            "status": "empty",
            "median": None,
            "q1_index": None,
            "q3_index": None,
            "share_positive": None,
        }
    return {
        "n": len(ordered),
        "status": "measured",
        "median": statistics.median(ordered),
        "q1_index": ordered[len(ordered) // 4],
        "q3_index": ordered[(3 * len(ordered)) // 4],
        "share_positive": sum(value > 0 for value in ordered) / len(ordered),
    }


def fitsverify_messages(check: dict, severity: str) -> list[str]:
    text = str(check.get("stdout", "")) + "\n" + str(check.get("stderr", ""))
    pattern = re.compile(rf"^\s*\*{{3}}\s*{re.escape(severity)}:\s*(.*?)\s*$", re.IGNORECASE)
    messages = []
    for line in text.splitlines():
        match = pattern.match(line)
        if match:
            message = re.sub(r"Keyword\s+#\d+\s*,", "Keyword #,", match.group(1), flags=re.IGNORECASE)
            messages.append(message)
    return messages


def reference_fitsverify_status(source_check: dict, candidate_check: dict) -> dict:
    """Require a clean candidate or only the exact warning-only source baseline."""
    source_rc = source_check.get("returncode")
    candidate_rc = candidate_check.get("returncode")
    source_warnings = fitsverify_messages(source_check, "Warning")
    candidate_warnings = fitsverify_messages(candidate_check, "Warning")
    source_errors = fitsverify_messages(source_check, "Error")
    candidate_errors = fitsverify_messages(candidate_check, "Error")
    if candidate_rc == 0 and not candidate_errors:
        status = "passed"
        reason = "fitsverify -l returned zero with no error diagnostics"
    elif (
        isinstance(candidate_rc, int)
        and candidate_rc > 0
        and candidate_rc == source_rc
        and candidate_rc == len(candidate_warnings)
        and candidate_warnings == source_warnings
        and not candidate_errors
        and not source_errors
    ):
        status = "passed_inherited_warnings"
        reason = "candidate has only the same warning diagnostics as the exact source; no new HEASARC diagnostics"
    else:
        status = "failed"
        reason = "candidate fitsverify result is non-zero or differs from the source warning-only baseline"
    return {
        "status": status,
        "reason": reason,
        "source_returncode": source_rc,
        "candidate_returncode": candidate_rc,
        "source_warning_count": len(source_warnings),
        "candidate_warning_count": len(candidate_warnings),
        "source_warning_messages": source_warnings,
        "candidate_warning_messages": candidate_warnings,
        "source_error_messages": source_errors,
        "candidate_error_messages": candidate_errors,
    }


def positive_control_passed(control: dict) -> bool:
    """Independently check every recorded component of the external control."""
    comparison = control.get("literal_header_storage_array_comparison") or {}
    funpack_argv = control.get("funpack_invocation") or []
    fitsverify_argv = control.get("fitsverify_invocation") or []
    command_results_ok = all(
        (control.get(name) or {}).get("returncode") == 0
        for name in ("fpack", "fitsverify", "funpack")
    )
    return (
        control.get("status") == "passed"
        and control.get("ok") is True
        and control.get("fixture_dtype") == "int16"
        and control.get("fpack_float_array_oracle_used") is False
        and command_results_ok
        and "-l" in fitsverify_argv
        and "-C" in funpack_argv
        and comparison.get("diagnostic_schema") == "tzim-funpack-roundtrip-diagnostics-v2"
        and comparison.get("diagnostics_complete") is True
        and comparison.get("ok") is True
    )


def run_funpack_positive_control(
    fpack: str,
    funpack: str,
    fitsverify: str,
    work: Path,
    logs: list[dict],
) -> dict:
    """Exercise the real CFITSIO fpack -> funpack -C -> literal comparison path.

    This deliberately uses an integer-only synthetic fixture. It checks that the
    external reader path can reproduce exact 80-column cards and storage bytes;
    fpack is not used as a float-array correctness oracle. Temporary FITS files
    are deleted and only their identities and comparison diagnostics are kept.
    """
    control = {
        "control_id": "cfitsio-fpack-funpack-C-integer-primary-v1",
        "control_type": "external_positive_control",
        "status": "failed",
        "expected": "fpack -> fitsverify -l -> funpack -C -> exact header-record and storage-array comparison",
        "fpack_float_array_oracle_used": False,
        "fixture_dtype": "int16",
        "funpack_checksum_mode": "-C; disable CFITSIO CHECKSUM/DATASUM updates",
        "literal_header_mode": "all 80-column header records including CHECKSUM/DATASUM and padding",
        "array_mode": "exact big-endian storage-array bytes and shape; integer fixture only",
    }
    if fits is None or np is None:
        control["reason"] = f"positive-control fixture dependencies unavailable: {FITS_IO_IMPORT_ERROR}"
        return control

    work.mkdir(parents=True, exist_ok=True)
    try:
        with tempfile.TemporaryDirectory(prefix="funpack-positive-control-", dir=work) as temporary_dir:
            temp = Path(temporary_dir)
            source = temp / "positive-control.fits"
            packed = Path(str(source) + ".fz")
            reconstructed = temp / "positive-control-reconstructed.fits"
            data = np.arange(64 * 96, dtype=np.int16).reshape(64, 96)
            primary = fits.PrimaryHDU(data=data)
            primary.header["OBJECT"] = "FUNPACK C POSITIVE CONTROL"
            primary.header.append(("HISTORY", "Integer-only external reader control"))
            primary.writeto(source, overwrite=True, checksum=False)
            source_bytes, source_sha = digest(source)
            control["source_identity"] = {"bytes": source_bytes, "sha256": source_sha}

            fpack_argv = [fpack, source.name]
            fpack_result = run(fpack_argv, logs, cwd=temp)
            control["fpack_invocation"] = fpack_argv
            control["fpack"] = fpack_result
            if packed.is_file():
                packed_bytes, packed_sha = digest(packed)
                control["fpack_output_identity"] = {"bytes": packed_bytes, "sha256": packed_sha}

            if packed.is_file():
                fitsverify_argv = [fitsverify, "-l", str(packed)]
                fitsverify_result = run(fitsverify_argv, logs)
                control["fitsverify_invocation"] = fitsverify_argv
                control["fitsverify"] = fitsverify_result

                funpack_argv = [funpack, "-C", "-O", str(reconstructed), str(packed)]
                funpack_result = run(funpack_argv, logs)
                control["funpack_invocation"] = funpack_argv
                control["funpack"] = {**funpack_result, "checksum_mode": "-C"}
                if reconstructed.is_file():
                    reconstructed_bytes, reconstructed_sha = digest(reconstructed)
                    control["reconstructed_identity"] = {
                        "bytes": reconstructed_bytes,
                        "sha256": reconstructed_sha,
                    }
                    comparison = compare_reference_funpack(source, reconstructed)
                    control["literal_header_storage_array_comparison"] = comparison
                else:
                    control["literal_header_storage_array_comparison"] = {
                        "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v2",
                        "diagnostics_complete": False,
                        "ok": False,
                        "reason": "funpack -C produced no reconstructed FITS output",
                        "failure_class_counts": {"funpack_failed": 1},
                        "mismatch_records": [],
                        "hdu_checks": [],
                    }
            else:
                control["fitsverify"] = {"status": "not_run", "reason": "fpack produced no candidate"}
                control["funpack"] = {"status": "not_run", "reason": "fpack produced no candidate"}
                control["literal_header_storage_array_comparison"] = {
                    "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v2",
                    "diagnostics_complete": False,
                    "ok": False,
                    "reason": "fpack produced no candidate",
                    "failure_class_counts": {"fpack_failed": 1},
                    "mismatch_records": [],
                    "hdu_checks": [],
                }

            control["ok"] = (
                fpack_result.get("returncode") == 0
                and packed.is_file()
                and control.get("fitsverify", {}).get("returncode") == 0
                and control.get("funpack", {}).get("returncode") == 0
                and reconstructed.is_file()
                and control.get("literal_header_storage_array_comparison", {}).get("ok") is True
            )
            control["status"] = "passed" if control["ok"] else "failed"
            control["reason"] = (
                "external CFITSIO positive control passed through the same funpack -C and exact comparison path"
                if control["ok"]
                else "external CFITSIO positive control did not pass; survey header/payload differences must not be attributed"
            )
    except Exception as error:
        control["ok"] = False
        control["status"] = "failed"
        control["reason"] = f"{type(error).__name__}: {error}"
        control["exception"] = control["reason"]
    return control


def validate_reference_candidate(
    source: Path,
    audit: dict,
    reference_candidate_dir: Path,
    work: Path,
    product_id: str,
    order: int,
    fitsverify: str,
    funpack: str,
    positive_control: dict,
    logs: list[dict],
) -> dict:
    """HEASARC-validate the exact Astropy candidate that produced a Tier-2 row."""
    reference = audit.get("reference_candidate") if isinstance(audit, dict) else None
    audit_status = audit.get("status") if isinstance(audit, dict) else "not_measured"
    if audit_status != "tier2" or not isinstance(reference, dict):
        return {
            "status": "not_applicable",
            "reason": f"no Tier-2 reference candidate (instrument status: {audit_status})",
        }

    positive_control_ok = positive_control_passed(positive_control)
    filename = reference.get("filename")
    result = {
        "status": "failed",
        "candidate_filename": filename,
        "registered_bytes": reference.get("bytes"),
        "registered_sha256": reference.get("sha256"),
        "astropy_verifier_exact": audit.get("exact"),
        "astropy_verifier_reason": audit.get("why"),
        "generation": reference.get("generation"),
        "eligible_images": reference.get("eligible_images"),
        "hdu_mapping": reference.get("mapping"),
        "fitsverify_invocation": "fitsverify -l <exact Astropy Tier-2 candidate>",
        "funpack_invocation": "funpack -C -O <temporary reconstructed FITS> <exact Astropy Tier-2 candidate>",
        "funpack_checksum_mode": "-C; prevent CFITSIO from synthesizing/updating CHECKSUM/DATASUM; compare those cards literally",
        "positive_control_status": positive_control.get("status", "missing"),
        "header_payload_attribution": (
            "enabled: external funpack -C positive control passed"
            if positive_control_ok
            else "withheld: external funpack -C positive control did not pass"
        ),
    }
    candidate = None
    unpacked = work / f"{order:02d}-{product_id}.reference-unpacked.fits"
    cleanup_errors = []
    try:
        if not isinstance(filename, str) or Path(filename).name != filename:
            result["reason"] = "missing or unsafe reference candidate filename"
            return result
        candidate = reference_candidate_dir / filename
        if not candidate.is_file():
            result["reason"] = "the exact reference candidate recorded by fits-measure.py is missing"
            return result
        candidate_bytes, candidate_sha = digest(candidate)
        identity_matches = candidate_bytes == reference.get("bytes") and candidate_sha == reference.get("sha256")
        result.update(
            observed_bytes=candidate_bytes,
            observed_sha256=candidate_sha,
            identity_matches_measurement_candidate=identity_matches,
        )

        source_check = run([fitsverify, "-l", str(source)], logs)
        candidate_check = run([fitsverify, "-l", str(candidate)], logs)
        result["fitsverify_source"] = source_check
        result["fitsverify_candidate"] = candidate_check
        result["fitsverify"] = reference_fitsverify_status(source_check, candidate_check)

        funpack_argv = [funpack, "-C", "-O", str(unpacked), str(candidate)]
        funpack_check = run(funpack_argv, logs)
        result["funpack"] = {
            **funpack_check,
            "checksum_mode": "-C; CFITSIO checksum updates disabled",
        }
        if funpack_check.get("returncode") == 0 and unpacked.is_file():
            unpacked_bytes, unpacked_sha = digest(unpacked)
            result["funpack_output"] = {"bytes": unpacked_bytes, "sha256": unpacked_sha}
            try:
                roundtrip = compare_reference_funpack(source, unpacked)
            except Exception as error:
                roundtrip = {
                    "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v2",
                    "diagnostics_complete": False,
                    "ok": False,
                    "reason": f"{type(error).__name__}: {error}",
                    "failure_class_counts": {"unreadable": 1},
                    "mismatch_records": [],
                    "hdu_checks": [],
                }
            roundtrip["attribution"] = (
                "candidate_comparison"
                if positive_control_ok
                else "not_attributed_positive_control_failed"
            )
            result["reconstructed_array_literal_header_comparison"] = roundtrip
        else:
            result["reconstructed_array_literal_header_comparison"] = {
                "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v2",
                "diagnostics_complete": False,
                "ok": False,
                "reason": "funpack -C failed or produced no reconstructed FITS output",
                "failure_class_counts": {"funpack_failed": 1},
                "mismatch_records": [],
                "hdu_checks": [],
                "attribution": (
                    "candidate_comparison"
                    if positive_control_ok
                    else "not_attributed_positive_control_failed"
                ),
            }

        fitsverify_ok = result["fitsverify"].get("status") in ("passed", "passed_inherited_warnings")
        funpack_ok = (
            funpack_check.get("returncode") == 0
            and result["reconstructed_array_literal_header_comparison"].get("ok") is True
        )
        failures = []
        if not positive_control_ok:
            failures.append("external funpack -C positive control failed; candidate header/payload differences are not attributed")
        if not identity_matches:
            failures.append("candidate identity differs from the bytes used by the reference measurement")
        if audit.get("exact") is not True:
            failures.append("existing Astropy reference verifier did not report an exact candidate")
        if not fitsverify_ok:
            failures.append(result["fitsverify"].get("reason", "fitsverify failed"))
        if not funpack_ok:
            roundtrip = result.get("reconstructed_array_literal_header_comparison", {})
            failures.append(roundtrip.get("reason", "funpack or reconstruction check failed"))
        if not positive_control_ok:
            result["status"] = "blocked_positive_control"
        elif not failures:
            result["status"] = "passed"
            result["reason"] = "exact candidate identity, HEASARC check, funpack -C, positive control, and literal-header/storage-array round trip passed"
        result["reason"] = "; ".join(failures) if failures else result.get("reason", "")
    except Exception as error:
        result["reason"] = f"{type(error).__name__}: {error}"
    finally:
        for temporary in (candidate, unpacked):
            if temporary is None:
                continue
            try:
                temporary.unlink(missing_ok=True)
            except OSError as error:
                cleanup_errors.append(f"{temporary.name}: {type(error).__name__}: {error}")
    if cleanup_errors:
        result["cleanup_errors"] = cleanup_errors
    return result


def threshold_disposition(
    rows: list[dict], tier1_stats: dict, tier2_stats: dict, positive_control: dict
) -> dict:
    """Apply issue #59's unchanged rule only after the -C control and candidate gates."""
    blockers = []
    positive_control_ok = positive_control_passed(positive_control)
    if not positive_control_ok:
        blockers.append("external funpack -C positive control did not pass; survey header/payload differences are not attributable")
    if len(rows) != 14:
        blockers.append(f"expected all 14 frozen products, found {len(rows)}")
    tier2_rows = []
    for row in rows:
        product_id = row.get("product_id", "unknown")
        if row.get("fetch_status") != "downloaded":
            blockers.append(f"{product_id}: source fetch did not complete")
        if row.get("measurement_status") != "measured":
            blockers.append(f"{product_id}: fixed-product measurement status is {row.get('measurement_status')}")
        audit = row.get("existing_two_tier_instrument") or {}
        if audit.get("status") == "not_measured":
            blockers.append(f"{product_id}: no row in the existing two-tier instrument")
        if audit.get("status") == "tier2":
            tier2_rows.append(row)
            if audit.get("exact") is not True:
                blockers.append(f"{product_id}: Astropy reference candidate did not pass its original verifier")
            validation = row.get("reference_candidate_validation") or {}
            if validation.get("status") != "passed":
                blockers.append(f"{product_id}: reference-candidate HEASARC/funpack gate is {validation.get('status')}")

    if blockers:
        return {
            "status": "not_applied",
            "decision": "threshold not applied",
            "reason": "positive control or one or more fixed-row/reference-candidate checks remain unresolved",
            "positive_control_status": positive_control.get("status", "missing"),
            "blockers": blockers,
            "tier2_threshold": {
                "median_gain_pct_at_least": 8.0,
                "share_of_frozen_corpus_pct_at_least": 15.0,
                "n": tier2_stats["n"],
                "median_gain_pct": tier2_stats["median"],
                "q1_index_gain_pct": tier2_stats["q1_index"],
                "q3_index_gain_pct": tier2_stats["q3_index"],
                "share_positive": tier2_stats["share_positive"],
                "share_of_frozen_corpus_pct": 100.0 * len(tier2_rows) / len(rows) if rows else None,
                "result": "not evaluated because the validation gate is incomplete",
            },
            "tier1_threshold": {
                "median_gain_pct_at_least": 2.0,
                "n": tier1_stats["n"],
                "median_gain_pct": tier1_stats["median"],
                "result": "empty set; not evaluated (not 0%)" if tier1_stats["n"] == 0 else "not evaluated because the validation gate is incomplete",
            },
        }

    tier2_share = 100.0 * len(tier2_rows) / len(rows) if rows else None
    tier2_clears = (
        tier2_stats["n"] > 0
        and tier2_stats["median"] is not None
        and tier2_stats["median"] >= 8.0
        and tier2_share is not None
        and tier2_share >= 15.0
    )
    tier1_clears = (
        tier1_stats["n"] > 0
        and tier1_stats["median"] is not None
        and tier1_stats["median"] >= 2.0
    )
    if tier1_stats["n"] == 0:
        tier1_result = "empty set; not evaluated (not 0%)"
    elif tier1_clears:
        tier1_result = "clears the ≥2% median bar; ships on by default inside the fits feature"
    else:
        tier1_result = "does not clear the ≥2% median bar"

    if tier2_clears and tier1_clears:
        decision = "Tier 1 clears for default-in-feature use; Tier 2 clears for opt-in --fits-tile use"
    elif tier2_clears:
        decision = "Tier 2 clears; opt-in --fits-tile use; Tier 1 is not evaluated because its set is empty"
    elif tier1_clears:
        decision = "Tier 1 clears for default-in-feature use; Tier 2 does not clear"
    else:
        decision = "neither tier clears; FITS refused after measurement"
    return {
        "status": "applied",
        "decision": decision,
        "positive_control_status": positive_control.get("status", "missing"),
        "blockers": [],
        "tier2_threshold": {
            "median_gain_pct_at_least": 8.0,
            "share_of_frozen_corpus_pct_at_least": 15.0,
            "n": tier2_stats["n"],
            "median_gain_pct": tier2_stats["median"],
            "q1_index_gain_pct": tier2_stats["q1_index"],
            "q3_index_gain_pct": tier2_stats["q3_index"],
            "share_positive": tier2_stats["share_positive"],
            "share_of_frozen_corpus_pct": tier2_share,
            "result": "clears" if tier2_clears else "does not clear",
        },
        "tier1_threshold": {
            "median_gain_pct_at_least": 2.0,
            "n": tier1_stats["n"],
            "median_gain_pct": tier1_stats["median"],
            "q1_index_gain_pct": tier1_stats["q1_index"],
            "q3_index_gain_pct": tier1_stats["q3_index"],
            "share_positive": tier1_stats["share_positive"],
            "result": tier1_result,
        },
    }


def main() -> int:
    global fits_measure
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-manifest", required=True)
    parser.add_argument("--root", required=True)
    parser.add_argument("--audit-rows", required=True)
    parser.add_argument("--tzim", required=True)
    parser.add_argument("--fitsverify", required=True)
    parser.add_argument("--fpack", required=True)
    parser.add_argument("--funpack", required=True)
    parser.add_argument("--work", required=True)
    parser.add_argument("--reference-candidate-dir", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--fits-measure", default=".github/scripts/fits-measure.py")
    parser.add_argument("--expected-source-sha", required=True)
    args = parser.parse_args()

    source_manifest_path = Path(args.source_manifest)
    source_manifest_bytes = source_manifest_path.read_bytes()
    manifest = json.loads(source_manifest_bytes)
    manifest_sha = hashlib.sha256(source_manifest_bytes).hexdigest()
    if manifest.get("schema") != "tzim-fixed-fits-source-manifest-v1" or manifest.get("source_decision_comment") != "6498702204":
        raise SystemExit("input source manifest does not match the owner pre-registration")
    if manifest.get("repository_source_sha") != args.expected_source_sha:
        raise SystemExit("FITS source manifest was not generated from the expected repository SHA")
    products = manifest.get("products", [])
    fixed_products = load_fixed_products(Path(__file__).with_name("fetch-fixed-fits.py"))
    fixed_by_id = {row["id"]: row for row in fixed_products}
    products_by_id = {row.get("id"): row for row in products if isinstance(row, dict)}
    if (
        len(products) != 14
        or len(products_by_id) != 14
        or set(products_by_id) != set(fixed_by_id)
    ):
        raise SystemExit("fixed #59 FITS manifest must retain the exact 14 owner-registered product IDs")
    identity_fields = ("id", "mission", "source_uri", "retrieval_url", "product", "rights_basis", "transport")
    for product_id, fixed in fixed_by_id.items():
        observed = products_by_id[product_id]
        if any(observed.get(field) != fixed.get(field) for field in identity_fields):
            raise SystemExit(f"fixed #59 source identity changed for {product_id}; do not substitute products")

    fits_measure_import_error = None
    try:
        fits_measure = load_fits_measure(Path(args.fits_measure))
    except Exception as error:
        fits_measure = None
        fits_measure_import_error = f"{type(error).__name__}: {error}"
    dependency_errors = [
        error for error in (FITS_IO_IMPORT_ERROR, fits_measure_import_error) if error is not None
    ]
    measurement_dependency_error = "; ".join(dependency_errors) or None
    audit_path = Path(args.audit_rows)
    audit_rows = [json.loads(line) for line in audit_path.read_text(encoding="utf-8").splitlines() if line.strip()] if audit_path.exists() else []
    audit_by_path = {Path(row.get("path", "")).name: row for row in audit_rows}
    input_root = Path(args.root).resolve()
    work = Path(args.work).resolve()
    reference_candidate_dir = Path(args.reference_candidate_dir).resolve()
    out = Path(args.out).resolve()
    work.mkdir(parents=True, exist_ok=True)
    reference_candidate_dir.mkdir(parents=True, exist_ok=True)
    out.mkdir(parents=True, exist_ok=True)
    logs: list[dict] = []
    products_by_id = {row["id"]: row for row in products}
    positive_control = run_funpack_positive_control(
        fpack=str(Path(args.fpack).resolve()),
        funpack=str(Path(args.funpack).resolve()),
        fitsverify=str(Path(args.fitsverify).resolve()),
        work=work,
        logs=logs,
    )
    positive_control_ok = positive_control_passed(positive_control)
    (out / "positive-control.json").write_text(
        json.dumps(positive_control, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    rows: list[dict] = []

    for order, product in enumerate(products, start=1):
        row = {
            "order": order,
            "product_id": product["id"],
            "mission": product["mission"],
            "product": product["product"],
            "source_uri": product["source_uri"],
            "requested_url": product["requested_url"],
            "rights_basis": product["rights_basis"],
            "fetch_status": product["fetch_status"],
            "source_manifest_sha256": manifest_sha,
            "source_product_record": product,
            "temporary_cleanup_errors": product.get("temporary_cleanup_errors", []),
            "measurement_dependency_error": measurement_dependency_error,
            "tier2_tzim": {"status": "not_run"},
            "external_checks": {},
        }
        if product["fetch_status"] != "downloaded":
            row["measurement_status"] = "fetch_failed"
            row["fetch_error"] = product.get("fetch_error")
            row["needed_owner_action"] = product.get("needed_owner_action")
            rows.append(row)
            continue
        source = input_root / product["measurement_path"]
        if not source.is_file():
            row["measurement_status"] = "input_missing"
            row["fetch_error"] = f"fetched input missing after manifest freeze: {product['measurement_path']}"
            rows.append(row)
            continue
        size, sha = digest(source)
        row.update(input_path=product["measurement_path"], input_bytes=size, input_sha256=sha)
        if size != product["measurement_bytes"] or sha != product["measurement_sha256"]:
            row["measurement_status"] = "manifest_integrity_failure"
            row["fetch_error"] = "FITS measurement input differs from frozen byte count or SHA-256"
            rows.append(row)
            continue

        audit = audit_by_path.get(source.name)
        if audit is None:
            audit = {"status": "not_measured", "reason": "input absent from fits-measure audit rows"}
        row["existing_two_tier_instrument"] = audit
        reference_validation = validate_reference_candidate(
            source=source,
            audit=audit,
            reference_candidate_dir=reference_candidate_dir,
            work=work,
            product_id=product["id"],
            order=order,
            fitsverify=str(Path(args.fitsverify).resolve()),
            funpack=str(Path(args.funpack).resolve()),
            positive_control=positive_control,
            logs=logs,
        )
        if isinstance(audit.get("reference_candidate"), dict):
            row["reference_candidate"] = {
                **audit["reference_candidate"],
                "astropy_verifier_exact": audit.get("exact"),
                "astropy_verifier_reason": audit.get("why"),
            }
        row["reference_candidate_validation"] = reference_validation
        row["external_checks"]["fitsverify_reference_source"] = reference_validation.get("fitsverify_source")
        row["external_checks"]["fitsverify_reference_candidate"] = reference_validation.get("fitsverify_candidate")
        row["external_checks"]["funpack_reference_candidate"] = reference_validation.get("funpack")
        if reference_validation.get("cleanup_errors"):
            row.setdefault("temporary_cleanup_errors", []).extend(reference_validation["cleanup_errors"])
        row["tier1"] = sample_tier1(source, fits_measure)
        if row["tier1"].get("status") == "failed":
            row["tier1_error"] = row["tier1"].get("error")

        candidate = work / f"{order:02d}-{product['id']}.candidate.fits"
        shutil.copyfile(source, candidate)
        before_size, before_sha = digest(candidate)
        tzim_argv = [
            str(Path(args.tzim).resolve()), "--formats", "fits", "--fits-tile",
            "--jobs", "1", "--json", "--no-progress", str(candidate),
        ]
        tzim_result = run(tzim_argv, logs)
        file_rows = []
        for line in tzim_result.get("stdout", "").splitlines():
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            if record.get("type") == "file":
                file_rows.append(record)
        file_row = file_rows[0] if file_rows else None
        after_size, after_sha = digest(candidate)
        tzim_record = {
            "argv": tzim_argv,
            "returncode": tzim_result.get("returncode"),
            "stdout": tzim_result.get("stdout", ""),
            "stderr": tzim_result.get("stderr", ""),
            "file_row": file_row,
            "candidate_bytes": after_size,
            "candidate_sha256": after_sha,
            "candidate_changed": after_sha != before_sha,
            "status": file_row.get("status") if file_row else "failed",
        }
        if tzim_result.get("error"):
            tzim_record["error"] = tzim_result["error"]
        elif tzim_result.get("returncode") != 0 or file_row is None:
            tzim_record["error"] = "tzim returned non-zero or emitted no file row"
        elif after_sha != before_sha and (after_size >= before_size or file_row.get("status") != "optimized"):
            tzim_record["error"] = "tzim changed bytes without a strictly smaller optimized result"
        elif after_sha == before_sha and file_row.get("status") == "optimized":
            tzim_record["error"] = "tzim reported optimized without changing the candidate bytes"
        elif after_sha != before_sha:
            tzim_record["gain_pct"] = 100.0 * (before_size - after_size) / before_size if before_size else 0.0
        tzim_record["unchanged_nonoptimized"] = (
            after_sha == before_sha
            and file_row is not None
            and file_row.get("status") != "optimized"
            and not tzim_record.get("error")
        )
        row["tier2_tzim"] = tzim_record
        if measurement_dependency_error:
            row["measurement_status"] = "measurement_dependency_failed"
        else:
            row["measurement_status"] = "measured" if not tzim_record.get("error") else "tzim_failed"

        qv_source = run([str(Path(args.fitsverify).resolve()), str(source)], logs)
        qv_candidate = run([str(Path(args.fitsverify).resolve()), str(candidate)], logs)
        row["external_checks"]["fitsverify_source"] = qv_source
        row["external_checks"]["fitsverify_candidate"] = qv_candidate
        row["external_checks"]["fitsverify_source_tool"] = "HEASARC fitsverify from cfitsio 4.6.2"

        unpack_source = work / f"{order:02d}-{product['id']}.source-unpacked.fits"
        unpack_candidate = work / f"{order:02d}-{product['id']}.candidate-unpacked.fits"
        funpack_source_argv = [str(Path(args.funpack).resolve()), "-C", "-O", str(unpack_source), str(source)]
        funpack_candidate_argv = [str(Path(args.funpack).resolve()), "-C", "-O", str(unpack_candidate), str(candidate)]
        # Run the same external -C reconstruction path for every frozen product,
        # including unchanged/non-optimized rows. The direct candidate-layer
        # compare_fits below still enforces raw byte identity for each untransformed
        # HDU; this is a separate storage/header reconstruction check, not a waiver.
        funpack_source = run(funpack_source_argv, logs)
        funpack_candidate = run(funpack_candidate_argv, logs)
        if funpack_source.get("returncode") == 0 and unpack_source.is_file():
            funpack_source_bytes, funpack_source_sha = digest(unpack_source)
            row["external_checks"]["funpack_source_output"] = {
                "bytes": funpack_source_bytes,
                "sha256": funpack_source_sha,
            }
        if funpack_candidate.get("returncode") == 0 and unpack_candidate.is_file():
            funpack_candidate_bytes, funpack_candidate_sha = digest(unpack_candidate)
            row["external_checks"]["funpack_candidate_output"] = {
                "bytes": funpack_candidate_bytes,
                "sha256": funpack_candidate_sha,
            }
        row["external_checks"]["funpack_source"] = funpack_source
        row["external_checks"]["funpack_candidate"] = funpack_candidate
        row["external_checks"]["funpack_checksum_mode"] = "-C used for source and candidate reconstructions; CHECKSUM/DATASUM are retained and compared"

        try:
            direct = compare_fits(source, candidate)
            row["array_header_roundtrip"] = direct
        except Exception as error:
            row["array_header_roundtrip"] = {"ok": False, "reason": f"{type(error).__name__}: {error}"}
        if unpack_source.is_file() and unpack_candidate.is_file():
            try:
                unpacked = compare_reference_funpack(unpack_source, unpack_candidate)
                unpacked["attribution"] = (
                    "candidate_comparison"
                    if positive_control_ok
                    else "not_attributed_positive_control_failed"
                )
                row["funpack_array_header_roundtrip"] = unpacked
            except Exception as error:
                row["funpack_array_header_roundtrip"] = {
                    "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v2",
                    "diagnostics_complete": False,
                    "ok": False,
                    "reason": f"{type(error).__name__}: {error}",
                    "failure_class_counts": {"unreadable": 1},
                    "mismatch_records": [],
                    "hdu_checks": [],
                    "attribution": (
                        "candidate_comparison"
                        if positive_control_ok
                        else "not_attributed_positive_control_failed"
                    ),
                }
        else:
            row["funpack_array_header_roundtrip"] = {
                "diagnostic_schema": "tzim-funpack-roundtrip-diagnostics-v2",
                "diagnostics_complete": False,
                "ok": False,
                "reason": "funpack -C source or candidate output missing",
                "failure_class_counts": {"funpack_failed": 1},
                "mismatch_records": [],
                "hdu_checks": [],
                "attribution": (
                    "candidate_comparison"
                    if positive_control_ok
                    else "not_attributed_positive_control_failed"
                ),
            }

        candidate_size, candidate_sha = digest(candidate)
        row["tier2_tzim"]["candidate_bytes"] = candidate_size
        row["tier2_tzim"]["candidate_sha256"] = candidate_sha
        # The evidence artifact retains only hashes and oracle output, never the
        # survey bytes, transformed candidates, or funpack-expanded arrays. Drop
        # each large temporary before moving to the next registered product.
        cleanup_errors = []
        for temporary in (candidate, unpack_source, unpack_candidate):
            try:
                temporary.unlink(missing_ok=True)
            except OSError as error:
                cleanup_errors.append(f"{temporary.name}: {type(error).__name__}: {error}")
        if cleanup_errors:
            row["temporary_cleanup_errors"] = cleanup_errors
        print(f"[{order}/14] {product['id']}: {row['measurement_status']} / {row['tier2_tzim']['status']}", flush=True)
        rows.append(row)

    rows_path = out / "raw-rows.jsonl"
    rows_path.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in rows), encoding="utf-8")
    (out / "external-tool-output.jsonl").write_text("".join(json.dumps(item, sort_keys=True) + "\n" for item in logs), encoding="utf-8")
    hdu_diagnostics = []
    for row in rows:
        reference = row.get("reference_candidate") or {}
        validation = row.get("reference_candidate_validation") or {}
        reference_comparison = validation.get("reconstructed_array_literal_header_comparison")
        if reference_comparison is not None:
            hdu_diagnostics.append({
                "product_id": row.get("product_id"),
                "comparison_kind": "astropy_reference_candidate_funpack_C_vs_source",
                "candidate_bytes": reference.get("bytes"),
                "candidate_sha256": reference.get("sha256"),
                "positive_control_status": positive_control.get("status"),
                "attribution": reference_comparison.get("attribution"),
                "diagnostics": reference_comparison,
            })
        tzim_comparison = row.get("funpack_array_header_roundtrip")
        if tzim_comparison is not None:
            hdu_diagnostics.append({
                "product_id": row.get("product_id"),
                "comparison_kind": "tzim_candidate_funpack_C_vs_source_funpack_C",
                "candidate_bytes": (row.get("tier2_tzim") or {}).get("candidate_bytes"),
                "candidate_sha256": (row.get("tier2_tzim") or {}).get("candidate_sha256"),
                "positive_control_status": positive_control.get("status"),
                "attribution": tzim_comparison.get("attribution"),
                "diagnostics": tzim_comparison,
            })
    positive_control_comparison = positive_control.get("literal_header_storage_array_comparison") or {}
    if positive_control_comparison:
        hdu_diagnostics.append({
            "product_id": positive_control.get("control_id", "external-integer-positive-control"),
            "comparison_kind": "external_integer_positive_control_funpack_C_vs_source",
            "positive_control_status": positive_control.get("status"),
            "attribution": "control_validation",
            "diagnostics": positive_control_comparison,
        })
    (out / "reference-hdu-diagnostics.jsonl").write_text(
        "".join(json.dumps(item, sort_keys=True, ensure_ascii=False) + "\n" for item in hdu_diagnostics),
        encoding="utf-8",
    )

    audit_status_counts = dict(sorted(Counter(row.get("status", "missing") for row in audit_rows).items()))
    audit_refusal_reasons = dict(sorted(Counter(
        row.get("reason", "unspecified") for row in audit_rows if row.get("status") == "refused"
    ).items()))
    audit_tier2_rows = [row for row in audit_rows if row.get("status") == "tier2"]
    audit_gains = [
        row["pct"] for row in audit_tier2_rows
        if row.get("exact") is True and isinstance(row.get("pct"), (int, float))
    ]
    tier1_gains = [
        row["tier1"]["gain_pct"] for row in rows
        if row.get("tier1", {}).get("status") == "measured_comparator"
    ]
    tzim_gains = [
        row["tier2_tzim"]["gain_pct"] for row in rows
        if isinstance(row.get("tier2_tzim", {}).get("gain_pct"), (int, float))
    ]
    tier1_stats = distribution_stats(tier1_gains)
    tier2_stats = distribution_stats(audit_gains)
    tzim_stats = distribution_stats(tzim_gains)
    reference_rows = [
        row for row in rows
        if (row.get("existing_two_tier_instrument") or {}).get("status") == "tier2"
    ]
    reference_validation_counts = dict(sorted(Counter(
        (row.get("reference_candidate_validation") or {}).get("status", "missing")
        for row in reference_rows
    ).items()))
    reference_fitsverify_counts = dict(sorted(Counter(
        ((row.get("reference_candidate_validation") or {}).get("fitsverify") or {}).get("status", "missing")
        for row in reference_rows
    ).items()))

    def comparison_diagnostics(rows_to_count: list[dict], comparison_field: str, nested_field: str | None = None) -> dict:
        failure_classes: Counter = Counter()
        header_card_mismatches = 0
        card_difference_counts: Counter = Counter()
        payload_mismatch_hdus = 0
        header_cards_equal_hdus = 0
        header_cards_mismatch_hdus = 0
        padding_only_hdus = 0
        hdu_count_mismatches = 0
        mismatch_records = 0
        complete_comparisons = 0
        comparisons_n = 0
        for row in rows_to_count:
            comparison = row.get(comparison_field) or {}
            if nested_field is not None:
                comparison = comparison.get(nested_field) or {}
            if comparison.get("diagnostic_schema") is None:
                continue
            comparisons_n += 1
            complete_comparisons += int(comparison.get("diagnostics_complete") is True)
            for name, count in (comparison.get("failure_class_counts") or {}).items():
                failure_classes[name] += count
            mismatch_records += len(comparison.get("mismatch_records") or [])
            for check in comparison.get("hdu_checks") or []:
                differences = check.get("header_card_differences") or []
                header_card_mismatches += len(differences)
                for kind, count in (check.get("header_card_difference_counts") or {}).items():
                    card_difference_counts[kind] += count
                payload_mismatch_hdus += int(check.get("storage_payload_equality") is False)
                header_cards_equal_hdus += int(check.get("header_cards_through_end_equality") is True)
                header_cards_mismatch_hdus += int(check.get("header_cards_through_end_equality") is False)
                padding_only_hdus += int(check.get("header_padding_only_difference") is True)
            hdu_count_mismatches += int(comparison.get("hdu_count_equal") is False)
        return {
            "comparisons_n": comparisons_n,
            "complete_comparisons_n": complete_comparisons,
            "failure_class_counts": dict(sorted(failure_classes.items())),
            "header_card_mismatch_count": header_card_mismatches,
            "header_card_difference_counts": {
                kind: card_difference_counts.get(kind, 0) for kind in ("inserted", "dropped", "changed")
            },
            "header_cards_through_end_equal_hdu_count": header_cards_equal_hdus,
            "header_cards_through_end_mismatch_hdu_count": header_cards_mismatch_hdus,
            "padding_only_hdu_count": padding_only_hdus,
            "payload_mismatch_hdu_count": payload_mismatch_hdus,
            "hdu_count_mismatch_count": hdu_count_mismatches,
            "structured_mismatch_record_count": mismatch_records,
            "diagnostics_file": "reference-hdu-diagnostics.jsonl",
        }

    reference_hdu_diagnostics = {
        "comparison_kind": "Astropy reference candidate funpack -C versus exact source",
        **comparison_diagnostics(rows, "reference_candidate_validation", "reconstructed_array_literal_header_comparison"),
    }
    tzim_hdu_diagnostics = {
        "comparison_kind": "Tzim candidate funpack -C versus source funpack -C",
        **comparison_diagnostics(rows, "funpack_array_header_roundtrip"),
    }
    positive_control_summary = {
        "control_id": positive_control.get("control_id"),
        "status": positive_control.get("status", "missing"),
        "ok": positive_control_ok,
        "fpack_exit": (positive_control.get("fpack") or {}).get("returncode"),
        "fitsverify_exit": (positive_control.get("fitsverify") or {}).get("returncode"),
        "funpack_exit": (positive_control.get("funpack") or {}).get("returncode"),
        "roundtrip_ok": (positive_control.get("literal_header_storage_array_comparison") or {}).get("ok") is True,
        "fpack_float_array_oracle_used": positive_control.get("fpack_float_array_oracle_used"),
        "funpack_checksum_mode": positive_control.get("funpack_checksum_mode"),
    }
    tier_distribution = {
        "tier1_comparator_n": len(tier1_gains),
        "tier1_comparator_product_ids": [
            row["product_id"] for row in rows if row.get("tier1", {}).get("status") == "measured_comparator"
        ],
        "tier2_reference_n": len(audit_tier2_rows),
        "tier2_reference_product_ids": [row.get("product_id") for row in reference_rows],
        "refused_n": sum(1 for row in audit_rows if row.get("status") == "refused"),
        "refusal_reasons": audit_refusal_reasons,
        "not_measured_n": sum(1 for row in rows if (row.get("existing_two_tier_instrument") or {}).get("status") == "not_measured"),
        "note": "Tier 1 recompression rows are comparators derived from the frozen products and may overlap a Tier 2 product; they are not new survey inputs.",
    }
    threshold_result = threshold_disposition(rows, tier1_stats, tier2_stats, positive_control)
    audit_stats = {
        "audit_rows": len(audit_rows),
        "tier2_candidate_rows": len(audit_tier2_rows),
        "status_counts": audit_status_counts,
        "refusal_reasons": audit_refusal_reasons,
    }
    aggregate = {
        "source_manifest_sha256": manifest_sha,
        "products_n": len(rows),
        "fetch_status_counts": dict(sorted(Counter(row.get("fetch_status", "missing") for row in rows).items())),
        "measurement_status_counts": dict(sorted(Counter(row.get("measurement_status", "missing") for row in rows).items())),
        "tzim_status_counts": dict(sorted(Counter(row.get("tier2_tzim", {}).get("status", "missing") for row in rows).items())),
        "existing_fits_measure_instrument": audit_stats,
        "tier_and_refusal_distribution": tier_distribution,
        "existing_fits_measure_tier2_gain_pct": {
            **tier2_stats,
            "min": min(audit_gains) if audit_gains else None,
            "max": max(audit_gains) if audit_gains else None,
        },
        "existing_tile_tier1_comparator_gain_pct": {
            **tier1_stats,
            "min": min(tier1_gains) if tier1_gains else None,
            "max": max(tier1_gains) if tier1_gains else None,
        },
        "tzim_tier2_gain_pct": {
            **tzim_stats,
            "min": min(tzim_gains) if tzim_gains else None,
            "max": max(tzim_gains) if tzim_gains else None,
        },
        "external_funpack_C_positive_control": positive_control_summary,
        "reference_candidate_gate": {
            "expected_candidates_n": len(reference_rows),
            "validation_status_counts": reference_validation_counts,
            "fitsverify_status_counts": reference_fitsverify_counts,
            "validated_candidates_n": sum(1 for row in reference_rows if (row.get("reference_candidate_validation") or {}).get("status") == "passed"),
            "candidate_identity_mismatches": sum(1 for row in reference_rows if (row.get("reference_candidate_validation") or {}).get("identity_matches_measurement_candidate") is False),
            "funpack_roundtrip_failures": sum(1 for row in reference_rows if (row.get("reference_candidate_validation") or {}).get("reconstructed_array_literal_header_comparison", {}).get("ok") is False),
            "header_payload_attribution_enabled": positive_control_summary["ok"],
        },
        "reference_hdu_diagnostics": reference_hdu_diagnostics,
        "tzim_hdu_diagnostics": tzim_hdu_diagnostics,
        "funpack_checksum_mode": "-C used for every available reference-candidate and all 14 Tzim source/candidate reconstructions; checksum cards are not filtered",
        "pre_registered_threshold_disposition": threshold_result,
        "fitsverify_candidate_nonzero": sum(1 for row in rows if row.get("tier2_tzim", {}).get("status") == "optimized" and row.get("external_checks", {}).get("fitsverify_candidate", {}).get("returncode") != 0),
        "array_header_roundtrip_passes": sum(1 for row in rows if row.get("array_header_roundtrip", {}).get("ok") is True),
        "array_header_roundtrip_failures": sum(1 for row in rows if row.get("array_header_roundtrip", {}).get("ok") is False),
        "funpack_roundtrip_passes": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("ok") is True),
        "funpack_roundtrip_not_applicable": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("status") == "not_applicable"),
        "funpack_failures": sum(1 for row in rows if row.get("funpack_array_header_roundtrip", {}).get("ok") is False),
        "reference_funpack_roundtrip_passes": sum(1 for row in reference_rows if (row.get("reference_candidate_validation") or {}).get("reconstructed_array_literal_header_comparison", {}).get("ok") is True),
        "reference_funpack_roundtrip_failures": sum(1 for row in reference_rows if (row.get("reference_candidate_validation") or {}).get("reconstructed_array_literal_header_comparison", {}).get("ok") is False),
        "measurement_script": ".github/scripts/fixed-fits-measure.py",
        "existing_two_tier_script": ".github/scripts/fits-measure.py",
        "existing_two_tier_script_sha256": hashlib.sha256(Path(args.fits_measure).read_bytes()).hexdigest() if Path(args.fits_measure).is_file() else None,
        "python_version": sys.version,
        "numpy_version": np.__version__ if np is not None else None,
        "astropy_version": astropy.__version__ if astropy is not None else None,
        "fitsverify_source": {
            "cfitsio_commit": "113781d8aa442cf8d3c27295d9084674b8b039f2",
            "cfitsio_archive_sha256": "d93c0de372a264156d2f3e1eab2d4159e132c35c2216f71c0af0009b92a72b5e",
            "version_line": next((
                line.strip()
                for row in rows
                for line in ((row.get("external_checks") or {}).get("fitsverify_reference_candidate") or {}).get("stdout", "").splitlines()
                if line.lstrip().startswith("fitsverify ")
            ), None),
        },
        "derived_tier1_is_comparator_only": True,
        "fpack_checksum_behavior": "documentation/comparator only; never used to admit or repair a candidate",
    }
    (out / "aggregate.json").write_text(json.dumps(aggregate, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    def distribution_text(label: str, stats: dict) -> str:
        if stats["n"] == 0:
            return f"{label}: n=0 (empty; not reported as 0%)"
        return (
            f"{label}: n={stats['n']}, median={stats['median']}%, Q1={stats['q1_index']}%, "
            f"Q3={stats['q3_index']}%, share-positive={100.0 * stats['share_positive']:.2f}%"
        )

    reference_gate = aggregate["reference_candidate_gate"]
    threshold = aggregate["pre_registered_threshold_disposition"]
    summary = [
        "# Fixed #59 FITS measurement",
        "",
        f"- Frozen 14-product source manifest SHA-256: `{manifest_sha}`",
        f"- Environment: Python `{sys.version.split()[0]}`, NumPy `{aggregate['numpy_version']}`, Astropy `{aggregate['astropy_version']}`; CFITSIO commit `{aggregate['fitsverify_source']['cfitsio_commit']}` (archive SHA-256 `{aggregate['fitsverify_source']['cfitsio_archive_sha256']}`).",
        f"- Existing two-tier instrument SHA-256: `{aggregate['existing_two_tier_script_sha256']}`; exact reference-candidate generation and check commands/results are retained in the rows/logs.",
        f"- Fetch statuses: `{json.dumps(aggregate['fetch_status_counts'], sort_keys=True)}`",
        f"- Measurement statuses: `{json.dumps(aggregate['measurement_status_counts'], sort_keys=True)}`",
        f"- Tzim Tier 2 statuses: `{json.dumps(aggregate['tzim_status_counts'], sort_keys=True)}`",
        f"- Existing two-tier instrument rows: {len(audit_rows)}; statuses `{json.dumps(audit_status_counts, sort_keys=True)}`",
        f"- {distribution_text('Tier 2 Astropy reference-candidate gain', aggregate['existing_fits_measure_tier2_gain_pct'])}",
        f"- {distribution_text('Tier 1 existing-tile comparator gain', aggregate['existing_tile_tier1_comparator_gain_pct'])}",
        f"- Refusals by reason: `{json.dumps(audit_refusal_reasons, sort_keys=True)}`",
        f"- External positive control: **{positive_control_summary['status']}** via pinned CFITSIO `fpack` → `fitsverify -l` → `funpack -C` → this same literal-header/storage-array comparator; integer-only fixture, no fpack float-array oracle.",
        f"- Reference candidate HEASARC/funpack -C gate: {reference_gate['validated_candidates_n']}/{reference_gate['expected_candidates_n']} passed; overall statuses `{json.dumps(reference_gate['validation_status_counts'], sort_keys=True)}`; `fitsverify -l` statuses `{json.dumps(reference_gate['fitsverify_status_counts'], sort_keys=True)}`.",
        f"- Reference per-HDU classes: `{json.dumps(reference_hdu_diagnostics['failure_class_counts'], sort_keys=True)}`; cards-through-END equal={reference_hdu_diagnostics['header_cards_through_end_equal_hdu_count']}, unequal={reference_hdu_diagnostics['header_cards_through_end_mismatch_hdu_count']}, padding-only={reference_hdu_diagnostics['padding_only_hdu_count']}, inserted/dropped/changed cards=`{json.dumps(reference_hdu_diagnostics['header_card_difference_counts'], sort_keys=True)}`, storage-payload mismatches={reference_hdu_diagnostics['payload_mismatch_hdu_count']}; attribution enabled={positive_control_summary['ok']}.",
        f"- Tzim per-HDU classes: `{json.dumps(tzim_hdu_diagnostics['failure_class_counts'], sort_keys=True)}`; cards-through-END equal={tzim_hdu_diagnostics['header_cards_through_end_equal_hdu_count']}, unequal={tzim_hdu_diagnostics['header_cards_through_end_mismatch_hdu_count']}, padding-only={tzim_hdu_diagnostics['padding_only_hdu_count']}, inserted/dropped/changed cards=`{json.dumps(tzim_hdu_diagnostics['header_card_difference_counts'], sort_keys=True)}`, storage-payload mismatches={tzim_hdu_diagnostics['payload_mismatch_hdu_count']}.",
        "- Non-truncated per-product/per-HDU evidence is included below and retained in `run/reference-hdu-diagnostics.jsonl` plus raw rows. The first differing card is represented by exact 80-byte hex; FITS payload bytes are never retained.",
        f"- Pre-registered #59 disposition: **{threshold['status']}** — {threshold['decision']}.",
        f"- Tzim Tier 2 median among strictly smaller candidates (separate implementation result): {aggregate['tzim_tier2_gain_pct']['median']}",
        f"- Direct Tzim array/header comparisons: {aggregate['array_header_roundtrip_passes']} passed, {aggregate['array_header_roundtrip_failures']} failed, {len(rows) - aggregate['array_header_roundtrip_passes'] - aggregate['array_header_roundtrip_failures']} not measured.",
        f"- Tzim-candidate funpack -C array/header comparisons: {aggregate['funpack_roundtrip_passes']} passed, {aggregate['funpack_failures']} failed across the 14 fixed source/candidate pairs; unchanged rows are also reconstructed and checked.",
        f"- Exact reference-candidate funpack round trips: {aggregate['reference_funpack_roundtrip_passes']} passed, {aggregate['reference_funpack_roundtrip_failures']} failed.",
        "",
        "All 14 fixed source rows are retained. Tier 1 is a comparator derived from each source, never an extra survey input. Every source/candidate pair, including unchanged rows, is reconstructed with `funpack -C`; checksum cards are preserved, not filtered. The direct candidate-layer check remains byte-for-byte strict for every untransformed HDU, including header and data padding. Only the separate reconstructed-header comparator excludes record padding after END; it compares literal cards through END. The artifact retains hashes and tool output only—no source FITS bytes, reference-candidate bytes, Tzim candidate bytes, reconstructed FITS bytes, or ROSAT data.",
        "",
        "## All fixed product rows — distinct reference and Tzim dispositions",
        "",
        "No row is dropped based on outcome. Candidate file identities are retained as byte counts and SHA-256; the FITS bytes themselves are temporary and are not uploaded.",
        "",
        "| Fixed product | Input bytes / SHA-256 | Astropy reference candidate bytes / SHA-256 | Reference `fitsverify -l` | Reference `funpack -C` header / array | Tzim status; candidate bytes / SHA-256 | Direct Tzim comparison | Tzim fitsverify |",
        "| --- | ---: | --- | --- | --- | --- | --- | --- |",
    ]
    for row in rows:
        tzim = row.get("tier2_tzim") or {}
        direct = row.get("array_header_roundtrip") or {}
        funpack_roundtrip = row.get("funpack_array_header_roundtrip") or {}
        reference = row.get("reference_candidate") or {}
        reference_validation = row.get("reference_candidate_validation") or {}
        checks = row.get("external_checks") or {}
        candidate_fitsverify = checks.get("fitsverify_candidate") or {}
        reference_fitsverify = reference_validation.get("fitsverify") or {}
        reference_roundtrip = reference_validation.get("reconstructed_array_literal_header_comparison") or {}
        if direct.get("ok") is True:
            direct_status = "PASS"
        elif direct.get("ok") is False:
            direct_status = "FAIL"
        else:
            direct_status = "not measured"
        if reference_roundtrip.get("ok") is True:
            reference_funpack_status = "PASS"
        elif reference_roundtrip.get("ok") is False and reference_roundtrip.get("attribution") == "not_attributed_positive_control_failed":
            reference_funpack_status = "mismatch observed; attribution withheld (positive control failed)"
        elif reference_roundtrip.get("ok") is False:
            reference_funpack_status = f"FAIL {reference_roundtrip.get('failure_class_counts', {})}"
        else:
            reference_funpack_status = "not run / not applicable"
        if reference_fitsverify.get("status") == "passed_inherited_warnings":
            reference_fitsverify_status = f"PASS (source warnings inherited; candidate exit {reference_fitsverify.get('candidate_returncode')})"
        elif reference_fitsverify.get("status") == "passed":
            reference_fitsverify_status = "PASS (exit 0)"
        else:
            reference_fitsverify_status = f"{reference_fitsverify.get('status', 'not applicable')} (exit {reference_fitsverify.get('candidate_returncode')})"
        if funpack_roundtrip.get("status") == "not_applicable":
            funpack_status = "not applicable (Tzim unchanged)"
        elif funpack_roundtrip.get("ok") is True:
            funpack_status = "pass"
        elif funpack_roundtrip.get("ok") is False and funpack_roundtrip.get("attribution") == "not_attributed_positive_control_failed":
            funpack_status = "mismatch observed; attribution withheld (positive control failed)"
        elif funpack_roundtrip.get("ok") is False:
            funpack_status = f"FAIL {funpack_roundtrip.get('failure_class_counts', {})}"
        else:
            funpack_status = "not run"
        if candidate_fitsverify.get("returncode") is None and candidate_fitsverify.get("error"):
            fitsverify_status = f"error: {candidate_fitsverify['error']}"
        elif tzim.get("unchanged_nonoptimized"):
            fitsverify_status = f"exit {candidate_fitsverify.get('returncode')} (unchanged source diagnostic)"
        else:
            fitsverify_status = f"exit {candidate_fitsverify.get('returncode')}"
        reference_identity = (
            f"{reference.get('bytes', '—')} / `{reference.get('sha256', '—')}`"
            if reference else "— (no Tier 2 reference candidate)"
        )
        summary.append(
            f"| `{row.get('product_id', 'unknown')}` | {row.get('input_bytes', '—')} / "
            f"`{row.get('input_sha256', '—')}` | {reference_identity} | {reference_fitsverify_status} | "
            f"{reference_funpack_status} | {tzim.get('status', 'not-run')}; "
            f"{tzim.get('candidate_bytes', '—')} / `{tzim.get('candidate_sha256', '—')}` | "
            f"{direct_status} | {funpack_status}; fitsverify {fitsverify_status} |"
        )

    def compact_first_difference(check: dict) -> str:
        difference = check.get("first_actual_difference")
        if not isinstance(difference, dict):
            return "none"
        kind = difference.get("mismatch_kind")
        if kind == "header_card":
            fields = (
                "kind", "keyword", "original_card_index", "reconstructed_card_index",
                "first_differing_byte_offset", "first_differing_original_header_byte_offset",
                "first_differing_reconstructed_header_byte_offset", "original_file_offset",
                "reconstructed_file_offset", "before_card_hex", "after_card_hex",
            )
        elif kind == "header_padding":
            fields = (
                "kind", "mismatch_kind", "first_differing_byte_offset", "original_file_offset",
                "reconstructed_file_offset", "original_byte_hex", "reconstructed_byte_hex",
            )
        elif kind == "storage_payload":
            fields = (
                "kind", "mismatch_kind", "first_differing_byte_offset", "original_file_offset",
                "reconstructed_file_offset", "original_byte_hex", "reconstructed_byte_hex",
            )
        else:
            fields = ("kind", "mismatch_kind", "side", "reason")
        compact = {field: difference.get(field) for field in fields if field in difference}
        return json.dumps(compact, ensure_ascii=True, separators=(",", ":"))

    compact_hdu_rows = []
    comparison_labels = {
        "astropy_reference_candidate_funpack_C_vs_source": "reference -C",
        "tzim_candidate_funpack_C_vs_source_funpack_C": "Tzim source/candidate -C",
        "external_integer_positive_control_funpack_C_vs_source": "external integer control -C",
    }
    for diagnostic in hdu_diagnostics:
        product_id = str(diagnostic.get("product_id") or "unknown-product")
        comparison_kind = comparison_labels.get(diagnostic.get("comparison_kind"), diagnostic.get("comparison_kind", "unknown"))
        comparison = diagnostic.get("diagnostics") or {}
        hdu_checks = comparison.get("hdu_checks") or []
        if not hdu_checks:
            compact_hdu_rows.append(
                f"| `{product_id}` | {comparison_kind} | — | unavailable | unavailable | unavailable | — | no HDU comparison |"
            )
            continue
        for check in hdu_checks:
            cards_equal = check.get("header_cards_through_end_equality")
            cards_status = "true" if cards_equal is True else "false" if cards_equal is False else "unknown"
            payload_equal = check.get("storage_payload_equality")
            payload_status = "true" if payload_equal is True else "false" if payload_equal is False else "unknown"
            if check.get("header_padding_only_difference") is True:
                padding_status = "yes"
            elif check.get("header_padding_equality") is True:
                padding_status = "no"
            elif check.get("header_padding_equality") is False:
                padding_status = "different-with-card-diff"
            else:
                padding_status = "unknown"
            counts = check.get("header_card_difference_counts") or {}
            count_text = (
                f"I={counts.get('inserted', 0)},D={counts.get('dropped', 0)},C={counts.get('changed', 0)}"
            )
            hdu_index = check.get("hdu_index", "—")
            first = compact_first_difference(check)
            cells = (product_id, comparison_kind, str(hdu_index), cards_status,
                     payload_status, padding_status, count_text, first)
            escaped = [str(cell).replace("|", "\\|").replace("\n", " ") for cell in cells]
            compact_hdu_rows.append("| " + " | ".join(escaped) + " |")

    summary.extend([
        "",
        "## Corrected FITS raw comparator diagnostics — per product and HDU",
        "",
        "Cards-through-END equality compares exact 80-byte cards through the actual END card; only post-END record padding is excluded. `Padding-only=yes` means cards through END are equal but padding bytes differ. `I/D/C` are inserted/dropped/changed keyword-card counts. The first-difference cell gives offsets and exact before/after card hex (or exact differing padding/payload bytes). Every per-product/per-HDU row is present and no diagnostic value is truncated; full structured records remain in the evidence JSONL. No FITS payload bytes are shown.",
        "",
        "| Product / control | Comparison | HDU | Cards through END equal | Storage payload equal | Padding-only | I/D/C keyword counts | First actual difference |",
        "| --- | --- | ---: | --- | --- | --- | --- | --- |",
        *compact_hdu_rows,
    ])

    def diagnostic_excerpt(value: object, limit: int = 600) -> str:
        text = str(value or "").strip()
        if not text:
            return "(empty)"
        if len(text) > limit:
            return text[:limit] + "\n...[truncated]"
        return text

    diagnostics = []
    for row in rows:
        product_id = row.get("product_id", "unknown-product")
        if row.get("fetch_status") != "downloaded":
            diagnostics.extend([
                f"### Fetch failure: {product_id}",
                f"- Source identity: `{row.get('source_uri')}`",
                f"- Exact requested URL: `{row.get('requested_url')}`",
                f"- Error: {diagnostic_excerpt(row.get('fetch_error'))}",
                f"- Needed owner action: {row.get('needed_owner_action') or 'not recorded'}",
                "",
            ])
        tzim = row.get("tier2_tzim") or {}
        if tzim.get("error") or tzim.get("returncode") not in (None, 0) or tzim.get("status") == "failed":
            diagnostics.extend([
                f"### Tzim Tier 2 failure: {product_id}",
                f"- Source identity: `{row.get('source_uri')}`",
                f"- Exact requested URL: `{row.get('requested_url')}`",
                f"- Error: {diagnostic_excerpt(tzim.get('error'))}",
                f"- Return code: `{tzim.get('returncode')}`",
                f"- Command: `{json.dumps(tzim.get('argv', []), ensure_ascii=False)}`",
            ])
            for label in ("stderr", "stdout"):
                text = diagnostic_excerpt(tzim.get(label))
                if text != "(empty)":
                    diagnostics.extend([f"- {label}:", "", "```text", text, "```"])
            diagnostics.append("")
        reference_validation = row.get("reference_candidate_validation") or {}
        for name, check in (row.get("external_checks") or {}).items():
            if not isinstance(check, dict) or (not check.get("error") and check.get("returncode") in (None, 0)):
                continue
            if (
                name == "fitsverify_reference_candidate"
                and (reference_validation.get("fitsverify") or {}).get("status") == "passed_inherited_warnings"
            ):
                continue
            if name == "fitsverify_reference_source":
                heading = f"### Reference source diagnostic: {product_id}"
            elif name in ("fitsverify_reference_candidate", "funpack_reference_candidate"):
                heading = f"### Reference candidate external-check failure: {product_id} / {name}"
            elif name.endswith("_source"):
                heading = f"### External source diagnostic: {product_id} / {name}"
            elif (row.get("tier2_tzim") or {}).get("unchanged_nonoptimized"):
                heading = f"### Inherited external diagnostic (unchanged Tzim source): {product_id} / {name}"
            else:
                heading = f"### External check failure: {product_id} / {name}"
            diagnostics.extend([
                heading,
                f"- Error: {diagnostic_excerpt(check.get('error'))}",
                f"- Return code: `{check.get('returncode')}`",
            ])
            for label in ("stderr", "stdout"):
                text = diagnostic_excerpt(check.get(label))
                if text != "(empty)":
                    diagnostics.extend([f"- {label}:", "", "```text", text, "```"])
            diagnostics.append("")
        if reference_validation.get("status") in ("failed", "blocked_positive_control"):
            details = json.dumps(reference_validation, sort_keys=True, ensure_ascii=False)
            diagnostics.extend([
                f"### Reference candidate gate {reference_validation.get('status')}: {product_id}",
                f"- Exact candidate identity: `{(row.get('reference_candidate') or {}).get('bytes')}` bytes / "
                f"`{(row.get('reference_candidate') or {}).get('sha256')}`",
                f"- Reason: {diagnostic_excerpt(reference_validation.get('reason'))}",
                "- Full structured per-HDU header/payload diagnostics, when produced, are retained without truncation in `reference-hdu-diagnostics.jsonl` and `raw-rows.jsonl`.",
                f"- Validation record excerpt: `{diagnostic_excerpt(details)}`",
                "",
            ])
        for label in ("array_header_roundtrip", "funpack_array_header_roundtrip"):
            check = row.get(label) or {}
            if check.get("ok") is False:
                details = json.dumps(check, sort_keys=True, ensure_ascii=False)
                diagnostics.extend([
                    f"### Oracle failure: {product_id} / {label}",
                    f"- Reason: {diagnostic_excerpt(check.get('reason'))}",
                    f"- Oracle details: `{diagnostic_excerpt(details)}`",
                    "",
                ])

    if diagnostics:
        summary.extend(["", "## Per-product diagnostics", "", *diagnostics])
    (out / "summary.md").write_text("\n".join(summary) + "\n", encoding="utf-8")

    failures = []
    for row in rows:
        if row.get("fetch_status") != "downloaded":
            failures.append(f"{row['product_id']}: fetch failed at {row.get('requested_url')}: {row.get('fetch_error')}")
        if row.get("measurement_status") in (
            "input_missing", "manifest_integrity_failure", "tzim_failed", "measurement_dependency_failed"
        ):
            failures.append(f"{row['product_id']}: {row['measurement_status']} {row.get('measurement_dependency_error') or row.get('fetch_error') or row.get('tier2_tzim', {}).get('error')}")
        if row.get("measurement_dependency_error"):
            failures.append(f"{row['product_id']}: measurement dependency unavailable: {row['measurement_dependency_error']}")
        if row.get("tier2_tzim", {}).get("error"):
            failures.append(f"{row['product_id']}: tzim failed: {row['tier2_tzim']['error']}")
        if row.get("array_header_roundtrip", {}).get("ok") is False:
            failures.append(f"{row['product_id']}: reconstructed array/header mismatch: {row['array_header_roundtrip'].get('reason')}")
        if row.get("funpack_array_header_roundtrip", {}).get("ok") is False:
            failures.append(f"{row['product_id']}: funpack array/header mismatch: {row['funpack_array_header_roundtrip'].get('reason')}")
        if (
            (row.get("existing_two_tier_instrument") or {}).get("status") == "tier2"
            and (row.get("reference_candidate_validation") or {}).get("status") != "passed"
        ):
            failures.append(
                f"{row['product_id']}: reference candidate validation did not pass: "
                f"{(row.get('reference_candidate_validation') or {}).get('reason')}"
            )
        unchanged_nonoptimized = bool(row.get("tier2_tzim", {}).get("unchanged_nonoptimized"))
        for name in ("fitsverify_source", "fitsverify_candidate", "funpack_source", "funpack_candidate"):
            check = row.get("external_checks", {}).get(name)
            if check is None or (not check.get("error") and check.get("returncode") in (None, 0)):
                continue
            # Source diagnostics establish provenance, not candidate validity. A
            # refused/no-gain row is judged by its Tzim status and byte identity;
            # per the FITS contract it is not required to pass external readers.
            if name == "fitsverify_source" or unchanged_nonoptimized:
                continue
            detail = check.get("error") or check.get("stderr") or check.get("stdout") or check.get("returncode")
            failures.append(f"{row['product_id']}: {name} failed: {diagnostic_excerpt(detail)}")
        if row.get("tier1_error"):
            failures.append(f"{row['product_id']}: existing tile Tier 1 comparator failed ({row['tier1_error']})")
        if row.get("temporary_cleanup_errors"):
            failures.append(f"{row['product_id']}: temporary cleanup failed: {row['temporary_cleanup_errors']}")
    if not positive_control_ok:
        failures.append(
            "external CFITSIO fpack/fitsverify/funpack -C positive control did not pass; "
            "survey header/payload differences are not attributable"
        )
    if failures:
        (out / "blockers.txt").write_text("\n".join(failures) + "\n", encoding="utf-8")
        print("\n".join(f"BLOCKER: {item}" for item in failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
