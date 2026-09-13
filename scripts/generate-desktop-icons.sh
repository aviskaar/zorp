#!/bin/sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
SVG="$ROOT_DIR/docs-site/book/favicon-de23e50b.svg"
ICONS_DIR="$ROOT_DIR/zorp-desktop/icons"

mkdir -p "$ICONS_DIR"
TMP_PNG="$ICONS_DIR/icon-1024.png"

if command -v qlmanage >/dev/null 2>&1; then
    qlmanage -t -s 1024 -o "$ICONS_DIR" "$SVG"
    mv "$ICONS_DIR/favicon-de23e50b.svg.png" "$TMP_PNG" 2>/dev/null || true
elif command -v rsvg-convert >/dev/null 2>&1; then
    rsvg-convert -w 1024 -h 1024 "$SVG" -o "$TMP_PNG"
fi

if [ -f "$TMP_PNG" ] && command -v sips >/dev/null 2>&1; then
    sips -z 32 32 "$TMP_PNG" --out "$ICONS_DIR/32x32.png"
    sips -z 128 128 "$TMP_PNG" --out "$ICONS_DIR/128x128.png"
    sips -z 256 256 "$TMP_PNG" --out "$ICONS_DIR/128x128@2x.png"
    sips -z 512 512 "$TMP_PNG" --out "$ICONS_DIR/icon.png"
    cp "$ICONS_DIR/32x32.png" "$ICONS_DIR/icon.ico"
fi

if [ -f "$TMP_PNG" ] && command -v iconutil >/dev/null 2>&1 && command -v sips >/dev/null 2>&1; then
    ICONSET="$ICONS_DIR/zorp.iconset"
    mkdir -p "$ICONSET"
    sips -z 16 16 "$TMP_PNG" --out "$ICONSET/icon_16x16.png"
    sips -z 32 32 "$TMP_PNG" --out "$ICONSET/icon_16x16@2x.png"
    sips -z 32 32 "$TMP_PNG" --out "$ICONSET/icon_32x32.png"
    sips -z 64 64 "$TMP_PNG" --out "$ICONSET/icon_32x32@2x.png"
    sips -z 128 128 "$TMP_PNG" --out "$ICONSET/icon_128x128.png"
    sips -z 256 256 "$TMP_PNG" --out "$ICONSET/icon_128x128@2x.png"
    sips -z 256 256 "$TMP_PNG" --out "$ICONSET/icon_256x256.png"
    sips -z 512 512 "$TMP_PNG" --out "$ICONSET/icon_256x256@2x.png"
    sips -z 512 512 "$TMP_PNG" --out "$ICONSET/icon_512x512.png"
    sips -z 1024 1024 "$TMP_PNG" --out "$ICONSET/icon_512x512@2x.png"
    iconutil -c icns "$ICONSET" -o "$ICONS_DIR/icon.icns"
    rm -rf "$ICONSET"
fi
