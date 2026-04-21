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

# macOS .icns (only runs if iconutil is available — i.e. on macOS).
if command -v iconutil >/dev/null 2>&1; then
    echo "Rendering macOS .icns..."
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
fi

echo "Done. Generated icons under $OUT_DIR/"
