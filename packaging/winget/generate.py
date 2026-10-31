#!/usr/bin/env python3
"""Render locally consumable winget manifests from the exact Windows release ZIP."""

from __future__ import annotations

import argparse
import hashlib
import re
import zipfile
from pathlib import Path

REPOSITORY = "RUdolf1517/tzim"
ASSET_NAME = "tzim-x86_64-pc-windows-msvc.zip"
TEMPLATE_DIR = Path(__file__).with_name("templates")
TEMPLATE_NAMES = {
    "version.yaml.in": "RUdolf1517.tzim.yaml",
    "installer.yaml.in": "RUdolf1517.tzim.installer.yaml",
    "locale.en-US.yaml.in": "RUdolf1517.tzim.locale.en-US.yaml",
}


def parse_tag(value: str) -> tuple[str, str]:
    tag = value.strip()
    if not tag.startswith("v"):
        tag = f"v{tag}"
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag):
        raise ValueError(f"expected a stable release tag such as v0.15.0, got {value!r}")
    return tag, tag[1:]


def hash_windows_asset(path: Path) -> str:
    if not path.is_file():
        raise FileNotFoundError(f"Windows release asset is missing: {path}")
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        if len(names) != 2 or set(names) != {"tzim.exe", "LICENSE"}:
            raise ValueError(f"unexpected Windows release ZIP entries: {names!r}")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest().upper()


def render(tag: str, version: str, archive_sha256: str, output_root: Path) -> tuple[Path, Path]:
    installer_url = f"https://github.com/{REPOSITORY}/releases/download/{tag}/{ASSET_NAME}"
    replacements = {
        "{{PACKAGE_VERSION}}": version,
        "{{RELEASE_TAG}}": tag,
        "{{WINDOWS_ARCHIVE_SHA256}}": archive_sha256,
    }
    manifest_dir = output_root / version
    manifest_dir.mkdir(parents=True, exist_ok=True)

    for source_name, destination_name in TEMPLATE_NAMES.items():
        source = TEMPLATE_DIR / source_name
        text = source.read_text(encoding="utf-8")
        for placeholder, replacement in replacements.items():
            text = text.replace(placeholder, replacement)
        if "{{" in text or "}}" in text:
            raise ValueError(f"unexpanded template placeholder in {source}")
        if destination_name.endswith(".installer.yaml") and installer_url not in text:
            raise ValueError("installer template did not produce the fixed release URL")
        with (manifest_dir / destination_name).open("w", encoding="utf-8", newline="\n") as destination:
            destination.write(text)

    bundle = output_root / f"tzim-winget-local-manifests-{tag}.zip"
    with zipfile.ZipFile(bundle, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for destination_name in TEMPLATE_NAMES.values():
            archive.write(manifest_dir / destination_name, f"{version}/{destination_name}")
    return manifest_dir, bundle


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", required=True, help="release tag, with or without the leading v")
    parser.add_argument("--windows-archive", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()

    tag, version = parse_tag(args.tag)
    archive_sha256 = hash_windows_asset(args.windows_archive)
    manifest_dir, bundle = render(tag, version, archive_sha256, args.output_dir)
    print(f"PackageVersion: {version}")
    print(f"InstallerUrl: https://github.com/{REPOSITORY}/releases/download/{tag}/{ASSET_NAME}")
    print(f"InstallerSha256: {archive_sha256}")
    print(f"Local manifest directory: {manifest_dir}")
    print(f"Local manifest bundle: {bundle}")


if __name__ == "__main__":
    main()
