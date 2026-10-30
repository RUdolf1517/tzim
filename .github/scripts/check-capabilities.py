#!/usr/bin/env python3
"""Assert the public capabilities contract for representative release builds."""

import argparse
import json
from pathlib import Path


def read(path: str) -> dict:
    value = json.loads(Path(path).read_text(encoding="utf-8"))
    assert value["type"] == "capabilities", path
    assert value["capabilities_schema_version"] == 1, path
    assert value["result_schema_version"] == 2, path
    assert value["side_effects"] == {
        "scans_files": False,
        "writes_files": False,
        "prompts": False,
    }, path
    assert value["selection_flags"][0]["semantics"] == "allowlist", path
    assert value["selection_flags"][0]["does_not_grant_handler_opt_ins"] is True, path
    return value


def check_default(value: dict) -> None:
    features = value["build_features"]
    assert features["default"] is True
    assert features["jpeg"] is True and features["jpeg-v1"] is True
    assert "jpeg" in value["compiled_formats"]
    assert "jpeg" in value["default_enabled_formats"]
    assert "fits" not in value["compiled_formats"]
    assert "fits" not in value["default_enabled_formats"]
    assert [item["format"] for item in value["handler_opt_ins"]] == ["jpeg"]
    assert all(item["flag"] != "--fits-tile" for item in value["transformations"])
    assert value["confirmation_policies"] == []


def check_codec_only(value: dict) -> None:
    features = value["build_features"]
    assert features["default"] is False
    assert features["jpeg"] is True and features["jpeg-v1"] is False
    assert value["compiled_formats"] == ["jpeg"]
    assert "jpeg" not in value["default_enabled_formats"]
    assert [item["format"] for item in value["handler_opt_ins"]] == ["jpeg"]
    jpeg = next(item for item in value["handler_opt_ins"] if item["format"] == "jpeg")
    assert jpeg["available"] is True and jpeg["default_enabled"] is False
    assert jpeg["codec_only_selection_flag"] == "--formats jpeg"


def check_all_features(value: dict) -> None:
    features = value["build_features"]
    assert features["default"] is True
    assert all(features[name] for name in ("jpeg", "jpeg-v1", "fits", "flac", "pdf"))
    assert all(name in value["compiled_formats"] for name in ("jpeg", "fits", "flac", "pdf"))
    assert all(name in value["default_enabled_formats"] for name in ("jpeg", "flac", "pdf"))
    assert [item["format"] for item in value["handler_opt_ins"]] == [
        "jpeg",
        "fits",
        "flac",
        "pdf",
    ]
    assert "fits" not in value["default_enabled_formats"]
    fits = next(item for item in value["handler_opt_ins"] if item["format"] == "fits")
    assert fits["available"] is True and fits["default_enabled"] is False
    assert fits["runtime_opt_in_flag"] == "--fits-tile"
    assert fits["allowlist_is_not_consent"] is True
    assert [policy["id"] for policy in value["confirmation_policies"]] == [
        "fits_tile_confirmation"
    ]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--default", required=True)
    parser.add_argument("--codec-only", required=True)
    parser.add_argument("--all-features", required=True)
    args = parser.parse_args()

    check_default(read(args.default))
    check_codec_only(read(args.codec_only))
    check_all_features(read(args.all_features))
    print("capabilities schema 1 / result schema 2: default, codec-only, all-features OK")


if __name__ == "__main__":
    main()
