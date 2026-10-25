#!/usr/bin/env python3
"""Check that every relative link and image in the Markdown files resolves.

Three failures are possible, and all three have shipped in this repository:

  * a link to a file that does not exist (`tests/fixtures/`, a deleted doc);
  * a link to a heading that was renamed (`#49-flac--contract-written-not-implemented`
    after the section became "refused after measurement");
  * a link to a heading that never existed, because the anchor was typed by hand
    and the numbering in the document had shifted by one since.

The checker parses nothing cleverly: it finds `[text](target)` and `[text]: target`,
ignores absolute URLs, fragments that are not ours, and code fences (where example
commands live). Anchors are derived the way GitHub derives them — lowercase, keep
letters, digits, spaces, hyphens and underscores, turn spaces into hyphens, drop
everything else — and de-duplicated with `-1`, `-2` suffixes for repeated headings.

Usage:  python3 .github/scripts/md-links.py [--quiet]
Exit:   0 when nothing is broken, 1 otherwise. The CI job greps for `broken: 0`.
"""

import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SKIP_DIRS = {".git", "target", "fuzz/corpus", "fuzz/artifacts", "node_modules"}

LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
REF = re.compile(r"^\s*\[[^\]]+\]:\s*(\S+)")
FENCE = re.compile(r"^\s*(```|~~~)")


def markdown_files():
    for base, dirs, files in os.walk(ROOT):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS)
        for name in sorted(files):
            if name.endswith(".md"):
                yield os.path.join(base, name)


def strip_code(text):
    """Blank out fenced blocks and inline code, so examples are not checked."""
    out = []
    inside = False
    for line in text.splitlines():
        if FENCE.match(line):
            inside = not inside
            out.append("")
            continue
        out.append("" if inside else line)
    text = "\n".join(out)
    return re.sub(r"`[^`\n]*`", "", text)


def slug(heading):
    """GitHub's anchor for a heading, approximately: it is stable for our text."""
    heading = heading.strip().lower()
    # Emphasis and inline code markers do not survive into the anchor.
    heading = re.sub(r"[`*_~]", "", heading)
    out = []
    for ch in heading:
        if ch.isalnum() or ch in "-_":
            out.append(ch)
        elif ch == " ":
            out.append("-")
    return "".join(out)


def anchors(path):
    found = set()
    counts = {}
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            match = re.match(r"^#{1,6}\s+(.*?)\s*$", line)
            if not match:
                continue
            base = slug(match.group(1))
            seen = counts.get(base, 0)
            counts[base] = seen + 1
            found.add(base if seen == 0 else f"{base}-{seen}")
    return found


def main():
    quiet = "--quiet" in sys.argv
    anchor_cache = {}
    broken = 0
    checked = 0
    links = 0
    for path in markdown_files():
        checked += 1
        rel = os.path.relpath(path, ROOT)
        text = strip_code(open(path, encoding="utf-8").read())
        targets = [m.group(1) for m in LINK.finditer(text)]
        targets += [m.group(1) for m in REF.finditer(text) if m.group(1)]
        for target in targets:
            links += 1
            target = target.split(" ")[0].strip("<>")
            if not target or target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            if target.startswith("//"):
                continue
            file_part, _, fragment = target.partition("#")
            if not file_part:
                continue
            resolved = os.path.normpath(os.path.join(os.path.dirname(path), file_part))
            if not os.path.exists(resolved):
                print(f"{rel}: no such file: {target}")
                broken += 1
                continue
            if not fragment or not resolved.endswith(".md"):
                continue
            if resolved not in anchor_cache:
                anchor_cache[resolved] = anchors(resolved)
            if fragment not in anchor_cache[resolved]:
                print(f"{rel}: no such anchor: {target}")
                broken += 1
    if not quiet:
        print(f"checked {checked} files, {links} links: broken: {broken}")
    else:
        print(f"broken: {broken}")
    return 1 if broken else 0


if __name__ == "__main__":
    sys.exit(main())
