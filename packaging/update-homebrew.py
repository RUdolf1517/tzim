"""Update the Homebrew formula from the checksums of a published release."""

import argparse
from pathlib import Path
import re


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("checksums", type=Path)
    args = parser.parse_args()

    version = r"[0-9]+\.[0-9]+\.[0-9]+"
    if not re.fullmatch(f"v{version}", args.tag):
        parser.error("expected a stable release tag in the form vX.Y.Z")

    formula = Path(__file__).resolve().parents[1] / "Formula/tzim.rb"
    source = formula.read_text(encoding="utf-8")
    version_line = rf'^  version "({version})"$'
    current = re.findall(version_line, source, re.MULTILINE)
    if len(current) != 1:
        raise SystemExit("Expected exactly one version in the Homebrew formula")
    if tuple(map(int, args.tag[1:].split("."))) < tuple(map(int, current[0].split("."))):
        return

    checksums = {}
    for line in args.checksums.read_text(encoding="utf-8").splitlines():
        checksum, asset = line.split(maxsplit=1)
        asset = asset.removeprefix("./")
        if asset in checksums or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise SystemExit(f"Invalid or duplicate checksum for {asset}")
        checksums[asset] = checksum

    for target in (
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-musl",
    ):
        asset = f"tzim-{target}.tar.gz"
        if asset not in checksums:
            raise SystemExit(f"Missing checksum for {asset}")
        url = "https://github.com/RUdolf1517/tzim/releases/download/"
        pattern = (
            rf'^      url "{re.escape(url)}v{re.escape(current[0])}/{re.escape(asset)}"\n'
            r'      sha256 "[0-9a-f]{64}"$'
        )
        replacement = f'      url "{url}{args.tag}/{asset}"\n      sha256 "{checksums[asset]}"'
        source, count = re.subn(pattern, replacement, source, flags=re.MULTILINE)
        if count != 1:
            raise SystemExit(f"Expected exactly one formula URL and checksum for {asset}")

    source = re.sub(version_line, f'  version "{args.tag[1:]}"', source, flags=re.MULTILINE)
    formula.write_text(source, encoding="utf-8")


if __name__ == "__main__":
    main()
