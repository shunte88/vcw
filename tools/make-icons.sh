#!/usr/bin/env bash
#
#  make-icons.sh
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Render the application icon set from app/src-tauri/icons/icon.svg (WP-19).
#
#  MIT License - see the header in any Rust source file for the full text.
#
# The SVG is the art; everything beside it is a rendering, and this is how the
# renderings are made. Run it after editing the SVG and commit what changes.
#
# `cargo tauri icon` is what builds the .ico and .icns containers, because
# getting those right by hand is a job nobody should do twice. It also emits an
# iOS and an Android set and a Windows Store set, none of which VCW targets, so
# they are deleted again rather than committed: a file in the tree that nothing
# reads is a file somebody will later wonder about.

set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
icons="$here/app/src-tauri/icons"
source_svg="$icons/icon.svg"
rendered=$(mktemp /tmp/vcw-icon-XXXXXX.png)
trap 'rm -f "$rendered"' EXIT

for tool in rsvg-convert cargo; do
  command -v "$tool" >/dev/null || { echo "need $tool" >&2; exit 1; }
done
cargo tauri --version >/dev/null 2>&1 || {
  echo "need the tauri CLI: cargo install tauri-cli --version '^2'" >&2
  exit 1
}

# 1024 because the .icns wants 512@2x and every other size divides into it.
rsvg-convert -w 1024 -h 1024 "$source_svg" -o "$rendered"
cargo tauri icon "$rendered" --output "$icons"

rm -rf "$icons/android" "$icons/ios"
rm -f "$icons"/Square*Logo.png "$icons/StoreLogo.png" "$icons/64x64.png"

echo
echo "icon set in $icons:"
ls -1 "$icons"
