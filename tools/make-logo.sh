#!/usr/bin/env bash
#
#  make-logo.sh
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Render the shipped logo from the master in assets/ (WP-19).
#
#  MIT License - see the header in any Rust source file for the full text.
#
# The master is 1361x1046 and 563 KB, which is the right size for a master and
# the wrong size for a file the application serves. The only place the logo
# appears in the UI is the empty library, where it is drawn at 260 CSS pixels,
# so what ships is twice that and nothing more. A webp in `public/` rather than
# an import: Vite copies `public/` verbatim, so the image stays a file the
# browser fetches once instead of becoming base64 inside the JavaScript bundle.
#
# Run it after editing the master and commit what changes. The square icon is a
# separate drawing, not a crop of this - see tools/make-icons.sh.

set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
master="$here/assets/vcw_logo_main.webp"
out="$here/app/ui/public/vcw-logo.webp"

command -v convert >/dev/null || { echo "need ImageMagick's convert" >&2; exit 1; }
[ -f "$master" ] || { echo "no master at $master" >&2; exit 1; }

mkdir -p "$(dirname "$out")"
convert "$master" -resize 520x -quality 82 "$out"

echo "$out: $(identify -format '%wx%h, %b' "$out")"
