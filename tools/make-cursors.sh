#!/usr/bin/env bash
#
#  make-cursors.sh
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Render the waveform's select cursor from the master in assets/.
#
#  MIT License - see the header in any Rust source file for the full text.
#
# A PNG and not the SVG itself, because WebKitGTK will not reliably take an SVG
# for `cursor: url()` and a cursor that silently falls back is a cursor nobody
# notices is missing. Two of them, because the window runs at a device pixel
# ratio above one and a cursor image is sized in CSS pixels: without the 2x the
# pointer is the one blurred thing on a sharp screen.
#
# Twenty-four square, which is the size the master is drawn at and the size
# every platform cursor is: the scrub and the ruler scroll use the system hand,
# and a select cursor that towered over it would read as a picture that had
# come loose rather than as a pointer. The first master was 48 by 14 and did
# exactly that.
#
# The hotspot is the middle of the page, which the CSS carries rather than the
# file: half the width and half the height, so it moves if these numbers do.

set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
master="$here/assets/select.svg"
out="$here/app/ui/public"

command -v rsvg-convert >/dev/null || { echo "need rsvg-convert" >&2; exit 1; }
[ -f "$master" ] || { echo "no master at $master" >&2; exit 1; }

mkdir -p "$out"
rsvg-convert -w 24 -h 24 "$master" -o "$out/cursor-select.png"
rsvg-convert -w 48 -h 48 "$master" -o "$out/cursor-select@2x.png"

for file in "$out/cursor-select.png" "$out/cursor-select@2x.png"; do
  echo "$file: $(identify -format '%wx%h, %b' "$file")"
done
