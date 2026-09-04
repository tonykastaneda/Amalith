#!/usr/bin/env bash
# Regenerate the app icon (crates/amalith-shell/assets/app-icon.png) from the
# master icon export.
#
# Source: branding/icn-comp-iOS-Default-1024@1x.png — a full-bleed 1024x1024
# iOS-style icon (squircle + gradient already baked in, 16-bit).
#
# macOS wants the artwork on its Dock grid: an ~824x824 content square centred
# in a 1024x1024 canvas (~100px transparent margin), so it doesn't read larger
# than the system icons. This script flattens to 8-bit and does that inset.
# The source already has rounded corners, so no extra corner mask is applied.
# Re-run after each re-export.
#
# Needs ImageMagick 7 (`magick`).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
src="$here/icn-comp-iOS-Default-1024@1x.png"
out="$here/../crates/amalith-shell/assets/app-icon.png"

magick "$src" -depth 8 -alpha on \
  -resize 824x824 -background none -gravity center -extent 1024x1024 \
  -define png:color-type=6 "$out"

echo "wrote $out"
magick identify "$out"
