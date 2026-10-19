#!/bin/sh
# Exports the macOS menu-bar templates in src-tauri/icons/tray from their SVGs (macOS only).
# Quick Look renders the SVG (ImageMagick's built-in renderer drops strokes); white becomes alpha.
# The SVG sets the shape; the export sets the size, since the menu bar scales it to 18pt tall.
set -eu
dir="$(cd "$(dirname "$0")/../src-tauri/icons/tray" && pwd)"
work="$(mktemp -d)"
for n in tray tray-running; do
  sed 's/width="44" height="44"/width="176" height="176"/' "$dir/$n.svg" > "$work/$n.svg"
done
qlmanage -t -s 176 -o "$work" "$work/tray.svg" "$work/tray-running.svg" >/dev/null 2>&1
for n in tray tray-running; do
  # Quick Look draws at its own scale in a corner, so crop to the ink and center it at 42/44px.
  magick "$work/$n.svg.png" -alpha off -colorspace Gray -negate -background black -alpha shape \
    -trim +repage -resize 42x42 -background none -gravity center -extent 44x44 "PNG32:$dir/$n.png"
done
echo "Wrote $dir/tray.png and $dir/tray-running.png"
