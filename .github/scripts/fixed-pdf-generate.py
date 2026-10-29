#!/usr/bin/env python3
"""Generate the exact 21 PDF inputs pre-registered in issue comment 6498702204.

The 18 repository-document outputs are produced by Pandoc/pdflatex, Pandoc DOCX
plus LibreOffice, and native AppKit/Quartz. The remaining three are scan-derived
PDFs made with pinned img2pdf; the source images are verified against the owner
manifest before conversion. Inputs and outputs are never selected by size or gain.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

SOURCE_COMMIT = "b457e9efa68c3a46be2470ffb222ed1bccc4dcbc"
SOURCE_URL = f"https://codeload.github.com/ocrmypdf/OCRmyPDF/tar.gz/{SOURCE_COMMIT}"
SOURCES = (
    ("readme", "README.md"),
    ("changelog", "CHANGELOG.md"),
    ("guarantees", "docs/guarantees.md"),
    ("formats", "docs/formats.md"),
    ("roadmap", "docs/roadmap.md"),
    ("measurement-status", "docs/measurement-status.md"),
)
SCANS = {
    "c03-29": {
        "path": "tests/resources/c03-29.pdf",
        "bytes": 167_938,
        "sha256": "c2ff83af7d028c95209cc7eebdf80fd5bb4cd292973ed6601e4bab7d1929f201",
        "description": "Project Gutenberg Huckleberry Finn, chapter 3 opening source scan",
        "permission": "REUSE.toml marks this source file public domain; tests/resources/README.rst identifies it.",
    },
    "linn": {
        "path": "tests/resources/linn.png",
        "bytes": 141_404,
        "sha256": "dcd90f08a6b8a733612f818330153636c13283dd548e2e092cad1fe628ec7bf3",
        "description": "Linn sequencer brochure scan",
        "permission": "(C) 1985 Forat Electronics; GFDL-1.2-or-later or CC-BY-SA-3.0; source README links the original 300-dpi scan.",
    },
    "typewriter": {
        "path": "tests/resources/typewriter.png",
        "bytes": 104_842,
        "sha256": "6f7a83685a83af954e9672b3e2db3253af165513d2826d780a48e175742f4469",
        "description": "Triumph typewriter text image",
        "permission": "(C) 2005 Ellywa; GFDL/CC-BY-SA licensing options with the Wikimedia source in REUSE.toml.",
    },
}


def digest(path: Path) -> tuple[int, str]:
    h = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while True:
            block = stream.read(1024 * 1024)
            if not block:
                break
            size += len(block)
            h.update(block)
    return size, h.hexdigest()


def command_record(argv: list[str], log: list[dict], cwd: Path | None = None) -> subprocess.CompletedProcess:
    try:
        result = subprocess.run(argv, cwd=cwd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        log.append({
            "argv": argv,
            "cwd": str(cwd) if cwd else None,
            "returncode": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        })
        return result
    except Exception as error:
        message = f"{type(error).__name__}: {error}"
        log.append({"argv": argv, "cwd": str(cwd) if cwd else None, "returncode": 127, "stdout": "", "stderr": message})
        return subprocess.CompletedProcess(argv, 127, "", message)


def checked_command(argv: list[str], log: list[dict], cwd: Path | None = None) -> subprocess.CompletedProcess:
    result = command_record(argv, log, cwd)
    if result.returncode:
        raise RuntimeError(
            f"command exited {result.returncode}: {argv!r}; "
            f"stdout: {result.stdout[-1000:]}; stderr: {result.stderr[-1000:]}"
        )
    return result


def tool_version(argv: list[str], log: list[dict]) -> str:
    result = command_record(argv, log)
    if result.returncode:
        return f"unavailable (exit {result.returncode}): {result.stderr[-500:]}"
    return (result.stdout or result.stderr).strip()


def extract_member(archive: tarfile.TarFile, suffix: str) -> bytes:
    matches = [member for member in archive.getmembers() if member.isfile() and member.name.endswith(suffix)]
    if len(matches) != 1:
        raise RuntimeError(f"expected exactly one archive member ending in {suffix!r}, found {len(matches)}")
    stream = archive.extractfile(matches[0])
    if stream is None:
        raise RuntimeError(f"could not read archive member {matches[0].name}")
    return stream.read()


def get_scan_sources() -> tuple[dict[str, bytes], dict[str, object]]:
    request = urllib.request.Request(SOURCE_URL, headers={"User-Agent": "tzim-fixed-measurement/1"})
    with urllib.request.urlopen(request, timeout=120) as response:
        archive_bytes = response.read()
        fetch = {
            "url": SOURCE_URL,
            "final_url": response.geturl(),
            "http_status": response.status,
            "content_length_header": response.headers.get("Content-Length"),
            "archive_bytes": len(archive_bytes),
            "archive_sha256": hashlib.sha256(archive_bytes).hexdigest(),
            "source_commit": SOURCE_COMMIT,
        }
    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:gz") as archive:
        names = {key: value["path"] for key, value in SCANS.items()}
        payloads = {key: extract_member(archive, "/" + path) for key, path in names.items()}
        permission_files = {
            "README.rst": extract_member(archive, "/tests/resources/README.rst"),
            "REUSE.toml": extract_member(archive, "/REUSE.toml"),
        }
    permission_info = {}
    for name, data in permission_files.items():
        permission_info[name] = {
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
            "relevant_lines": [
                line.decode("utf-8", "replace")
                for line in data.splitlines()
                if any(word in line.lower() for word in (b"linn", b"typewriter", b"c03-29", b"huckleberry", b"forat", b"ellywa"))
            ],
        }
    return payloads, {"fetch": fetch, "permission_files": permission_info}


def render_manifest(args: argparse.Namespace) -> dict:
    root = Path(args.source_root).resolve()
    out_root = Path(args.out_root).resolve()
    out_root.mkdir(parents=True, exist_ok=True)
    generation_log: list[dict] = []
    tools = {
        "python": sys.version,
        "pandoc": tool_version(["pandoc", "--version"], generation_log) if args.kind == "ubuntu" else None,
        "pdflatex": tool_version(["pdflatex", "--version"], generation_log) if args.kind == "ubuntu" else None,
        "soffice": tool_version(["soffice", "--version"], generation_log) if args.kind == "ubuntu" else None,
        "swift": tool_version(["swift", "--version"], generation_log) if args.kind == "quartz" else None,
        "xcodebuild": tool_version(["xcodebuild", "-version"], generation_log) if args.kind == "quartz" else None,
    }
    try:
        import img2pdf
        tools["img2pdf"] = getattr(img2pdf, "__version__", "unknown") if args.kind == "ubuntu" else None
    except ImportError:
        tools["img2pdf"] = None
    try:
        import pymupdf
        tools["pymupdf"] = {
            "version_bind": getattr(pymupdf, "VersionBind", "unknown"),
            "version_extra": getattr(pymupdf, "VersionFitz", "unknown"),
        } if args.kind == "ubuntu" else None
    except ImportError:
        tools["pymupdf"] = None
    if args.kind == "ubuntu":
        tools["img2pdf_cli"] = tool_version(["img2pdf", "--version"], generation_log)

    records: list[dict] = []
    for slug, source_name in SOURCES:
        if args.kind == "quartz":
            output_rel = f"quartz/{slug}.pdf"
        elif args.kind == "ubuntu":
            # Both Ubuntu producers are expanded below, retaining one row per output.
            output_rel = ""
        else:
            raise ValueError(args.kind)
        source = root / source_name
        if not source.is_file():
            for category in (("quartz",) if args.kind == "quartz" else ("latex", "libreoffice")):
                records.append({"id": f"{category}/{slug}", "path": f"{category}/{slug}.pdf", "status": "failed", "error": f"source file missing: {source_name}"})
            continue
        source_bytes, source_hash = digest(source)
        source_rec = {"path": source_name, "bytes": source_bytes, "sha256": source_hash, "repository_commit": args.source_sha}
        categories = ("quartz",) if args.kind == "quartz" else ("latex", "libreoffice")
        for category in categories:
            output_rel = f"{category}/{slug}.pdf"
            output = out_root / output_rel
            output.parent.mkdir(parents=True, exist_ok=True)
            row = {
                "id": f"{category}/{slug}",
                "path": output_rel,
                "status": "failed",
                "source": source_rec,
                "producer": "AppKit NSPrintOperation.pdfOperation with native Quartz PDF output" if category == "quartz" else (
                    "Pandoc + pdflatex" if category == "latex" else "Pandoc Markdown-to-DOCX + headless LibreOffice"
                ),
                "commands": [],
                "tool_versions": tools,
                "environment": {"kind": args.kind, "platform": platform.platform(), "python": platform.python_version()},
            }
            try:
                if category == "latex":
                    header = root / ".github/scripts/pdflatex-unicode-header.tex"
                    header_size, header_sha256 = digest(header)
                    row["generation_support_files"] = [
                        {
                            "path": header.relative_to(root).as_posix(),
                            "bytes": header_size,
                            "sha256": header_sha256,
                        }
                    ]
                    argv = [
                        "pandoc", "--from=gfm", "--standalone", "--pdf-engine=pdflatex",
                        "--verbose", "--include-in-header", str(header),
                        "-V", "geometry:margin=1in", "--output", str(output), str(source),
                    ]
                    checked_command(argv, generation_log)
                    row["commands"].append(argv)
                elif category == "libreoffice":
                    work = Path(args.work_root).resolve() / "docx"
                    work.mkdir(parents=True, exist_ok=True)
                    docx = work / f"{slug}.docx"
                    pandoc_argv = [
                        "pandoc", "--from=gfm", "--to=docx", "--standalone",
                        "--output", str(docx), str(source),
                    ]
                    checked_command(pandoc_argv, generation_log)
                    lo_argv = [
                        "soffice", "--headless", "--convert-to", "pdf",
                        "--outdir", str(output.parent), str(docx),
                    ]
                    checked_command(lo_argv, generation_log)
                    row["commands"].extend([pandoc_argv, lo_argv])
                    generated = output.parent / f"{slug}.pdf"
                    if not generated.is_file():
                        raise RuntimeError(f"LibreOffice returned success but produced no {generated}")
                else:
                    swift_script = root / ".github/scripts/quartz-markdown.swift"
                    argv = ["swift", str(swift_script), str(source), str(output), slug]
                    checked_command(argv, generation_log)
                    row["commands"].append(argv)
                    pdf_bytes = output.read_bytes()
                    if b"AppKit/Quartz" not in pdf_bytes and b"Quartz" not in pdf_bytes:
                        raise RuntimeError("Quartz producer marker is missing from the generated PDF bytes")
                    row["producer_metadata_check"] = "Quartz marker is embedded in PDF producer metadata"
                size, sha = digest(output)
                row.update(status="generated", bytes=size, sha256=sha)
            except Exception as error:  # retain every pre-registered output row
                row["error"] = f"{type(error).__name__}: {error}"
            records.append(row)

    scan_provenance = None
    source_meta: dict[str, dict] = {}
    if args.kind == "ubuntu":
        try:
            source_payloads, provenance = get_scan_sources()
            scan_provenance = provenance
            source_meta: dict[str, dict] = {}
            source_bytes_by_name: dict[str, bytes] = {}
            for slug, definition in SCANS.items():
                raw = source_payloads[slug]
                observed = hashlib.sha256(raw).hexdigest()
                if len(raw) != definition["bytes"] or observed != definition["sha256"]:
                    raise RuntimeError(
                        f"pinned scan source mismatch for {definition['path']}: "
                        f"{len(raw)} bytes / {observed}, expected {definition['bytes']} / {definition['sha256']}"
                    )
                source_bytes_by_name[slug] = raw
                source_meta[slug] = {**definition, "observed_bytes": len(raw), "observed_sha256": observed}

            import pymupdf
            import img2pdf
            with tempfile.TemporaryDirectory(prefix="tzim-fixed-pdf-scans-") as temp_name:
                temp_root = Path(temp_name)
                for slug in ("linn", "typewriter"):
                    ext = Path(SCANS[slug]["path"]).suffix
                    image_path = temp_root / f"{slug}{ext}"
                    image_path.write_bytes(source_bytes_by_name[slug])
                    output_rel = f"scans/{slug}.pdf"
                    output = out_root / output_rel
                    output.parent.mkdir(parents=True, exist_ok=True)
                    argv = ["img2pdf", str(image_path), "--output", str(output)]
                    row = {
                        "id": output_rel.removesuffix(".pdf"),
                        "path": output_rel,
                        "status": "failed",
                        "source": {"repository": "ocrmypdf/OCRmyPDF", "commit": SOURCE_COMMIT, **source_meta[slug]},
                        "producer": "img2pdf lossless image-to-PDF conversion",
                        "tool_versions": {"img2pdf": tools["img2pdf"], "pymupdf": tools["pymupdf"]},
                        "commands": [argv],
                        "environment": {"kind": args.kind, "platform": platform.platform(), "python": platform.python_version()},
                        "content_and_permission": {"description": SCANS[slug]["description"], "basis": SCANS[slug]["permission"]},
                        "permission_source_evidence": provenance["permission_files"],
                    }
                    try:
                        checked_command(argv, generation_log)
                        size, sha = digest(output)
                        row.update(status="generated", bytes=size, sha256=sha,
                                   image_sha256=source_meta[slug]["sha256"], extraction="no extraction; pinned PNG source passed directly to img2pdf")
                    except Exception as error:
                        row["error"] = f"{type(error).__name__}: {error}"
                    records.append(row)

                slug = "c03-29"
                source_pdf = temp_root / "c03-29.pdf"
                source_pdf.write_bytes(source_bytes_by_name[slug])
                image_paths: list[Path] = []
                extracted: list[dict] = []
                with pymupdf.open(source_pdf) as document:
                    seen_xrefs: set[int] = set()
                    for page_number, page in enumerate(document, start=1):
                        for image_info in page.get_images(full=True):
                            xref = int(image_info[0])
                            smask = int(image_info[1]) if len(image_info) > 1 else 0
                            if xref <= 0 or xref in seen_xrefs:
                                continue
                            seen_xrefs.add(xref)
                            item = document.extract_image(xref)
                            ext = str(item.get("ext", "bin")).lower()
                            data = item["image"]
                            method = "PyMuPDF Document.extract_image (original embedded image stream; no resampling)"
                            if smask > 0:
                                base_pixmap = pymupdf.Pixmap(document, xref)
                                mask_pixmap = pymupdf.Pixmap(document, smask)
                                combined = pymupdf.Pixmap(base_pixmap, mask_pixmap)
                                ext = "png"
                                data = combined.tobytes("png")
                                method = "PyMuPDF Pixmap + soft-mask merge, lossless PNG encoding; no resampling"
                            image_path = temp_root / f"c03-29-{len(image_paths):03d}.{ext}"
                            image_path.write_bytes(data)
                            image_paths.append(image_path)
                            extracted.append({
                                "page": page_number, "xref": xref, "soft_mask_xref": smask or None,
                                "path": image_path.name, "bytes": len(data),
                                "sha256": hashlib.sha256(data).hexdigest(), "extension": ext,
                                "method": method,
                            })
                output_rel = "scans/c03-29.pdf"
                output = out_root / output_rel
                output.parent.mkdir(parents=True, exist_ok=True)
                row = {
                    "id": "scans/c03-29", "path": output_rel, "status": "failed",
                    "source": {"repository": "ocrmypdf/OCRmyPDF", "commit": SOURCE_COMMIT, **source_meta[slug]},
                    "producer": "PyMuPDF lossless embedded-image extraction + img2pdf",
                    "tool_versions": {"img2pdf": tools["img2pdf"], "pymupdf": tools["pymupdf"]},
                    "commands": [
                        "pymupdf.Document.extract_image(xref) for every unique embedded image XRef in source page order; soft masks, if any, are merged with Pixmap and losslessly PNG-encoded without resampling",
                        ["img2pdf", *[str(path) for path in image_paths], "--output", str(output)],
                    ],
                    "environment": {"kind": args.kind, "platform": platform.platform(), "python": platform.python_version()},
                    "content_and_permission": {"description": SCANS[slug]["description"], "basis": SCANS[slug]["permission"]},
                    "permission_source_evidence": provenance["permission_files"],
                    "extracted_images": extracted,
                    "provenance": provenance,
                }
                try:
                    if not image_paths:
                        raise RuntimeError("the pinned source PDF contains no embedded images to extract")
                    argv = ["img2pdf", *[str(path) for path in image_paths], "--output", str(output)]
                    checked_command(argv, generation_log)
                    size, sha = digest(output)
                    row.update(status="generated", bytes=size, sha256=sha)
                except Exception as error:
                    row["error"] = f"{type(error).__name__}: {error}"
                records.append(row)
        except Exception as error:
            # Keep source provenance and all three pinned scan rows, even if a tool is unavailable.
            archive_fetch_failed = scan_provenance is None
            if scan_provenance is None:
                scan_provenance = {"url": SOURCE_URL, "source_commit": SOURCE_COMMIT}
            scan_provenance["generation_error"] = f"{type(error).__name__}: {error}"
            present = {row.get("path") for row in records}
            for slug, definition in SCANS.items():
                output_rel = f"scans/{slug}.pdf"
                if output_rel in present:
                    continue
                row = {
                    "id": output_rel.removesuffix(".pdf"), "path": output_rel, "status": "failed",
                    "source_uri": SOURCE_URL,
                    "source": {
                        "repository": "ocrmypdf/OCRmyPDF", "commit": SOURCE_COMMIT,
                        **source_meta.get(slug, definition),
                    },
                    "producer": "img2pdf scan-derived PDF", "error": f"{type(error).__name__}: {error}",
                    "permission_source_evidence": (scan_provenance or {}).get("permission_files"),
                    "provenance": scan_provenance,
                    "environment": {"kind": args.kind, "platform": platform.platform(), "python": platform.python_version()},
                }
                if archive_fetch_failed:
                    row["needed_owner_action"] = (
                        "Make the exact pinned OCRmyPDF source archive retrievable or provide an authorized mirror "
                        f"of the same commit; do not substitute the source product: {SOURCE_URL}"
                    )
                records.append(row)

    package_versions = None
    if args.kind == "ubuntu":
        packages = [
            "pandoc", "texlive-latex-base", "texlive-latex-recommended",
            "texlive-latex-extra", "texlive-fonts-recommended", "lmodern",
            "librsvg2-bin", "libreoffice-writer", "fonts-dejavu-core", "fonts-liberation",
        ]
        package_versions = command_record(
            ["dpkg-query", "-W", "-f=${binary:Package}=${Version}\\n", *packages],
            generation_log,
        ).stdout
    elif args.kind == "quartz":
        package_versions = {
            "sw_vers": command_record(["sw_vers"], generation_log).stdout,
            "xcodebuild": tools["xcodebuild"],
        }
    output_manifest = {
        "schema": "tzim-fixed-pdf-generation-v1",
        "source_decision_comment": "6498702204",
        "kind": args.kind,
        "repository_source_sha": args.source_sha,
        "environment": {
            "platform": platform.platform(),
            "python": sys.version,
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "tool_versions": tools,
            "tool_package_versions": package_versions,
            "tool_version_commands": generation_log[:6],
        },
        "source_archive": scan_provenance,
        "outputs": records,
    }
    manifest_path = Path(args.manifest)
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps(output_manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    log_path = out_root / "generation-commands.json"
    log_path.write_text(json.dumps(generation_log, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    failed = [row for row in records if row.get("status") != "generated"]
    print(f"generated {len(records) - len(failed)} / {len(records)} pre-registered PDFs")
    for row in failed:
        print(f"FAILED {row.get('path')}: {row.get('error', 'no output')}", file=sys.stderr)
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=("ubuntu", "quartz"), required=True)
    parser.add_argument("--source-root", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--out-root", required=True)
    parser.add_argument("--work-root", required=True)
    parser.add_argument("--manifest", required=True)
    return render_manifest(parser.parse_args())


if __name__ == "__main__":
    raise SystemExit(main())
