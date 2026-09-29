#!/usr/bin/env bash
#
#  stage-cli.sh
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Build the `vcw` CLI and stage it where the bundler expects a sidecar (WP-19).
#
#  MIT License - see the header in any Rust source file for the full text.
#
# A package that installs the window but not the command line makes most of
# USER-GUIDE.md unusable: recording a side, importing an Audacity project,
# detecting tracks and exporting are all documented as `vcw ...` lines, and
# `vcw bundle` is the document a person is asked to send when something breaks.
# So the CLI ships inside the package, as a Tauri `externalBin`, which wants the
# file named with its target triple beside `tauri.conf.json`.
#
# Usage: tools/stage-cli.sh [target-triple]
#
# With no argument it stages for the host. `cargo tauri build` must be able to
# find `binaries/vcw-<triple>` for the triple it is building, or it stops.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/.." && pwd)

host=$(rustc -vV | sed -n 's/^host: //p')
triple=${1:-$host}

# The `ship` profile is release without the debug sections: see the note beside
# it in Cargo.toml. Cargo only puts the triple in the path when it was asked for
# a triple, so do not ask when it is the host's own.
if [ "$triple" = "$host" ]; then
  (cd "$repo" && cargo build --profile ship -p vcw-cli --bin vcw)
  built="$repo/target/ship/vcw"
else
  (cd "$repo" && cargo build --profile ship -p vcw-cli --bin vcw --target "$triple")
  built="$repo/target/$triple/ship/vcw"
fi

suffix=
case "$triple" in
*windows*)
  suffix=.exe
  built="$built$suffix"
  ;;
esac

dest="$repo/app/src-tauri/binaries/vcw-$triple$suffix"
mkdir -p "$(dirname "$dest")"
cp "$built" "$dest"

printf 'staged %s (%s)\n' "${dest#"$repo"/}" "$(du -h "$dest" | cut -f1)"
