#!/usr/bin/env bash
# Regenerate raster icon assets from packaging/icons/rylus.svg.
#
# Requires: ImageMagick (`magick` or `convert`). For macOS .icns, also requires
# `iconutil` (run on a macOS host).
#
# Usage: ./scripts/gen-icons.sh

set -euo pipefail

cd "$(dirname "$0")/.."

SVG="packaging/icons/rylus.svg"
OUT_DIR="packaging/icons"

if [[ ! -f "$SVG" ]]; then
    echo "error: $SVG not found" >&2
    exit 1
fi

if command -v magick >/dev/null 2>&1; then
    IM=(magick)
elif command -v convert >/dev/null 2>&1; then
    IM=(convert)
else
    echo "error: ImageMagick not found (install 'magick' or 'convert')" >&2
    exit 1
fi

echo "Rendering PNG variants from $SVG..."
for size in 16 32 48 64 128 256 512; do
    "${IM[@]}" -background none -density 384 "$SVG" \
        -resize "${size}x${size}" "$OUT_DIR/rylus-${size}.png"
done

# The canonical desktop icon for AUR's hicolor/scalable is the SVG itself;
# 256px PNG is kept for fallbacks and for CI bundling.
cp "$OUT_DIR/rylus-256.png" "$OUT_DIR/rylus.png"

# macOS .icns. Two paths:
#   1. iconutil (macOS only) — used when available, matches Apple's own tool.
#   2. rsvg-convert + a small inline Python assembler (Linux-capable) — used
#      as a fallback so this script also works in Linux CI and on this dev
#      box, where iconutil does not exist. Both paths produce an icns
#      covering 16px up to 1024px (1x/2x).
if command -v iconutil >/dev/null 2>&1; then
    echo "Rendering macOS .icns (iconutil)..."
    ICONSET="$OUT_DIR/rylus.iconset"
    rm -rf "$ICONSET"
    mkdir -p "$ICONSET"
    for size in 16 32 64 128 256 512; do
        cp "$OUT_DIR/rylus-${size}.png" "$ICONSET/icon_${size}x${size}.png"
        # Retina variant: next size up named @2x.
        case "$size" in
            16)  cp "$OUT_DIR/rylus-32.png"  "$ICONSET/icon_16x16@2x.png" ;;
            32)  cp "$OUT_DIR/rylus-64.png"  "$ICONSET/icon_32x32@2x.png" ;;
            128) cp "$OUT_DIR/rylus-256.png" "$ICONSET/icon_128x128@2x.png" ;;
            256) cp "$OUT_DIR/rylus-512.png" "$ICONSET/icon_256x256@2x.png" ;;
        esac
    done
    mkdir -p packaging/macos
    iconutil -c icns "$ICONSET" -o packaging/macos/icon.icns
    rm -rf "$ICONSET"
elif command -v rsvg-convert >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
    echo "Rendering macOS .icns (rsvg-convert + python3, Linux-capable fallback)..."
    mkdir -p packaging/macos
    python3 - "$SVG" packaging/macos/icon.icns <<'PYEOF'
# Build a PNG-backed .icns per the Apple Icon Image (ICNS) format: an 8-byte
# header ("icns" magic + big-endian total length), followed by a sequence of
# entries (4-byte OSType + big-endian 4-byte chunk length + PNG data).
#
# OSType-to-size table verified against the modern icns layout emitted by
# `iconutil` from a standard .iconset (cross-checked against the rust-icns
# crate's IconType definitions and the Wikipedia "Apple Icon Image format"
# OSType table): icp4/ic11/icp5/ic12/ic07/ic13/ic08/ic14/ic09/ic10.
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

svg_path, out_path = Path(sys.argv[1]), Path(sys.argv[2])

ICON_TYPES = [
    ("icp4", 16),    # 16x16 @1x
    ("ic11", 32),    # 16x16 @2x
    ("icp5", 32),    # 32x32 @1x
    ("ic12", 64),    # 32x32 @2x
    ("ic07", 128),   # 128x128 @1x
    ("ic13", 256),   # 128x128 @2x
    ("ic08", 256),   # 256x256 @1x
    ("ic14", 512),   # 256x256 @2x
    ("ic09", 512),   # 512x512 @1x
    ("ic10", 1024),  # 512x512 @2x
]

entries = []
with tempfile.TemporaryDirectory() as tmp:
    rendered = {}
    for _, size in ICON_TYPES:
        if size not in rendered:
            png_path = Path(tmp) / f"{size}.png"
            subprocess.run(
                [
                    "rsvg-convert",
                    "--width", str(size),
                    "--height", str(size),
                    "--keep-aspect-ratio",
                    "--output", str(png_path),
                    str(svg_path),
                ],
                check=True,
            )
            rendered[size] = png_path.read_bytes()
    for ostype, size in ICON_TYPES:
        data = rendered[size]
        entries.append(ostype.encode("ascii") + struct.pack(">I", 8 + len(data)) + data)

body = b"".join(entries)
out_path.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)
print(f"wrote {out_path} ({out_path.stat().st_size} bytes)")
PYEOF
else
    echo "warning: neither iconutil nor (rsvg-convert + python3) found; skipping .icns generation" >&2
fi

echo "Done. Generated icons under $OUT_DIR/"
