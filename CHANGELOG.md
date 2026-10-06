# Changelog

*VCW - The Vinyl Capture Workstation. (c) 2026 Stue Hunter. MIT License - see
the header in any Rust source file for the full text.*

This file records what changed in each release, for the person installing it.
Every entry is something somebody can see: a capability, a fix, a refusal that
used to be a crash. The work packages behind them are in `PROJECT_PLAN.md`, and
`docs/STATUS.md` is the running snapshot of what is built and what is not.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
versions are [semantic](https://semver.org/spec/v2.0.0.html). While the major
version is 0 the project format may change between releases; every release from
0.1.1 onwards reads every project written by an earlier one, and says so out
loud if it cannot.

## 0.1.2-alpha

*2026-10-06.*

### Export

* **FLAC now takes the format VCW records by default.** A device negotiation
  asks for the widest integer format the hardware offers, so an ordinary rip is
  32-bit, and 32-bit had no FLAC path at all: `vcw export --format flac` refused
  it and sent you to WAV. The same went for anything above 96 kHz, which ruled
  out a 192 kHz capture outright. Both ceilings are gone. FLAC carries up to
  32-bit integer at any rate VCW records, and the refusal that used to name a
  rate limit no longer exists.
* The one capture FLAC still refuses is a **32-bit float** one, and for a reason
  that has not changed: FLAC is an integer codec, and choosing how to bring float
  down to integers is a decision about headroom that belongs to a person. Export
  those as WAV.
* Worth knowing when you pick a width: **FLAC has no mid/side at 32 bits.** The
  difference channel needs one bit more than the samples and the format stops at
  32, so a 32-bit stereo file cannot use one channel to predict the other, and
  compression levels `-0` and `-1` come out byte-identical. A 24-bit capture is
  three quarters of the samples *and* the width where the encoder can still do
  its best work.

### Under the hood

* The FLAC encoder is now [`flac-codec`](https://crates.io/crates/flac-codec)
  rather than `flacenc`, which is where both ceilings came from. Still pure Rust,
  still MIT OR Apache-2.0, no C library to relink. Output was checked against
  reference libFLAC: a 32-bit 192 kHz file passes `flac -t`, which verifies the
  decoded audio against the MD5 in the stream, and lands within 0.08% of
  `flac -8` on real material.

## 0.1.1-alpha

*2026-10-06. The first public release.*

An alpha: the capture, recovery, editing, export and cataloging paths all work
end to end and are covered by the test suite, but the product has not been used
in anger by anybody except its author, and the format limits in
[`README.md`](README.md) are real. Everything below is what this release
contains rather than a history of changes to it, because there is nothing
earlier to have changed from.

### Installing

* Packages for every Tier 1 platform, built by CI: a **deb** and an
  **AppImage** for Linux on x86_64 and arm64, an **MSI** for Windows, and a
  **.app** and **DMG** for macOS on Apple silicon.
* **The command line ships inside the package.** Every workflow in
  `docs/USER-GUIDE.md` is a `vcw ...` line and `vcw bundle` is what to send when
  something breaks, so neither needs a Rust toolchain: the deb puts `vcw` on
  your `PATH` at `/usr/bin/vcw`.
* Downloads come with a `SHA256SUMS`, and `tools/verify-release.py` or
  `sha256sum -c` will check them. Signed releases are signed; an unsigned build
  says so in its own job summary rather than hoping nobody notices, because
  Gatekeeper and SmartScreen will both have something to say about it.
* Linux needs ALSA at runtime (`libasound2`), which the deb asks for.

### Capture

* Bit-perfect capture on ALSA, WASAPI and CoreAudio, with the configuration
  **negotiated and reported** rather than assumed: a request the device cannot
  honor is refused or recorded as a divergence, never silently resampled.
* The operating system is asked to **confirm** the stream it opened, because a
  backend reporting 24-bit while the mixer resamples is the failure a level
  meter cannot show. A capture that could not be confirmed is recorded as
  unconfirmed rather than claimed as bit-perfect.
* **Continuous commit**: audio is written and fsynced every 250 ms, so a crash
  or a power cut costs the last block and nothing else. There is no "save".
* Overruns, underruns, dropped frames and stream errors are counted per capture
  and kept with it.

### Recovery

* `vcw recover` finds captures left unfinished by a crash, **reports before it
  writes**, and closes them honestly. A recovered capture is marked `recovered`
  rather than `finalised`.
* `--repair` is the only thing in VCW that makes recovery discard audio, and it
  has to be asked for.
* Every block carries a CRC-32 of its own samples, and `--verify` recomputes
  all of them.

### Editing and identification

* Track detection from three detectors and a resolver, with the **evidence kept
  for every boundary**: a boundary a person has touched is locked, so
  re-detection cannot undo hand work.
* Non-destructive editing throughout: split, merge, move, delete, lock and
  retitle are statements about where the music is, and no edit rewrites a
  sample.
* Metadata from MusicBrainz and Discogs, with genre normalization, artwork, and
  an offline mode that is a first-class path rather than a simulation.
* **Credentials are never stored in a project or in a settings file**: they come
  from the environment, and the application reports which are configured without
  printing them.

### Import and export

* Audacity `.aup3` and `.aup4` projects import as ordinary captures, with labels
  becoming locked boundaries and tags becoming the release. The source is opened
  read-only and never touched.
* WAV and FLAC export from a plan that is resolved in full before a byte is
  written, with tagging, artwork and naming templates. A format that cannot
  carry the audio is refused up front rather than half way through a library.
* **Files are named by the position on the label**, `A1`, `B2`, the way VRipr
  named them: the default template writes `A1 - The Rainbow.flac`. A track's
  number in its tags counts across the disc's sides, so a two-sided record no
  longer produces two tracks numbered 2, and the position itself is kept in a
  `VINYL_POSITION` tag - the one thing a vinyl rip knows that a CD rip does
  not.

### Interface

* A window with six workspaces and a full keyboard map: every workflow is
  completable from the keyboard alone, and the map in `docs/USER-GUIDE.md` is
  checked against the application's own binding table by a test.
* A command line that can do all of it with no window, including a whole capture
  session driven from a script.
* **A side that will not play says why.** A device that cannot play a capture's
  rate is refused rather than resampled, and the refusal now reaches the window
  as a refusal: the transport stops showing itself as playing and the reason is
  on screen. It used to arrive as a generic warning, which left a transport
  claiming to play something that had never opened.

### Diagnostics

* `vcw bundle` writes one JSON document describing the build, the machine, the
  audio backend, the devices and the project's integrity, and is **safe to
  send**: no recorded audio, no titles, no credentials, not even the project's
  path. There are tests whose only job is to prove those absences.
* `vcw doctor` reports the platform and what it can see; `--log` and `VCW_LOG`
  turn structured logging up per crate. Nothing on the audio callback path
  logs, by rule and by test.
* **The waveform keeps up with the stylus, and there is a number for it.**
  `vcw soak` measures how far the drawable waveform trails the device while a
  capture runs: a quarter of a second at worst, which is the length of one
  commit. A build where it falls behind by more than a second fails its own
  test rather than shipping.

### For other tools

* The project format is documented in `docs/SCHEMA.md` in enough detail to be
  read without VCW, and `tools/vcw-read.py` is a working third-party reader in
  the standard library alone. A test drives it over projects the product wrote
  and requires the two to agree.
* `docs/PROJECT-API.md` is the supported Rust surface for reading, validating,
  recovering and migrating a project, with a compiled example beside it.
* `tools/verify-release.py` checks a download against the release's
  `SHA256SUMS`, needs nothing installed, and says plainly that a checksum
  proves integrity and not authorship.
