# Third-Party Notices

VCW's own source code is licensed under the MIT License - see [LICENSE](LICENSE).

VCW binaries are **statically linked** Rust executables: the compiled artifacts embed
the object code of VCW's dependencies. Some of those dependencies carry licenses whose
notice and source-availability terms apply to anyone redistributing those binaries.
This file records them.

A full machine-readable inventory of every dependency and its version is
[`Cargo.lock`](Cargo.lock); each release tag pins the exact versions used to build that
release's binaries. [`deny.toml`](deny.toml) encodes which licenses are acceptable, and
CI fails the build on anything outside that set - so this file cannot silently fall out
of date with what is actually linked.

---

## Current state

Everything under `crates/` links permissive dependencies, with one exception: the MP3
encoder, which is weak copyleft and has its own section below. Otherwise:
`MIT OR Apache-2.0` predominantly, with a smaller number under Apache-2.0,
BSD-2-Clause, BSD-3-Clause, ISC, Zlib, BSL-1.0, Unicode-3.0, CC0-1.0,
CDLA-Permissive-2.0 and Unlicense. These require attribution and nothing more; their
copyright notices are carried in their respective crate sources, referenced by
`Cargo.lock`.

Two dependencies are worth naming because they are load-bearing rather than incidental:

- **`cpal`** (Apache-2.0) - the audio host abstraction. It is an ordinary crates.io
  dependency, used unmodified. VCW briefly carried a patched fork; both defects behind
  that fork were fixed upstream in 0.18.2 and the fork was deleted.
- **`rusqlite`** with the `bundled` feature (MIT) - which compiles SQLite itself
  (public domain) into the binary. Bundling is deliberate: it removes platform SQLite
  variance from a file format we have to be able to recover.

One obligation is not a dependency at all: twelve of the icons are third-party art,
inlined into the stylesheet and therefore into the binary, and their license is not
recorded. That section is the only open item in this file, and it is already on the
way out: those twelve are placeholders and are being redrawn.

The sections below describe each obligation beyond attribution. The MP3 one is live -
`deny.toml` names it and CI enforces it. The `chromaprint-next` one is written ahead of
the dependency, while the decision is fresh, and its `deny.toml` entry stays commented
out until the crate is actually in the graph.

---

## LGPL-2.1-or-later - `chromaprint-next` (Phase 2)

VCW will use [`chromaprint-next`](https://github.com/attilagyorffy/chromaprint-next) to
compute AcoustID audio fingerprints in-process (REQUIREMENTS §25). That crate is
licensed `MIT AND LGPL-2.1-or-later`:

- Most of the crate is MIT (Copyright (c) 2010-2016 Lukas Lalinsky for the original
  C/C++ Chromaprint; Copyright (c) 2026 Attila Györffy for the Rust port).
- Its resampler module (`src/audio/resample.rs`) is a port of FFmpeg's `av_resample`
  (Copyright (c) 2004 Michael Niedermayer) and is licensed **LGPL-2.1-or-later**.

The full text of the GNU Lesser General Public License, version 2.1, is included in
this repository as [LICENSE-LGPL-2.1](LICENSE-LGPL-2.1) and will ship alongside every
released binary that links the crate.

The crate is used **unmodified**, as an ordinary Cargo dependency. The dependency of
record is the published release on crates.io, not a local checkout or a fork - see
`docs/adr/0004-license-and-toolchain.md`.

### Notice to users of VCW binaries containing LGPL code

Under section 6 of the LGPL you have the right to modify the LGPL portion and relink it
into VCW. VCW supports this as follows:

1. **Complete corresponding source.** The complete source of VCW is this repository,
   under the MIT license. The complete source of the LGPL component is published on
   [crates.io](https://crates.io/crates/chromaprint-next) and at the upstream
   repository linked above. The exact version used by any release is recorded in
   `Cargo.lock` at that release's tag.
2. **Relinking.** Because the complete source of the "work that uses the Library" is
   available under the MIT license, you can modify `chromaprint-next` and rebuild VCW
   yourself to produce a binary incorporating your modified version:

   ```toml
   # Cargo.toml
   [patch.crates-io]
   chromaprint-next = { path = "../my-modified-chromaprint-next" }
   ```

   ```sh
   cargo build --release
   ```

3. **No further restrictions.** VCW does not impose terms on the LGPL portion beyond
   those in LGPL-2.1, and the binaries are not obfuscated or license-restricted in a
   way that would prevent reverse engineering for debugging your modifications.

---

## LGPL-3.0 - the MP3 encoder (shipped, and optional)

VCW writes MP3 (REQUIREMENTS §33) through
[`mp3lame-encoder`](https://crates.io/crates/mp3lame-encoder), a safe wrapper over
[`mp3lame-sys`](https://crates.io/crates/mp3lame-sys), which vendors libmp3lame's C
source and compiles it into the binary. **Both crates declare `LGPL-3.0`.**

Two licenses are in play and it is worth being exact about which is which:

- **libmp3lame itself** is under the GNU Library General Public License, version 2,
  "or any later version" - its own `COPYING`, carried in the `mp3lame-sys` crate
  source and published on crates.io with it.
- **The two Rust crates** declare LGPL-3.0, which is that option being exercised.
  LGPL-3.0 is written as a set of additional permissions on top of the GPL version 3,
  so both texts are needed to read it.

The stricter of the two governs what VCW ships, so this repository carries
[LICENSE-LGPL-3.0](LICENSE-LGPL-3.0) and [LICENSE-GPL-3.0](LICENSE-GPL-3.0), and both
ship alongside every released binary that links the crate.

Note that D5 in `PROJECT_PLAN.md` recorded this as LGPL-2.1. That was wrong about the
version, and it is corrected there; the obligation is the same shape as
`chromaprint-next`'s and a different document.

### The feature, and why there is one

`vcw-export` has a `mp3` cargo feature, **on by default**. It is on because §33 names
MP3 as an initial export format and a build that cannot write one does not meet the
requirement. It is a feature - rather than an unconditional dependency - so that
anyone redistributing VCW who cannot carry the LGPL-3.0 obligation can drop it without
forking the tree:

```sh
cargo build --release --no-default-features --features ogg   # in crates/export
```

Such a build still understands `--format mp3` and refuses it with a sentence saying
this binary was compiled without it, which is a better answer than not knowing the
word. FLAC and WAV - the archival formats, and the ones VCW's bit-exactness claim is
about - are unaffected either way: `flac-codec` is pure Rust and MIT OR Apache-2.0.

### Notice to users of VCW binaries containing MP3 support

The relink rights described for `chromaprint-next` above apply here too, under LGPL-3.0
section 4 rather than LGPL-2.1 section 6. The complete source of the LGPL component is
published on [crates.io](https://crates.io/crates/mp3lame-sys) and at the upstream
repository; the exact version used by any release is recorded in `Cargo.lock` at that
release's tag. Because the complete source of the work that uses the library is this
repository under the MIT license, you can modify the library and rebuild:

```toml
# Cargo.toml
[patch.crates-io]
mp3lame-sys = { path = "../my-modified-mp3lame-sys" }
```

---

## BSD-3-Clause - the Ogg Vorbis encoder (shipped)

VCW writes Ogg Vorbis through [`vorbis_rs`](https://crates.io/crates/vorbis_rs), over
`aotuv_lancer_vorbis_sys` (an aoTuV- and Lancer-patched libvorbis) and `ogg_next_sys`
(libogg). All three are **BSD-3-Clause**, like the rest of Xiph's work: attribution
only, no copyleft, and nothing new for anyone redistributing a VCW binary.

D5 recorded Ogg Vorbis as LGPL, which was simply a mistake - corrected in
`PROJECT_PLAN.md`. Ogg export carries no relink obligation at all. It sits behind a
`ogg` cargo feature anyway, also on by default, so that the two lossy containers can be
turned off together; there is no license reason to turn this one off.

---

## The icons - SVG Repo (unresolved)

VCW's button and tab glyphs are inlined into the stylesheet as `mask-image` data URIs,
so the art is in the shipped binary, not just the repository. Six of them - `library`,
`tracks`, `metadata`, `settings`, `log` and `keys` - were drawn for VCW and are MIT with
the rest of the source. The other twelve came from [SVG Repo](https://www.svgrepo.com)
and were then recolored and resized to match: `play`, `pause`, `stop`, `record`,
`capture`, `export`, `marker`, `ffwd`, `select`, `scales`, `globe-lines` and
`globe-filled`.

**This entry is open, and the twelve are placeholders.** SVG Repo is a host, not a
licensor: it carries several collections under different terms - CC0, MIT, and CC
Attribution among them - and the only provenance the downloaded files carried was the
line `Uploaded to: SVG Repo, www.svgrepo.com, Generator: SVG Repo Mixer Tools`, which
names no collection and no license. So the obligation cannot be stated here, only the
fact that twelve glyphs have one and it is unrecorded.

They are being replaced rather than traced. The icon set simply ran out of time before
0.2 did; the six VCW-drawn glyphs took an afternoon and a 60-line generator
(`tools/make-glyphs.py`), and they already define the house style the twelve were
matched to, so the remaining twelve are scheduled work and not a research problem.

Until they land, VCW should not be called unambiguously MIT end to end. That is the
only reason this is written down rather than left to the release.

---

## Regenerating a complete per-crate listing

```sh
cargo install cargo-about
cargo about generate --format json
```

`cargo deny check licenses` is the cheaper day-to-day check and runs in CI on every
push.
