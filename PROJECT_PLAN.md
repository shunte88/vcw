# Vinyl Capture Workstation - Delivery Plan

**Version:** 0.1 (draft for review)
**Date:** 2026-09-22
**Basis:** [REQUIREMENTS.md](REQUIREMENTS.md) (§ references throughout point at it)
**Delivery model:** solo developer + AI pairing, full-time (40+ h/week)
**Progress measure:** milestones, not calendar - estimates below are relative weights only
**Repository:** `vcw` - The Vinyl Capture Workstation (renamed from `aio-vripr` 2026-09-22)

---

## 1. Strategy on one page

The requirements describe a product in which the hard, unforgiving part - a real-time
capture path that must never lose a sample, feeding an incrementally-committed SQLite
project that must survive a power cut - sits *underneath* everything users will judge it
by. So the plan is deliberately inverted against the temptation to build screens first:

1. **Prove the foundation before building on it.** Two spikes (§47, §48) answer the only
   questions that can invalidate the architecture: can CPAL give us a trustworthy,
   honestly-reported capture stream on all three platforms, and can SQLite absorb
   24/192 stereo indefinitely while analysis reads concurrently. Nothing else starts
   until those pass.
2. **Build the engine headless.** §4.5 already requires the core to be testable and
   usable without the GUI. Treating that as a *delivery sequence*, not just a design
   property, is the single strongest defense of §2 (no logic in the frontend): a
   `vcw-cli` that can record, detect, identify, edit markers and export means logic
   physically cannot leak into React, because React does not exist yet.
3. **Add the shell last, and thin.** Tauri 2 + React arrives once the command/event
   contract is stable and exercised by the CLI. The UI then binds to a proven API
   instead of co-evolving with one.
4. **Gate, don't schedule-and-hope.** Five gates with explicit exit criteria. Each gate
   is a point at which the plan may legitimately change shape.

**Gates**

| Gate | Name | Passed when | Weight |
|------|------|-------------|--------|
| **G0** | Feasibility | Spikes pass on Tier 1 hardware; block strategy and IPC transport chosen on measured evidence; AUP3/AUP4 schemas documented | 17 |
| **G1** | Headless core | Record → store → recover → detect → edit → export, all from the CLI, no GUI in existence | 96 |
| **G2** | MVP release 0.1 | §44 scope plus Audacity import, GUI, packaged for every Tier 1 platform | 53 |
| **G3** | Release 0.2 | §45 scope (chromaprint-next, AcoustID, evidence resolver, MP3/OGG) | 47 |
| **G4** | Release 0.3+ | §46 scope (processing, click removal, remote interface §52, Android, plugins), plus i18n and the ALAC container (AIFF shipped in 0.2; AAC withdrawn 2026-10-10) | re-plan at G3 |

*Weight* is relative effort in sessions (~3 focused hours), used for sequencing and for
noticing when something is running away - not for forecasting dates.

---

## 2. Inherited assets - audit

An honest count first: **the existing VRipr contributes roughly a third of the eventual
core, almost entirely in analysis and metadata.** Everything on the capture, storage,
playback, encoding and UI axes is new. This matters for estimation - "porting VRipr"
is not the shape of this project.

### 2.1 From `/data2/vripr` (Rust, ~10.9k LOC, egui + Audacity pipe)

| Asset | Verdict | Notes |
|-------|---------|-------|
| `src/audio/mod.rs` - RMS, spectral-flatness, HMM, guided detectors (~1.2k LOC) | **Port + refactor** | Algorithms are sound and land in `signal/`. But every entry point takes `path: &Path` and decodes via Symphonia. The refactor is to **split decode from analyze**: a `FeatureStream` (windowed RMS / flatness) fed either by the live capture tap or by a block reader. The analysis maths transfers largely unchanged. |
| `DetectorConfig`, `GuidedDetectorConfig` | **Port** | Well-tuned vinyl defaults (gap-fill for pops, onset hysteresis for crackle) - real domain knowledge, keep verbatim as starting values. |
| `src/audio/onnx_detect.rs` (+ `ort`, `ndarray`) | **Defer** | Optional cargo feature, Phase 3. Not in MVP; ORT has no macOS x86_64 prebuilts. |
| `src/metadata/discogs.rs` (758 LOC) | **Port** | Behind a new provider trait (§28). |
| `src/metadata/identify.rs` | **Split** | AcoustID + MusicBrainz HTTP → `fingerprint/acoustid` + `metadata/musicbrainz`; `duration_agreement`, `rank_score` → `identify/confidence` as the seed of the evidence model (§23). |
| `src/metadata/genre.rs` | **Port as-is** | §32 requires genre normalization retained. |
| `src/metadata/mod.rs` - `split_by_discogs_durations`, `assign_discogs_titles`, `compare_duration_report` | **Port** | Becomes metadata-duration evidence (§24). |
| `src/tagging.rs` (lofty) | **Port** | Into `export/tagging`. |
| `src/workers/export.rs` - token/template engine, sanitization, Levenshtein token suggestions | **Port the templating** | ~300 LOC of naming-template logic transfers cleanly. |
| `src/workers/export.rs` - actual encoding | **Does not exist** | VRipr delegates all encoding to Audacity over the pipe. **Encoders are net-new work** (see D5). |
| `src/track.rs` `TrackMeta`, alpha numbering | **Remodel** | Becomes project schema entities (§29); alpha numbering retained. |
| `src/config.rs` | **Remodel** | Informs §39 settings; storage moves to app config + project DB split. |
| `src/pipe.rs`, `src/app.rs`, `src/ui/*`, `src/fonts.rs` (~4.3k LOC) | **Discard** | Audacity IPC and egui. `ui/waveform.rs` is worth reading for interaction behavior before writing the React editor. |
| `tests/*` | **Port selectively** | `test_track`, `test_template`, `test_tagging`, `test_identify` carry over with their subjects. |
| `THIRD-PARTY-NOTICES.md`, `LICENSE-LGPL-2.1` | **Port now** | The LGPL relink story for chromaprint-next is already correctly worked out; do not re-solve it. |

### 2.2 Other assets

| Asset | Use |
|-------|-----|
| `/data2/vripr_training` - ~100 labeled 512 kB WAV excerpts + JSON labels, plus `boundary_detector.onnx` | **Boundary fixture corpus** (§41). Immediately usable as the regression set for the detector port. |
| `/data2/chromaprint-next` - local checkout, `Fingerprinter::start/feed(&[i16])/finish`, bit-identical to C reference | Phase 2 dependency. Streaming `feed()` is exactly the shape §25 needs. Vendor/pin (see R6). |
| `/data2/vriprpy`, `/data2/archive_vriprtk` (Python lineage) | Reference only. `Red Exposure.wav` (784 MB) is a useful long-capture test input. |

### 2.3 Net-new capability (no ancestor)

Capture pipeline · device management · SQLite block storage · transaction/checkpoint
strategy · recovery · playback engine · metering · progressive waveform pyramid ·
evidence resolver · encoders (FLAC/WAV/MP3/OGG) · project format + migrations ·
Tauri/React application.

---

### 2.4 Test hardware and platform tiers

Confirmed 2026-09-22. Development is on Linux x86_64.

**These are test instruments, not design assumptions.** The product targets whatever
CPAL enumerates on the host: typically a phono stage into a USB interface, which is the
common case and must be a first-class path. Device capability is discovered and
exposed (§7, §8), never assumed. The rigs below exist so that *we* can verify claims,
particularly at the 24/192 ceiling.

| Rig | Role |
|-----|------|
| **Pi 5 + HiFiBerry DAC+ADC Pro** (Burr-Brown, 24-bit 44.1–192 kHz **both directions**, RCA + balanced in, −12…+32 dB gain, no input anti-alias filter) | Primary capture *and* playback rig, and the aarch64 target. Exercises the full 24/192 requirement on a clean ALSA `hw:` path. Also hosts the `snd-aloop` software-loopback verification |
| **Windows x86_64 + Michell Orbe / Rega / Roksan chain** | Primary real-world vinyl rig; WASAPI shared and exclusive verification |
| **Tascam DA-3000** | Reference master recorder: generates a known-good 24/192 PCM corpus from the same vinyl passes. Its AES/EBU + S/PDIF I/O also allows an opportunistic digital loopback check (S1, method 3) |
| **Android devices** | Phase 3 (AAudio). aarch64 work on the Pi de-risks this early |
| **macOS** | **No hardware available.** See below |

**Platform tiers.** §1 of the requirements names macOS a primary platform; without an Apple
device that cannot be honored on the same terms as the others, so the plan states the
distinction rather than papering over it:

- **Tier 1 - verified on hardware:** Linux x86_64, Linux aarch64 (Pi 5), Windows x86_64.
- **Tier 2 - built and unit-tested in CI, no device verification:** macOS (aarch64 + x86_64) via GitHub Actions runners. Every non-device test runs; CoreAudio capture paths are exercised only by the file-backed simulation source. Released as "community-tested", with the gap stated in the README rather than discovered by a user.
- **Tier 3 - future:** Android.

Cross-compilation to all Tier 1 targets is established practice from VRipr. macOS is the
exception: packaging and notarisation need Apple tooling, so CI runners do that job.

**Consequence for §37/§41:** the acceptance runs (90-minute 24/192 soak, crash recovery,
concurrent-read contention) execute on the Pi 5 and the Windows rig. The Pi is the more
interesting of the two - lower I/O headroom on SD/NVMe makes it the honest worst case,
so if the SQLite write path holds there it will hold on a desktop.

---

## 3. Decisions to lock

Each has a recommendation and a deadline. Recording them as ADRs in
[`docs/adr/`](docs/adr/) keeps §49's "openly documented" promise honest from day one.
Four are written: D1, D2, D7+D10, and the workspace layout WP-01 had to settle.

| # | Decision | Recommendation | Lock by |
|---|----------|----------------|---------|
| **D1** | Native project format and extension, and the degree of Audacity compatibility (§12) | **Locked 2026-09-22.** Native `.vcw`, schema a deliberate superset of AUP4's (identical `sampleblocks` shape and summary columns) plus our own tables; SQLite `application_id` + `user_version` so the file self-identifies. Audacity interop is **import-only** (`.aup3` and `.aup4`, tier A). Export to Audacity is declined. *Validated against real AUP4 bytes 2026-09-24: `sampleblocks` is column-for-column identical between AUP3 and AUP4 and converts byte-identically, so the superset claim rests on measurement rather than on the 3.x schema plus an assumption.* | Locked |
| **D2** | SQLite binding | **Locked 2026-09-25, [ADR-0002](docs/adr/0002-sqlite-binding.md).** `rusqlite` with the `bundled` feature - synchronous, predictable, no async runtime on the writer thread; bundled build removes platform SQLite variance, which matters because §15 makes recovery a correctness requirement and recovery behavior depends on WAL semantics that vary by SQLite version. `sqlx` is async-first and wrong here. | Locked |
| **D3** | Block layout & size | **Firmed 2026-09-25 by WP-05's soak**, which is the firmed-config run S2 could not do: per-channel (AUP4-compatible) blocks of **250 ms**, **batch 1**, WAL, `synchronous=FULL`, ring >= 500 ms, and now a WAL ceiling **stated in bytes** (`Config::wal_bytes`, 4 MiB). That last part is a correction: SQLite's autocheckpoint counts pages and VCW's are 64 KiB, so the stock threshold was a 64 MiB log rather than the 4 MiB one S2 measured on a default-page-size harness - bounded, but 13.7x larger and with a materially worse commit tail. Rationale otherwise unchanged and still inverted from the starting hypothesis: throughput is a non-issue, so the budget buys *recovery granularity*. Confirm on Pi 5 before locking the platform story. See `docs/spikes/S2-sqlite-capture.md` and `docs/STATUS.md` | G0 (pending Pi 5) |
| **D4** | Sample representation at rest | Store the device's bytes **verbatim** plus a format tag. §9 forbids conversion; converting to f32 at rest would silently break the bit-perfect claim. | WP-02 |
| **D5** | Encoder stack | WAV: own writer (trivial, avoids `hound`'s format limits). FLAC: `flac-codec` (pure Rust, MIT OR Apache-2.0), **revised 2026-10-06 from `flacenc`**, whose 24-bit and 96 kHz ceilings are a library's and not the format's and left a default S32 capture with no FLAC path at all; `flac-codec` takes 1 to 32 bits and rates to 2^20, writes within 0.08% of `flac -8` on a real rip, and reference libFLAC verifies the output. MP3: `mp3lame-encoder`, which links a vendored libmp3lame. Ogg Vorbis: `vorbis_rs`, over aoTuV-Lancer libvorbis and libogg. **Two licenses here were recorded wrong and are corrected from the registry (2026-10-04):** `mp3lame-encoder` and `mp3lame-sys` declare **LGPL-3.0**, not LGPL-2.1 - libmp3lame's own `COPYING` is the GNU *Library* GPL v2 "or any later version" and the wrapper exercises the later-version option, so `LICENSE-LGPL-3.0` and `LICENSE-GPL-3.0` both ship and `deny.toml` names an exception the `chromaprint-next` one does not cover. `vorbis_rs` and both its `-sys` crates are **BSD-3-Clause**, not LGPL, so **Ogg export adds no copyleft obligation at all**. Licensing consequence: MP3 alone extends the relink obligation, under LGPL-3.0 §4. | WP-14 (FLAC/WAV), WP-25 (MP3/OGG) |
| **D6** | High-rate IPC transport *and* waveform rendering | **Revised from S3 (2026-09-24); the original wording was wrong in two of its three clauses.** Channels for meter/waveform/position - chosen for API shape (typed, per-invocation, no global event namespace), *not* throughput, which is indistinguishable from the event bus at §35 payload sizes. Hand-built compact JSON; **never** `InvokeResponseBody::Raw` for small frames - under Tauri's 1024-byte direct-execute threshold it is eval'd as a decimal JSON array, 42% *larger* than the JSON it replaces. **Do not coalesce sends**: the webview absorbed the full 750 Hz worker rate with zero loss, no added main-thread cost and a quarter of the delivery latency. Coalesce *paints* instead - one read of latest state per `rAF`. **Draw the waveform incrementally, in an `OffscreenCanvas` worker**: full-canvas main-thread redraw costs 29% of the main thread against 0.5% in a worker. Acceptance metric is **main-thread occupancy, not fps** - WebKitGTK does not pace `rAF` to vsync. See `docs/spikes/S3-tauri-ipc.md` | G0 (met on Linux) |
| **D7** | License posture | **Locked 2026-09-25, [ADR-0004](docs/adr/0004-license-and-toolchain.md).** MIT core; `THIRD-PARTY-NOTICES.md` and `LICENSE-LGPL-2.1` in the repository, written ahead of the Phase 2 obligation. **No longer permissive-only as of 2026-10-04:** WP-25 shipped `mp3lame-sys`, so an **LGPL-3.0** component is compiled into the default build and `LICENSE-LGPL-3.0` and `LICENSE-GPL-3.0` are carried too. The notices file is current and **WP-28 now puts it in front of a user**, in the window and in `vcw doctor`, generated from the cargo features rather than written out. `deny.toml` carries the allowlist and `cargo deny check` runs on every push. **WP-21 landed `chromaprint-next` on 2026-10-04**, so the LGPL-2.1-or-later exception that was deliberately held commented out until its dependency existed is now live, and a built binary is "MIT AND LGPL-2.1-or-later AND LGPL-3.0": two relink obligations, one of them behind no cargo feature, because §25 is not optional and there is no second fingerprinter to fall back to. | Locked |
| **D8** | Concurrency model | **Locked 2026-09-25, [ADR-0005](docs/adr/0005-concurrency-model.md).** Dedicated OS threads on the capture path - device callback, writer, engine - with `mpsc` between them and no async runtime anywhere near audio or SQLite. **No async runtime anywhere, as it turned out**: WP-12 was where tokio was expected to enter and did not, because metadata networking is a handful of blocking `ureq` calls behind a `Transport` trait, and a thread that is waiting on an HTTP response is a thread doing exactly what it should. The workspace still has no `tokio` dependency. Two clauses of the original wording changed on contact with the work: the engine thread is not a matter of taste, because `cpal`'s stream handle is **`!Send`** and the thread that opens a device must be the thread that keeps it; and **elevated priority is not implemented**, because WP-05's 192 kHz soak showed no overruns at ordinary priority on any rig tested. It stays available for a platform that needs it rather than applied speculatively. | Locked |
| **D9** | Typed Rust↔TS contract | **Locked 2026-09-27, [ADR-0007](docs/adr/0007-rust-typescript-contract.md).** `ts-rs` 12, one generated and committed declaration file, a drift test that reports the first differing line, and `git diff --exit-code` over it in CI. `specta` was the other candidate and lost on the exit criterion rather than on merit: its Tauri half generates bindings *from the command definitions*, which requires the crate declaring them to depend on Tauri - and that crate is a core crate. Two settings had to be corrected from the defaults: `u64` renders as `bigint`, which typechecks and is wrong at run time, so it is `number` with a 2^53 frame ceiling; and `rename_all` renames variants only, so fields need `rename_all_fields` as well. The surface is a core crate (`vcw-contract`) with every unit resolved on the Rust side, which is most of WP-16's *no business logic in TS* gate discharged before the gate is reached | WP-15 |
| **D10** | Toolchain floor | **Locked 2026-09-25, [ADR-0004](docs/adr/0004-license-and-toolchain.md).** Rust edition 2024, MSRV **1.90** declared in `[workspace.package]` and enforced by a CI job pinned to 1.90 - an untested floor is not a floor. `rust-toolchain.toml` stays on `stable` so daily work gets current diagnostics. Node 22 LTS in `.nvmrc`; pnpm. | Locked |

### 3.1 Candidate crate shortlist

Deliberately leaning on mature Rust crates rather than hand-rolling. Each is a default
to be confirmed, not a commitment; `cargo deny` policy (D7) applies to all of them.

| Area | Candidate | Note |
|------|-----------|------|
| Capture/playback | `cpal` | Mandated by §5. Exclusive/hog-mode paths exercised in S1 |
| Lock-free PCM handoff | `rtrb` (or `ringbuf`) | SPSC, allocation-free, exactly the §10 shape |
| SQLite | `rusqlite` (bundled) | D2 |
| Decode (fixtures, import) | `symphonia` | Already proven in VRipr |
| FFT / spectral | `realfft` (+ `rustfft`) | VRipr uses `rustfft`; `realfft` is the better fit for real input |
| Sample conversion / DSP | `dasp` | Format conversion **off** the bit-perfect path only |
| Resampling | `rubato` | Fingerprint feed and future processing only - never the capture path |
| FLAC encode | `flac-codec` | Pure Rust, MIT OR Apache-2.0. Was `flacenc` until 2026-10-06, which stopped at 24 bits and 96 kHz and so refused a default capture |
| Tagging | `lofty` | Already proven in VRipr |
| HTTP | `reqwest` (rustls) | Already proven in VRipr |
| Fingerprint | `chromaprint-next` | Phase 2; vendor and pin (R6) |
| Rust↔TS types | `ts-rs` or `specta` | D9 |
| Errors / logging | `thiserror`, `anyhow`, `tracing` | §42 |
| Testing | `proptest`, `criterion`, `insta`, `cargo-mutants` | §8 |

### 3.2 Audacity compatibility - what the evidence says

Researched 2026-09-22. Audacity 4.0.0 is released and does use a new `.aup4` SQLite
project format, so the requirement is grounded in something real. The detail matters:

**What an `.aup4` file contains:** four tables - `project`, `autosave`, `sampleblocks`,
`project_history`. *(Measured 2026-09-24: confirmed, and the parenthetical guess that
once stood here - that a `tags` table had disappeared - was wrong. There never was a
`tags` table in either version; metadata has always been `tags`/`tag` elements inside the
document, and AUP4 keeps them unchanged. See S5.)* `sampleblocks` carries `blockid`, `sampleformat`, `summin`, `summax`,
`sumrms`, `summary256`, `summary64k`, `samples` - roughly 1 MB, ~5 s of **mono** audio
per block, blocks never updated in place.

**The audio side is the easy half, and is genuinely good news.** That block table is
almost exactly what §13 describes, and `summary256`/`summary64k` are a ready-made
two-level waveform pyramid for §19. Adopting that shape for our own storage costs us
nothing and buys mechanical convertibility.

**The document is the hard half.** The project itself lives in a binary
`ProjectSerializer` blob - a dictionary plus binary-XML encoding of Audacity's internal
C++ object tree (wavetrack → clip → sequence → waveblock, envelopes, appearance data,
thumbnails). It is undocumented, version-coupled to Audacity's internals, and its
failure mode is severe: issue #12224 shows a project that will not open at all because
the document references a `sampleblocks` row that isn't there. AUP3 → AUP4 conversion is
also one-way - Audacity itself will not write back to AUP3 - though it is
non-destructive: the original `.aup3` survives beside the new `.aup4` (measured
2026-09-24).

**Three separable tiers, with different risk profiles:**

| Tier | Capability | Risk | Verdict |
|------|-----------|------|---------|
| **A** | **Import** AUP3 and AUP4 - read `sampleblocks`, parse the document, bring audio + clip boundaries into a project for metadata assignment, splitting and tagged export | Low. Read-only; a parse failure is a message to the user, never data loss. Directly serves the stated deliverable and the large AUP3 install base | **Take.** High value per unit of risk |
| **B** | **Export** to AUP4 (and AUP3) - write a project Audacity can open | Medium. We must emit a valid document blob, but failures are immediately visible and cannot harm the user's master | **Declined.** The bridge runs one way: material comes *into* the workstation, and everything downstream of capture - splitting, tagging, export - is ours to do. Writing another project's undocumented binary document for a workflow nobody needs is cost without return |
| **C** | **Native format *is* `.aup4`** - our working project file is literally an Audacity project | **High.** See below | **Declined** |

**Why tier C is the one to decline.** §12–§16 require us to own the project format: our
own schema versioning, transactional migrations, recovery semantics, and - critically -
storage for things Audacity has nowhere to put (capture diagnostics, fingerprints,
identification evidence, disc/side topology, marker provenance and confidence, export
settings). Tier C means our persistence layer, the component the requirements are
strictest about, is defined by an undocumented binary format owned by another project
that changes it between major versions and has no obligation to us. Our extra tables
would also be at the mercy of Audacity rewriting the file. There are three further
snags: the `.aup4` extension collides on desktop file association, so a double-click
opens the wrong application; Audacity's `sampleformat` set (int16, int24, float32) has
no place for the 32-bit integer captures §8 requires, so tier C would quietly cap our
format support; and Audacity is GPL, so the format must be reimplemented from
observation rather than by lifting code into an MIT project.

**The recommended shape** keeps almost all of the benefit: a native `.vcw` project whose
schema is a **deliberate superset of AUP4's** - same `sampleblocks` table, same summary
columns, same never-update-a-block discipline - plus our own tables for everything
Audacity cannot hold. Conversion in both directions is then mechanical rather than
lossy, we keep control of versioning and recovery, and "Open in Audacity" becomes an
export action with an honest, testable contract. Architectural compatibility is what
§12 actually asks for; this delivers it without handing over the foundations.

**Decision (2026-09-22): tier A only, on a native `.vcw` AUP4-superset schema.** S5
still runs - it is what makes the import parser possible - but its job narrows to
understanding the two formats well enough to read them, and to fixing the exact
`sampleblocks` shape our own schema will mirror.

The superset discipline is worth stating precisely, because it is what keeps §12's
"architecturally compatible" promise real: our `sampleblocks` table matches AUP4's
column for column, we honor the never-update-a-block rule, and we compute the same
`summary256`/`summary64k` pyramids. Everything the requirements need and Audacity has no
room for - capture diagnostics, fingerprints, identification evidence, disc and side
topology, marker provenance and confidence, export settings, our own schema versioning -
lives in additional tables under our control. A third-party tool that understands AUP4
can therefore already read our audio; §49's open specification documents the rest.

---

### 3.3 Non-goals

Recorded so they are not re-argued, and so the documentation can state them plainly
rather than leaving users to discover them:

| Not doing | Decided | Why |
|-----------|---------|-----|
| **DSD / DSF support of any kind** - capture, import, split or export | 2026-09-22 | PCM only. Live DSD capture is in any case unreachable under the §5 CPAL mandate (CPAL exposes integer and float PCM; ALSA's `DSD_U8/U16/U32` are not surfaced), and DSD cannot carry the fades, gain or normalization the processing roadmap assumes. Out of scope end to end; other tools serve it |
| **Export to Audacity** (`.aup4` / `.aup3` writing) | 2026-09-22 | The bridge runs one way. Everything downstream of capture - splitting, tagging, export - is ours to do; emitting another project's undocumented binary document for a workflow nobody needs is cost without return |
| **Native project format being literally `.aup4`** | 2026-09-22 | Would define our persistence layer by an undocumented, version-coupled format we do not control, cap us at Audacity's `sampleformat` set (no 32-bit integer, contra §8), and collide on desktop file association. Superseded by the AUP4-superset schema (D1) |
| **Requiring particular capture hardware** | 2026-09-22 | Any CPAL-visible device, capability-driven. The rigs in §2.4 are test instruments |
| **macOS device-level verification before 0.1** | 2026-09-22 | No Apple hardware. Tier 2: CI-built and unit-tested, gap stated in the README (R14) |

---

---

## 4. Phase 0 - Feasibility spikes (Gate G0)

Five spikes, ~17 sessions. Throwaway-by-default: the value is the evidence, though S1
becomes a permanently useful diagnostic tool.

### S1 - `vinyl-audio-test` (§47) - 6 sessions - **LINUX COMPLETE** (re-verified on CPAL 0.18.2)

> **Result (2026-09-22):** all twelve §47 behaviors implemented and demonstrated on
> Linux/x86_64. Bit-perfect 24/192 capture into SQLite, kernel-confirmed, zero dropped
> frames - measured *while the S2 soak was saturating the same disk*. Playback returns
> every frame in the stored format with no conversion. `SIGKILL` recovery is exact to
> the block, 3/3, matching S2's synthetic figure with real audio.
> Write-up: [`docs/spikes/S1-cpal-capture.md`](docs/spikes/S1-cpal-capture.md).
>
> **Two findings that change work downstream:**
> - **CPAL's ALSA device list described the plug layer, not the hardware.** 0.16 hardcoded
>   `plughw:`, so advertised configs were fiction, some advertised rates failed at stream
>   build, and - the serious one - conversion was invisible. Capturing from the default
>   device, CPAL reported "48 kHz / 2 ch / I32, request honored exactly" while the
>   hardware ran **8 kHz mono S16**. A per-platform verifier against the OS is therefore
>   **mandatory, not optional**, and must land in WP-04.
> - **A CPAL bug delivered zero frames on every ALSA capture** (timestamp capability
>   probed before the stream starts) - the difference between 0 and 960,152 frames.
>
> **Re-verified 2026-09-23 on CPAL 0.18.2, stock from crates.io.** Both defects are fixed
> upstream, and 0.18 adds `HostTrait::device_by_id` plus `hw:` enumeration - the exact API
> this spike had recommended upstreaming. The vendored copy and `[patch.crates-io]` entry
> are **deleted**; bit-perfect 24/192 capture (2,883,584 frames, 0 dropped,
> kernel-confirmed), clean playback and 3/3 crash recovery all re-measured on the stock
> crate. The verifier stays mandatory regardless: a better device list makes a silent
> conversion less likely, not detectable.
>
> **New finding - recovery loss has a floor the config cannot cross.** Loss is
> `commit granularity + driver buffer`, rounded to a block boundary, not
> `block_ms × batch_blocks` alone. Ring capacity was swept 100–1000 ms and makes **no
> difference**; the ALSA buffer (32768 frames = 170 ms at 192 kHz) is what sets the floor.
> This refines D3: 250 ms blocks sit *at* that floor, not inside it, and below roughly the
> driver buffer duration smaller commits stop buying durability.
>
> Outstanding: Windows/WASAPI exclusive, Android, macOS (no hardware), the HiFiBerry and
> Tascam converters, `snd-aloop` bit-exactness, and the capture-mode matrix G0 asks for.

Implements all twelve listed behaviors: enumerate devices and formats, open a stream,
create a SQLite project, capture into blocks, live peak/RMS, diagnostics, playback from
SQLite, checksum verification, simulated interruption and recovery, requested-vs-
negotiated format reporting, over/underrun and dropped-frame counts.

**Verifying bit-perfection - three methods, none requiring special hardware.** §9 forbids
claiming bit-perfect operation merely because CPAL is in use, so the claim needs
evidence. In descending order of strength:

1. **Software loopback (primary, zero cost, CI-able).** On Linux, the `snd-aloop` kernel
   module presents a virtual device whose playback subdevice appears as a capture
   subdevice. Play known PCM into it, capture it back, assert the bytes are identical,
   at every rate and format. This tests our whole path - buffering, format handling,
   block writing, readback - with no audio hardware involved, and runs on any developer's
   machine and in CI.
2. **Hardware-path evidence (per device, cheap).** On Linux, `/proc/asound/card*/pcm*/sub*/hw_params`
   reports the parameters the hardware is actually running at, which is direct evidence
   that no resampling was inserted. On Windows, a WASAPI exclusive-mode stream *is* the
   hardware format by definition, so the negotiated config is the proof. This is what
   `vinyl-audio-test` records and reports per device, and it is what the application
   surfaces to the user (§9's "report requested and negotiated configurations").
3. **Digital loopback (strongest, opportunistic).** Where a digital input happens to
   exist, a known file out of a digital output and back in is the most complete proof.
   Nice to have if the kit allows; not a prerequisite for anything, and not a reason to
   buy hardware.

**None of this constrains the product.** Any CPAL-visible input device is supported, and
device support is capability-driven: enumerate what the device offers, expose only that
(§8), report honestly what was negotiated (§9). The rigs listed in §2.4 are test
instruments for our own confidence, not assumptions about the user's chain - the common
case is a phono stage into a USB interface, and that must be a first-class path, not a
degraded one.

*Exit criteria:* runs on Linux x86_64, Linux aarch64 (Pi 5, ALSA `hw:` direct **and**
PipeWire), Windows (WASAPI shared **and** exclusive); builds and runs its non-device
tests on macOS CI. For each host it produces a written report of which
capture modes are reachable, what CPAL actually negotiates, and where conversion is
known to occur. **Explicitly acceptable outcome:** "exclusive mode is not reachable on
host X" - that is a finding, not a failure, and it feeds §9's honesty requirement.

### S2 - SQLite capture benchmark (§48) - 6 sessions - **ACCEPTANCE MET ON x86_64/SSD**

> **First results (2026-09-22):** zero dropped frames at 24/192 on SSD, 4× real-time
> abuse absorbed, write amplification 1.01×, WAL bounded at 4 MiB, crash recovery exact
> to the block (kill at 7.00 s → recover 6.75 s, integrity clean, pattern verified).
> Throughput is not the constraint; recovery granularity is the real design variable.
> Full write-up: [`docs/spikes/S2-sqlite-capture.md`](docs/spikes/S2-sqlite-capture.md).
>
> **90-minute soak (2026-09-22):** `PASS`. 5400.196 s, real-time factor 0.99996,
> 1,036,800,512 frames, **0 dropped** / 0 overruns, 21,601 blocks, 8.29 GB audio into an
> 8.41 GB database. Commit p99 63.2 ms and worst-ever commit 102.3 ms against a 250 ms
> budget · peak WAL 4.57 MiB · RSS flat at 8.1 MiB · 207,707 reader queries over
> 4,236,810 blocks with **0 checksum failures** and both readers' p99 under 9.5 ms. The
> commit distribution is stationary: p50/p99 were no worse than the 20-second run, only
> the extreme grew, as it should with 270× the samples.
>
> Every §48 acceptance clause is therefore met on x86_64/SSD, recovery via the separate
> `SIGKILL` test. Two qualifications: the soak ran the harness defaults
> (`synchronous=NORMAL`, interleaved) rather than the D3-firmed `FULL`+per-channel, so
> **D3 could not close on it**; and this is still one platform on the fastest storage in
> the fleet. The first of those is **resolved 2026-09-25** - WP-05's exit soak is the
> firmed-config run, made with the product code.
>
> Outstanding: soak in the firmed config, Pi 5 (aarch64, the honest worst case) and
> Windows runs, disk-full and fsync-stall injection, `VACUUM`/live-copy, page-size sweep,
> WAL2, full parameter sweep.
The one you flagged: **24/192 stereo → bounded buffer → batched BLOB writes →
simultaneous analysis reads**, under deliberate abuse.

*Sweep:* block size {0.25, 0.5, 1, 2, 4, 8 s} × transaction batch {1, 4, 16, 64 blocks} ×
journal mode {WAL, WAL2 if available} × sync {NORMAL, FULL} × page size {4 k, 8 k, 16 k,
32 k} × formats {24/96, 24/192, 32f/192}.

*Concurrent load:* two reader threads simulating waveform and fingerprint workers,
reading recently-committed blocks while writing continues.

*Abuse:* `SIGKILL` mid-transaction · power-fail simulation · disk-full · induced fsync
stalls · WAL checkpoint under sustained write · 90-minute continuous run · copy/backup
of a live project · VACUUM behavior.

*Acceptance:* zero dropped frames over 90 min at 24/192 with readers active · writer
transaction p99 within one block-duration · WAL growth bounded under a stated ceiling ·
after every kill, recovery reconstructs to the last committed block with all checksums
passing · memory flat across the run.

*Go/no-go:* if SQLite cannot sustain this, the documented fallback is **sidecar block
file + SQLite index** (project becomes a directory or a container, losing §12's
single-file property). Deciding this *now*, on measurements, is far cheaper than
discovering it at M4 - and §48 already says capture reliability outranks database
elegance.

### S3 - Tauri 2 IPC throughput - 2 sessions - **COMPLETE on Linux/x86_64**
60 Hz meter frames + progressive waveform deltas + position updates pushed into a React
canvas for 30 minutes. Measure frame pacing, main-thread blocking, memory. Decides D6
and whether waveform rendering needs a worker/OffscreenCanvas.

> **Acceptance met 2026-09-24, and it moved D6 rather than confirming it.** Thirteen
> arms × 60 s: **zero send errors, `recv/sent` = 1.000 and zero sequence gaps in every
> arm**, including two that pushed 45,000 messages at the full 750 Hz worker rate.
> Transport is a wash (channels vs events: occupancy 1.7–1.8%, RTT 10.0–10.7 ms, no
> ordering). `Raw` is **disproved** - a 30 B meter frame becomes 116 B of evaluated JS
> (3.9×), making binary 42% *larger* than compact JSON. Coalescing to 60 Hz is
> **rejected**: 750 Hz cost no extra occupancy, produced *fewer* long frames, and cut
> delivery latency from 10.5 ms to 2.8 ms. The producer thread costs 0.14–0.19% of a
> core (0.75% at 750 Hz), send p99 ≤64 µs. **The real finding is rendering:**
> full-canvas main-thread redraw = **29.0%** of the main thread with 8.9% of frames
> over 20 ms, against **0.5%** for an `OffscreenCanvas` worker, which was the only
> configuration that never exceeded a 16 ms frame in a minute (3988 frames, max
> 16.0 ms). Also established what this platform can measure at all: `performance.now()`
> is clamped to 1 ms and `rAF` is not vsync-paced.
>
> **30-minute soak on the recommended configuration (channel + `manual` + worker):**
> 180,000 messages, **zero send errors, zero loss, zero sequence gaps**; the worker
> rendered 119,133 frames with a **maximum interval of 18.0 ms**; whole-app CPU flat at
> ~86–89% of one core. **One open item:** total RSS grew 491 → 538 MiB, a steady
> **+1.46 MiB/min with no plateau** (~88 MiB/hour extrapolated). Time-based, not
> per-arm. Orthogonal to D6, but it must be isolated before **G2** - VCW stays open
> for multi-hour sessions. See R8.

### S4 - chromaprint-next streaming - 1 session - **COMPLETE on Linux/x86_64**
Feed live-shaped PCM chunks (96 kHz → i16 downmix) through `Fingerprinter::feed()` and
assert the fingerprint equals the offline fingerprint of the same region. Confirms §25's
progressive-region approach before Phase 2 commits to it.

> **Acceptance met 2026-09-24, with more margin than asked for.** Every chunk shape
> tried - down to one frame per `feed()` call, plus the ALSA period and buffer sizes,
> S2's 250 ms block, and ragged drains - reproduces the offline fingerprint **bit for
> bit** at 48 kHz and 192 kHz. The worker costs **0.79 MiB and 0.6% of one core** per
> stream, `feed()` taking 0.3% of its 250 ms block budget at p99; a whole 22-minute
> 192 kHz side streams in 6.5 MiB. Eight concurrent region fingerprinters agree
> bit-for-bit and cost 2 ms of a 250 ms budget combined.
>
> The finding the spike was not asked for is the one that changes Phase 2:
> **region-boundary error is bounded at ~0.064 BER**, reached at half a sub-fingerprint
> step (62 ms), against 0.47–0.49 for unrelated audio and 0.0009 for an MP3 320k
> round-trip. A whole-step error is a pure shift the matcher absorbs entirely. So the
> detector does **not** need accurate boundaries for identification's sake, and no
> re-fingerprint pass is needed after boundary refinement. Capture rate (192k vs
> 44.1k), gain (−20 dB to +3 dB) and i16 narrowing (truncate vs round) are all
> measurably free - fingerprint straight off the capture stream.
>
> Cross-checked against the C reference rather than trusting the crate's
> bit-identical claim: the whole pipeline after the resampler is exact on 9 of 9
> recordings; a resampler difference against the distro `fpcalc` exists, is bounded
> at 0.0002, and is characterised but not root-caused. `chromaprint-next 0.1.0` from
> crates.io is the dependency of record, and the local checkout's two SIMD commits
> were verified fingerprint-neutral. See `docs/spikes/S4-chromaprint-streaming.md`.

### S5 - Audacity AUP3/AUP4 format probe - 2 sessions - **COMPLETE**

> **Result (2026-09-22, AUP4 added 2026-09-24):** the document blob is decoded for both
> versions. An eleven-tag grammar parses all 30 corpus projects in `/data2/vinyl_rips`
> (25 AUP3 + 5 AUP4) to the last byte, zero dangling block references, zero orphan
> blocks, reconstructing readable XML. **The import is tractable and D1 stands.**
> Write-up: [`docs/spikes/S5-audacity-format.md`](docs/spikes/S5-audacity-format.md),
> decoder: `spikes/aup-format-probe/probe.py` (clean-room, from file bytes only).
>
> Findings that change downstream work:
> - Audacity blocks are **mono**, 262144 samples / 1 MiB - the per-channel layout S2
>   measured and AUP4 compatibility now agree rather than trade off.
> - Audacity has **no 32-bit integer sample format** (int16 / int24 / float32 only),
>   confirming the §8 conflict that justified the superset in D1.
> - **`project/@rate` is an editor preference, not the sample rate.** It reads
>   `192000.0` in all 30 files regardless of content; `wavetrack/@rate` is
>   authoritative, confirmed against source WAV headers, exact frame counts and label
>   extents. **This reverses the 2026-09-22 conclusion, which had already reached WP-20's
>   acceptance criteria and would have played 22 of 25 rips at 4× speed.** WP-20 now
>   takes `wavetrack/@rate`.
>
> **AUP4 (2026-09-24), from five albums converted by Audacity 4.0.0, giving matched
> AUP3/AUP4 pairs spanning both rates (48/192 kHz), both formats (float32/int24), both
> page sizes (65536/4096), 451 MB to 4.7 GB, and both single-clip and 19-clip-per-channel
> layouts:** the delta is small and lands almost entirely in the document.
> - **The conversion is byte-identical on the audio layer** - blake2b over all
>   `sampleblocks` samples, and over formats + summaries, matches exactly in all five
>   pairs. Nothing is resampled, reformatted or repacked: int24 survives, a 4096-byte
>   page size survives, and sparse `blockid` ranges are preserved rather than
>   renumbered. An exhaustive attribute diff (1,647 to 8,941 shared attributes per
>   pair) finds only 8 to 22 differences, all of them version stamps, metadata
>   reordering, an assigned track `colorindex`, editor selection state, invented
>   unity envelope points, or a 2.7e-15 s re-round of `trimLeft`. So the honest claim
>   is **blocks byte-identical, audio-bearing f64 attributes preserved to within a
>   ULP** - fixture tests must compare timings with a tolerance. **R12 was aimed at
>   the document, and the part we depend on did not move.**
> - **AUP4 adds `waveblock/@length`**, the block's sample count, and it matched
>   `sampleblocks` in all 5,664 cases. Free integrity check for WP-20, before reading
>   a byte of audio.
> - **Blocks are shared between clips**: the clip-split project has 532 `waveblock`
>   references to 456 distinct blocks, one referenced three times. Reference counting
>   is mandatory; anything that frees a block on a single clip's removal corrupts
>   another clip.
> - Same `application_id` (`"AUDY"`); only `user_version` distinguishes the versions
>   (`0x03070000` vs `0x04000001`).
> - **One new table**, `project_history(generation, saved_at, dict, doc)`, one complete
>   document per save; generation 1 is byte-identical to the live `project` row.
> - **One new record**, tag `0x10`, a length-prefixed binary blob - used only for
>   `project/thumbnail/@data`, a PNG screenshot of the editor window.
> - Dictionary 61 → 81 names; every addition is view/selection/spectrogram state.
>   Metadata (`tags`/`tag`) is unchanged in content but **reordered**, so fixture
>   diffs must compare as sets.
>
> Remaining AUP4 coverage is narrow and blocks nothing: all five files were *converted*
> rather than created natively in Audacity 4, and `project_history` generation 2 needs
> an `.aup4` saved a second time.

Groundwork for the import parser (WP-20), and it fixes the `sampleblocks` shape our own
schema mirrors. All of it is now done against real files rather than generated ones:
both schemas dumped exactly, the document blob decoded to full byte consumption on 27
projects, the dictionary + binary-XML encoding confirmed, the summary-column layout
confirmed byte-compatible, and the metadata question answered (it is in the document as
`tags`/`tag` elements, in both versions). What remains is coverage breadth, not
understanding: int24 and a 4096-byte page size through an AUP4 conversion, a natively
created AUP4 project, and the unexercised `envelope` and `autosave` paths.

With export declined, the awkward question - what Audacity does to unknown extra tables
on open-modify-save - no longer needs answering. That is a real simplification: it was
the one unknown with no graceful fallback.

Clean-room discipline: Audacity is GPL. The format is established from generated files
and published descriptions, and what we learn is written up as our own specification
(which §49 wants anyway). No Audacity source is copied into this MIT codebase.

*Exit criteria:* documented AUP3 and AUP4 schemas, a decoder for the document blob good
enough to enumerate clips and labels, and a confirmed column-for-column target for our
own `sampleblocks`.

**Gate G0 exit:** D3 and D6 locked with data; per-platform capture-mode matrix
published; bit-perfect loopback result recorded per host; schema v1 draft written
against the measured block strategy and the AUP4 findings; any fallback architecture
documented and costed.

---

## 5. Phase 1 - Headless core → MVP (Gates G1, G2)

Sizes are in **sessions** (~3 focused hours, AI-paired) and serve as relative weights for
sequencing, not as a forecast. Dependencies are hard unless noted.

### 5.1 Work packages

| WP | Scope | Req | Deps | Sess | Exit criteria |
|----|-------|-----|------|------|---------------|
| **01** | Workspace scaffold: cargo workspace per §6, CI matrix (Linux x86_64/aarch64, Windows, macOS), clippy/fmt/deny gates, MSRV pin, license + notices ported | §6, D7, D10 | - | 5 | **Built 2026-09-25.** Ten `vcw-*` crates under `crates/`, one per §6 group with §6's leaves as modules ([ADR-0003](docs/adr/0003-workspace-layout.md)); spikes moved to their own excluded workspace; `fmt`/`clippy -D warnings`/`test`/`cargo deny check` all clean locally on Linux x86_64. The aarch64, Windows and macOS legs are asserted by `.github/workflows/ci.yml` and **unverified until it runs on a push** - aarch64 Linux runs natively on `ubuntu-24.04-arm` rather than cross-compiled, because `alsa-sys` and bundled SQLite are what a cross-build gets wrong quietly |
| **02** | `project`: schema v1 as an **AUP4 superset** (`sampleblocks` column-for-column, same summary pyramids, never-update-a-block), `application_id`/`user_version`, transactional migrations, create/open/validate, integrity check | §12, §16, §49 | S2, S5 | 8 | **Built 2026-09-25.** Schema v1 in `vcw-project`: create/open/read-only-open/close, transactional migrations, `validate()` with 17 finding codes, `integrity_check()`. 44 tests. Round-trip proves bytes survive across all five storage formats; `tests/migrations.rs` proves resumption from any version reaches the same schema and that a failing step leaves no trace; [`docs/SCHEMA.md`](docs/SCHEMA.md) is generated from the DDL and `tests/schema_doc.rs` fails on drift *and* cross-checks the parse against `PRAGMA table_info`; `tests/aup4_shape.rs` diffs `sampleblocks` against DDL extracted verbatim from a corpus `.aup3` **and** `.aup4`. Not exercised at the time: anything a real capture writes - WP-05 closed that, and the schema needed no change to take it |
| **03** | `audio/devices`: enumeration, capability probing, independent in/out selection, persistence, hot-unplug handling | §7, §8 | S1 | 4 | **Built 2026-09-25, on Linux x86_64 only.** `vcw-audio`: `DeviceKey` (`host:id`) as the one identity, transport classification (`hw:` direct / `plughw:` converting / virtual), an advertised-versus-confirmed capability matrix, in/out preferences persisted by id, and `Snapshot::diff` for hot-plug. 48 tests plus the `vcw devices` and `vcw formats` verbs. Measured live: `hw:` confirmed all 8 of its advertised combinations, while a mono webcam's `plughw:` path advertised 1536 and accepts one channel - S1's plug-layer fiction, reproduced. **Exit criteria only partly met:** the matrix is reported on *one* OS, not three - Windows and macOS are unverified; and only the *idle* unplug case is covered, because unplug during record needs WP-04's stream |
| **04** | `audio/capture`: CPAL stream, `CaptureMode` negotiation, bounded lock-free ring, RT-safe callback, diagnostics counters, **and a per-platform format verifier** (S1 finding 1 - CPAL alone cannot detect a silent resample) | §9, §10, §38 | 03 | 9 | **Built 2026-09-25, on Linux x86_64 only.** `vcw-audio`: `Request`/`Negotiated` with every dishonored field named, a wait-free SPSC ring with a 500 ms floor (S2), an RT-safe callback, atomic counters, an ALSA `hw_params` verifier, a `Source` trait, and a `Simulated` source with fault injection. `vcw-types::capture` carries the shared vocabulary; `vcw-project::session` persists it, and `vcw-core` is where a test can finally watch a capture's counters land in a project file. 185 tests across the workspace (106 in `vcw-audio`, 57 in `vcw-project`, 17 in `vcw-types`, 5 cross-crate in `vcw-core`), plus the `vcw capture` verb. **Callback provably allocation-free:** `Sink::on_data` is an ordinary function over a byte slice, and `tests/rt_safety.rs` runs it 1000 times under a counting global allocator - zero allocations on the ordinary path, on overrun, on starvation and on a recorded stream error - with a control test that proves the counter can see an allocation at all. Lock-freedom is measured only as far as it can be: a consumer parked forever cannot make a callback take 100 ms. **Bit-perfect has one source of truth**, the free function `verdict()`, which confirms only on an agreeing OS reading, a fully honored request, a direct-hardware transport and four zero counters; `Source::verdict` is defaulted so no implementation can override it, and a simulated capture is refused on two independent grounds. Measured live on `hw:CARD=0,DEV=0`: 96 kHz/2 ch/S32 requested and granted, confirmed against `/proc/asound/card0/pcm0c/sub0/hw_params` reading `S32_LE 96000 Hz 2 ch`, 294912 frames in 3 s with every counter at zero; the same device through `plughw:` was refused the claim on both mode and transport. R9 is closed by fault injection: an unplugged device goes quiet, is counted as a stream error, and the bytes delivered before it went are byte-for-byte correct and the project still validates. **Exit criteria partly met:** the verifier is Linux/ALSA only - Windows (WASAPI exclusive format) and macOS return `Unavailable`, which is a refusal to claim rather than a pass, but it is not the cross-check the criterion asks for on those platforms |
| **05** | `project/persistence`: capture writer thread, batched transactions, checkpoint policy from S2 | §13, §14 | 02, 04 | 6 | **Built 2026-09-25.** `vcw-project::persistence`: `Config` (D3's 250 ms per-channel blocks, batch 1, WAL, `synchronous=FULL`), a synchronous `Writer` testable with no thread, and `spawn`/`Handle` around it. The ring and the writer are joined by a two-method `PcmSource` trait in `vcw-types`, so `vcw-audio` and `vcw-project` still do not depend on each other and the same writer runs in CI off a generator and on the bench off a turntable. `captures.frames` advances **inside** the block transaction, so the count can never run ahead of the data; a failed commit stops the writer rather than leaving a gap no later write could close. The summary pyramid was measured against the corpus rather than ported from the spike, whose scaling would have been 256x wrong for padded 24-bit. **One finding firms D3:** SQLite's
autocheckpoint threshold counts *pages* and VCW's are 64 KiB, so the stock 1000 would
have meant a 64 MiB log rather than S2's 4 MiB; `Config::wal_bytes` states the ceiling
in bytes and converts against the file's real page size. 216 tests. **Exit criterion met:** 90-min 24/192 soak on x86_64/ext4, real-time factor 1.00001, 1,036,824,960 frames in 43,202 blocks and 21,601 commits, commit p50 5.7 / p95 16.2 / p99 29.8 / max 79.4 ms against a 250 ms budget, WAL peak 4.81 MiB, all four counters zero, `validate` clean, and all 6,220,949,760 sample bytes recomputed from each block's own stored frame index and matched. **D3 is firmed by this run** - it is the firmed-config soak, run on product code. Unrun elsewhere: Pi 5 (SD and NVMe) and Windows | 
| **06** | `project/recovery`: unfinished-session detection, reconstruction, diagnostics, WAL/SHM lifecycle | §15 | 05 | 6 | **Built 2026-09-25.** `vcw-project::recovery`: `survey`/`assess` read the project without changing it, `recover`/`recover_all` write, and `Plan` (`DryRun` → `Commit` → `Repair`) is the ladder between them. Detection is `finished_at IS NULL` and not the state column, because the absence of a write is the one thing a crash cannot forge. Reconstruction believes the **blocks** over the `captures` row: `walk` is deliberately a per-channel traversal and not `SUM(frame_count)`, which would tell you a capture with a hole in it has all its frames, and the usable length is the shortest contiguous prefix across the declared channels. `finished_at` is set from the last block's `committed_at`, never `now()` - a recovered capture should say when the audio stopped, not when someone got round to recovering it - and the state becomes a new `CaptureState::Recovered`, distinct from `Interrupted` (the writer *saw* that fault; nothing saw this one). No schema migration was needed for either. D4 is enforced rather than documented: blocks stranded past the recoverable end are refused with `Error::StrandedBlocks` until `--repair` says the loss is accepted. `Sidecars::inspect` reads the `-wal`/`-shm` **before** anything opens the project, because opening it is what makes the evidence disappear. Two supporting gaps closed on the way: `validate` gained `check_coverage` (`missing-channel`, `ragged-channels`, `frame-count-mismatch`, taking it to 20 codes) because recovery's whole method is to trust the blocks and nothing previously checked that the blocks agreed with the row or with each other; and the writer now persists its counters on a 2 s timer (`Config::diagnostics_millis`) rather than only at `finish()`, because a killed capture used to leave four zeros, which is the spelling of a flawless one. `vcw recover <project> [--apply|--repair] [--verify] [--json]` is the operator surface. 234 tests. **Exit criterion met:** `crates/cli/tests/kill_and_recover.rs` spawns a real `vcw soak` child, `SIGKILL`s it at a pseudorandom point, and audits the result - out-of-process because dropping a writer in-process runs `sqlite3_close`, which checkpoints and deletes the sidecars, reproducing a crash's database state but not its filesystem state. Every recovered byte is recomputed from the frame index stored in its own block and compared against the generator, so the claim is not "a plausible frame count" but "exactly the audio the device delivered, at the offsets it delivered it, and not one invented sample". **94 random kills this session, every one recovered, audited byte-for-byte and left validating clean with checksums verified.** The measured loss is tighter than the model allowed for: every recovered length came back an exact multiple of the 250 ms block and the shortfall never reached one whole block, with a 1000 ms ring in play throughout - so **the ring is not part of the crash loss**, confirming S1's correction to S2's floor, and the test asserts the tight bound rather than the safe one. Not proven: `SIGKILL` ends a process, it does not cut power, so this exercises SQLite's crash recovery and not the storage stack's. `synchronous=FULL` should make the difference nothing, but "should" is the honest word and closing it needs real power cuts or a fault-injecting filesystem - both on S2's open list. Linux x86_64/ext4 only |
| **07** | `core/engine`: recording state machine, command/event bus, worker supervision, **`vcw-cli`** | §11, §35, §36, §4.5 | 05 | 8 | **Built 2026-09-25.** `vcw-core` is now four modules that compose the two below it. `state` is §11 as a **typestate**: five concrete phase types (`Idle`, `Armed<D>`, `Recording<D>`, `Paused<D>`, `Stopped<D>`), transitions that consume one and return the next, and therefore no implementation for an illegal move to reach - `Idle` has no `stop`, `Stopped` has no `record`, and a `Paused` that has been resumed no longer exists to be resumed again. A `Deck` trait is what the phases drive, so §11 can be exercised with no device, no disk and no project; `Refused<S, E>` hands the phase *back* when a deck says no, which is §36's "task failure shall be isolated" written into the signature rather than the prose. `commands` and `events` are §35's two halves: a `Command` carries a *description* of what to open (`Setup`) and never a device, because it crosses a process boundary at WP-15 and because `cpal`'s stream handle is `!Send`; a `Bus` fans events out to any number of subscribers, prunes the ones that have gone, and cannot fail the engine. `engine` joins them - `Recorder` is the real deck (device, project, writer), and `Engine` is a dedicated OS thread that owns the transport as a **local variable**, which is what removes the lock and the possibility of a sixth in-transition phase. §11's `Armed` is implemented as a writer that is **running but paused**, so one mechanism serves both §50's "set the level before you drop the needle" and PAUSE, the ring is always drained by the thread built to drain it, and overrun counters stay honest while nothing is committed. Three corrections the work forced: `Deck::frames` reads the final count out of the *report*, because finalizing flushes the part-filled block and the position taken before a stop is short by up to one; `capture-finished` is published when the capture stops rather than when it is reset, because an operator who stops and walks away still has to be told what was recorded; and `closed` is published from a **drop guard**, so a panicking engine thread cannot leave a consumer blocked on the stream for ever. D8 is locked by this work as [ADR-0005](docs/adr/0005-concurrency-model.md). 277 tests. **Exit criterion met, both halves.** Type level: six `compile_fail` doctests, each paired with the legal twin that must still compile - and paired deliberately, because stable rustdoc was measured to **ignore the error code** in ```compile_fail,E0599```, so `compile_fail` alone proves only "did not compile". The proof was verified live by making one illegal snippet legal and confirming the doctest failed. From the CLI: `vcw session <project>` reads transport verbs from stdin or `--script` and prints every event as it happens, and `crates/cli/tests/session_from_cli.rs` drives a whole side - arm, record, pause, resume, stop, reset - through the **shipped binary** with no frontend compiled at all, then re-opens the project and checks the audio against what the transcript claimed. `crates/core/tests/transport_session.rs` does the same eight ways at the API level, including two sides into one project, a device that cannot be opened, and a shutdown mid-capture that finalizes rather than abandons |
| **08** | `signal/meter`: peak, RMS, peak-hold, clip latch, snapshot generation | §17, §18 | 04 | 3 | **Built 2026-09-25.** `vcw-signal::meter`: a `Meter` fed interleaved bytes in the capture's own storage format, reporting per channel a peak, a sliding-window RMS, a peak-hold needle and a clip latch. Each choice is a trade-off made deliberately. **Peak is since the last read**, so a transient between two polls is never missed - the cost is that the figure depends slightly on poll rate, which a decaying peak would have hidden by losing transients instead. **RMS is a true sliding window** of 16 buckets over 300 ms, so it measures the audio and not the UI's frame rate; a per-snapshot mean would have reported a different level to a UI that redrew at 30 Hz than to one at 60. **The hold needle starts falling from the moment of the peak**, not from the moment the signal stops, and falls in dB/s. **Full scale is asymmetric and per format**, which is the finding this module turns on: the largest positive `i16` code is 32767/32768 = 0.99997 and the most negative is exactly -1.0, so a detector comparing `abs() >= 1.0` never fires on integer input at all. Each end is tested separately against the ceiling and the floor for the format in hand. One measured limitation is documented rather than papered over: at 32 bits `f32`'s 24-bit mantissa rounds the top few hundred `i32` codes to exactly 1.0, so clip detection there is a few codes early. The fan-out that feeds it is [`Tee`](crates/audio/src/buffers.rs) in `vcw-audio`, sitting **after** the ring rather than inside the callback - one insertion point covers both the device path and the simulated one, which is what keeps §4.5's no-hardware development honest, and the RT path is untouched. Taps are lossy by construction: a stalled meter worker cannot cost a frame, while a stalled *writer* freezes the meter, which is the direction §10 requires. `vcw-core::metering` is §36's meter worker - one thread, one tap, one `Bus` clone, publishing `meter-update` at 50 Hz, the middle of §17's 30-60 - and it runs from `Armed`, because §50 sets the level before the needle goes down and the paused writer is already draining the ring. A tick that finds no new audio publishes nothing, since reading a snapshot resets the peak and a needle that flicks to the floor on scheduler jitter is a lie. 299 tests. **Exit criterion met.** `crates/signal/tests/known_levels.rs` is 13 tests against signals whose levels are known before the meter runs: sines at -0.5, -6 and -20 dBFS across all five storage formats, each read back to within 0.02 dB with RMS exactly 3.0103 dB below peak; a constant reading the same peak and RMS; silence reading the floor in every format; independent channels; RMS proven independent of poll rate and proven to forget what has left its window; a single full-scale sample latching and staying latched; the top integer code clipping although it is not 1.0; an n-consecutive clip rule; and the hold needle measured falling at the rate it was given. `crates/core/tests/metering_live.rs` closes the other half - that what reaches the meter is the capture stream - by asserting the engine reports **-4.771 dBFS**, the RMS of a uniform distribution, which is a figure derived from the source's distribution and not from a previous run. Throughput measured at 192 kHz stereo: 10 s of audio metered in 0.377 s debug and 0.033 s release, roughly a third of one percent of a core. Linux x86_64 only, like everything above it |
| **09** | `signal/waveform`: multi-resolution pyramid, progressive build during capture, persisted summaries, regeneration from PCM | §19 | 05 | 7 | **Built 2026-09-25.** Two halves, split on the rule that matters. `vcw-signal::waveform` is the renderer and knows nothing about SQLite: a `Levels` ladder of 1, 256, block and 65,536 frames, a `choose` that takes the coarsest rung still filling a column, and a `Painter` that folds whatever it is handed into exactly `pixels` buckets. `vcw-project::waveform` is the only thing that opens the database, per ADR-0003, and issues one query shape for every rung. The triplet itself moved to `vcw-types::summary`, so the writer that computes it and the reader that folds it share one definition; it gained `merge`, which is exact, because **RMS composes under weighting by true sample count** - `sqrt((n1*r1^2 + n2*r2^2)/(n1+n2))` - and a column drawn from stored triplets is therefore the same number the samples would have given. Audacity weights its own 64k level by block *capacity* instead, which makes its RMS slightly high; measured against `/data2/vinyl_rips/simples_test.aup3` and pinned in a test. **`Summary64k` is dead weight for anything VCW records** and a test says so: at a 250 ms block it is *coarser* than the block level (65,536 frames against 12,000 at 48 kHz and 48,000 at 192 kHz), so it is written for AUP4 compatibility (§49) and read only for imported Audacity blocks. `rebuild` regenerates the pyramid from the stored PCM, by default only where a summary is missing, and never writes `samples` or touches a block with no `capture_blocks` row; on the real side, every `summary256` and `summary64k` blob nulled and then rebuilt - 12,528 of 12,528 blocks in 78.7 s - gives a drawing **byte-identical** to the one the writer's own summaries produced, at both rungs, which is the claim worth making about a pyramid: it holds no information the audio does not. **The finding is a storage one, and it cost two indexes.** A `sampleblocks` row carries a 192 KB samples blob at 24/192, so it occupies 64 KiB pages of its own, and reading nothing but three floats out of 12,528 of them is 784 MiB of page faults to obtain 150 KB of triplets: a cold full zoom-out of a real 26-minute side took **3.77 s**, against a requirement of sub-second. `sampleblocks_levels` and `sampleblocks_summary256` hold the two coarse rungs beside the key, away from the audio, and the query names them with `INDEXED BY` because SQLite left to itself prefers the integer primary key. They cost 1.4% of the file. The second one has to repeat the whole-block triplet as well, twelve bytes beside two kilobytes: without those three columns it is a lookup rather than a covering index, SQLite fetches the row after all, and the 28 MiB buys nothing - measured, then fixed, then locked down by a structural test on the query plan rather than a timing one. 329 tests. **Exit criterion met, and measured on real music rather than on a generated signal.** A 26-minute 192 kHz stereo side from `/data2/source_rips` was pushed through the writer onto ext4 - 300,627,479 frames, 12,528 blocks, 2.33 GiB - and drawn with the page cache evicted before every read: whole side 17.0 ms at 4000 px, 17.6 ms at 1920 px, 21.8 ms at 160 px; an eight-minute span at 1920 px 74.0 ms; 100 s 19.9 ms; 10 s 5.7 ms; the sample level 6.2 ms. Worst case anywhere in the sweep 304 ms, for a whole-side draw at 8000 px that no display can ask for. **Cost is independent of length, not of span**, which is what §37 actually claims: at any zoom coarse enough to reach the block level the read touches the same rows however wide the drawing, and the same 10 s of audio costs the same at 48 kHz and at 192 kHz (267 µs against 292 µs, four times the samples); the same 5 s span drawn out of a 10 s capture and a 200 s capture costs 18.8 ms against 19.3 ms, twenty times the recording for 2.5% more time. `vcw waveform side-a.vcw --pixels 160` draws it at the terminal and reports which rung it read and how long it took, so the claim is checkable on any machine. **What the two indexes cost the capture path is measured, and it is nothing.** The 90-minute real-time 24/192 soak re-run on an idle machine passes: real-time factor 1.00001, commit p50 6.9 ms, p95 18.4 ms, p99 32.8 ms, **max 102.6 ms against the pre-index reference's 102.3 ms** over 21,601 commits, zero overruns, zero dropped frames, `validate` clean, and every one of 6,220,938,240 bytes matched against what the source must have generated. The one visible cost is the write-ahead log, up 15 % from 4.57 MiB to 5.25 MiB as the index pages pass through it, and the file itself up 1.4 %. An earlier attempt at the same run failed and was thrown away because I ran the full gate on the same machine while it went; it is kept only for what it proved incidentally, that a byte-for-byte verifier distinguishes dropped audio from corruption - the mismatching byte was the one the source produced exactly 53,760 frames later, exactly the reported drop count. Linux x86_64 only, like everything above it |
| **10** | `audio/playback`: engine, transport, seek, region/track/boundary audition, native playback where supported | §21 | 04, 05 | 7 | **Built 2026-09-26.** Split three ways by ADR-0003: `vcw-audio::playback` owns the output stream, `vcw-project::pcm` reads the PCM back and reassembles the per-channel blocks, `vcw-core::playback` is the transport, and `vcw play` drives all of it. §21's four targets - whole capture, region, track, boundary audition - resolve to one `Span`, so everything below `Scope` plays a span and knows nothing else; a boundary audition is 3 s each side, which is the only number §21 left to be invented. Six verbs, of which `SKIP` is `seek` with the arithmetic done first, at a fixed 10 s until WP-13 makes them next-and-previous-boundary. **The queue is epoch-tagged chunks and not a byte ring**, because playback inverts capture's producer: a feeder cannot clear a ring it does not consume, so a seek would leave up to a second of the old position queued to be heard. Every chunk carries the epoch it was filled in and the frame it starts at, a seek bumps the epoch, and the callback discards what no longer matches, unplayed. Two things fall out free: the position is exact rather than inferred, and `drained` is per-epoch, so the end of a span is not an underrun while running dry mid-span still is. **No resampler, deliberately** ([ADR-0006](docs/adr/0006-playback-rate-policy.md)): a capture plays at its own rate or not at all, and `Error::RateUnavailable` names the rates the device does offer. The format preference is the *opposite* of capture's - the one that matches what is on disk, since anything else is a conversion - which `convert::natural` states, mapping Int24Padded to S24 rather than S32 and therefore losing nothing for any stored format. **Two findings, both measured and neither visible from the code.** The first live run played at half speed with 16 underruns while every other counter reported health: ALSA's own default buffer here is 350 ms and the queue was 160 ms, and a queue shallower than one callback underruns on every callback whatever the feeder does. Playback now asks for a buffer it chose (`TARGET_BUFFER_MILLIS`, four chunks, 80 ms; `buffer 3840 frames, fixed` on this device) and derives the queue depth from it, with a 1 s queue for a backend that will not be told. The second survived that fix: five seeks still cost five underruns and 34,560 frames of silence, one buffer each, because when a seek lands every chunk is stale *and* every empty chunk is in the queue, so the feeder has nothing to fill and cannot take one back out of an SPSC queue it produces into. `Feeder::hold_back` keeps one callback's worth in reserve and the feeder spends it the instant the epoch changes, queueing the new position behind the audio the seek invalidated so the callback walks past the stale chunks in the same pass; the idle sleeps became interruptible with it, a 50 ms nap being 50 ms of an 80 ms budget. After both, 0 underruns over three identical runs. `Fidelity` is three-way like capture's verdict, with one refutation unique to this side - the samples were converted for the device - and a render is never verified against hardware because there is none under it. 411 tests. **Exit criterion met, both halves.** Gapless in CI with no sound card: `playback::render` drives the same `Pump`, the same queue and the same `Source::on_data` synchronously and writes only the audio frames, so playing 0-2 s then seeking to 4 s must equal `whole[..2 s] ++ whole[4 s..]` byte for byte, asserted through the **shipped binary** in `crates/cli/tests/play_from_cli.rs`. Cues are reported rather than rounded away - a cue fires at the first period boundary at or after the frame it names, so `Rendered::applied` carries `{ verb, after, landed }` and the test computes its expectation from the join that happened. Gapless and measured on hardware: five seeks per run on `alsa:hw:CARD=PCH,DEV=1`, **min 11.7 ms, median 19.8 ms, max 20.4 ms**, which is one chunk. That measurement was worthless first time round and the fix is worth recording: it polled `Player::position` after `Player::seek`, and `seek` *stores* the frame it asked for, so it timed its own write. The callback now records `Cursor::delivered`, the epoch it last copied audio out of, and a seek has joined when `delivered` catches up with `epoch`. Four allocation tests cover `Source::on_data` - ordinary, seek, starved, stalled - because the seek path runs on the audio thread by design. Linux x86_64 and ALSA only; WASAPI exclusive, CoreAudio and AAudio are untried for output and the buffer negotiation is exactly where they will differ |
| **11** | `signal` detection port: decode/analyze split, RMS + flatness + HMM on feature streams, live provisional markers, post-capture refine, observation emission | §22, §23, §24 | 09 | 10 | **Built 2026-09-26.** The split VRipr did not have: its detectors take a file path and decode it, VCW's take feature frames, because §22 wants live analysis while the record is still turning and there is no file yet. `features::Windows` turns capture bytes into `Frame { rms, flatness }` and carries a straddling frame between calls, so the live pass and the refine pass see the same windows; `regions` holds VRipr's shared pipeline and its defaults; `silence`, `spectral` and `hmm` are the three detectors, each publishing §24 boundaries with provenance, confidence and evidence rather than touching tracks (§23); `resolve` clusters them into decisions, counts agreement rather than multiplying it, and cannot move a boundary a person placed. `vcw-core::detection` is both halves of §22: a live worker on a second lossy tap publishing §35's `track-detected` about 1.2 s behind the needle, and `refine`, one spectral extraction of the committed side through all three detectors. `vcw detect` prints the lot, evidence included. **Exit criterion met on both halves.** Parity, measured over all 595 snippets of `/data2/vripr_training` against VRipr's own answers under both thresholds (`crates/signal/tests/vripr_parity.rs`): **97.6% to 99.7% of VRipr's boundaries reproduced, every one of them at the identical frame**, with the residue traced to one documented cause - VRipr compares gaps in accumulated seconds, so a gap of exactly `min_silence_secs` computes as 0.7999999999999993 and gets bridged. Against the corpus labels VCW scores 17.1%, 11.3% and 30.9% where VRipr itself scores 16.3%, 10.8% and 30.7%: the labels are ambient records that segue, which is why VRipr was training an ONNX detector on them |
| **12** | `metadata`: provider trait, Discogs, MusicBrainz, genre normalization, artwork, caching, rate limits, timeouts, cancellation | §28, §32, §40 | 07 | 9 | **Built 2026-09-26.** Three layers, and the ordering between them is the whole design. A `Provider` knows one service's grammar - which URLs to build, how to read the body back - and nothing about time, retries or the network. A `Client` is the request path: **cache, then rate limit, then retry, then timeout, then cancel**, in that order, because a cached answer must not spend a rate-limit slot (reading something already downloaded does not involve the service, and getting this backwards makes a warm cache *slower* than a cold one). Underneath, a `Transport` is the only thing in the crate that can perform I/O, and there are three - `Offline`, `Recorded` and `Agent` - which is what turns §40 from an aspiration into a property of the build: `cargo test -p vcw-metadata --no-default-features` passes 148 lib tests with **no HTTP code compiled at all**, and it is a gate leg. Offline is also the *default*, and refusing is an ordinary answer with an ordinary message rather than an exception. **The clock is a trait too**, so no test in the crate sleeps: `TestClock` advances virtual time and the limiter's spacing and the retry backoff are asserted as numbers. Rate limits are kept as promises rather than hopes - Discogs 60/min authenticated, MusicBrainz 1/s sustained, both with a self-identifying User-Agent - and `Limiter::reserve` claims a slot so concurrent callers queue while a canceled one releases it. Cancellation is cooperative and honest about its granularity: checked before each attempt and every 50 ms of any wait, while a request already on the wire is bounded by its timeout, not the token. **§39 closed a hole VRipr had.** The Discogs token moved out of the query string and into an `Authorization` header, because the URL is the cache key, the log line and the thing pasted into a bug report; `Token` has no `Serialize`, no `Display` and a `Debug` that prints a character count, and `no_url_anywhere_in_a_discogs_exchange_carries_the_token` asserts it as a property of the traffic. **§32's genre normalization is a port held to VRipr's own output, not to my reading of its code**: `assets/genre.dat` is VRipr's file byte for byte (639 rows, 632 distinct keys), and `tests/fixtures/vripr_genres.jsonl` is 1,819 answers recorded from a verbatim copy of VRipr's `genre.rs` running out of tree, every one reproduced. That port found a latent nondeterminism VRipr shipped: 22 keys collide under case folding, 5 with *differing* answers, and VRipr resolved them by `HashMap` iteration order, so `HARDROCK` depended on the hash seed. VCW resolves by first spelling in file order, with a 32-iteration stability test, and those 5 folds are excluded from the parity fixture because VRipr's answer there is not a fact. **Two findings only a live query could produce**, which is why eight `#[ignore]`d live tests exist at all. MusicBrainz's `format:` matches the *exact* medium format name, so `format:vinyl` returns nothing and the filter has to name all four values MB actually holds (`Vinyl`, `12" Vinyl`, `7" Vinyl`, `10" Vinyl`); and a MB release's `genres` can be empty while its release-group's are populated, so extraction falls back and title-cases, MB tags being lowercase by convention where Discogs' are not. Provider position grammars stay in `positions`, out of `vcw-types`: letter-runs (`AA` = A2), separators, heading rows skipped, and a wholly numeric tracklist split A/B at the medium's halfway point - all guesses about a label, so nothing there is allowed to fail. Discogs letters restart at A on every medium and MusicBrainz's run straight through, which `side_for` reconciles. 153 lib + 3 parity + 11 offline + 9 doc tests. `vcw metadata {search,fetch,genres,credentials}` drives it. **Exit criterion met, both halves:** the offline tests are the fixture-backed ones and the offline build is enforced. Proven against the real services once - 8 live tests in 6.90 s, *Amber* read back as 2 records, sides A-D, 11 tracks - and the two Discogs live tests have never run, because `VCW_DISCOGS_TOKEN` is not set here |
| **13** | Vinyl data model + editing: release/disc/side/track topology, alpha numbering, add/move/delete/split/merge/lock, all non-destructive | §29, §31 | 02, 11 | 6 | **Built 2026-09-26.** Schema v2, one purely additive migration (asserted as additive by `tests/migrations.rs`, so an existing capture-only project gains the vinyl model without rewriting a row); `FORMAT_VERSION` stays 1 because the *meaning* of what was already there has not changed. **A track is its two boundaries.** `tracks` carries `start_boundary` and `end_boundary` and no frame columns at all - its extent comes from the join, which is why moving a boundary moves whichever tracks it bounds with no cascade, no second copy to keep in step and nothing to get out of step. A test reads the table's column names and fails if any of them contains *frame*, because the tempting optimization here is exactly the bug. **No `discs` table, deliberately**: `Side::disc()` is `index / 2 + 1`, so a disc row would store a fact the side already implies, and `disc.rs` is an arithmetic view over `sides` instead. `releases.discs` is the operator's claim and the side rows are the reality, so `disc::missing()` is the gap between them, in playing order - the answer to *what still needs recording* rather than an error. Sides are created explicitly by `side::ensure`, never conjured by a capture arriving: inventing side A for an unnamed recording would quietly relabel a mislabeled one instead of leaving the question open. `sides.capture_id` is pointedly **not** unique, so both faces can share one take, and that is what makes `track::move_to_side` meaningful at all; a move to a side holding different audio is refused, since the boundaries are frames into *that* side's capture. Nullable `artist`/`composer`/`comments` mean take the release's, so `Update` distinguishes `None` - leave it alone - from `Some("")` - clear it back to inherited. **§24's lock is the one rule in the model that is not advisory.** `move_boundary` and `delete_boundary` refuse a locked row with `Error::BoundaryLocked`; `move_boundary_forced` is the operator's override and claims the boundary as `User` on the way past, because a person who overrides a lock is the new author of that position. **`merge` is not blocked by a lock** and that is a decision, not an oversight: a lock binds *analysis*, not the person who set it, so merging leaves the locked boundary in place as a marker of where the join was. One consequence had to be paid for elsewhere - a retained boundary sitting inside a merged track would otherwise be paired with the next free end on the following pass, re-splitting what the operator just joined - so `adopt::pair` excludes boundaries *inside* an existing track as well as the two that bound it. Adoption lives in `vcw-core`, not `vcw-project`, because ADR-0003 will not let the project layer see `vcw-signal`, and its public functions take `&Project` rather than `&Connection` so `vcw-core` still does not link `rusqlite`. `Policy::min_sources` defaults to **2**, which is WP-11's finding turned into code rather than advice: on the real 2.33 GiB side that took 270 decisions to 6, and 6 boundaries to 3 tracks (0-282.8 s, 283.8-686.0 s, 687.2-1561.4 s) on a record that has three. `validate` grew `check_topology` - `empty-track`, `overlapping-tracks`, `track-numbering`, `boundaries-without-audio` - and deliberately does **not** report a boundary that bounds no track, since that is the normal state of a side that has been analyzed and not yet edited. **One defect found by reading real output rather than by a test:** a stored boundary's evidence is a *decision's* case, so each measurement in it already carries the name of the detector that took it, and the resolver prefixes an observation's evidence again as it absorbs it - so every re-analysis pass renamed `hmm.at` to `hmm.hmm.at` and one real boundary was carrying 40 measurements, most of them the same number under a longer name. `adopt::as_observation` peels the run of self-prefixes and drops exact duplicates, which heals rows the old code wrote as well as making the round trip idempotent; the real side's 40 became 24 and stayed there. 751 tests. **Exit criterion met, both halves, each asserted separately.** *Edits never touch committed blocks*: `tests/edits_are_nondestructive.rs` fingerprints every byte of `sampleblocks` and `capture_blocks` - blob contents included, each nullable summary column with it - runs 15 editing verbs through it (add, split, merge, remove, update, lock, move, forced move, delete, renumber, reassign, relabel, detach, remove side), and requires the fingerprint identical after each one, then a clean `validate` with `verify_checksums: true`. A companion test mutates one block's sample bytes and requires the fingerprint to *change*, because a hash that never moves proves nothing. *Locked boundaries immune to re-analysis*: `crates/core/tests/reanalysis.rs` runs the whole loop - `adopt::observations` into `detection::refine` into `adopt::adopt` - with a boundary moved 200 ms off the detectors' answer and the second pass configured 6 dB differently on purpose, and asserts the frame, the provenance, the lock and that exactly one nearby end boundary exists afterwards. That test also records a finding worth keeping: `already_locked` comes back **zero**, and that is not a failure. The resolver sees the operator's boundary alongside the detectors', merges them into one decision that `Provenance::User` wins, and so no separate detector decision ever reaches adoption to be skipped - §24 is being honored one layer earlier than the counter measures. Linux only, and there is no UI yet: every verb above is reachable through `vcw tracks` and `vcw release`, which is how the whole sequence was exercised against the real side on disk |
| **14** | `export`: splitter from blocks + edit instructions, WAV + FLAC encoders, tagging, artwork, naming templates (ported) | §33 | 13 | 9 | **Built 2026-09-27.** Five modules, and the first decision is that the splitter does **no arithmetic on blocks at all**. `pcm::Reader` already reassembles the per-channel blobs and hands out interleaved frames from any frame in the capture, so a track boundary that lands mid-block costs nothing and there is exactly one thing in the codebase that knows where a block edge is. A track's audio is the span between its two boundaries, clamped to what was committed, and nothing else: no fade, no lead-in, no gap trimming, because an exporter that quietly added 200 ms of run-in would make the bit-exactness this work package exists to prove untestable. **Plan, then run**: `plan` resolves every track to a path, a span and a set of tags and writes nothing, so an unknown template token, two tracks that want the same file, and a file already on disk are all found before the first byte - an export is minutes of work over gigabytes, and a collision discovered at track nine is discovered too late. `--dry-run` and the list a UI wants to show are then free. **The FLAC writer had to be streaming and the header patched at the end**: `flacenc::encode_with_fixed_block_size` builds the whole stream in RAM, which is fine for a track and not for a 2.33 GiB side, so `Flac` drives `encode_fixed_size_frame` per block and rewrites `STREAMINFO` in `finish`, with an 8 KiB `PADDING` block written at create time so the tagger can add a `VORBIS_COMMENT` in place instead of rewriting the file. **Three findings that only came from handing our output to somebody else's reader.** `STREAMINFO` must declare the *nominal* block size at both ends: honestly recording the short final frame as the minimum makes min != max, which tells libFLAC the stream is variably blocked - frame headers then carry sample numbers rather than frame numbers - and `flac -t` warned once per frame about non-increasing numbering and possible unseekability, on a file whose audio frames were byte-identical to the reference encoder's. `flac 1.5.0` declares min = max = 4096 for a 48000-frame input and for a 300-frame one, so `set_block_sizes(block_frames, block_frames)` is the fix. The `fmt` chunk goes `WAVE_FORMAT_EXTENSIBLE` when `channels > 2 || bits > 16`: all 46 WAVs in `/data2/source_rips` are 32-bit stereo with a 16-byte `fmt` and format tag 1, so the corpus said plain PCM was fine at any width, and then `flac 1.5.0` read a 24-bit file we had written and said *"legacy WAVE file has format type 1 but bits-per-sample=24"*. And lofty maps `ItemKey::EncoderSoftware` to the Vorbis **vendor string**, not to an `ENCODER` comment, which is the right place for it in a FLAC file and not where anyone would look - `metaflac --show-vendor-tag` prints `VCW 0.1.0` and `--export-tags-to` never mentions it. **What a container will not take is said out loud rather than worked around.** WAV stops at 4 GiB because RIFF sizes are 32-bit, reachable by a long unsplit side and refused with FLAC named as the answer. FLAC is an integer codec, so a `Float32` capture - legitimate under §8, and what Audacity produces - is refused with WAV named, because choosing dither and headroom is a person's decision. And **`flacenc` 0.5.1 stops at 24 bits and 96 kHz**, both the library's limits and neither the format's, while §8 requires 192 kHz and §33 requires FLAC: that is a real gap in the requirement, `the_flac_library_really_does_stop_where_we_say_it_does` fails the day either cap is lifted, and the 24-bit half has teeth because a device negotiation takes the widest integer format on offer and so a default capture is S32 and cannot be exported as FLAC. Narrowing it silently was considered and **rejected on measurement**: a real 32-bit rip from `/data2/source_rips` uses the whole low byte (`OR` of every low byte = `0xff`, max abs 2,092,715,264), so dropping eight bits is lossy and is not the exporter's call. `--format s24` produces a project FLAC will take; the general remedies are a 32-bit-capable encoder through bindings, at the cost of D5's pure-Rust choice, or an explicit operator-chosen dither. **Tagging goes in twice by necessity and twice more by choice.** lofty 0.25 made `ItemKey` a closed `Copy` enum with no `Unknown`, so a freeform key cannot be named through the generic tag at all: the mapped fields are built once as a `lofty::tag::Tag` and that is then converted into `VorbisComments` for FLAC or `Id3v2Tag` for WAV, where `DISCOGS_RELEASEID` and `STYLUS` can be named - as a `TXXX` frame in the latter. Two VRipr conventions are ported deliberately: a multi-value field is one string split on `';'` and written as **separate items**, so a player that understands multi-value shows two artists and one that does not shows the first rather than showing punctuation; and where the canonical Vorbis key differs from VRipr's, both are written - `LABEL` **and** `ORGANIZATION`, `RELEASECOUNTRY` **and** `COUNTRY` - so a library built with the old tool keeps its shape. VRipr wrote no embedded artwork at all, only a `folder.jpg` beside the tracks, so `--artwork both` is the default here and `metaflac --export-picture-to` returns the bytes that went in. **A tag write must not move a sample**, and that is asserted per container: FLAC by digest, since the stream MD5 covers the samples and an unchanged `--show-md5sum` with a clean `flac -t` means only metadata moved, and WAV by locating the `data` chunk before and after and comparing it byte for byte. Three fixes on the way past, each found by a test rather than by reading: VRipr's `apply_path_template` sanitizes segments *after* substitution, so a provider's `AC/DC Medley` becomes two directories and its `sanitize_filename` never touches `..`, so a title of `..` climbs out of the output tree - the port runs `Values::sanitized()` first instead, which is what keeps the operator's `/` in `{album}/{title}` apart from a provider's, and keeps the per-segment pass for hazards typed into the template itself; `suggest("titel")` returned nothing, this one ours rather than inherited, because the port tightened VRipr's `max(2, shorter/3)` budget to 1 while still scoring a transposition as two edits, now Damerau-Levenshtein; and the container's extension was never appended at all, so `plan` produced `01 - Europe Endless` with no suffix - appended through `OsString::push` rather than `with_extension`, which would read `Symphony No. 5` as a file called `Symphony No` and replace the ` 5`. One older defect surfaced with it: `vcw session --format` was silently ignored by the simulated source, hard-coded to S32, so there was no way to build a FLAC-exportable project without hardware. 820 tests, 69 of them new: 36 in the `vcw-export` lib (15 `encoder`, 15 `naming`, 6 `tagging`), 14 in `tests/third_party.rs`, 13 in `tests/from_a_project.rs` and 6 in `crates/cli/tests/export_from_cli.rs`. **Exit criterion met, both halves, each in its own file.** *Bit-exact WAV extraction against source blocks*: `tests/from_a_project.rs` records a known pattern through `vcw-project`'s own capture writer, cuts it at frames deliberately not block-aligned (1000, 40001, 90123), exports, and compares each file's data chunk against the exact slice of what went in - for `Int16`, `Int32` and four-channel `Int32`, where a stored frame and a WAV frame are the same bytes, so the comparison is an identity rather than a transform - with `Int24Padded` getting its own test for the dropped pad byte and a fourth handing a FLAC export to the reference decoder. *Tags validated by third-party readers*: `tests/third_party.rs` believes `ffprobe`, `flac`, `metaflac`, python `mutagen` and `sox` over us, a missing tool skips its own test so the suite passes on a bare runner, and `at_least_one_verifier_is_installed` fails when *nothing* is available, because the gate can be unverified or green and not both. `crates/cli/tests/export_from_cli.rs` drives the whole verb through the **shipped binary** on a machine with nothing plugged in and cross-checks the exported WAV against what `vcw play --render` renders over the same frames, asserting the span first so it cannot pass by comparing the right bytes to the wrong region. The project is opened **read-only**, because §33 says an export reads immutable blocks and edit instructions and opening it writable would make a crash mid-export a risk to the one thing that cannot be redone. No UI until WP-16; MP3 and Ogg are §33 formats deferred to G3 behind D5's LGPL consequence; nothing has been exported from the real 2.33 GiB side yet, so the streaming claim rests on tracks of seconds and no export has been timed |
| **15** | Tauri 2 shell: command/event surface, generated TS types, drift check in CI | §5, §35, D9 | 07 | 6 | **Built 2026-09-27.** Two new things: **`crates/contract`**, the eleventh crate and §35's typed surface, and **`app/`**, a cargo workspace of its own holding the Tauri binary and a React frontend. **Exit criterion met, and structurally rather than by discipline.** The `core-is-ui-free` job has walked `cargo tree --workspace` at the repository root since WP-01, failing on `tauri`, `wry`, `tao` or `webkit2gtk` anywhere in the tree - and that check is only worth running while the shell is *not a member of that workspace*. Put it inside and Tauri is in the tree by construction, and the job has to be weakened to an allow-list, which is the moment the criterion stops meaning anything. The second benefit is that WebKitGTK stays off the four-target matrix. The cost is paid openly: root `fmt`, `clippy`, `test` and `doc` do not reach `app/`, so the gate has four more legs (`appfmt`, `appclippy`, `apptest`, and the frontend's `uicheck`) and CI has two more jobs - **`shell`**, which installs the WebKitGTK stack and runs fmt/clippy/test in `app/src-tauri`, and **`bindings-are-current`**, which regenerates the TypeScript and then runs `git diff --exit-code` over it. `app/Cargo.toml` repeats the root's `[workspace.lints]` verbatim, because a rule that fired in one workspace and not the other would make moving code between them an argument about lints. **The contract is a core crate because units are application behavior (§2).** It holds §35's three groups: `Wire`, every `vcw_core::Event` flattened into one union tagged on `kind`, where the tag is exactly the string `Event::name` already returns - so a new core event that fell through to the catch-all fails a test rather than arriving at the frontend as an unlabeled warning; `Request`, the eight verbs that change something, parsed from JSON with refusals that name the field at fault; and the view models, which resolve every unit on the Rust side - dBFS rather than amplitudes, seconds beside frames, `A3` rather than a position index. Each of those is a calculation, and a calculation in a React component is a second opinion the moment `vcw --json` prints the first one. It is also most of WP-16's *no business logic in TS* review gate discharged in advance. **D9 is locked** as one committed file, `app/ui/src/bindings/vcw.d.ts`, 911 lines and 30 declarations generated by ts-rs 12.0.1, rewritten with `VCW_BLESS=1 cargo test -p vcw-contract --test bindings` and compared by a drift test that reports the first differing line. One file rather than ts-rs's file-per-type export, for a reason visible only in the failure case: a directory of generated files is compared entry by entry, and a *deleted* type is what that comparison gets wrong. Three ts-rs findings had to be probed for: `u64` renders as `bigint` by default, which typechecks and is wrong at run time because `serde_json` writes a number and `JSON.parse` returns a double, so `with_large_int("number")` is set and a test keeps it; `rename_all` renames variants and fields need `rename_all_fields` as well; and every generated line ends in a space, which is stripped so the first save of the file is not a 911-line whitespace diff. **One event channel and a thread per long command.** Everything is emitted on `vcw://event` carrying the whole `Wire` union, nothing coalesced and nothing binary, which is S3's measurement honored: the boundary is free, the main-thread waveform draw is the cost, and `InvokeResponseBody::Raw` is a pessimisation at these sizes. A synchronous `#[tauri::command]` runs on the thread drawing the window, so export spawns - after planning on the command thread, so a bad naming template is a refused command with a field name - and playback spawns because `Player` owns a `cpal::Stream` and is not `Send`, taking verbs over a channel and calling `tick()` on a 16 ms timeout without which no playhead is published. **861 tests, 37 new** (33 in `vcw-contract`, 4 in the shell), eleven gate legs green. The shell's surface test parses `"command": "..."` out of the *generated* TypeScript rather than a hand-written list, so a ninth contract verb fails it without anyone remembering the file exists, and the frontend's event log switches exhaustively over all seventeen kinds, which is the argument for D9 in one function. **Not verified:** nothing has been driven through a real capture by hand and no screenshot was captured; `search_metadata` and `select_release` are declared and refused with `not-wired`, because where the shell keeps a provider client, a cache and a §39 credential is a WP-16 design question; a playback open failure arrives as a `capture-warning` coded `playback-failed`, because the bus has no playback-refused event (closed 2026-09-29: `Event::Denied` is that event, and the refusal is terminal by design - see *closing the loose ends* in `docs/STATUS.md`); `bundle.active` is `false`, the icon is a placeholder and `csp` is `null`, all of which WP-19 has to settle before anything ships |
| **16** | React UI: project browser, capture workspace, transport, meters, waveform display, track editor, metadata browser, export UI, settings, full keyboard map | §34, §43 | 15, S3 | 18 | Every §44 workflow completable by keyboard alone; no business logic in TS (review gate) **Built 2026-09-27.** Eleven panels under `app/ui/src/panels/`, a root that owns the layout and nothing else, and 4,486 lines of TypeScript and TSX across 22 files. **Both halves of the exit criterion are met, and both are asserted by a test rather than by a reading of the diff.** The keyboard half needed a third test nobody had planned: `pnpm check` already made a §44 workflow with no binding a *type* error, via `COVERAGE: Record<Workflow, readonly Action[]>` over the literal map, and `keymap.test.ts` already proved every chord spellable and collision-free in its scope - and between them they proved the map complete and consistent while **four workflows had no handler behind them at all** (`arm`, `search-metadata`, `choose-release`, `export`). So `wiring.test.ts` reads every `.tsx` through Vite's `import.meta.glob`, finds every `useKeys(` block by counting brackets, and asserts that every action is handled somewhere and every §44 workflow has a handled action - the criterion itself rather than a proxy for it. It was verified to fail before it was trusted, and its first extractor was wrong in the quiet direction: it required a block ending in a newline and `};`, so a single-line `useKeys` call was invisible and the test passed by not looking. A second, structural finding came first: every scoped binding needs its panel in front and **nothing could bring a panel forward from the keyboard**, so the criterion was unmeetable no matter how much was wired - eight `navigate` bindings were added (`Ctrl+1`..`Ctrl+6`, `Ctrl+D`, `Escape`), with `Ctrl` and a digit rather than a bare digit because a bare digit is the first thing taken away when somebody types a catalog number. The map is now **32 actions covering all 20 of §44's workflows**. The *no business logic in TS* half is a review gate, so it was reviewed, and it caught three of my own unit conversions, all moved into Rust rather than defended: a hard-coded 44,100 in `Export.tsx` (the plan carries no rate, and a two-rate project makes any divisor wrong, so it shows frames), `capture.rate` arithmetic in `Waveform.tsx` (`view::Waveform` gained `start_seconds`/`end_seconds`) and `row.end - row.start` in `Tracks.tsx` (`view::Track` gained `seconds`). All four deferrals land in code with the argument beside them: the waveform is **polled** (WP-09 - the panel measures its own column count with a `ResizeObserver`, which a pushed event could not do), a rejected boundary is **shown** dimmed with its confidence and agreeing detectors (WP-11 - a person cannot promote what the picture hides, and `min_sources` is a policy that gets tuned), a recovered capture is **shown and never resumed** (WP-07 - appending to a capture that stopped for an unestablished reason is the one operation that can lose a rip), and a skip lands **on** the mark (WP-13). That last one turned out to be more than a UI opinion and is now live end to end: `track::edges_of_capture` returns both ends of every track across both faces of a capture, ascending and deduplicated; `Audition::marks` carries the frames and `Player::skip_forward`/`skip_back` fall back to `SKIP_SECONDS` only on an unanalyzed side; the `render` driver mirrors it so the behavior is testable without a sound card; and the shell and the CLI both fill it, in the same read-only open that resolves the scope. WP-15's open design question is answered by the shell keeping **none** of it: the provider client is built per call, the cache is a directory under `app_cache_dir`, and the §39 credential is read from the environment at the call site and dropped when the command returns - only a `Cancel` token is held, and both commands are `async` over `spawn_blocking` because §40 boxes a search at ten seconds and a synchronous command would freeze the window for all of them. **All 17 commands in `Request` are wired**, so `NOT_WIRED`, `refused` and `Error::NotWired` are deleted rather than left empty, which turns `every_command_in_the_contract_is_wired` from a reminder into a requirement. The gate gained a twelfth leg, `uitest`, for exactly the reason above. **Amended by WP-16a, 2026-09-27: the criterion was not met when this was first written.** First light showed that three of the four row selections in the application could only be made with a mouse, so `navigate`, `edit-track-metadata`, `delete-marker`, `move-marker` and `choose-release` could not be *started* from the keyboard whatever the map said. `wiring.test.ts` passed because it asks whether an action has a handler, not whether a person can reach the state the handler reads - the two come apart exactly there. WP-16a added eight arrow bindings, one shared `step()` mover with seven tests, and a fourth gate assertion counting movers against selectable lists; it also fixed an unhandled read that left the window describing a project the shell did not have open, and put the browser's `problem` reason on the row instead of in a tooltip. The criterion is met now, and the list assertion is what holds it. **Not verified:** no capture has been driven through the window from a live device, which is M4's job; a playback open failure still arrives as a `capture-warning`; `waveform-update` and `fingerprint-match` are declared and nothing produces them; the Transport refuses to guess a side on a capture holding two faces, which is honest and is the missing side extent showing through; and the layout has met one browser engine and one font stack |
| **17** | Test corpus + soak harness: file-backed capture simulation, multi-hour runs, memory growth, contention, WAL stress, dropped-frame injection | §41 | 05 | 7 | Nightly CI job; regressions fail the build **Built 2026-09-28** (committed at `b3b6e02`). `scripts/soak-harness.sh` holds the legs, `soak` runs a short one on every push and `soak-nightly` two real-time hours on schedule. The rule the day was run by is that **every gate was run in anger before it was wired to a verdict**, and three of them found something: `--fast` had never passed and could not have, because `Pace::Fast` overruns the ring by construction and §10 makes a full ring cost the whole callback; a device that goes silent was being filed as a flawless capture, because four counters describe events that happened and a vanished device is the absence of one; and nothing checked the WAL size, which readers going flat out will starve the checkpoint into growing without bound while the same readers at 60 Hz will not. The longest verified capture the project has came out of it: 90 minutes at 24/192 on media2026, 5.79 GiB of samples, rtf 1.00001, worst commit 17.0 ms against a 250 ms budget and already 17.0 ms at minute 15, WAL peak 5.25 MiB with no writer checkpoint, and every one of 6,220,892,160 bytes matching what the source generated. A hosted runner cannot make that claim, which is what `VCW_SHARED=1` is for |
| **18** | Docs: open project-format specification, recovery/validation API, user guide, diagnostic bundles | §42, §49 | 02, 06 | 5 | A third-party tool can read a project using the spec alone **Built 2026-09-28.** The theme is that a document is a claim and a claim wants a test, and four of the five defects this pass found were in prose I had just written and believed. The exit criterion is permanent: `crates/project/tests/third_party_spec.rs` drives `tools/vcw-read.py` over projects the product wrote and requires two independent implementations of one document to agree on the contents, on the bytes of all five storage formats, on a track span, on a damaged block and on what to refuse - six deliberate mutations of the reader were needed to show the five tests can fail, and the fourth found that the half-open-span test could not, because the only track it checked ended at the end of the capture where `read_frames` clamps. `vcw bundle` is §42's diagnostic document and is built around its negative: a searchable marker written as PCM must appear nowhere raw or hex, no string may exceed 4,096 characters, no title, album, artist or project path may appear, and credentials appear as a presence and a character count. That test was the second one in this pass that could not fail, for two reasons at once - blocks are per channel, so an interleaved marker was shredded by de-interleaving, and 200 frames was under the length guard. The bundle went from 7.7 MB to 99 KB by summarizing the device survey, digesting each capability fingerprint and compressing channel lists, and reads its table names from `sqlite_master` because a hardcoded list reported a `waveform_blocks` that does not exist as `null`. `tracing` is adopted at the seams that diagnose, with the subscriber installed only by the two binaries, stderr only so the JSON verbs stay pipeable, and §42's "routine audio callbacks shall not log" enforced by a source-reading test over the four callback modules and both `on_data` bodies - whose third test requires the rest of the crate to log, because a rule satisfied by doing nothing is not a rule. `docs/PROJECT-API.md` has a compiled example beside it and verifying its prose found `Plan::Salvage`, which does not exist. `docs/USER-GUIDE.md` has both its tables generated and checked: a Rust test in `app/src-tauri` parses `keymap.ts` and requires every chord, label, scope and §43 suggestion to be listed (Rust because vite's `fs.allow` denies reading `docs/`, and loosening a desktop app's dev-server allowlist for a doc check is a poor trade), and a CLI test checks every documented command line against the binary's own `--help`, which found `--out` for `--into`, `--split` for a `split` subcommand, and `metadata release` for `metadata fetch`. On the way past, `kill_and_recover`'s wall-clock recovery floor was repaired: it failed at 3.255 s of clock against 3.000 s of audio because the simulated source falls behind real time on a host running the rest of the gate, and it is now three checks - the recovered count is an exact multiple of the commit block (timing-free, and the one that actually states the claim), the old ceiling, and a floor against `ran_for * rtf`, the pacing the writer last reported. Gate green across fifteen legs at 1013 tests |
| **19** | Packaging: AppImage/deb (x86_64 + aarch64), MSI, macOS bundle via CI, signing, release notes, checksum verification tool | - | 16 | 7 | Clean install and first-run capture on every Tier 1 platform; macOS bundle builds and passes non-device tests **Built 2026-09-28.** Shipping is the first exercise that runs the product the way a stranger will, and three of the five findings could not have come from anywhere else. The package now carries the **CLI beside the shell** as a Tauri `externalBin` sidecar staged by `tools/stage-cli.sh`, because every workflow in `USER-GUIDE.md` is a `vcw ...` line and `vcw bundle` is what somebody is asked to send when something breaks; the deb puts it on `PATH` as `/usr/bin/vcw`, which is the only state in which anybody reads the guide. The debug sections came out, measured: `debug = 1` gave a 95 MB binary, a 25.7 MB deb and a 104 MB AppImage, and `strip = "debuginfo"` leaves **16.4 MB**, an **11.2 MB deb that also carries the CLI** and an **88.6 MB AppImage**, keeping the symbol table so a backtrace still names its frames and loses only line numbers - the CLI gets a separate `[profile.ship]` so a local release build keeps its line tables for the soak. Packaging fails at build time and the build is four minutes here and twenty across the matrix, so eight fast tests read `tauri.conf.json` and ask what the bundler would; ten mutations confirmed nine of them and the tenth showed a missing icon **passes the test and panics `generate_context!`**, so the compiler is the guard for that entry. The largest finding was not in the product: `.gitignore` held `/tools` from VRipr training material that had moved out, **hiding WP-18's exit criterion** (`tools/vcw-read.py`), the checksum tool CI names five times and the icon renderer, and no gate leg could ever have seen it because every leg runs against a working tree where the files are present. The guard asks git *which rule* ignores a path rather than whether one does, so build output stays legal and a lost file does not; `check-ignore -v` reporting a negation as the matching rule was one of three findings from writing it. **Linux x86_64 is installed and evidenced**: the deb laid out as an install, its `/usr/bin/vcw` enumerating devices and recording **589824 frames at 96 kHz S32 with 0 overruns, 0 underruns and 0 drops**, `recover --verify` clean. Clicking the packaged window found two frontend defects invisible to 1013 tests - a project created in the window was absent from the list until a restart because `reload()` re-reads the open project and `onLibraryChanged()` re-reads the directory, and the picker's first option said "Host default" while `Arm.device: null` means the **simulated source**, so a first run metered a tone generator - both now held by tests, the first of them the frontend's first rendered test. Signing is secret-conditional and loud about which branch it took, release notes come from the new `CHANGELOG.md`, and the AppImage ignoring synthetic input turned out to be Wayland dropping XTEST rather than a product bug. **Windows, macOS and Linux aarch64 need the rigs**: they are built by the matrix and have never been installed by hand, and where the sidecar lands in an MSI or a `.app` is an open question since neither directory is on `PATH`. **Amended 2026-10-09: the aarch64 CLI has its first outside witness.** A piCorePlayer user unpacked the `linux-aarch64` deb under `/home/tc` on TinyCore - an in-memory OS with no `dpkg`, no GTK and no WebKit - and ran `vcw` to record a side, search for tracks and write FLAC. That is the first time the sidecar has run on aarch64 anywhere outside CI, and it is the strongest evidence the project has that the CLI's dependency closure is really ALSA and libc: TinyCore is a harsher host than the `ldd` survey modelled. It does not close the criterion. It was an unpack, not an install, so the deb's `Depends` are still unexercised on aarch64; it is a report rather than a measurement, so there are no loss counters, no `recover --verify` and no negotiated-format evidence, and §9 forbids reading bit-perfection out of the absence of complaints; and pCP is ALSA `hw:` direct only, so S1's PipeWire half on aarch64 is untouched. What converts it is one command the user already has: `vcw bundle <project> --out pcp.json`, which reports the version, the machine, the backend, the device survey and the project's integrity and capture counters, and is built to be safe to send to a stranger |
| **20** | **Audacity import (tier A):** open `.aup3` and `.aup4`, read `sampleblocks`, decode the document to recover clips and labels, land it as a project for metadata assignment, splitting and tagged export | §12 | S5, 13 | 10 | **Built 2026-09-28.** The twelfth crate, and the decision that shaped it is the landing: import **re-blocks the assembled timeline through `persistence::Writer`** rather than adopting `sampleblocks` rows. Adoption looks free - the blocks are already 1 MiB of immutable mono PCM - but `waveclip/@offset` is the sequence origin and not the audible start, so a clip's first audible sample almost never falls on a 262,144-sample boundary; an adopted block would need a per-block sample offset `capture_blocks` has no column for and `validate()` no way to check, and every reader in the project would need a special case for audio that came in rather than was recorded. Re-blocking costs one copy at import time and buys structural identity: **an imported capture is a capture**, which is the whole point of the work package. `CaptureMode::Imported` was added rather than defaulting to a lie, because the other three variants all answer "how did VCW ask for this device" and an import never did. D4 holds on the way in - `Int24Padded` keeps Audacity's own four-byte layout and no sample is converted - and gaps are written as silence because `capture_blocks.sequence` is contiguous and closing a gap would slide every label off the audio it names. CI reads **real Audacity bytes**: the shrinker deletes byte slices rather than writing a file, so a 68 KB fixture is still Audacity's dictionary, structure and attributes, and a fixture written here would have proved only that our encoder agrees with our decoder. Two defects came out of the tests, neither in the parser: a `Timeline` that trusted sortedness its input type does not carry (now a sort and an overlap refusal), and a label sitting past the end of the audio, which is the ordinary shape of a project somebody deleted a clip from - now reported and skipped rather than landed as a track pointing at nothing. The corpus oracle is the source file itself: for every clip on every channel the test reads the source `sampleblocks` row directly and compares the head and tail of its audible span at the frame the document puts it at, then checks the gaps are silent. The exit criterion is a test - both generations of `simples_test` imported, tracked, tagged, exported as WAV and compared byte for byte in the audio chunk, streamed a megabyte at a time. Driven by hand on a real 612 MB rip: 79,141,433 frames a channel, three tracks from labels, then 574.2 MiB of tagged WAV out of `vcw export`, with FLAC refused before the first file because float32 is not narrowable without somebody deciding about headroom. Exit criteria: parses all 30 corpus projects (25 AUP3 + 5 AUP4) with full byte consumption and zero dangling block refs, diffed against the Python oracle; round-trips one of each version into a tagged export; **takes `wavetrack/@rate` and ignores `project/@rate`** (S5: the reverse would play 22 of 25 rips at 4× speed); switches on `user_version` not on the extension; opens with `mode=ro` not `immutable=1` so a populated WAL is honored; reads `project` never `project_history`; skips the `0x10` thumbnail blob by length; **reference-counts sample blocks** (S5: 532 refs to 456 distinct blocks in the clip-split project, one shared three times) and never assumes dense or 1-based `blockid`s; validates AUP4's `waveblock/@length` against `sampleblocks` where present; compares timing f64s with a tolerance rather than `==`; treats an all-unity `envelope` as absent; refuses cleanly and informatively on anything it cannot parse |

**Total Phase 1: 149 sessions.** WP-12 (metadata) and WP-17/18 are the deliberately
detachable ones - network-bound or documentation work that can absorb a session when the
capture path needs a longer uninterrupted run at it.

### 5.2 Critical path

S2 leads, ahead of S1: with CPAL's cross-platform bit-perfect capability taken as read,
the SQLite write path is now the only spike that can still change the architecture, and
it is worth knowing first.

```
S2 → S1 → S5 → 02 → 04/05 → 06 → 07 → 09 → 11 → 13 → 14 → 15 → 16 → 19 → 20
                                          (08, 10, 12 parallel off 07)
```

WP-08, WP-10 and WP-12 branch off WP-07 and can be interleaved freely. WP-17 and WP-18
must be built *incrementally alongside* their subjects rather than saved up - a soak
harness written after the fact tests the code you already believe in.

### 5.3 Product notes held for the work package that owns them

Ideas raised while other work was in flight. Recorded here rather than acted on
early, because each one belongs to a package that is not built yet.

- **New project should ask for artist, recording title and catalog number** -
  a helper, never a requirement. Raised 2026-09-26. Empty fields stay empty and
  everything downstream still works; the point is that a person holding the sleeve
  already knows these three things, and typing them once saves the automation
  guessing. They are also exactly three of §28's search criteria, so a filled-in
  new-project dialog is a metadata query that needs no further input: catalog
  number alone usually identifies a single pressing, which is the difference
  between picking a release and picking a *pressing*. Owned by **WP-16** (the
  new-project flow), served by **WP-12**'s `Query`, and stored by **WP-13**'s
  release record. Nothing about it is required before then.

---

## 6. Phase 2 (§45) - Gate G3

| WP | Scope | Sess |
|----|-------|------|
| 21 | `fingerprint/chromaprint-next` integration: progressive region fingerprinting fed from capture tap - **built 2026-10-04**. See the note below | 6 |
| 22 | `fingerprint/acoustid` + MusicBrainz recording resolution | 5 |
| 23 | `identify`: evidence/candidate/confidence/resolver - combining fingerprint, timing, metadata and signal evidence (§26, §27) | 12 |
| 24 | Metadata-assisted boundaries; release/side topology inference constraining detection | 7 |
| 25 | MP3 + Ogg export (D5 licensing consequences) - **built 2026-10-04**, ahead of its gate, while the export code was open. See the note below | 5 |
| 26 | Advanced capture diagnostics + diagnostic bundle export | 4 |
| 27 | UI: identification review, candidate comparison, confidence surfacing | 8 |
| 28 | About dialog: version, build and the third-party notices, MP3's LGPL-3.0 obligation named in the running app - **built 2026-10-04**, out of order because it was a live compliance gap. See the note below | 1 |

**Total ≈ 48 sessions.** The resolver (WP-23) is the intellectually hardest
piece in the whole project and deserves a design document before code.

**WP-25, built 2026-10-04.** Taken out of order because WP-14's export code was
open and the only thing holding MP3 and Ogg in G3 was D5's licensing question,
which turned out to be half the size it looked. `cargo info` disagreed with D5
on both crates: `mp3lame-encoder` is **LGPL-3.0** rather than LGPL-2.1, and
`vorbis_rs` is **BSD-3-Clause** rather than LGPL, so Ogg carries no obligation
and MP3's is a version of the license the `chromaprint-next` exception does not
cover. Both are **default-on cargo features** - §33 requires the formats, so a
build that cannot write them does not meet the requirement; the feature is there
so a redistributor who cannot carry LGPL-3.0 can drop MP3 without forking the
tree, and `Container::Mp3` exists in every build either way so that the contract,
the bindings, the CLI and the panel are identical across feature combinations and
a build without the encoder refuses by name instead of not knowing the word.

The design decisions worth recording. **Everything goes through `f32`** - both
libraries want float, so `fan_out` de-interleaves the stored frame into planar
buffers once, scaling by a power of two so that full-scale negative is exactly
-1.0 rather than a hair past it where libvorbis clips. **Three quality levels,
not a bitrate**: `transparent`/`high`/`compact` map to V0/V2/V5 and q8/q6/q3, and
the name survives in the settings file, the JSON command, the CLI flag and the
report, because a VBR stream does not record which `-V` made it and the run log
is the only place the setting lives afterwards. **The quality is a no-op on the
lossless containers rather than a refusal**, so the panel can hold one value
while the format changes under it. **MP3 refuses a rate MPEG never defined**:
nine rates, nothing above 48 kHz, so the five 192 kHz rips in `/data2/source_rips`
are refused by name - resampling means choosing an anti-alias filter, which is
the same argument that refuses silent dither in WP-14 - while the other 54 go
untouched. Ogg refuses almost nothing and is the only container besides WAV that
takes a `Float32` capture. **No new tagging code was needed**: two backends
already cover four containers, because what differs is the tag format and not the
codec - ID3v2 for WAV and MP3, Vorbis comments for FLAC and Ogg.

**A refusal's advice is generated, not written.** Found by exporting real records
rather than by any test: the eight refusals in `vcw-export` ended in sentences
naming a container, and four of them had drifted - three FLAC refusals still said
"Export this one as WAV" from before Ogg existed, MP3 offered FLAC above 96 kHz
where `flacenc` stops too, and the WAV size ceiling offered FLAC for a capture
FLAC refuses on bit depth. Correcting them individually introduced the next
defect each time, so `encoder::alternatives(refused, spec)` now asks every other
container whether it would carry *this* capture and composes the clause. A fifth
container will appear in every message that should mention it. The limits were
split into a `*_why` returning the reason alone plus a wrapper that appends the
advice, because otherwise the generator and the limits call each other, and the
seam between them has its own test.

**An untitled track is named, not abbreviated.** The same real records showed the
default template producing `A2 -.ogg` for the six of seven tracks whose provider
row had no title. `naming::expand` substitutes `naming::UNTITLED` for an absent
`{title}` exactly as it already substituted `00` for an absent `{tracknum}` - in
the **file name only**, because `Tags` is built separately and an empty title tag
is the truth about the record. A bracket group still wins at any depth, so
`[{title}]` keeps its *only if there is one* meaning. Parallel export, raised by
the same investigation, is on hold by instruction.

Four API traps, each found by a test rather than by reading. `InterleavedPcm`
hardcodes `len()/2`, so it is stereo-only and a mono track has to go through
`MonoPcm`. `vorbis_rs` has **no empty-block guard** and passes the sample count
straight to `vorbis_analysis_wrote`, where zero is libvorbis's end-of-stream
signal - so a zero-length write would truncate the file rather than do nothing.
`mp3lame-encoder`'s `std` feature is **not default**, without which its errors do
not implement `std::error::Error`. And the Xing/LAME VBR header is emitted as a
blank placeholder inside the *first* encode call's output and has to be patched
by seeking back to byte zero at the end; skipping that leaves a file every player
opens and reports the wrong length for - three seconds of audio came back as
**2.83 s** when the patch was removed on purpose.

**Two defects in existing code came out of testing the new code.** `Report::bytes`
was taken from `Writer::finish`, which returns before the tagger opens the file,
so every export under-reported its own size by the size of its tags - a 4 MB
sleeve scan across a ten-track side is 40 MB missing from the one number a person
checks against the disk space they just used. It is now measured from the
filesystem after tagging. And `Error::Unencodable`'s reason was a `&'static str`,
so the refusals could describe MPEG's rate table but not name the rate they were
looking at; it is a `Cow` now, and three refusals say what the capture actually
is.

**Verified by readers we did not write**, which for a lossy codec is the only
kind of verification there is: `ffprobe` on codec, rate, channels and duration;
`ogginfo` with no warnings, which checks the page structure rather than just
decoding; `sox` reading a 1 kHz left and 3 kHz right tone back out of each
container through `ffmpeg`'s channel split, because a planar fan-out is exactly
the kind of code that swaps two channels and still produces a file every player
happily plays - with FLAC as a control, so the measurement is trusted before any
claim is made about the encoders; and python `mutagen` on ID3v2 in an MP3 and on
a `METADATA_BLOCK_PICTURE` in an Ogg. **Both ends of every new check were
proven**: swapping the fan-out, dropping the quality argument and removing the
VBR patch each failed exactly the test that exists for it, and nothing else.
`cargo test`, `cargo clippy --all-targets -- -D warnings` and the whole suite
pass in **all four feature combinations**.

**WP-28, the About dialog, is a license obligation and not a credits panel.**
WP-25 changed D7's position: shipping `mp3lame-sys` compiles **libmp3lame into
the binary** under LGPL-3.0, inside a product whose own code is MIT. The
paperwork is done and correct - `THIRD-PARTY-NOTICES.md` names the component and
the obligation, `LICENSE-LGPL-3.0` and `LICENSE-GPL-3.0` are in the tree, and
`deny.toml` has the allowance - but **nothing in the running application points
at any of it**. Someone who installs the `.deb` and never opens the repository is
told nothing, and the shell knows its own version well enough to write it to
stderr at startup and never shows it to anyone.

So the dialog carries, in this order of importance: the version and build
identity (which is also the first thing wanted in a bug report), the MIT line for
VCW itself, and the third-party notices with the LGPL-3.0 component named and its
source offer. The credits belong there too, and they are the part that is a
courtesy.

It pairs with the cargo features by construction: a build compiled without `mp3`
has **no LGPL component to declare**, so what the dialog says has to be derived
from the features rather than written as prose - the same rule WP-25 arrived at
for refusal advice, which drifted four ways precisely because it was written by
hand. That makes the natural exit criterion a test rather than a screenshot:
**the notices the dialog shows agree with the features the binary was built
with**, asserted in each feature combination, which is a condition that can fail
on purpose. One session, and it wants doing before anything is handed to a person
who is not the author.

**Built 2026-10-04.** The notices are generated: `vcw_export::notices` hangs a license
record off `Container` and filters it through a new `Container::compiled_in`, which is
the only place the feature question can be answered - `cfg!(feature = "mp3")` written in
`app/src-tauri` asks about the shell's features and written in a webview cannot ask at
all. The dialog reads `CARGO_PKG_VERSION`, `CARGO_PKG_LICENSE`, `CARGO_PKG_AUTHORS` and
`CARGO_PKG_REPOSITORY` from the manifest, the two format versions from `vcw-project`
and SQLite's run-time string from the bundled library, and contains no license prose of
its own. `vcw doctor` prints the same list, because the CLI is redistributed in the same
package and links the same encoders. The exit criterion holds as written: the notices
agree with the features, asserted in `vcw-export` so the `features` leg runs it in all
four combinations, and removing the filter passes in the default build and fails in the
other three - which is the condition failing on purpose.

**WP-21, built 2026-10-04.** §25's two halves, and neither of them is the
algorithm: `chromaprint-next` does that. The work was the two seams around it.

The first is **frame alignment**. S4 found `AudioProcessor::consume` only
`debug_assert!`s that it was given whole frames, and the thing feeding it is a tap,
which hands over whatever was in the ring when it was read - so a release build would
swap the channels from the first ragged read onwards and report nothing.
`chromaprint::Builder` carries a part-frame to the next `push`, and the test feeds the
same eight seconds in chunks of 3, 777, 4,099 and 65,536 bytes and demands one identical
fingerprint.

The second is **what a region is**. The live worker takes its regions from
`Event::Detected` on the bus, so §25's "progressively rather than repeatedly
fingerprinting the entire recording" comes out of the detector that already exists
rather than out of a second analysis. It is spawned before `Detectors` so its
subscription predates the first publish, and stopped after it so the detector's last
boundary still lands. A region whose tap dropped a byte while it was open is **counted
and thrown away**, because a fingerprint with a hole in it is shifted from the hole
onwards and matches nothing while looking exactly like one that works. Nothing is
persisted: a fingerprint is evidence for a lookup, WP-22 is the lookup, and
re-fingerprinting committed audio costs 0.6% of a core.

The exit criterion is S4's claim asserted rather than quoted - a region off the tap is
**bit for bit** what the same audio fingerprints to offline - and `vcw fingerprint` is
the offline twin at the command line, by span or by implied track. Driving it on a real
ten-minute side found the one defect the suite could not: the implied-track pairing left
the first region open to the end of the side, overlapping every region after it. A start
now closes whatever was open, which is the live worker's rule.

## 7. Phase 3 (§46) - Gate G4

Non-destructive processing chain · **selectable playback equalization curves** ·
click/pop detection and removal · optional normalization · ONNX detector revival ·
advanced archival metadata · improved multi-disc workflow · plugin/provider architecture ·
Android (AAudio, the largest single unknown - needs its own feasibility spike). Not
estimated; re-plan at G3.

**ALAC replaces AAC, 2026-10-10.** §33 carries the reasoning; this is the shape of the
work. It is two pieces, and only one of them is a codec. `alac-encoder` 0.3.0 is a Rust
port of Apple's own ALAC library, Apache-2.0 OR MIT, no C and no system library, and it
hands back encoded packets plus the 24-byte magic cookie that describes them - which is
all the codec half needs. The other half is a container: the crate muxes nothing, and
an ALAC file that players will open is an MP4, so `Container::Alac` means writing a
minimal `.m4a` by hand the way `Container::Aiff` writes a `FORM`. That is `ftyp`,
`moov` down through `stbl` with the cookie inside an `alac` sample entry, and `mdat`,
with the sample-size table built as the packets come back. Bigger than AIFF's 54 fixed
bytes and bounded: nothing in it varies with the content except three tables.

Two things make the exit criterion cheap. lofty 0.25 already reads MP4 and already
recognizes ALAC - `Mp4Codec::ALAC`, with `alac_properties` parsing the very cookie we
will have written - so handing our own file back to the tagger that will tag it is a
real check of the container, not a self-consistent one. And lossless means the exit
criterion is the one WP-14 already set for WAV: decode the export and compare it to the
source blocks byte for byte. `ffmpeg` and `afconvert` are the third-party readers.
Watch the width. ALAC takes 16, 20, 24 and 32-bit integer, so a default S32 capture has
a path where FLAC once did not, but a `Float32` capture is refused through the same
`Width` machinery FLAC and AIFF use, for the same reason: choosing dither is a person's
decision.

**Playback equalization curves, for the people who care most.** RIAA has only been
the standard since 1954. Records cut before it - and a good many 78s after it - were
cut to the label's own curve: Columbia LP, Decca FFRR, EMI, HMV, AES, NAB/NARTB,
Teldec, and others besides. Played back through a RIAA stage they are audibly wrong in
the bass and the treble, and the listener who owns those records is exactly the
listener who will notice. Each curve is three numbers - a bass turnover, a treble
rolloff, and in RIAA's case a bass shelf, with the IEC amendment's rumble filter as a
separate option - so the implementation is a small parameterised filter and a table.
**The table has to be sourced from a citable reference and the filter's response
measured against it, not transcribed from memory**: the per-label numbers are widely
repeated and widely wrong, and a curve that is close is worse than no curve because
nobody can hear that it is close. The exit criterion is therefore a measurement - sweep
a known signal through each curve and check the response against the published one
within a stated tolerance - which is a condition that can fail on purpose.

**One half of it cannot wait for Phase 3, and did not: `CaptureEq` shipped
2026-10-04.** Applying a curve is only meaningful if what
was captured is known: a flat transfer wants the curve applied, and a capture that has
already been through a RIAA phono stage wants RIAA *undone* first, which amplifies
noise and is not something to do by guesswork. So the capture has to **record what
equalization the hardware applied** - flat, RIAA, or unknown - as part of the capture
row, and that is cheap now and impossible later: every rip made without it is a rip
nobody can correctly re-equalize, including the 62 in `/data2/source_rips` and the 25
Audacity projects. A field at capture time, an honest `unknown` as the default for
imports, and the curve itself as a **stored decision in the processing chain** rather
than anything baked into the blocks, which is the architecture §33 and the edit
instructions already have.

That field now exists: schema v3 adds `captures.capture_eq`, the operator states it
once in Settings or per run with `--capture-eq`, imports land `unknown`, and a typo is
refused rather than defaulted. The curves themselves remain Phase 3 work - what is
done is the part that could not be done later.

Specified as **§51, drafted 2026-10-04**, and listed in §46 with the rest of Phase 3.
It was numbered after §50 rather than inserted near §33 where it belongs thematically,
because the section numbers are cited from source files, tests and this plan, and
renumbering a spec to tidy its order is how a citation silently starts pointing at the
wrong requirement. §51 settles three things the discussion above only raised: the
**±0.5 dB from 20 Hz to 20 kHz** tolerance that makes the measurement a pass or a fail,
that an `Unknown` provenance is **refused rather than guessed**, and that the curve in
force is written into exported metadata - an archival file whose equalization is
unrecorded cannot be reproduced.

**A remote interface, because the first outside report of it arrived from a user who
had already succeeded.** Someone running piCorePlayer - LMS on a Raspberry Pi, TinyCore
underneath, the filesystem resident in RAM - unpacked the `linux-aarch64` `.deb` under
`/home/tc`, captured a record with `vcw`, searched for tracks and wrote FLAC. All of
that worked. The window did not, and could not: they went looking for a listener on
port 5173, which is the Vite dev server and `devUrl`, and a release build never opens a
socket at all.

The measurement is the argument. `vcw` links three packages beyond libc - ALSA, libc,
libgcc - for **12.2 MB**, nearly all of it already present. `vcw-app` links **135
packages for 259 MB**, of which `libwebkit2gtk-4.1-0` is 95 MB and
`libjavascriptcoregtk-4.1-0` another 32 MB. On an ordinary desktop that is a shrug; on
a host whose filesystem is RAM it is the whole machine. A turntable on a headless Pi
beside the amplifier is not a strange way to use this program - it may be the best one
- and that operator currently has no interface above the command line.

**The seam is already there, and §2 is why.** `app/ui/src/api.ts` is the only file in
the frontend that touches Tauri: 38 functions over `invoke`, one subscription over
`listen`, carrying one `Wire` union on one event name. Nothing in it decides anything,
because §2 anticipated exactly this - "a second frontend would have to make the same
choices and two implementations of a policy is one too many". The `core-is-ui-free` CI
job keeps the layer beneath it honest. So the transport is small: the same commands
over one HTTP endpoint, an event stream, `app/ui/dist` served statically, and that one
module swapped.

**The transport is not the work; the trust boundary is.** The shell trusts its caller
absolutely, and is right to - the caller is a window owned by the same user on the same
machine. Those same 38 commands on a LAN socket read and write arbitrary paths, open
audio devices and start exports, on a host that is probably also running a music
server. §52 therefore makes loopback the default *address* and a secret the price of
every request at any address, roots the path browser and refuses to leave it, and keeps
the whole thing behind a cargo feature that is off until the verb is invoked. The
trusted-loopback exemption was considered and **rejected on 2026-10-10**, which is the
decision that unblocked this work: the deployment it exists for is browsed from another
machine, so the authenticated path is the common one, and loopback is reachable by every
local account and by any page that points a name it controls at 127.0.0.1. Two smaller
consequences: `tauri-plugin-dialog`'s native chooser has no equivalent and needs that
rooted browser, and audio auditioned remotely comes out of the host rather than the
listener's laptop - which for this deployment is correct, and has to be said rather
than left to look like a fault.

Specified as **§52, drafted 2026-10-09** with its authentication model **settled
2026-10-10**, listed in §46, and **pulled forward into 0.3 the same day it was drafted** rather than left to sit behind the processing chain. The
argument for jumping the queue is not that it is easy: it is that an outside user has
already done the hard half themselves, on a platform nobody targeted, and is using the
product daily through the one interface that fits. Everything else in §46 adds capability
for people who already have a window. This gives a window to somebody who has none, and
the report that it is wanted came from use rather than from speculation.

The estimate stays unwritten, and that is not the same as unestimated: the command
surface is mechanical and already enumerated, while the authentication model is a design
decision nobody has taken. Writing a number against the second would be inventing one.
Take the auth decision first, and the rest is transcription.

**Built 2026-10-10**, and the transcription was the easy half, as predicted. Two new
crates in the root workspace: `vcw-shell`, which is every command body with no window
around it, behind a four-method `Host` trait the window and the listener answer
differently; and `vcw-serve`, a `tiny_http` listener with a thread per request, a
37-arm dispatch and an `/events` stream. `app/src-tauri` is now a window, a plugin, an
adapter and the wrappers. One `dist` serves both deployments and picks its transport at
run time by asking whether `__TAURI_INTERNALS__` is on `window`, so a build flag never
decides it. `vcw serve` is a cargo feature, on by default and provably removable.
The two consequences §52 named are both answered rather than documented away: the
rooted path browser is the `browse` command, fenced by `Host::browse_root`, which the
window answers `None` and `--files` fills in - and which has no default, because the
convenient one is the operator's whole account; and the transport bar says where the
audio is coming out, because the thing it explains is silence.

---

## 8. Quality strategy

**Test pyramid** (§41)
- *Unit:* detection maths against synthetic signals with known answers; template/tag/genre logic; state-machine transition exhaustiveness.
- *Property:* schema migrations (any vN project opens as vN+1 losslessly); block round-trip; marker edits never alter PCM.
- *Fixture:* `vripr_training` corpus for boundaries; recorded HTTP fixtures for Discogs/MusicBrainz/AcoustID.
- *Loopback:* `snd-aloop` bit-exactness runs at every rate and format, in CI on Linux - the cheapest continuous guard against a conversion sneaking into the capture path.
- *Simulation:* file-backed capture source implementing the same trait as CPAL - gives deterministic, device-free capture tests in CI, and is the single highest-leverage testing decision in the plan. Build it in WP-04, not later.
- *Fault injection:* kill-at-offset, disk-full, fsync stall, device unplug, network timeout.
- *Soak:* nightly multi-hour capture with memory and WAL tracking.
- *Manual, device-backed:* real-interface capture on three platforms each gate - cannot be automated on hosted CI.

**CI matrix:** Linux x86_64, Linux aarch64, Windows, macOS × stable Rust; clippy `-D warnings`; `cargo deny`;
TS type-drift check; nightly soak. Real-device tests run locally on the Tier 1 rigs against a checklist; macOS has no device leg (R14). Audacity fixture projects, one per format version, are checked into the repo and parsed on every CI run so upstream format drift surfaces as a test failure rather than a support ticket.
`cargo mutants` (already anticipated in `.gitignore`) on the `signal` and `project`
crates, where silent wrongness is most dangerous.

**Review gate for §2:** every PR touching `app/ui` is checked for logic that belongs in
Rust. Cheap, and the architectural rule dies without it.

---

## 9. Risk register

| # | Risk | L | I | Mitigation / early signal | Fallback |
|---|------|---|---|---------------------------|----------|
| **R1** | SQLite cannot sustain 24/192 with concurrent reads | **L** (was M) | **Critical** | Largely retired by S2 on x86_64/SSD: 4× real-time headroom, zero drops, bounded WAL. Residual risk is the Pi 5 on SD/NVMe - re-measure there before closing | Sidecar block file + SQLite index; project becomes a container (costs §12's single-file property) |
| **R2** | CPAL's device abstraction can hide the hardware's real capabilities, so bit-perfection cannot be assumed from the API | **L** (was M, was L) | M | **Re-assessed 2026-09-22 from S1 evidence.** The *audio path* is fine - raw bytes arrive unconverted. *Device discovery* is not: CPAL's ALSA list is the plug layer's, and a silent 8 kHz→48 kHz upsample was reported as an honored request. Mitigation is now concrete: verify every negotiated format against the OS (`/proc/asound` on Linux, the WASAPI exclusive format on Windows) and refuse to claim bit-perfect without it. Built and working in S1; must land in WP-04. **Revised 2026-09-23:** CPAL 0.18.2 enumerates `hw:` PCMs and adds `HostTrait::device_by_id`, so the device list is no longer the plug layer's fiction and devices are selectable by PCM id - likelihood drops back to **L**. Impact stays M and the verifier stays mandatory: a better list makes the lie less likely, not detectable | Report negotiated path honestly (§9 demands this regardless); select devices by PCM id (now a stock CPAL API); stay current on CPAL rather than pinning |
| **R3** | Tauri IPC can't carry 60 Hz meters + waveform | **L** (was M) | **L** (was M) | **Retired by S3 on Linux/x86_64 - and the risk was mis-aimed.** The IPC boundary carried 12.5× the required rate with zero loss; two of the three mitigations listed here (coalescing, binary channels) are measurably counter-productive. The live concern is the one that was only a footnote: **main-thread waveform rendering**, at 29% occupancy naive. Residual risk is Windows/WebView2 and Pi 5, where Tauri's direct-execute thresholds differ | `OffscreenCanvas` worker (now the default, not the fallback); incremental self-blit redraw; drop waveform update rate before touching transport |
| **R4** | Encoder licensing/quality (MP3/Ogg LGPL) | M | L | D5 at WP-14; `cargo deny` | **Closed at WP-25 (2026-10-04).** Half the risk was not there: Ogg is BSD-3-Clause. The mitigation landed as cargo features that are **on by default** - §33 requires both formats, so default-off would fail the requirement, and the features exist for the license rather than for the size. `cargo build --no-default-features --features ogg` is the escape for a redistributor who cannot carry LGPL-3.0, and `deny.toml` fails the build if any other copyleft crate reaches the graph |
| **R5** | **Scope** - very large surface, single developer | M | **H** | Gates; headless-first; scope levers (§10.2) | Ship a CLI-only 0.1 if the GUI slips; it is genuinely useful alone |
| **R6** | `chromaprint-next` is 0.1.0, single-maintainer | M | M | Vendor the local checkout, pin exactly, run its test suite in our CI | `rusty-chromaprint` (known gaps) or upstream C via FFI - the latter violates §25, so this is a real loss |
| **R7** | Detector port regresses vs VRipr | M | M | A/B harness on the labeled corpus from day one of WP-11 | Keep VRipr binary available for comparison. **Closed 2026-09-26.** `crates/signal/tests/vripr_parity.rs` reproduces 97.6-99.7% of VRipr's boundaries over all 595 snippets, exact to the frame, and fails if any detector drops below 97% or below VRipr's own agreement with the labels. VRipr's answers are checked in as `tests/fixtures/vripr_answers.jsonl`, computed out of tree at `/data2/vcw-scratch/parity` from a verbatim copy of VRipr's `src/audio/mod.rs`, so the reference outlives the other repo |
| **R8** | Memory growth over multi-hour captures | **M** (now observed, not hypothetical) | **H** | Nightly soak from WP-05 onward. **First hard evidence 2026-09-24 from S3's soak, and it is on the webview side:** total RSS +1.46 MiB/min over 30 min (491 → 538 MiB), decelerating only slightly, no plateau - ~88 MiB/hour. The bench's UI allocates almost nothing per frame (preallocated decode targets, fixed `Int16Array` ring, allocation-free histograms), so application code is the *least* likely cause and WebKit or tauri per-message bookkeeping the most likely. Cheap next experiments: re-soak with the producer stopped, then with `echo` disabled, to split baseline from per-message cost. Must be isolated before **G2** | Bounded caches; mmap'd summaries; periodic webview reload at a safe point (between sides), which argues for keeping authoritative state Rust-side so a reload is cheap |
| **R9** | Device removal mid-capture corrupts project | **Closed** (was L, was M) | **H** | **Closed for the capture path 2026-09-25 in WP-04.** `source::Faults::unplug_after` injects it, and `capture_path.rs` plus `core/tests/capture_to_project.rs` assert the whole chain: the device goes quiet rather than ending the stream, one stream error is counted, the frames delivered before it went are byte-for-byte correct, the counters reach the file, and the project still validates. A cable cannot be pulled by CI, so it is pulled here instead. **Closed outright 2026-09-25 in WP-05.** The writer is now in the chain: `core/tests/capture_writes_audio.rs` unplugs the device mid-capture while blocks are being committed, and asserts that everything delivered before the cut is in the file byte-for-byte, that nothing after it was invented, that the session reads `interrupted` with the ring's own counters, and that the project still validates with checksums verified. Impact stays H because the consequence of being wrong has not changed | Treat as stream error, finalize capture as `interrupted`, keep committed blocks |
| **R10** | API keys/rate limits in an OSS app | M | L | User-supplied credentials, OS keychain, never in project files (§39) | Documented degraded/offline mode (§4.3 already requires it) |
| **R12** | AUP4's document blob is undocumented, version-coupled and changes under us | **L** (decoded 2026-09-24 for both versions, 30/30 corpus. The 3.7→4.0 delta is one table, one record type and 25 dictionary names, all of it UI state; the audio layer converted byte-identically. The blob does change under us, but the part we read did not) | L | Contained by D1: it now affects only the import parser, never our own persistence. S5 decodes it against generated ground truth; fixture projects per Audacity version parsed in CI so upstream drift fails a build | Import degrades to "audio plus clip boundaries only, metadata by hand" - still useful |
| **R13** | GPL contamination while implementing an Audacity-compatible format | L | **H** | Clean-room: generated files and published descriptions only; our own written spec; no Audacity source in the tree; documented in CONTRIBUTING | Reimplement from scratch if provenance is ever doubted |
| **R14** | macOS ships without device-level verification | **H** | M | Stated plainly in README and release notes; CI builds and runs all non-device tests; file-backed capture source covers the logic | Recruit a macOS tester from the OSS community before 0.1, or mark macOS experimental |
| **R11** | Continuity across a long unbroken run at capture/storage internals (M1→M4) with no visible output | M | M | M1/M2/M3 each end in a demonstrable capability; keep the CLI genuinely pleasant to use | Re-cut scope at any gate |

---

## 10. Sequencing and scope levers

### 10.1 Order of attack

Progress is gauged by the five milestones, each of which produces something usable in
its own right:

| # | Milestone | Proven when | After | Weight | Cum. |
|---|-----------|-------------|-------|--------|------|
| **G0** | Foundation proven | 24/192 sustained into SQLite under abuse with concurrent analysis reads and clean crash recovery; bit-perfect loopback measured; capture-mode matrix published; AUP3/AUP4 schemas documented | S1–S5 | 17 | 17 |
| | *status 2026-09-24* | **All five spikes have returned a verdict on their primary platform.** S1 met on Linux/x86_64 on stock CPAL 0.18.2, no patches (Windows, Android, macOS, real converters outstanding) · S2 acceptance met on x86_64/SSD incl. the 90-min soak (firmed-config soak, Pi 5 + Windows outstanding) · S3 met on Linux/WebKitGTK with 12.5× headroom; **D6 rewritten rather than confirmed** - the constraint is main-thread waveform rendering, not IPC (Windows/WebView2 + Pi 5 outstanding) · S4 met on Linux/x86_64, §25 confirmed and the boundary tolerance quantified (Pi 5 + Windows outstanding) · **S5 complete** - AUP3 and AUP4 both decoded, 30/30 corpus projects parsed to the last byte, the AUP3→AUP4 audio layer proved byte-identical, and the 2026-09-22 sample-rate conclusion corrected before it reached code (`wavetrack/@rate`, not `project/@rate`). **All five spikes have delivered; G0's remaining exposure is not spike work but hardware coverage** - Windows, Pi 5, Android, macOS and real converters. D3's firmed-config soak, listed here as outstanding, ran on 2026-09-25 as WP-05's exit criterion | | | |
| **M1** | *It records* | CLI captures to a project, survives `SIGKILL` at any point, recovers to the last committed block with checksums intact | WP-06 | 37 | 54 |
| | *status 2026-09-25* | **Met.** `vcw capture` and `vcw soak` both record to a `.vcw` project; `crates/cli/tests/kill_and_recover.rs` kills a real capture process with `SIGKILL` at a pseudorandom point and `vcw recover --apply --verify` closes it to the last committed block, with every byte recomputed from the frame index stored in its own block and `validate` re-checksumming the lot. 94 random kills, no failures. Two honest qualifications: it is Linux x86_64/ext4 only, and `SIGKILL` is process death rather than power loss (see WP-06). Neither is a reason to hold the milestone open, because both are re-runs of a harness that now exists rather than work that does not | | | |
| **M2** | *It plays back* | Capture → progressive waveform → playback → seek, all headless | WP-10 | 25 | 79 |
| | *status 2026-09-26* | **Met.** The whole chain is headless and was run end to end on hardware: `vcw session --script "arm, record, sleep 20, stop, quit"` recorded 960,480 frames with no loss, `vcw waveform --pixels 100` drew them, and `vcw play --script "play, sleep 2, seek 15, sleep 2, skip-back, sleep 2, stop"` played 288,000 frames across a seek and a skip with **0 gaps** and `bit-perfect playback, confirmed against the OS`. The seek half is proved twice over, byte-for-byte in CI through the shipped binary and measured at a median 19.8 ms on a real device. Same two qualifications as M1: Linux x86_64 only, and the progressive waveform is built as the writer commits but read by polling rather than pushed, which is a WP-16 question about what the view wants | | | |
| **M3** | *It finds tracks* | **Met 2026-09-26.** Parity measured at 97.6-99.7% of VRipr's boundaries on the labeled corpus, exact to the frame; every boundary carries provenance, confidence and its evidence | WP-11 | 10 | 89 |
| **M4** | *It delivers* (**G1**) | Whole §50 workflow end to end from the CLI, including tagged FLAC/WAV export | WP-14 | 24 | 113 |
| **M5** | *It has a face* (**G2**) | §44 MVP plus Audacity import, keyboard-complete, packaged for every Tier 1 platform | WP-20 | 53 | 166 |
| **G3** | Identification | §45 scope | WP-27 | 47 | 213 |

At a full-time cadence the interesting consequence is that **M1 through M4 are a
continuous run at the hardest part of the product** - capture integrity and project
robustness - with no UI work to break it up. That is the right shape for this codebase
but it is also the stretch most likely to drift, so M1/M2/M3 exist specifically to force
demonstrable output along the way. If a milestone's weight overruns by more than about
half, that is the signal to re-plan rather than push on.

### 10.2 Scope levers (agreed in advance, applied only if G2 overruns badly)

Drop from 0.1 and defer to 0.2, in this order: multi-disc UI (keep the data model) ·
project browser (open-file dialog only) · settings UI (TOML file + CLI) · guided
detection (keep basic RMS/HMM) · Discogs (MusicBrainz only) · region playback (whole
capture + track only). Worth ~25 sessions.

Never cut: recovery, diagnostics honesty, the non-destructive guarantee, the open format
spec. Those are the product's integrity, and a 0.1 that compromises them is worse than a
later 0.1.

## 11. Definition of done

**Per work package:** tests written and green on 3 platforms · public API documented ·
requirement §s referenced in the PR · no clippy warnings · CHANGELOG entry ·
demonstrable from CLI or UI.

**Per gate:**
- **G0:** spike reports published; D1/D3/D6 locked; fallbacks costed.
- **G1:** full §50 workflow from CLI; recovery suite green; format spec published; detector parity demonstrated.
- **G2:** every §44 item present; §37 performance targets measured and met; keyboard-complete (§43); installers for 3 platforms; no known data-loss defect.
  *§37's status, 2026-09-29:* zero sample loss, bounded memory, multi-hour
  projects and waveform-cost-independent-of-length are measured and gated;
  sub-second waveform latency is measured and gated as of today (p99 250 ms);
  *real-time-feeling meters* has no number yet; *responsive UI during capture*
  is measured on the Rust side only, with S3's frontend figures for the other
  half. Every figure is from x86_64 Linux.
- **G3:** §45 items present; identification never silently overrides user-confirmed metadata (§26), covered by test.

---

## 12. Immediate next actions

*Updated 2026-09-28. The three actions that stood here - repo groundwork, S1, S2 - are
all done, and so are S3, S4 and S5. **Every work package in Phase 1 is now built**:
WP-01 through WP-20, with WP-06 meeting M1, WP-10 M2 and WP-11 M3, and D9 locked by
WP-15. What is left in the phase is not a build at all. It is a **run** on the turntable
(M4, below), a green CI, and a hand install on the three Tier 1 platforms this machine
cannot speak for.*

**Every stage of §50's chain now exists behind a CLI verb.** Connect is `vcw devices`,
select album is `vcw metadata search`/`fetch` and `vcw release set`, set level is
`vcw session arm` - which is a writer running and paused, so the level is set before the
needle drops - record and flip are `record`/`stop` twice into one project, review is
`vcw analyze` then `vcw adopt` then `vcw tracks list`, correct is the eleven `vcw tracks`
verbs, and export is `vcw export`. That is what **M4** asks for, and the one thing
standing between the chain existing and M4 being signed off is that **it has never been
run as a single continuous sequence on a real record with a tagged export at the end**.
Each link has been exercised against the real 2.33 GiB side in isolation. Doing the whole
run once, from `devices` to a FLAC on disk, is the cheapest remaining item on this list
and it is what would close G1.

1. **Done, 2026-09-28** - **WP-17, the test corpus and soak harness**, at weight 7.
   All six of §41's gaps are closed: file-backed capture through `vcw soak --from-file`,
   which retires the out-of-tree `realrip/` feeder; the four R9 device faults reachable
   from the CLI with an inverted, non-vacuous pass condition; RSS sampled and gated;
   reader threads drawing the waveform against a live project; the WAL size gated for
   the first time; and `scripts/soak-harness.sh` wired to two CI jobs, one on every push
   and a `schedule:` nightly - which is the exit criterion.

   **It found two product defects, not just harness gaps**, which is the argument for
   having run each gate in anger before wiring it to a verdict. A device that goes
   silent - R9's real shape, no error and no empty callback - was being filed as a
   `finalised` capture, because `Diagnostics`' four counters can only describe events
   that *happened* and three call sites derived the capture state from `is_clean()`
   alone. And nothing checked the WAL size: a run that reached 90 MiB against a 4 MiB
   budget printed "pass: bounded WAL". Both are fixed, both have tests with control
   arms, and the whole account is in `docs/STATUS.md`.

   What it does **not** give is a timing claim from CI. A hosted runner shares its CPU
   and its disk, so the nightly's endurance and correctness results stand and its commit
   tail does not. The real-time numbers of record still come off the rigs - and the first
   one from anything other than this box's SATA disk arrived today: 90 minutes of 24/192
   on media2026's NVMe held a worst commit of **17.0 ms** flat from minute 15, against
   102.6 ms here, with RSS growing about **0.5 MiB an hour**.

   **§37's sub-second waveform latency is measured and gated as of 2026-09-29**, which
   was the last of the four performance claims about a capture in progress that nothing
   checked. `vcw soak` now records, on every redraw, the gap between the newest frame the
   device has handed to the ring and the newest frame a read-only connection can draw:
   at 96 kHz with four readers the waveform is behind the stylus by **p50 130 ms, p99
   250 ms, max 300 ms**, and the figure is the commit interval and nothing else - half an
   interval on average, one interval at the p99, one interval plus a commit at the worst.
   `--waveform-budget-millis` gates the p99 at 1000 by default, and the regression it
   catches is a real one: `--batch-blocks 8` is a writer with a perfect record of every
   other kind that leaves the window 2 s stale, and the soak used to pass it.

   What is measured is the **Rust half** of *responsive UI during capture*, and writing
   that down found the other half's position worth stating: `Waveform.tsx` polls, by a
   WP-16 decision that still holds, and its redraw depends on the capture, the range, the
   panel width and a generation counter - so **the waveform does not grow while a record
   is being recorded**. The engine can now be shown to serve a picture 130-300 ms old for
   about 4 ms of query, and S3's frontend numbers put a 2 Hz live redraw at roughly 1% of
   the webview's main thread, so making the panel follow a capture is cheap and is a
   product judgment rather than a performance one. Recorded in *closing the loose ends*,
   deliberately not changed here. The meters (§37's *real-time-feeling* has no number
   yet), the webview's own RSS over a long session (risk R8) and every platform other
   than x86_64 Linux remain unmeasured.

   **The run that matters more than the next work package.** Every stage of §50's chain
   has a CLI verb and every stage of §44's workflows has a keybinding, and **neither
   sequence has been performed once from end to end.** M4 asks for the CLI chain:
   `devices`, `metadata search`, `release set`, `session arm`, record, flip, record,
   `analyze`, `adopt`, the `tracks` verbs, `export`, on a real record with a tagged FLAC
   at the end. It needs the turntable, so it belongs at the rig.

   **The UI half of this has now been done, and it paid for itself in an hour.** First
   light, 2026-09-27: the window opened against a four-project library, every panel drew
   real data, and five defects came out of a tree that was gate-green at 910 tests. Two of
   them could not have been caught by any test in the repository as it stood - a row
   selection no key could move, which broke this plan's own WP-16 exit criterion, and an
   unhandled read that left the window describing a project the shell did not have open.
   They are WP-16a, written up in `docs/STATUS.md`. The lesson generalizes and is the
   reason M4 should not slip: **a wired application and a working one are different
   claims, and only one of them can be asserted from a diff.**

   **What WP-16 and WP-16a leave behind.** The window runs. Eleven panels, 40 bindings
   over all 20 of §44's workflows, every one proved to reach a handler *and* every
   selectable list proved to be movable by key - the second of those is WP-16a's addition
   and the first one alone is what let the criterion be claimed wrongly. Screenshots exist
   under `/data2/vcw-scratch/firstlight/`. **No capture has been driven through the window
   from a live device**, which is what M4 is for. `waveform-update` and
   `fingerprint-match` are declared in §35 and nothing produces them - and after
   WP-16's decision to poll the waveform, the first of those is not missed. The Transport refuses to guess which face a marker belongs to on a capture
   holding two sides, which is honest and is the missing side extent showing through the
   UI for the first time. And the layout has met **one browser engine and one font
   stack**.

   **What WP-14 leaves behind, in the order it will be asked about.** §33 lists MP3 and
   Ogg as required initial formats and **both are now built** - WP-25, 2026-10-04, taken
   out of order because the licensing decision turned out to be the only thing holding
   them and half of it was not real (Ogg is BSD-3-Clause). Default-on cargo features,
   with MP3's LGPL-3.0 obligation live in `deny.toml` and `THIRD-PARTY-NOTICES.md`. **A default capture cannot be exported as FLAC**, because a device negotiation
   takes the widest integer format on offer, that is S32 here, and `flacenc` 0.5.1 stops
   at 24 bits; `--format s24` sidesteps it, and the real fix is either a 32-bit-capable
   encoder through bindings - which costs D5's pure-Rust choice - or an explicit
   operator-chosen dither, which is a §33 conversation and not a defect to patch
   quietly. `flacenc`'s 96 kHz ceiling is the same shape and worse, because §8 requires
   192 kHz outright; the test that asserts both caps fails the day either is lifted.
   The real side has now been exported and timed (2026-09-29: 26 minutes of 24/192
   in 28.36 s, 55x real time, and the FLAC refusal above arriving on the first real
   project put to it), and doing it found two things a suite of second-long tracks
   could not: a two-sided record collided with itself in the naming template, and
   **tagging a WAV costs the file's own size in resident memory** because lofty
   rewrites a RIFF container in memory. Both are in *closing the loose ends* in
   `docs/STATUS.md`; the first is fixed, the second is recorded against §37's
   bounded-memory claim and is not.

   **WP-13's skip is done, and it was not only a UI opinion.** `SKIP FORWARD` and
   `SKIP BACK` now land on the next and previous track edge, falling back to ten seconds
   only on a side nothing has been analyzed from. `vcw_project::track::edges_of_capture`
   is the query - both ends of every track across both faces, ascending and deduplicated,
   because adjacent tracks share a frame and a duplicated mark is a skip that appears to
   do nothing - `vcw_core::playback::Audition::marks` carries the frames, and the shell
   and the CLI both fill it in the same read-only open that resolves the scope. An earlier
   version of this paragraph said a skip might land on "the track start §33 pads out to";
   **there is no padding in §33 and none in the exporter**, which cuts exactly the span
   between two boundaries, and the phrase is corrected rather than left to be read as a
   requirement.

   **The promotion floor is a policy, and policies get tuned.** `Policy::min_sources`
   is 2, which is WP-11's over-segmentation finding honored, and on the real side it
   is the difference between 270 candidate boundaries and 6. But it is a blunt rule:
   a genuine quiet passage that only the level detector catches is turned down by the
   same arithmetic that turns down the HMM's noise. `adopt --dry-run` exists so the
   figure can be argued from a real record rather than from taste, and WP-16's track
   editor now shows a rejected boundary dimmed beside its confidence and the detectors
   that agreed - the rows carry `confidence` and `sources` precisely so that it could.

   The fan-out is ready for whichever comes first: `vcw_audio::buffers::Tee`
   takes any number of taps, the meter uses one, and a playback monitor or a live
   waveform worker is a second `tap()` call and a second thread, with no change to the
   capture path and no second reader of the device.

   **WP-09 leaves one thing for the UI to decide rather than the reader.** The ladder
   has a gap: at a 250 ms block the rungs are 256 frames and then the block itself,
   which at 192 kHz is 48,000 - a factor of 187. Every zoom in between is served by
   reading `summary256`, and the covering index makes that cheap enough (74 ms for an
   eight-minute span at 1920 px, cold) that no third rung is justified on the numbers.
   But the numbers are from a SATA SSD. On the Pi 5's SD card the same read is the one
   most likely to hurt, and that is the measurement to take before anyone adds a rung
   on instinct.

   **The waveform reader is polled, and WP-16 decided to keep it that way.** §19 asks
   for a progressive build; the pyramid is built by the writer as it commits, which is the
   right half to have, and the reader asks rather than being told. `Waveform.tsx` measures
   its own column count with a `ResizeObserver` and requests exactly that many, which a
   pushed event could not have done - it would deliver columns at the writer's rate rather
   than at the width the view happens to be. Nothing publishes `waveform-extended` and
   nothing needs to.

   **One thing WP-07 deliberately did not do: re-enter the state machine from a
   recovered project.** A project opened at launch with an unfinished capture in it is a
   state `vcw recover` reports on and the transport knows nothing about. That is
   correct for now - recovery closes a capture rather than resuming one, and §15 asks
   for exactly that. WP-16 decided what the UI does with it: `Capture.tsx` reports a
   `recovered` or `interrupted` capture with how much of it survived, offers nothing but
   play, and says so - "Nothing has been resumed or removed: play it, and decide".
   Appending to a capture that stopped for a reason nobody has established is the one
   operation in the application that can lose a rip, so it is not offered.

   **WP-03 through WP-10 all stay open on portability.** The device
   matrix and the OS format verifier are Linux x86_64 only, and on Windows and macOS
   the verifier returns `Unavailable` - a refusal to claim rather than a false pass,
   which is the right failure, but not the cross-check the criterion asks for.
   Playback adds one of its own: the buffer size it asks a device for is honored by
   ALSA here, and the fallback path for a backend that refuses has never run. The
   90-minute soak has run on x86_64/ext4 and nowhere else, and so has the kill suite.
   The kill suite is the cheapest of these to re-run: one `cargo test -p vcw-cli --
   --ignored` on each rig, no sound card and no operator.

   **WP-06 leaves one thing genuinely unproven, and it is not a portability gap.**
   `SIGKILL` ends a process; it does not cut power. The page cache survives, so the
   suite exercises SQLite's crash recovery and not the storage stack's.
   `synchronous=FULL` fsyncs every commit before it returns, which *should* make the
   difference nothing, but nothing in the repo demonstrates that. Closing it needs
   either real power cuts on a rig nobody minds losing or a fault-injecting
   filesystem; both are already on S2's open list, and neither blocks M1.

   **And then CI, which had never been green.** Checking WP-17's own exit criterion the
   next afternoon showed every one of the twenty runs GitHub still holds had failed, back
   to 2026-09-25, while the local gate was green on every one of those commits. Five
   causes: a local toolchain four releases behind CI's stable, so a new clippy lint could
   not fail a local run; the export suite's third-party verifiers absent from the runners,
   which its own anti-vacuity test correctly refused to pass without; the soak job
   asserting a commit-latency budget on a shared runner, which this plan had already said
   it should not; a Phase 0 spike that only compiles where its `dist/` was built; and
   **WP-17's memory gate live on Linux and silently absent on Windows and macOS**, where
   it reported "not measured" and printed pass.

   The last of those is the one worth remembering, because it is this plan's own lesson
   failing one day after it was written down. Fixing it also turned up that every
   third-party export check was unreachable on Windows, for the same reason: a helper
   that works on the developer's platform and quietly disables a class of verification
   elsewhere. Both now refuse rather than pretend, and **the gate grew from twelve legs
   to fifteen** on a new rule - it must have a counterpart for every CI job. The two it
   was missing were exactly where the rot was. `docs/STATUS.md` has the account.

   **And then the first run on the repaired workflow, which was still red, and that is
   what it was for.** Everything the repair aimed at worked; what failed was
   `cargo test --workspace` on three of four platforms, each differently, with only the
   platform this is developed on passing. Four more defects, and the shape of all four is
   the same as the shape of the five above: a number that is true on the dev box,
   asserted everywhere.

   The one with teeth: **Windows gives a main thread 1 MiB where Linux and macOS give 8,
   and a debug build of the CLI needs between 1.0 and 1.5 MiB to parse an argument**,
   because clap's derive builds every `Command` and `Arg` as a local of one unoptimized
   function. Release runs in 256 KiB, so nothing shipped was ever affected - but no
   integration test that spawns the binary could pass on Windows, and cargo stops at the
   first failing target, so the size of that hole is still unknown. `main` now runs on a
   thread with a stack it asks for, which unlike the MSVC linker's `/STACK:` is something
   `ulimit -s 1024` can check from here.

   The other three were all in one test file, and two of them were the test lying rather
   than the product failing: a recovery-loss floor that charged 1.9 s of process start-up
   to commit granularity, a child process whose stderr was piped and discarded so a
   writer that died on startup looked exactly like one that was killed, and a case that
   returned early and asserted nothing whenever the machine was slower than 120 ms. The
   third is **WP-17's vacuous-gate lesson for the third time in two days**, which is
   itself the finding: this class does not get caught by remembering it, only by running
   the thing somewhere that is not here.

2. **Done, 2026-09-28** - **WP-20, the Audacity import**, at weight 10. The largest
   item in Phase 1, and the last one that adds a capability. `vcw-import` is the twelfth
   crate: all 30 corpus projects parse byte-complete with zero dangling block
   references and are diffed against the Python oracle, three committed fixtures give CI
   real Audacity bytes to read (the shrinker deletes byte slices rather than writing a
   file), and `vcw import` lands a project as an **ordinary capture** so that every verb
   from WP-04 onwards reads it unchanged.

   **The mechanism is the interesting part.** Adopting Audacity's `sampleblocks` rows
   looks free and does not work: `waveclip/@offset` is the sequence origin rather than
   the audible start, so a trimmed clip's first audible sample almost never falls on a
   262,144-sample boundary, and an adopted block would need a sample offset the schema
   has no column for and `validate()` no way to check. Re-blocking through
   `persistence::Writer` costs one copy at import time and buys structural identity.

   **The exit criterion is a test rather than a demonstration**: both generations of
   `simples_test` imported, tracked, tagged, exported as WAV and compared byte for byte
   in the audio chunk. Driven by hand as well, on a real 612 MB rip: 79,141,433 frames a
   channel, three tracks from labels, 574.2 MiB of tagged WAV out. Two defects came out
   of the tests and neither was in the parser - a timeline that trusted sortedness its
   input type does not carry, and a label sitting past the end of the audio, which is
   the ordinary shape of a project somebody deleted a clip from.

3. **Done, 2026-09-28** - **WP-18, the documents and the diagnostic bundle**, at
   weight 5. The theme is that a document is a claim and a claim wants a test, and four
   of the five defects found were in prose that had just been written and believed.
   `crates/project/tests/third_party_spec.rs` makes the exit criterion permanent by
   driving `tools/vcw-read.py` over projects the product wrote: two independent
   implementations of one document must agree on the contents, on the bytes of all five
   storage formats, on a track span, on a damaged block and on what to refuse.
   `vcw bundle` is §42's diagnostic document, built around its negative - no raw or hex
   marker, no title, album, artist or project path, and credentials as a presence and a
   character count - and it went from 7.7 MB to 99 KB. `docs/PROJECT-API.md` has a
   compiled example beside it and `docs/USER-GUIDE.md` has both its tables generated and
   checked, which found `--out` for `--into` and `metadata release` for `metadata fetch`.

4. **Done, 2026-09-28** - **WP-19, packaging and release**, at weight 7. Shipping is the
   first exercise that runs the product the way a stranger will. The package now carries
   the **CLI beside the shell** as an `externalBin` sidecar, on `PATH` as `/usr/bin/vcw`
   from the deb, so the user guide's command lines work on an installed system; the debug
   sections came out for a measured 95 MB to 16.4 MB, an 11.2 MB deb and an 88.6 MB
   AppImage; signing is secret-conditional and says out loud which branch it took; and
   release notes come from the new `CHANGELOG.md`. Eight fast tests read
   `tauri.conf.json` so a four-minute build is not the feedback loop for a JSON file.
   **The largest finding was not in the product**: `.gitignore` held `/tools` from VRipr
   training material that had moved out, hiding WP-18's exit criterion, the checksum tool
   CI names five times and the icon renderer, and no gate leg could ever have seen it
   because every leg runs against a working tree where the files are present.
   **Linux x86_64 is installed and evidenced** - the deb laid out as an install, its
   `/usr/bin/vcw` recording 589824 frames at 96 kHz S32 with no losses and a clean
   `recover --verify`. Clicking the packaged window found two frontend defects invisible
   to 1013 tests. **Windows, macOS and Linux aarch64 need the rigs**, which is the half
   of the exit criterion this machine cannot sign off.

5. **In parallel, on the machine's own time** - the measurement jobs still queued from
   Phase 0, none of which need attention while they run:
   - **The soak on other platforms.** `scripts/soak-harness.sh` is the harness and needs
     no new code: Pi 5 on SD *and* on NVMe (S2 expected those to differ), and Windows.
     These are what would close WP-05's portability gap and the last of S2's. The NVMe
     half of that question now has one data point from media2026 rather than none, and
     it was a large one - a worst commit of 17.0 ms against 102.6 ms on SATA.
   - **S3's `cpu-matrix.sh`**, written and unrun. Until it does, whether the
     `OffscreenCanvas` worker reduces *total* CPU rather than main-thread blocking is
     unmeasured - and that is the figure the Pi 5 decision needs.
   - **S3's two R8 isolation soaks** (producer stopped; `echo` disabled), which split
     the +1.46 MiB/min webview growth into baseline versus per-message cost. Needed
     before G2, not before G0.

   **D3's firmed-config soak is no longer on this list.** WP-05's exit soak *is* that
   run, with the product code rather than the spike harness.

6. **Then** - **the first §50 run end to end**, which is what M4 asks for and what
   would close G1. Every link exists behind a CLI verb and every link has been exercised
   against the real 2.33 GiB side in isolation; the run itself is the outstanding item,
   not any part of it. It is cheap, it needs no new code, and it is the only thing that
   will say whether the chain holds together when nobody is stopping between steps.
   **WP-17 and WP-20 are now done**, so the harness that stops the capture path quietly
   regressing is in place, an Audacity project can be brought in as a capture, and this
   run is what remains. It is the next thing to do, and it needs
   the turntable rather than a keyboard.

**The remaining G0 exposure is hardware, not spikes.** Windows, Pi 5, Android, macOS
and the real converters (HiFiBerry DAC+ADC Pro, Tascam DA-3000) are all re-runs of
harnesses that already exist, which is a much smaller task than the original spike was.
Whether those rigs are to hand is the one question that shapes the schedule, and it is
not a question the code can answer.
