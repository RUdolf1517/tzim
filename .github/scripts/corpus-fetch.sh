#!/usr/bin/env bash
# Downloads the pinned upstream archives, verifies their SHA-256, and assembles
# the real-file corpus used by the corpus run and by the benchmarks.
#
# The corpus is never committed: it is large, and its provenance is better
# expressed as a URL plus a checksum, both of which live here.
#
# Usage: corpus-fetch.sh <work-dir>
#   produces <work-dir>/corpus/{images,gzip,zip} and prints the file count
#   exits non-zero when an archive does not match its checksum or expands to
#   fewer files than expected

set -euo pipefail

work="${1:?usage: corpus-fetch.sh <work-dir>}"
src="$work/src"
corpus="$work/corpus"

rm -rf "$work"
mkdir -p "$src" "$corpus/images" "$corpus/gzip" "$corpus/zip"

fetch() {
    local url="$1" want="$2" out="$3"
    echo "  ${url##*/}"
    curl -fsSL --retry 3 --retry-delay 2 -o "$out" "$url"
    echo "${want}  ${out}" | sha256sum -c --quiet -
}

echo "== pinned sources =="
fetch "https://codeload.github.com/pnggroup/libpng/tar.gz/refs/tags/v1.6.44" \
    "0ef5b633d0c65f780c4fced27ff832998e71478c13b45dfb6e94f23a82f64f7c" "$src/libpng.tar.gz"
fetch "https://codeload.github.com/libjpeg-turbo/libjpeg-turbo/tar.gz/refs/tags/3.1.0" \
    "35fec2e1ddfb05ecf6d93e50bc57c1e54bc81c16d611ddf6eff73fff266d8285" "$src/libjpeg-turbo.tar.gz"
fetch "https://codeload.github.com/image-rs/image/tar.gz/refs/tags/v0.25.6" \
    "e547553fc4e4353b9576a4f2f22c49a1cdaa4aeb683e363c78223fb9b94b8c42" "$src/image.tar.gz"
fetch "https://codeload.github.com/python-pillow/Pillow/tar.gz/refs/tags/11.3.0" \
    "fa4aca745b1e1c733589ebf0ef19491b145dd4225c4aa06958963b4e7f0734cf" "$src/pillow.tar.gz"

for archive in "$src"/*.tar.gz; do
    tar xzf "$archive" -C "$src"
done

# Flattens a path into a unique file name: the source tag plus the path under
# the archive, with separators replaced. Keeps the provenance visible in the
# report, which is what makes a failure actionable.
copy_into() {
    local dir="$1" tag="$2" root="$3"
    local f rel
    while IFS= read -r -d '' f; do
        rel="${f#"$root"/}"
        cp "$f" "$corpus/images/$tag-${rel//\//_}"
    done < <(find "$dir" -type f \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' \
        -o -iname '*.tif' -o -iname '*.tiff' \) -print0 | sort -z)
}

echo "== assembling =="
copy_into "$src/libpng-1.6.44/contrib/pngsuite" "pngsuite" "$src/libpng-1.6.44/contrib/pngsuite"
copy_into "$src/image-0.25.6/tests/images" "image-rs" "$src/image-0.25.6/tests/images"
copy_into "$src/libjpeg-turbo-3.1.0/testimages" "libjpeg-turbo" "$src/libjpeg-turbo-3.1.0/testimages"
copy_into "$src/Pillow-11.3.0/Tests/images" "pillow" "$src/Pillow-11.3.0/Tests/images"

# Containers built on the spot from those files, so gzip and ZIP are exercised on
# real payloads rather than on handwritten fixtures.
gzip -9 -n -c "$corpus/images/pngsuite-basn2c08.png" > "$corpus/gzip/pngsuite-basn2c08.png.gz"
gzip -9 -n -c "$corpus/images/pngsuite-basn6a08.png" > "$corpus/gzip/pngsuite-basn6a08.png.gz"
gzip -9 -n -c "$corpus/images/libjpeg-turbo-testorig.jpg" > "$corpus/gzip/testorig.jpg.gz"
(cd "$corpus/images" && zip -q -9 "$corpus/zip/pngsuite-subset.zip" \
    pngsuite-basn0g08.png pngsuite-basn2c08.png pngsuite-basn4a08.png pngsuite-basn6a08.png)

# zlib streams, built the same way and for the same reason (#36): the handler
# was measured by the benchmark but never regression-gated on real data. Level 6
# with the default strategy, which is what every other program produces and what
# leaves room for a better encoder to win.
mkdir -p "$corpus/zlib"
python3 - "$corpus" <<'PY'
import sys, zlib

corpus = sys.argv[1]
# Two kinds of payload: image bytes (already compressed, so the gain is small
# and a wrong byte is obvious) and a text blob (compressible, so the encoder is
# actually exercised).
sources = {
    "pngsuite-basn2c08.png.zlib": "images/pngsuite-basn2c08.png",
    "pngsuite-basn6a08.png.zlib": "images/pngsuite-basn6a08.png",
    "libjpeg-turbo-testorig.jpg.zlib": "images/libjpeg-turbo-testorig.jpg",
}
streams = 0
for out, source in sources.items():
    with open(f"{corpus}/{source}", "rb") as fh:
        data = fh.read()
    with open(f"{corpus}/zlib/{out}", "wb") as fh:
        # A zlib stream: two-byte header, DEFLATE payload, Adler-32 trailer.
        fh.write(zlib.compress(data, 6))
    streams += 1

text = (b"tzim shrinks the file, never the information. " * 400)
with open(f"{corpus}/zlib/text.zlib", "wb") as fh:
    fh.write(zlib.compress(text, 6))
streams += 1
print(f"zlib streams: {streams}")
PY

total="$(find "$corpus" -type f | wc -l | tr -d ' ')"
images="$(find "$corpus/images" -type f | wc -l | tr -d ' ')"
echo "corpus: $total files ($images images, $((total - images)) containers)"
if [ "$images" -lt 400 ]; then
    echo "::error::only $images images were assembled; the pinned archives did not expand as expected"
    exit 1
fi

