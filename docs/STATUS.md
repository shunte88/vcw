# VCW - project status

**As of:** 2026-10-04
**Phase:** 1 is complete and committed - WP-01 through WP-20 are built, plus WP-16a, and
WP-25, WP-28, WP-21 and WP-22 have been taken out of Phase 2, all on Linux x86_64 only.

> This snapshot was last read end to end on 2026-09-28. The sections written since
> then are dated where they sit: WP-19's packaging, *closing the loose ends*, the UI
> redesign and WP-25's lossy encoders. Everything before them is as it was; where a
> count or a license statement has moved on, the later section is the current one.
All five Phase 0 spikes returned verdicts on their primary platform; gate G0 remains
open on hardware coverage, WP-05's soak settled D3's firmed-config run, **WP-06 closes
milestone M1, *it records*,** WP-07 locks D8, WP-08 adds the meters and the §10 fan-out
they read through, WP-09 draws the waveform - and cost the schema two covering
indexes to do it in milliseconds rather than seconds - **WP-10 closes milestone M2,
*it plays back*,** with a seek that joins in a median 19.8 ms on hardware and byte-exactly
in CI, and **WP-11 closes milestone M3, *it finds tracks*,** at 97.6% to 99.7% parity
with VRipr over 595 labeled snippets, exact to the frame, and **WP-12 makes §40's
offline promise a property of the build** rather than a flag - the HTTP agent is behind a
feature, and `cargo test -p vcw-metadata --no-default-features` is a gate leg, and
**WP-13 turns a detection into a record** - schema v2, the release/side/track topology
and every §31 editing verb, with both halves of its exit criterion asserted by test
rather than argued: a byte-level fingerprint over both audio tables held constant across
15 edits, and a locked boundary surviving a real second detection pass, and **WP-14 turns a record
into files** - a splitter that reads committed blocks and writes nothing back, WAV and
FLAC written streaming, VRipr's naming templates and tag conventions ported, and both
halves of its exit criterion asserted separately: exported bytes compared against the
recorded blocks for three stored formats, and the tags read back by four pieces of
software nobody here wrote. Every stage of §50's chain now has a CLI verb behind it,
which is what **M4** asks for; what remains for M4 is running the chain end to end in one
pass on a real record, and **WP-15 puts a window in front of all of it** - an eleventh
crate, `vcw-contract`, holding §35's typed surface with the units already resolved, a
Tauri 2 shell in a cargo workspace of its own so that the root workspace's tree cannot
reach Tauri at all, and D9 locked as 1,422 lines of generated TypeScript with a drift test
and two new CI jobs behind it, and **WP-16 makes that window usable from the keyboard
alone** - eleven panels, 32 bindings covering all 20 of §44's workflows, and the exit
criterion asserted by a test that was verified to fail rather than by a reading of the
diff, which is how four workflows that had a binding and no handler were found. It also
answers the four questions earlier work packages deferred to it, WP-07's, WP-09's,
WP-11's and WP-13's, each in a file with the argument beside it.
WP-17 then turned §41's twelve kinds of test into a harness and **found the defect of the
week with it** - a device that goes silent was being filed as a flawless capture, because
every counter in `Diagnostics` describes an event that happened and none of them can
describe data that never arrived - and **WP-20 opens the door to twenty years of other
people's rips**: an Audacity `.aup3` or `.aup4` is re-blocked through the same writer a
live capture uses, so what lands is a capture and every verb from WP-04 onwards reads it
without being told where the audio came from.
**Branch:** `main` at `93a0ab8`, with WP-20 uncommitted in the working tree.

This is the running snapshot: where Phase 0 actually stands, what is proven versus
assumed, what is waiting on a decision, and what is waiting on hardware. The plan of
record is [`PROJECT_PLAN.md`](../PROJECT_PLAN.md); the spec is
[`REQUIREMENTS.md`](../REQUIREMENTS.md).

---

## The rename - complete

`aio-vripr` → **VCW, The Vinyl Capture Workstation**. Fully landed: GitHub repo renamed,
`origin` on `github.com/shunte88/vcw`, working directory `/data2/vcw`, Phase 0 spikes
committed, and the naming convention adopted across the deliverables including the `.vcw`
project extension. The only surviving mention of the old name is the deliberate
provenance note in the plan.

Predecessor **VRipr** keeps its own name - VCW is its successor, not a rebrand - and
`/data2/vripr` remains the source of the ~30 % of ported functionality.

## Phase 0 spikes

| Spike | Question | Verdict |
|---|---|---|
| **S1** | Bit-perfect capture and playback through CPAL? | **Yes on Linux/x86_64**, on stock CPAL 0.18.2. Other platforms open. |
| **S2** | Can SQLite absorb sustained 24/192 and survive a kill? | **Yes on x86_64**, 90-minute soaks passed on both a SATA disk (worst commit 102.6 ms) and NVMe (17.0 ms). Other platforms open. |
| **S3** | Will Tauri IPC carry meter and waveform rates? | **Yes, with 12.5× headroom** on Linux/WebKitGTK, incl. a 30-min soak with zero loss. The constraint is main-thread *rendering* (29% naive vs 0.5% in a worker), not IPC. One open item: webview RSS +1.46 MiB/min (R8). Other platforms open. |
| **S4** | Does `chromaprint-next` fingerprint from a stream? | **Yes, bit-identically.** Linux/x86_64. Other platforms open. |
| **S5** | Is the Audacity project format readable? | **Yes, decisively - AUP3 and AUP4 both.** |

~6,200 lines of Rust across five spike crates, plus ~1,240 lines of TypeScript
(the S3 bench frontend) and ~570 lines of Python analysis and format probing.
Write-ups in [`docs/spikes/`](spikes/).

### S1 - what it proved, and what upstream then fixed

Proven: CPAL's *audio path* is genuinely conversion-free. Re-measured 2026-09-23 on
**stock CPAL 0.18.2, no patches**: 2,883,584 frames at 24/192, zero drops, kernel
confirming the negotiated format; clean playback; 3/3 crash recovery.

The spike's durable output is the **kernel cross-check**. On 0.16 CPAL's ALSA device list
was the plug layer's fiction: a request for 48 kHz / 2 ch / I32 was reported as honored
while the hardware ran 8 kHz mono S16 - a silent upsample no CPAL API surfaced. The check
against `/proc/asound/.../hw_params` caught it, and it is a mandatory WP-04 obligation:
*bit-perfection is never claimed without OS confirmation.*

**Both 0.16 defects are fixed in the released 0.18.2**, which also ships
`HostTrait::device_by_id` and enumerates `hw:` PCMs - the exact API S1 had recommended
upstreaming. The vendored fork is deleted. R2 went L → M on the 0.16 evidence and back to
**L** on 0.18, with the verifier keeping it there.

**New finding: recovery loss has a floor the config cannot cross.** Loss is
`commit granularity + driver buffer`, rounded to a block boundary - not
`block_ms × batch_blocks` alone. Ring capacity swept 100–1000 ms changed nothing; the
170 ms ALSA buffer sets the floor. 250 ms blocks sit *at* that floor, which refines D3 and
corrected a crash-test budget that had been failing correct runs.

### S2 - acceptance met on one platform

The 90-minute soak passed: 5400.196 s, real-time factor 0.99996, 1,036,800,512 frames,
**zero dropped**, 8.29 GB of audio into an 8.41 GB database. Commit p99 63.2 ms and
worst-ever commit 102.3 ms against a 250 ms budget; peak WAL 4.57 MiB; RSS flat at
8.1 MiB; 207,707 concurrent reader queries across 4,236,810 blocks with zero checksum
failures. The commit distribution is stationary over 90 minutes.

The load-bearing conclusion: **throughput is not the constraint and is not near being
one.** The real design variable is recovery granularity, so the budget goes to smaller,
more frequent commits rather than bigger batches - which inverts the usual instinct.
R1's sidecar-file fallback looks unnecessary; §12's single-file project survives.

S2 measured worst-case loss as exactly `block_ms × batch_blocks`, which is true *of this
harness* - it has no audio device. S1 Finding 4 above shows the rest of the picture on
real hardware, and puts a floor under it that smaller commits cannot cross.

One honest gap, **closed 2026-09-25 by WP-05**: this soak ran the harness defaults
(`synchronous=NORMAL`, interleaved), not the D3-firmed `FULL` + per-channel. The short
matrix said the firmed config should be no worse, but *should be* is not *measured*.
WP-05's exit soak is that run, with the product code rather than the harness - see the
WP-05 section below, which also records the one thing the firmed config changed that
nobody had predicted: the WAL ceiling has to be stated in bytes, not pages.

### S3 - the experiment was aimed at the wrong thing

Thirteen arms × 60 s on the real compositor. The boundary itself is a non-issue: **zero
send errors, `recv/sent` = 1.000 and zero sequence gaps in every arm**, including two
that pushed 45,000 messages at the full 750 Hz worker rate (192 kHz, 256-frame
callbacks - the worst case §35 has to survive). The producer thread costs 0.14–0.19% of
a core coalesced and 0.75% uncoalesced, with send p99 ≤64 µs.

D6 asked three questions and got "doesn't matter", "backwards" and "backwards":

- **Channels vs the event bus** is indistinguishable at §35 payload sizes - 1.7–1.8%
  main-thread occupancy and 10.0–10.7 ms round trip for both, with the gap counts
  ordered *against* D6's prediction. Reading `tauri` 2.11.6 first explained why: under
  the 8192-byte threshold both transports are the same `webview.eval()` call.
- **Binary payloads are a pessimisation.** Under `MAX_RAW_DIRECT_EXECUTE_THRESHOLD`
  (1024 B) Tauri renders an `InvokeResponseBody::Raw` as a *decimal JSON array*. A 30 B
  meter frame becomes 116 B of evaluated JavaScript - 3.9× inflation, and 42% *more*
  source through the parser than the compact JSON it was supposed to beat.
- **Coalescing to 60 Hz is worse than not coalescing.** At 750 Hz occupancy was
  unchanged (1.6% vs 1.7%), long frames were *fewer* (71 vs 140), and delivery latency
  fell from 10.5 ms to **2.8 ms**. Coalescing sends bought nothing and cost ~7 ms.

**The load-bearing finding is one D6 never mentioned.** A full-canvas main-thread
redraw of a 1400×220 waveform at 60 Hz costs **29.0% of the main thread**, with 8.9% of
frames over 20 ms and nearly every frame over 4 ms. The same waveform in an
`OffscreenCanvas` worker costs **0.5%**, and was the only configuration measured that
never exceeded its frame budget - 3988 frames in a minute, maximum interval 16.0 ms,
where every main-thread arm reached 18–25 ms. So the engineering belongs in the
renderer, and the IPC layer can be chosen on ergonomics and never revisited.

The spike's other durable output is methodological, and it is why the numbers above are
stated the way they are. `performance.now()` on WebKitGTK is clamped to exactly 1 ms,
which made the first draft report every client-side latency as zero; the fix was to time
a round trip entirely on the Rust clock. The clamp truncates *timestamps*, not
durations, so the **mean** of clamped samples is unbiased while every individual sample
is useless - hence occupancy from means, corroborated by exact counts of frames over
1/4/8 ms, and no client-side percentile quoted as if it were precise. And
`requestAnimationFrame` is **not** vsync-paced here: 56–97 callbacks/s on a fixed 60 Hz
output, so fps is not a quality measure and **main-thread occupancy is the acceptance
metric instead**.

The 30-minute soak on the recommended configuration (channel + `manual` + worker) held
everything: 180,000 messages, **zero send errors, zero loss, zero sequence gaps**; the
worker rendered 119,133 frames with a **maximum interval of 18.0 ms**; main-thread draw
cost *fell* over the run (JIT warm-up) and whole-app CPU was flat at ~86–89% of one
core. Three frames out of 160,856 exceeded 33 ms.

**One thing did not hold, and it is now R8's first hard evidence.** Total RSS grew
491 → 538 MiB - a steady **+1.46 MiB/min with no plateau**, ~88 MiB/hour extrapolated.
That settles what the soak was run to settle (the matrix's 476 → 509 MiB drift was
time-based, not per-arm) and replaces it with a sharper question. It is orthogonal to
D6, and it is *not* diagnosed: this bench's UI allocates almost nothing per frame by
design, which makes application code the least likely cause and WebKit or tauri
per-message bookkeeping the most likely. It must be isolated before G2, because VCW is
meant to stay open across a multi-hour session.

Also worth recording because it nearly escaped: **every figure the bench reports is time
inside a callback it owns**, so it cannot see its own cost. Sampled from `/proc`, the
recommended configuration runs at ~89% of one core - 68% in `WebKitWebProcess`, 18% on
the tauri process's GTK main thread dispatching evals. The "0.5% occupancy" headline is
a main-thread *blocking* figure, not a CPU budget, and the Pi 5 conclusion will have to
rest on the latter. `cpu-sample.sh` and `cpu-matrix.sh` exist for that.

Honest gaps: one run per arm, so only the order-of-magnitude findings carry weight; this
rig mirrors a 59.98 Hz and a 29.96 Hz output, so absolute jank counts are contaminated
and only differences from the controls mean anything; the coalescing-raises-latency
effect is consistent across five arms but was not isolated by a controlled experiment;
and the bench UI is two canvases and a table, so these figures are a floor for a real
editor window.

### S4 - the acceptance was the easy half

Streamed and offline fingerprints are **bit-identical for every chunk shape tried**,
including one frame per `feed()` call, the ALSA period and buffer sizes, S2's 250 ms
block, a prime size and ragged ring drains - at 48 kHz and 192 kHz. Cost: **0.79 MiB and
0.6% of one core per stream**, with `feed()` taking 0.3% of its 250 ms block budget at
p99. A whole 22-minute 192 kHz side streams through in 6.5 MiB. Eight concurrent region
fingerprinters agree bit-for-bit and cost 2 ms of a 250 ms budget between them, so §46's
isolation requirement is satisfied several orders of magnitude over.

The more useful finding is one the plan did not ask for. **Region-boundary error is
bounded at ~0.064 BER**, reached at half a sub-fingerprint step (62 ms); a whole-step
error is a pure shift the matcher absorbs entirely. Unrelated audio scores 0.47–0.49 and
an MP3 320k round-trip costs 0.0009, so the worst boundary error a detector can make
still leaves a comfortable match. Consequence for Phase 2: **no re-fingerprint pass after
boundary refinement**, and the detector's precision requirement belongs to *editing*, not
identification. Capture rate (192k vs 44.1k), gain (−20 dB to +3 dB) and i16 narrowing
(truncate vs round) are all measurably **free** - fingerprint straight off the capture
stream at whatever rate the user chose.

Rather than trust the crate's "bit-identical to C" claim, it was checked against `fpcalc`
on byte-identical input. The pipeline after the resampler is exact on **9 of 9**
recordings. A resampler difference does exist - the packaged `fpcalc` links
`libswresample`/`libsoxr`, not chromaprint's bundled `av_resample` which the crate ports -
worth 2 to 6 single-bit flips out of 30,336, i.e. **five times smaller than an MP3 320k
round-trip**. Characterised, bounded, immaterial, and deliberately not root-caused.

One trap recorded for WP-11: `feed()` only `debug_assert!`s frame alignment, so a release
build handed a partial frame silently transposes the channel interleave and returns a
plausible wrong answer. The worker must guarantee whole frames at the type level.

The same hazard turned up in WP-11's own extractor, and the note is why it was looked
for: `features::Windows` was dropping the orphan sample at the end of each call rather
than transposing it, which is quieter and just as wrong. It now carries the part-frame
between calls. The fingerprint worker still has to do the same when it arrives.

### S5 - the format is not a mystery any more, in either version

The `ProjectSerializer` document format is decoded and verified against **all 30**
real vinyl projects in the corpus - 25 AUP3 and 5 AUP4: full byte consumption, zero
dangling block references. The acceptance test was chosen to be unforgiving - a wrong
tag-length grammar desynchronizes within a few records, so 30/30 clean parses is evidence
rather than optimism. Clean-room throughout: Audacity is GPL, this codebase is MIT.

Consequences worth carrying forward:

- Audacity has **no 32-bit integer sample format**, which is the concrete justification
  for D1's superset schema rather than an aesthetic preference.
- Audacity stores **mono 1 MiB blocks**, so the per-channel layout S2 measured and AUP4
  compatibility now *agree*. The D4 tension dissolves.
- **AUP3 to AUP4 conversion is byte-identical on the audio layer.** Five albums were
  converted by Audacity 4.0.0 on 2026-09-24, giving matched pairs across both rates,
  both sample formats, both page sizes and 451 MB to 4.7 GB. Hashing all `sampleblocks`
  samples, and separately the formats and both summary pyramids, matches exactly in
  every pair: nothing is resampled, reformatted or repacked, int24 and a 4096-byte page
  size both survive, and sparse `blockid` ranges are preserved rather than renumbered.
  The file grows by a handful of pages regardless of size. **R12 was aimed at the
  document blob, and the document is exactly where all the change landed - the part we
  depend on did not move**, so R12 drops to L/L.
- **An exhaustive attribute diff narrows the claim usefully.** Comparing every attribute
  by element path and sibling index (1,647 to 8,941 shared per pair), only 8 to 22
  differ: version stamps, the metadata reordering, an assigned track `colorindex`,
  editor selection state, invented unity envelope points, and a 2.7e-15 s re-round of
  `waveclip/@trimLeft` in two clips. So the correct statement is **blocks
  byte-identical, audio-bearing f64 attributes preserved to within a ULP** - fixture
  tests must compare timings with a tolerance, not `==`. That 2.7e-15 s is 5.1e-10 of a
  sample, so it cannot move a clip onto a different sample.
- The AUP4 delta is one table (`project_history`, one full document per save), one new
  record (tag `0x10`, a length-prefixed blob used only for a PNG screenshot that
  Audacity renders as the preview tile in its recent-projects list), and ~25 new
  dictionary names of which all but three are view or spectrogram state. Metadata did
  not move: it has always been `tags`/`tag` elements in the document, and it is
  unchanged in content - but **reordered**, so fixture comparisons must be set-based.
- **Two findings that are worth real money to WP-20.** AUP4 adds `waveblock/@length`,
  the block's sample count, and it matched `sampleblocks` in all 5,664 cases - a free
  integrity check on the document before reading any audio. And **sample blocks are
  shared between clips**: the clip-split project has 532 `waveblock` references to 456
  distinct blocks, one referenced three times. Reference counting is mandatory, and
  anything that frees a block when one clip stops referencing it corrupts another clip.
  Only a clip-split project reveals that; the four single-clip albums are all 1:1.

And a finding about the existing library rather than the code: **24 of the 25 rips are
stored float32**, so the current Audacity workflow has never been bit-perfect. Those
files are good masters, but they are not captures. That is a decision for the user, not a
defect to fix.

#### The correction that matters most

The 2026-09-22 write-up had the sample rate backwards, and the wrong rule had already
reached WP-20's acceptance criteria. It said to trust `project/@rate` (which reads
`192000.0`) over `wavetrack/@rate` (`48000.0`), on the assumption that these were 192 kHz
captures. Three independent checks say the opposite: label extents are recorded in
seconds and fit `numsamples/48000` to within six seconds while overrunning
`numsamples/192000` fourfold; the same albums exist as WAV in `/data2/source_rips` with
matching exact frame counts and a 48000 header; and `project/@rate` is `192000.0` in all
30 files regardless of content, which is not what a measurement looks like.

**`wavetrack/@rate` is authoritative.** The importer rule is now the reverse of what was
written, and the cost of the error would have been playing 22 of 25 rips at 4x speed,
silently. `probe.py` reports `project_rate` and `track_rates` as separate fields with a
`rate_disagrees` flag so a caller cannot collapse them again.

## G0 exit criteria still outstanding

- **Per-platform capture-mode matrix.** Windows/WASAPI exclusive, Android/AAudio, macOS
  (no hardware available - see the test-hardware gap).
- **S2 on Tier 1 hardware.** Pi 5 (aarch64, SD *and* NVMe - the honest worst case) and
  the Windows rig. Expect materially worse tails on the Pi.
- **S2 abuse matrix.** Disk-full, induced fsync stalls, `VACUUM`/compaction, live-project
  copy, page-size sweep, WAL2, and the full `sweep` matrix to completion.
- **Real converters.** Everything so far is onboard audio. The HiFiBerry DAC+ADC Pro and
  the Tascam DA-3000 are untested, as is `snd-aloop` bit-exactness (needs root).
- **S3's memory growth (R8).** +1.46 MiB/min with no plateau over 30 minutes, measured
  and undiagnosed. Two cheap experiments split it: re-soak with the producer stopped
  (WebKit baseline vs per-message cost), then with `echo` disabled (tauri's
  per-invocation bookkeeping). Needed before G2, not before G0.
- **S3's whole-app CPU per arm.** `cpu-matrix.sh` is written and ready but unrun: it
  samples `/proc` across `control-raf` → `control-nodraw` → `channel-manual` →
  `render-naive` → `render-worker`, which is the comparison the bench cannot make
  itself. Until it runs, **whether the worker reduces total CPU - as opposed to
  main-thread blocking - is unmeasured**, and that is the figure the Pi 5 decision needs.
- **S3 on other webviews.** Windows/WebView2 and the Pi 5. Tauri's two direct-execute
  thresholds were tuned upstream against WebView2 v135 and macOS, and the 1 ms clock
  clamp plus non-vsync `rAF` that shape every S3 conclusion are WebKit-specific. The
  `Raw`-inflation finding in particular is threshold-dependent and should be re-checked,
  not assumed portable.
- **S4 on other platforms.** Pure Rust, so low risk, but the local checkout's SIMD paths
  are NEON/x86-specific and deserve the aarch64 cross-check on the Pi 5.
- **AUP4 breadth**, now narrow enough to block nothing. Covered: both rates, both
  sample formats, both page sizes, 451 MB to 4.7 GB, single-clip and 19-clips-per-channel.
  Still unmeasured: `project_history` generation 2 (needs one `.aup4` saved a second
  time), a project *created* natively in Audacity 4 rather than converted, an envelope
  with more than one point or a non-unity `val`, and a populated `autosave`.

## Decisions resolved 2026-09-27

1. **D9 locked** ([ADR-0007](adr/0007-rust-typescript-contract.md)): `ts-rs` 12, one
   generated and committed declaration file, a drift test, and `git diff --exit-code`
   over it in CI. `specta` lost on WP-15's exit criterion rather than on merit - its
   Tauri half generates bindings from the command definitions, which would require the
   crate declaring them to depend on Tauri, and that crate is a core crate.
2. **The typed surface is a core crate, and the shell is a workspace of its own.** Two
   halves of one decision: `vcw-contract` resolves every unit on the Rust side so that
   §2's rule has nothing left to violate, and `app/` sits outside the root workspace so
   that `core-is-ui-free` stays a statement about structure. ADR-0007 amends
   [ADR-0003](adr/0003-workspace-layout.md), which had said `app/` would arrive at WP-15
   and left open whether it would be a member.
3. **64-bit integers cross the boundary as `number`.** `serde_json` writes a frame count
   as a JSON number and `JSON.parse` returns a double, so `bigint` - which is `ts-rs`'s
   default - is honest about the type and wrong about the value. The accepted ceiling is
   2^53 frames, about a billion years at 192 kHz.

## Decisions resolved 2026-09-25

1. **One crate per §6 group, not one per leaf** ([ADR-0003](adr/0003-workspace-layout.md)).
   §6's thirty-five leaves become modules. The boundaries worth enforcing - core
   independent of the UI, analysis independent of device access, one crate owning the
   database - are all group-level, and splitting further is cheap later and expensive
   to undo.
2. **`vcw-types` exists although §6 does not list it.** Sample formats, capture modes
   and rates are spoken everywhere; without a shared leaf they would live in
   `vcw-audio` and drag CPAL into the dependency closure of every crate that merely
   wants an enum. The test that makes the case concrete: WP-11's A/B harness has to run
   against the labeled corpus on a machine with no audio stack.
3. **The Phase 0 spikes leave the product workspace.** Finished evidence, not shipped
   code. A Linux-only CI job keeps them compiling so `docs/spikes/` stays reproducible,
   while the product's four-target matrix stays about the product.
4. **D2 locked** ([ADR-0002](adr/0002-sqlite-binding.md)): `rusqlite` with `bundled`.
   The bundling is not convenience - §15 makes recovery a correctness requirement and
   recovery depends on WAL semantics that vary across the SQLite versions distributions
   ship. A recovery test that passes in CI and fails on a Pi because the OS shipped an
   older SQLite is not a test.
5. **D7 and D10 locked** ([ADR-0004](adr/0004-license-and-toolchain.md)). The two
   clauses that are easy to get wrong: the LGPL exception in `deny.toml` stays
   commented out until `chromaprint-next` actually lands, and the MSRV is enforced by a
   pinned CI job because an untested floor is not a floor.
6. **D8 locked** ([ADR-0005](adr/0005-concurrency-model.md)): dedicated OS threads on
   the capture path, `mpsc` between them, no async runtime near audio or SQLite, and
   Tokio only when network I/O arrives at WP-12. Two clauses of the original wording
   changed on contact with WP-07 - the engine thread is *required* rather than
   preferred, because `cpal`'s stream handle is `!Send`, and elevated thread priority
   is not implemented, because no soak has yet needed it.

## Decisions resolved 2026-09-24

1. **`chromaprint-next 0.1.0` from crates.io is the dependency of record**, applying the
   CPAL policy below to the second dependency that had a local checkout. The checkout at
   `/data2/chromaprint-next` sits two SIMD commits ahead of the release; both were
   verified **fingerprint-neutral** out of tree rather than patched in. Same rule as
   cpal: fork only for a defect not already fixed upstream, never a `[patch.crates-io]`
   path copy.
2. **Fingerprinting runs off the capture stream, at the capture rate, with plain `>> 16`
   narrowing.** S4 measured rate, gain and narrowing to be free, so no staging file, no
   pre-decimation and no dither on the fingerprint path.
3. **D6 rewritten, not confirmed.** S3 rejected two of its three clauses. Channels are
   kept for API shape, not speed (indistinguishable from the event bus). `Raw` binary
   payloads are **out** - Tauri eval's them as decimal JSON arrays, making them 42%
   larger than the compact JSON they replace. Send-side coalescing is **out** - 750 Hz
   cost no extra main-thread time and a quarter of the latency; coalesce *paints*
   instead. And D6 gained the clause that actually mattered: **the waveform renders in
   an `OffscreenCanvas` worker**, against 29% of the main thread for a naive redraw.
   Acceptance metric is main-thread occupancy, not fps.
4. **Main-thread occupancy is the UI acceptance metric**, because `rAF` on WebKitGTK is
   not vsync-paced (56–97/s on a fixed 60 Hz output), so fps is not comparable across
   configurations. Corollary recorded the hard way: occupancy is a *blocking* measure and
   says nothing about CPU, which needs external `/proc` sampling.
5. **The Audacity importer takes `wavetrack/@rate` and ignores `project/@rate`**,
   reversing the 2026-09-22 S5 conclusion, which had already reached WP-20's acceptance
   criteria. `project/@rate` is a stored editor preference and reads `192000.0` in all 29
   corpus projects regardless of content. Trusting it would have played 22 of 25 rips at
   4x speed, silently.
6. **AUP4 is a superset of AUP3 in practice, not just in intent.** Same
   `application_id`, one added table, one added record type, ~25 added dictionary names,
   and an audio layer that converts byte-for-byte across five matched pairs spanning
   both rates, both sample formats and both page sizes. Version dispatch is on
   `user_version`, never on the extension or the magic.

## Decisions resolved 2026-09-23

1. **The project extension is `.vcw`** (was `.vripr`). Applied to the spike CLIs,
   `.gitignore` patterns, D1 in the plan, the local-dev config path (`~/.config/vcw/`,
   `vcw.toml`) and every doc.
2. **CPAL: track the released crate.** `shunte88/cpal` is the fork of record for any
   defect we find that is not already fixed upstream - fixes go there first, then
   upstream. It is currently **unused**, because 0.18.2 fixed both defects we had found,
   which is the better outcome. Standing policy: prefer the released crate; fork only
   with a reproduction we cannot get upstream in time; never carry a `[patch.crates-io]`
   path copy again - that is what we had, and it quietly hid how stale our pin was.

## Still open

- **Corpus fixture strategy.** The 30 real projects are the import regression set; they
  need a shrinker (WP-20) so fixtures are committable.
- ~~**Crate naming.**~~ Settled at WP-01 - see [ADR-0003](adr/0003-workspace-layout.md).

## Phase 1 - WP-01, the workspace scaffold

Built 2026-09-25. Ten crates under `crates/`, one per REQUIREMENTS §6 group with §6's
leaves as modules, named `vcw-*`, binary `vcw`. The full reasoning - including why not
one crate per leaf, and why `vcw-types` exists when §6 does not list it - is
[ADR-0003](adr/0003-workspace-layout.md).

What landed:

- **`crates/`**: `types`, `audio`, `signal`, `fingerprint`, `identify`, `metadata`,
  `project`, `export`, `core`, `cli`. Every module file states its scope, its
  requirement sections and the work package that fills it; `missing_docs` is a lint CI
  treats as an error, so a module cannot be added without saying what it is for.
- **Real content where a decision already fixed it.** `vcw-types` carries
  `SampleFormat` with the Audacity code mapping S5 measured - including the `None` arm
  for 32-bit integer, which is the concrete reason D1 is a superset and not a clone -
  plus `CaptureMode` from §9 and §8's six standard rates. `vcw-project::sqlite` carries
  the `.vcw` `application_id` and asserts it is not Audacity's. Seven tests, all
  passing.
- **`vcw doctor`** runs and prints host APIs, the bundled SQLite version and the
  supported rates. A scaffold that runs is worth more than one that only builds.
- **The spikes moved to their own workspace** at `spikes/`, excluded from the root.
  They are finished evidence, not shipped code; a Linux-only CI job keeps them
  compiling so the numbers in `docs/spikes/` stay reproducible.
- **CI** (`.github/workflows/ci.yml`): build + test on four targets, `fmt`, `clippy -D
  warnings`, an MSRV job pinned to 1.90, `cargo deny check`, the spikes job, and an
  assertion that **no crate under `crates/` depends on `tauri`, `wry`, `tao` or
  `webkit2gtk`** - §2 as a test rather than a code-review habit.
- **License gates**: `deny.toml` with the permissive allowlist, `THIRD-PARTY-NOTICES.md`
  rewritten for VCW's actual dependency set, and `LICENSE-LGPL-2.1` ported. D7 and D10
  locked in [ADR-0004](adr/0004-license-and-toolchain.md).
- **ADRs 0001-0004** written, with an index at [`docs/adr/`](adr/).

#### What is verified, and what is not

`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace` and `cargo deny check` are all clean **on Linux x86_64 with
Rust 1.94.1**. The aarch64, Windows and macOS legs exist only as workflow YAML and are
**unverified until the workflow runs on a push** - as is the MSRV 1.90 job, since only
1.94.1 is installed locally. WP-01's exit criterion is "green CI on four targets", so
WP-01 is built but not yet met.

One choice worth flagging: aarch64 Linux runs **natively** on `ubuntu-24.04-arm` rather
than cross-compiled. `alsa-sys` and bundled SQLite are exactly the dependencies a
cross-build gets wrong quietly, and aarch64 is the Pi 5 target, so the cross-compile
saves nothing worth having.

## Phase 1 - WP-02, the `.vcw` schema

Built 2026-09-25. Schema v1 lives in `vcw-project` and is a deliberate superset of
Audacity's AUP4: `sampleblocks` is reproduced column for column, including the
AUTOINCREMENT key and the *absence* of `NOT NULL`, so an imported block needs no
rewriting and stays byte-identical. Everything else is the half Audacity has nowhere
to put.

| module | what it is |
|---|---|
| `schema.rs` | the v1 DDL, heavily commented, plus the identity constants |
| `migrate.rs` | the transactional runner: one transaction per step, `user_version` moved inside it |
| `sqlite.rs` | `Project::create` / `open` / `open_read_only` / `close`, connection pragmas, `block_checksum` |
| `meta.rs` | the six required `meta` keys, §16's versions among them |
| `validate.rs` | `validate()` with 17 finding codes, and `integrity_check()` |
| `error.rs` | a `NotAProject` that tells an Audacity file it wants the importer |
| `doc.rs` | generates [`SCHEMA.md`](SCHEMA.md) from the DDL, comments and all |

Six tables. `sampleblocks` is Audacity's; `captures`, `capture_blocks`,
`capture_diagnostics`, `meta` and `schema_migrations` are ours. `capture_blocks` is the
key one: Audacity keeps block provenance inside its document blob, and recovery cannot
parse a document that was never written, so ours is a table that recovery can read from
committed rows alone.

**Sample formats.** §8 requires 32-bit integer capture and D4 stores 24-bit verbatim,
and Audacity has a code for neither - it knows three formats and pads 24-bit to four
bytes. `StorageFormat` in `vcw-types` reuses Audacity's three codes unchanged and adds
`Int24Packed` (`0x00030002`) and `Int32` (`0x00040002`) in unused space, using
Audacity's own `(width << 16) | type` encoding with a type code it never emits. Nothing
imported is touched; nothing exported is a lie.

#### What is verified, and what is not

44 tests in `vcw-project`, 55 across the workspace, all green, plus `fmt`, `clippy -D
warnings` and `cargo deny check`.

- `tests/aup4_shape.rs` diffs `sampleblocks` against DDL extracted **verbatim** from a
  corpus `.aup3` *and* `.aup4`, column for column via `PRAGMA table_info`, and asserts
  the two Audacity versions agree with each other. This is D1's central claim, now
  checked by CI instead of asserted in prose.
- `tests/roundtrip.rs` writes and reads blocks in all five storage formats across seven
  shapes and compares bytes. It also proves a read-only open honors the `-wal` - the
  writer is held open so the rows exist *only* in the WAL, which is the S5 trap that
  `immutable=1` falls into.
- `tests/migrations.rs` runs a synthetic multi-step set, because one real migration
  proves nothing about the machinery: resumption from any intermediate version reaches
  the same schema, re-applying is a no-op, and a step that fails halfway leaves
  `user_version`, the table set and `schema_migrations` untouched and retryable.
- `tests/schema_doc.rs` regenerates `docs/SCHEMA.md` and fails on drift, then
  cross-checks the parse against `PRAGMA table_info` on a real database - a generator
  is only as trustworthy as its parser, and a self-consistent wrong document is worse
  than none.

Not verified *at the time*: anything a real capture writes. Every test in this section
builds its blocks synthetically. The writer thread, batching and checkpoint policy
arrived with WP-05, which is where the schema first held bytes a capture produced; the
kill-at-random-point recovery suite is still WP-06. D3's block parameters are baked
into `schema.rs` as constants and WP-05's soak is what settled them.

## Phase 1 - WP-03, devices

Built 2026-09-25, **on Linux x86_64 only**. `vcw-audio` answers §7's question - what can
this machine record, through which path, and is that path capable of being bit-perfect -
and §8's - which rates, formats and channel counts will it really accept.

| module | what it is |
|---|---|
| `devices.rs` | `DeviceKey`, `Transport`, `DeviceReport`, `Snapshot`, and `Snapshot::diff` for hot-plug |
| `probe.rs` | the §8 capability matrix, and the advertised-versus-confirmed distinction |
| `selection.rs` | independent in/out preferences, persisted by id, and `resolve()` |
| `error.rs` | errors that name the device and say what happened, including "unplugged" |

**One identity, and it is not the name.** Everything keys on `DeviceKey`, CPAL's
`host:id` - `alsa:hw:CARD=0,DEV=0`. Selection by name is *refused when ambiguous* rather
than resolved to the first match, because on ALSA the same card appears as `hw:` and
`plughw:` under one name and only one of them can be bit-perfect. Picking the wrong one
silently is the failure mode §9 exists to prevent.

**Transport classification.** `hw:` is direct hardware and the only bit-perfect
candidate; `plughw:` is converting; `default`, `pipewire`, `pulse`, `dsnoop`, the rate
converters and the rest are virtual. `Transport::can_be_bit_perfect()` returns
`Some(true)` for exactly one of those three, and `None` where the platform does not say -
an honest "unknown" rather than an optimistic guess.

**Advertised is not confirmed.** A `SupportedStreamConfigRange` is a claim, and S1 found
claims that fail at stream build. The matrix therefore carries three states -
`Advertised`, `Confirmed` (a stream was built and dropped), `Rejected` - and only
`--confirm` promotes anything.

#### What is verified, and what is not

103 tests across the workspace, 48 of them in `vcw-audio`, all green, plus `fmt`,
`clippy -D warnings` and `cargo deny check`.

Measured live on this host, through the new `vcw formats`:

- `hw:CARD=0,DEV=0` (ALC1150 analog in) advertised 8 configurations - 44.1/48/96/192 kHz
  x S16/S32, 2 ch - and **confirmed all 8**. A direct hardware path tells the truth.
- `plughw:CARD=2,DEV=0` (a mono 8 kHz webcam) advertised **1536**: every channel count
  from 1 to 64, at all six §8 rates, in all four formats. Confirming the first two
  channel counts shows it accepts one channel at every rate, plus a stereo fiction at
  48/96/192 kHz that the plug layer manufactures. This is S1's plug-layer fiction,
  reproduced and now machine-checkable.

That second measurement forced a design change: `confirm` opens the device once per
entry, so `Matrix::with_channels_at_most` bounds the sweep and `vcw formats --confirm`
defaults to 8 channels. The *advertisement* is still reported in full - what the backend
claimed is a fact about the backend.

- `tests/hotplug.rs` drives `Snapshot::diff` over synthetic snapshots: appear, disappear,
  reconfigure, rename, and the case that matters most - unplugging a card removes *every*
  path to it, `hw:` and `plughw:` alike, as one event and not three.
- `tests/preferences.rs` proves the rule with no exceptions: **an absent device resolves
  to `Missing`, never to a substitute**, even when the platform default and a plug path
  to the same card are both present and would work. S1 finding 3 is why - the platform
  default here is PipeWire at 44.1 kHz F32, a desktop-audio default that would quietly
  make an archival capture worse than the one asked for.
- `tests/enumerate.rs` runs against whatever hardware is actually present and asserts
  invariants rather than a device list, so it is meaningful on a Pi and on a CI runner
  with no sound card at all.

**Exit criteria only partly met**, and this is the honest position:

- *"Device matrix reported on 3 OS"* - reported on **one**. Windows and macOS are
  unverified. The code has no Linux-specific paths outside `Transport::classify`, but
  untested is untested.
- *"unplug during idle/record is non-corrupting"* - **idle only**. Unplug during record
  needs a running stream, which is WP-04. R9 keeps its fault-injection obligation there.

Two known warts, neither blocking. ALSA's C library writes diagnostics straight to
stderr during enumeration (`snd_pcm_dmix_open ... supports only playback stream`);
silencing it needs `snd_lib_error_set_handler` through `alsa-sys`. And CPAL enumerates
the same card twice on this host, once as `CARD=PCH` and once as `CARD=0` - harmless,
because both keys open the same PCM, but it makes the list longer than the hardware.

## Phase 1 - WP-04, capture

Built 2026-09-25, **on Linux x86_64 only**. This is the work package the whole project
turns on: §9's bit-perfect capture, §10's real-time callback, and §38's provenance. Its
one non-negotiable rule is that the application never claims bit-perfection on the
audio API's word.

| module | what it is |
|---|---|
| `types/capture.rs` | `CaptureState`, `Diagnostics`, `CaptureInfo` - the vocabulary `vcw-audio` and `vcw-project` share without depending on each other |
| `audio/buffers.rs` | the wait-free SPSC ring: whole-chunk-or-nothing writes, a 500 ms floor from S2 |
| `audio/verify.rs` | the OS cross-check. Reads `/proc/asound/.../hw_params` and compares rate, channels and format |
| `audio/capture.rs` | `Request` -> `Negotiated`, the RT callback as `Sink::on_data`, the counters, and `verdict()` |
| `audio/source.rs` | the `Source` trait, and `Simulated` - a device-free capture with fault injection |
| `project/session.rs` | the `captures` and `capture_diagnostics` rows: begin, advance, record, finish |
| `cli/capture.rs` | `vcw capture`, which drives all of it headlessly (§4.5) |

**The callback is provably allocation-free, not assertedly.** The whole body of the
CPAL callback is `Sink::on_data(&mut self, bytes: &[u8])`, an ordinary function over a
byte slice. That is what makes it testable at all - a closure inside a live stream
cannot be driven by a test. `tests/rt_safety.rs` installs a counting global allocator
and runs it 1000 times on the ordinary path, 1000 times while overrunning, 1000 times
on an empty callback, and once while recording a stream error. Every count is zero. The
file's first test is the control: it allocates a `Vec` and asserts the counter *saw*
it, because a broken counter would otherwise "prove" everything.

Lock-freedom is claimed only as far as it is measured. That rtrb is wait-free SPSC and
the counters are relaxed atomics is an argument from construction; what the file
actually demonstrates is the consequence that matters - a consumer parked forever
cannot make a callback take 100 ms, and the ring overruns instead.

**Bit-perfection has exactly one source of truth**, the free function `verdict()`. It
returns `Confirmed` only when all four hold: the OS agrees with the negotiated format,
every field the caller pinned came back unchanged, the transport is direct hardware,
and all four counters are zero. Anything else is `Refuted` with reasons or
`Unconfirmed` with the gap named - never a pass by default. `Source::verdict` is a
*defaulted* trait method precisely so no implementation can override it; a source that
could override it could also lie. And `Negotiated::simulated` reports an unknown
transport and a shared mode, so a simulated capture is refused the claim on two
independent grounds however clean its counters are.

**The verifier is pointed only at `hw:`.** Resolving a `plughw:` id to the card beneath
it would confirm a format the application never received, which is worse than not
checking. `plughw:`, `default`, `pipewire` and `pulse` all resolve to `None` and report
`Unavailable`, and path traversal in a device id is rejected.

**Counters are persisted, not merely counted.** `session.rs` writes both rows in one
transaction, advances `frames` as the capture runs rather than once at the end, and
keys recovery on `finished_at IS NULL` - the absence of a write, which is the one thing
a crash cannot forge.

#### What is verified, and what is not

185 tests across the workspace, all green, plus `fmt`, `clippy -D warnings` and
`cargo deny check`. 106 are in `vcw-audio`, 57 in `vcw-project`, 17 in `vcw-types`, and
5 in `vcw-core`, which is where a test can finally watch a capture's counters land in a
project file - the two crates are deliberately independent, so neither could prove it
alone.

Measured live on this host, through the new `vcw capture`:

- `hw:CARD=0,DEV=0` with nothing pinned negotiated 96 kHz / 2 ch / S32 in exclusive mode
  over a direct hardware path, and `/proc/asound/card0/pcm0c/sub0/hw_params` read back
  `S32_LE 96000 Hz 2 ch`. 294912 frames in 3 s, every counter zero, verdict **bit-perfect,
  confirmed against the OS**. That is the first end-to-end confirmation in the project.
- The same card through `plughw:CARD=0,DEV=0` was refused: exclusive mode was downgraded
  to shared and reported as a divergence, the transport is converting, and the verifier
  declined to resolve the id at all. Two reasons, no claim.
- A request for 22050 Hz - not a §8 rate and not one the card offers - is an error
  naming what the device does offer, not a quiet capture at 44100.

Device-free, in CI:

- `audio/tests/capture_path.rs` runs the deterministic source through the ring and
  compares every byte against a recomputed expectation. "The capture completed" becomes
  "the capture contains exactly the right bytes in exactly the right order".
- **R9, device removal mid-capture**, is closed for the capture path.
  `Faults::unplug_after` makes the source go quiet without ending the stream; the tests
  assert one stream error counted, no overruns, byte-exact data up to the moment it
  went, the counters reaching the file, and `validate()` still clean afterwards. A cable
  cannot be pulled by CI, so it is pulled here instead.
- `core/tests/capture_to_project.rs` also drops a project handle mid-capture with no
  `finish()` call, reopens the file, and asserts recovery can see how far it got and
  that an unfinished capture does not read as a damaged project.

**Exit criteria partly met.** "Callback provably allocation-free and lock-free",
"requested vs negotiated reported" and "counters persisted" are done and demonstrated.
The third clause - *"negotiated format cross-checked against the OS"* - is met **on
Linux only**. Windows needs the WASAPI exclusive-mode format and macOS needs its own
reading; both return `Unavailable` today, which downgrades the verdict to `Unconfirmed`
rather than passing it, so the failure is in the safe direction. But an honest refusal
to claim is not the same as a cross-check, and until those two exist WP-04 stays open.

One deliberate omission: `vcw capture` drains the ring and discards the samples. WP-05
owns the writer, and a second block-committing implementation that nothing else uses
would be worse than none.

## Phase 1 - WP-05, the persistence writer

Built 2026-09-25. WP-02 gave the schema a shape and WP-04 gave the capture path a
voice; until now nothing had ever written a byte of real audio into a `.vcw` file.
Every block in every test was synthetic. This is what closes that.

| module | what it is |
|---|---|
| `types/format.rs` | `StorageFormat::decode_sample` - one stored sample to a normalized `f32`, with the scaling **measured against the corpus**, not assumed |
| `types/capture.rs` | the `PcmSource` trait: `read` and `is_finished`, and nothing else |
| `audio/buffers.rs` | `impl PcmSource for RingReader` - four lines, and the only thing joining the two halves |
| `project/persistence.rs` | `Config`, `Checkpoint`, `Summary`, `pyramid`, `Latencies`, `Progress`, `Writer`, `Outcome`, `spawn`/`Handle` |
| `cli/capture.rs` | `vcw capture --project` now writes the audio, not just the row |
| `cli/soak.rs` | `vcw soak` - the long-run measurement, with a byte-for-byte readback |

**The two crates still do not know about each other.** `vcw-audio` owns the ring and
`vcw-project` owns the writer, and making one depend on the other to join them would
have undone the layering the whole design rests on. Instead `vcw-types` gained a
two-method trait, `vcw-audio` implements it for its own `RingReader`, and the writer
takes `impl PcmSource`. No newtype, no orphan rule, no dependency - and the same writer
can be driven by a generator in CI and by a turntable on the bench with nothing
changing between them.

**The frame count moves inside the block transaction.** `captures.frames` is advanced
in the same transaction that commits the blocks. Were it a separate write, a crash
between the two would leave a project claiming more audio than it holds, and recovery
would have to choose which of two committed facts to believe. Written together, the
count can never run ahead of the data.

**A failed commit stops the writer.** Blocks tile each channel's timeline with no gaps;
`validate()` enforces it and WP-06 will depend on it. Carrying on after a failed commit
would punch a hole no later write could close, so the writer stops, the session is
marked interrupted and the error is surfaced. A short capture that says why it is short
beats a long one with a hole in it.

**The summary pyramid was measured, not ported.** The spike's summarizer scaled every
format by `2^(8*bps-1)`, which for Audacity's padded 24-bit would have been 256 times
too quiet - that format is a little-endian `i32` holding a value in +/-2^23, not a
left-justified one. Reading the corpus instead of the spike also turned up two things
worth knowing about `summary256`/`summary64k`, both confirmed against real files:
Audacity sizes the arrays to the block's *capacity* and pads the tail with
`(FLT_MAX, -FLT_MAX, 0)`, and it builds the 64k level from the 256 level by weighting
every group as a full 256 samples before dividing by the true count, so its 64k rms
runs slightly high wherever a block does not divide evenly. VCW computes each level
from the samples with true denominators and emits exactly the groups that exist. The
divergence is under 0.1 % and only in a final partial group; imported Audacity blocks
keep their own summaries untouched under D4, so the two conventions coexist without
either being rewritten.

**A finding that changes D3: the WAL ceiling has to be stated in bytes.** S2 measured a
peak write-ahead log of 4.57 MiB, on a harness using SQLite's default 4 KiB pages. VCW's
pages are 64 KiB, and SQLite's autocheckpoint threshold counts *pages*, so the stock
setting of 1000 is a 64 MiB log rather than a 4 MiB one. `Config::wal_bytes` now states
the ceiling in bytes and converts to pages against the file's actual page size, and a
unit test reads `PRAGMA wal_autocheckpoint` back to prove the conversion happened.

Measured as a pair on `/data2` (ext4), three minutes at 192 kHz each, everything but
the ceiling identical:

| Ceiling | WAL peak | commit p50 | p95 | p99 | max |
|---|---|---|---|---|---|
| 64 MiB (the stock 1000 pages) | 64.71 MiB | 4.9 ms | 8.3 ms | 84.0 ms | 100.1 ms |
| 4 MiB (`Config::wal_bytes`) | **4.50 MiB** | 5.1 ms | 15.8 ms | **31.9 ms** | **54.3 ms** |

4.50 MiB is within 2 % of S2's 4.57 MiB, which is the point: the spike's figure was
right and VCW was silently not reproducing it. The tail improves by 2.6x at p99 and
1.8x at the maximum, because a checkpoint that has 4 MiB to fold back finishes inside
a block period and one with 64 MiB does not. The trade is real and visible at p95,
which gets *worse* (8.3 -> 15.8 ms): checkpoints are more frequent, so more commits pay
a small share of one. That is the right way round for real-time capture, where a rare
100 ms stall is the thing that costs audio and a common 16 ms one against a 250 ms
budget costs nothing.

An earlier version of this section quoted four figures measured on `/tmp`, which is
tmpfs on this machine. They were RAM numbers presented as storage numbers and have been
replaced by the table above.

#### What is verified, and what is not

**The exit criterion is met.** 90 minutes of 24/192 stereo on x86_64/ext4, product
code rather than a spike harness, finished 2026-09-25 11:01:

```
ran         5400.1 s wall, 5400.1 s of audio, real-time factor 1.00001
written     1036824960 frames, 43202 blocks, 5.79 GiB of samples in 21601 commits
commit      p50 5.7 ms, p95 16.2 ms, p99 29.8 ms, max 79.4 ms, budget 250 ms
prepare     p50 6.1 ms, max 18.7 ms (deinterleave, summaries, crc)
wal         peak 4.81 MiB, 0 writer checkpoint(s)
file        5.94 GiB
counters    0 overruns, 0 underruns, 0 dropped frames, 0 stream errors
validate    clean
bytes       every one of 6220949760 matches what the source generated
```

Three things in there are worth more than the headline:

- **"Every byte" means every byte.** `vcw soak` does not compare a checksum. It streams
  `capture_blocks JOIN sampleblocks` in timeline order and recomputes
  `Simulated::expected_sample(frame, channel)` from the frame index *stored in each
  block*, so all 6,220,949,760 sample bytes were checked against what the generator
  would have produced at that exact offset on that exact channel. A block written at
  the wrong offset, on the wrong channel, or after a silent gap fails; it cannot pass on
  its own internal consistency.
- **The worst commit of 21,601 was 79.4 ms against a 250 ms budget**, and the ring holds
  1000 ms. The margin that matters is not the average, it is that the single worst
  moment in an hour and a half still left 170 ms of slack and never came close to the
  ring. Zero overruns is the consequence, not a separate result.
- **The WAL never grew and the writer never checkpointed.** Peak 4.81 MiB across 5.94
  GiB of project, with SQLite's autocheckpoint doing all of it at the threshold
  `Config::wal_bytes` computed; `Checkpoint::Automatic` needed no help. The `-wal` and
  `-shm` sidecars were gone after close, which is what a clean shutdown looks like and
  what WP-06 will use as its signal.

The real-time factor of 1.00001 is the generator's pacing, not a performance figure.
What it establishes is that the writer never became the bottleneck: had it fallen
behind, the ring would have overrun and the counter would say so.

**D3 is firmed by this run.** 250 ms per-channel blocks, batch 1, WAL, `synchronous=FULL`,
a 4 MiB log ceiling and a >= 500 ms ring are now measured on the shipping code rather
than inferred from a spike.


Device-free, in CI:

- `project/src/persistence.rs` holds 19 unit tests: block splitting at the configured
  duration, a short final block written rather than discarded, a trailing partial frame
  held over rather than padded, the frame count checked against the committed blocks
  after *every* commit, per-block checksums, batch size changing the transaction count
  and nothing else, summaries on and off, the pyramid's group count and its short final
  group, the WAL ceiling honored in pages derived from bytes, progress visible while
  the writer runs rather than only after, and a zero-channel capture refused at
  `begin` rather than spun on - the one input that would make the block loop drain
  nothing for ever.
- `core/tests/capture_writes_audio.rs` is the cross-crate proof, and the only place it
  can live: the real ring on one side, the real writer on the other, joined by
  `PcmSource`. Every sample is checked against `Simulated::expected_sample` recomputed
  from the frame index *stored in the block*, so a block written at the wrong offset, on
  the wrong channel, or after a gap fails rather than passing on its own internal
  consistency. It also covers R9 with the device vanishing mid-capture, a writer dropped
  without `finish()` leaving a project that is valid and visibly unfinished, and the
  stored summaries matching the audio actually in each block.
- `the_writer_keeps_up_with_a_192k_device_in_real_time` is the soak in miniature, short
  enough for CI: three seconds at 192 kHz in a debug build, zero loss, every commit
  inside the block budget.

**R9 can now be closed outright.** WP-04 closed it for the capture path but left it open
because "nothing is writing blocks while the device vanishes". Something is now, and the
test asserts that everything delivered before the cut is in the file, byte-exact, with
the session marked interrupted and the project still valid.

**What is not verified.** The soak is one platform and one filesystem: x86_64 on ext4.
The Pi 5 - on SD *and* on NVMe, which S2 expected to differ - and Windows are both
unrun, and S2's other open questions stay open: disk-full behavior, induced fsync
stalls, `VACUUM` and compaction, copying a live project, a page-size sweep, and WAL2.
The writer has also never been driven by a real converter for 90 minutes; the long run
is simulated, deliberately, because only a generated source can be checked byte for
byte afterwards.

**One unexplained test failure, recorded rather than forgotten.**
`a_device_that_vanishes_leaves_everything_it_did_deliver` failed once, on 2026-09-25,
during a full-workspace run, and its message was not captured. It has not recurred in
roughly a hundred subsequent executions, including sixteen full-workspace passes and
four with every core saturated. A separate and genuine race in a *different* test in
the same file was found and fixed in the same session - a delivered-frame count read
one line before the feeder thread was joined, so a callback landing in between made
the writer legitimately report more frames than the snapshot saw - but that cannot
explain this one, and no mechanism has been found that does. The assertion now prints
the outcome and the device counters, so a recurrence will be diagnosable. Until then
R9's automated proof should be read as strong but not yet unblemished.

## Phase 1 - WP-06, recovery

**Built 2026-09-25. Exit criterion met, and with it milestone M1.**

§15 asks that the next launch detect unfinished sessions and *offer* recovery.
`crates/project/src/recovery.rs` is that, plus the `vcw recover` verb that drives it.

### What a crash actually leaves behind

The audio survives and the bookkeeping does not. Every block was committed inside its
own transaction with `synchronous=FULL`, so it is on the disk or it never existed;
there is no half-written block. What is missing is the one write that was always going
to come last: the `captures` row's `finished_at`, and with it the final frame count and
state. So a crashed project is not damaged. It is *unfinished*, and the whole job is to
finish it the way the writer would have, using only what the writer already committed.

### The three decisions that shape the module

**Detection is `finished_at IS NULL`, not the state column.** The state says
`recording` because that is what it said while recording, and a crash cannot change it.
`finished_at` is different: it is the absence of a write, which is the one thing a
crash cannot forge. `CaptureState::is_unfinished` still exists but its documentation
now says it is advisory - a hint for a UI, never the thing recovery branches on.

**The blocks outrank the row.** `walk()` is deliberately a per-channel traversal and
not `SELECT SUM(frame_count)`, because a sum would report that a capture with a hole in
the middle has all its frames. It reads the blocks ordered by channel and sequence,
takes the longest *contiguous prefix* of each channel, and the usable length is the
shortest of those prefixes across all declared channels. Anything after a discontinuity
is stranded, not counted.

**`finished_at` is the last block's `committed_at`, never `now()`.** A capture that
died at 14:02 and is recovered at 09:15 the next morning did not run for nineteen
hours. Taking the timestamp from committed data is both honest and reproducible: run
recovery twice and it gives the same answer.

The state becomes a new `CaptureState::Recovered`, kept distinct from `Interrupted`
because the two mean different things. Interrupted means the writer *observed* the
fault and recorded it - a device unplugged, a stream error - so the counters are real
evidence. Recovered means nothing observed anything and every figure was inferred
afterwards. Collapsing them would throw away exactly the distinction an operator needs.
Neither state needed a schema migration: `captures.state` is TEXT, and
`captures.recovered_at` was considered and rejected as a column that would have to be
migrated to store something already derivable.

### D4 enforced rather than documented

Recovery will not silently discard audio. If the walk finds blocks stranded past the
recoverable end, `recover()` refuses with `Error::StrandedBlocks` and the CLI exits
non-zero. `--repair` is how an operator says the loss is accepted, and only then are
the rows removed - from `capture_blocks` and `sampleblocks` both, in foreign-key order,
because deleting one and not the other leaves an orphan that `validate` will rightly
complain about.

### The sidecars, and a finding about them

`Sidecars::inspect` stats the `-wal` and `-shm` **before** anything opens the project,
because opening it is what makes the evidence disappear.

That mattered more than expected. The first version of this was tested in-process, by
dropping a `Project` and reopening it - and the log was never there. Dropping a
`Project` runs `sqlite3_close`, and SQLite checkpoints and *deletes* the sidecars when
the last connection to a file goes. **So an in-process drop reproduces a crash's
database state and not its filesystem state.** Only a process that is really killed
leaves a hot log, which is why WP-06's exit criterion had to be out-of-process. A
second attempt, holding a keep-alive connection open, also failed: a SQLite connection
that has never *read* does not attach to the `-wal`/`-shm` at all, so it is not a
reference that keeps them alive. Both facts are now written into the tests that found
them.

The consequence for the operator is worth stating plainly, because it is the one thing
about `vcw recover` that could surprise someone: **a dry run writes nothing to the
database, but it is not side-effect-free on the filesystem.** Opening the project
replays the log and closing it folds the log into the main file and removes the
sidecars. That is the right behavior - the log is committed data and folding it in is
how it stops being at risk - but it means a dry run is a report, not a snapshot.
Preserving the crashed state means copying the file and both sidecars together, before
running anything. The test `recovery_reports_before_it_writes` asserts this so nobody
starts believing otherwise.

### Two gaps closed on the way

**`validate` could not see the thing recovery relies on.** Recovery's whole method is
to trust the blocks over the row, and nothing checked that the blocks agreed with the
row or with each other. `check_coverage` adds three codes - `missing-channel`,
`ragged-channels`, `frame-count-mismatch` - taking `validate` to 20. The middle one is
the interesting case: a capture where one channel holds a block more than another would
play as a widening time offset between left and right, and nothing else in the file
would have noticed. The state check also stopped hardcoding its vocabulary and now
calls `CaptureState::parse`, so the list cannot drift from the type.

**The diagnostics were lying about killed captures.** The writer persisted its counters
only at `finish()`, which a killed capture never reaches, so the row held four zeros -
which is the spelling of a *flawless* capture. They are now written on a timer
(`Config::diagnostics_millis`, default 2000) with `updated_at` kept meaningful, so
recovery can report that the counters are, say, two seconds stale rather than quietly
present them as final. `stale-counters` is one of the seven note codes an assessment
can carry.

### The exit criterion

`crates/cli/tests/kill_and_recover.rs` spawns a real `vcw soak` child, sleeps a
pseudorandom interval, and `SIGKILL`s it. Then, before anything opens the file, it
asserts a hot log is present - the state the in-process tests provably cannot create.
Then it runs `vcw recover --apply --verify` as a child process, as an operator would.

The audit afterwards does not trust `captures.frames`, or `sequence`, or the order rows
come back in. Every sample is recomputed from the frame index **stored in that block**
and compared against the generator, which makes the claim not "a plausible frame count"
but "exactly the audio the device delivered, at the offsets it delivered it, and not one
invented sample". It then asserts contiguity from frame 0, equal length on every
channel, `state = recovered`, a non-null `finished_at`, an empty `survey`, and a clean
`validate` with checksums recomputed.

**94 random kills across this session, every one recovered and audited, no failures.**

A real `vcw recover` on a capture killed 3.4 seconds in:

```
/data2/vcw_soak/rec/demo.vcw
  log         4.44 MiB left behind: the last process to hold this file did not close it
  capture 1   156000 frames on every channel, 3.250 s, 26 block(s)
              48000 Hz, 2 ch, Int32, started 1790356014
              0 overrun(s), 0 underrun(s), 0 dropped frame(s), 0 stream error(s), last written 1 s before the end
  verdict     1 capture(s) recoverable; nothing written to the project. Re-run with --apply
```

3.4 seconds of process life, 3.25 seconds recovered: one uncommitted block.

### The loss is smaller than the model allowed for

Across all 94 kills, every recovered length came back an **exact multiple of the 250 ms
block**, and the shortfall never reached one whole block - with a 1000 ms ring in play
the entire time. So the ring contributes nothing to crash loss: a writer that keeps up
drains it before the crash matters. That confirms the correction S1 made to S2's floor,
**crash loss = commit granularity + driver buffer, and ring size is irrelevant**, and
the test now asserts the tight bound rather than the safe one, because a regression
that let the ring leak into the loss would sail through the loose one.

#### What is verified, and what is not

Verified: detection, reconstruction, the stranded-block refusal, the sidecar lifecycle,
the timestamp provenance, the periodic counters, the three new validate codes, and the
byte-for-byte audit after 94 out-of-process kills. 234 tests, full gate green.

**Not verified, and it is not a portability gap.** `SIGKILL` ends a process; it does
not cut power. The page cache survives, so this exercises SQLite's crash recovery and
not the storage stack's. `synchronous=FULL` fsyncs every commit before it returns,
which *should* mean the two are the same, but "should" is the honest word and nothing
here demonstrates it. Closing the gap needs real power cuts on an expendable rig or a
fault-injecting filesystem; both are already on S2's open list.

Also unverified: every platform except this one. The kill suite is the cheapest of the
outstanding portability runs, needing neither a sound card nor an operator - one
`cargo test -p vcw-cli -- --ignored` per rig.

## Phase 1 - WP-07, the engine

**Built 2026-09-25. Exit criterion met, both halves.** `vcw-core` was a directory of
module stubs; it is now the layer that composes `vcw-audio` and `vcw-project` into a
transport, and `vcw session` is the operator surface that drives it. About 2,800 lines
across four modules, plus 730 lines of integration test in two places.

### §11 as a type, not a check

The state machine is a **typestate**. Five concrete phase types - `Idle`, `Armed<D>`,
`Recording<D>`, `Paused<D>`, `Stopped<D>` - and each transition *consumes* the phase it
leaves and returns the one it enters. There is no `Phase::Recording` variant to be in
while the deck says otherwise, and no runtime guard to forget: `Idle` has no `stop`
method for anyone to call, `Stopped` has no `record`, and a `Paused` that has been
resumed no longer exists to be resumed a second time.

Two additions to §11's diagram, both documented as additions rather than slipped in.
`Armed -> Idle` exists because an operator who opens a device to set a level has to be
able to change their mind, and `Stopped -> Idle` exists because §11 says stopping does
not close the project, which only means anything if there is a way back to record the
second side.

What the phases drive is a `Deck` trait rather than a `Recorder` directly, which is what
lets §11 be exercised exhaustively with no device, no disk and no project. `Rehearsal`
is the test deck and it is *public*: the compile proofs need a concrete `Deck` that does
not need a sound card, and a UI being developed with nothing plugged in needs the same
thing.

`Refused<S, E>` is the part worth pointing at. A transition a deck declines hands the
**phase back** - `Err(Refused { from: Recording, error })` - so a failed pause does not
lose a capture. That is §36's "task failure shall be isolated wherever possible"
expressed in a signature rather than in a comment.

### The bus is §35's two halves and nothing else

`Command` in, `Event` out. A `Command` carries a *description* of what to open, never a
device: it crosses a process boundary at WP-15, and a `cpal` stream handle is `!Send`
and could not cross a thread boundary let alone that one. `Bus::publish` fans out to
any number of subscribers, prunes the ones that have gone, and **cannot fail the
engine** - a poisoned lock returns zero rather than propagating, which is the one place
that trade-off is made deliberately.

Both enums are `#[non_exhaustive]`, so `play`, `seek` and `export` are additions at
WP-10 onwards rather than breaking changes. Inside `vcw-core` the attribute does
nothing, which is the useful half: the engine's `match` over `Command` is exhaustive on
purpose, so adding a variant fails to compile until someone decides what the transport
does with it.

### Armed is a writer that is running and paused

§11's `Armed` could have been "device open, writer not started". It is instead "writer
started, and paused", and that one choice pays for three things at once: §50's *set the
level before you drop the needle* and §11's PAUSE become the same mechanism; the ring is
always drained by the thread built to drain it, so overrun counters stay honest while
nothing is being committed; and §15's early session row falls out for free, because the
capture exists in the project from the moment the device opens.

It needed a small addition to WP-05's writer: `Config::start_paused`, a `pause`/`resume`
pair on the handle, and a writer loop that reads the ring and **drops** what it reads
while paused. Pausing flushes the part-filled block on the way in, so the audio captured
before the pause is committed rather than held.

### One thread, and it is not a matter of taste

The engine is a dedicated OS thread, and the transport is a **local variable** moved
through its loop. No `Arc<Mutex<Machine>>`, and therefore no sixth "in transition" phase:
between any two statements the transport is exactly one of §11's five. The typestate only
works because a single thread owns it - a shared, locked transport would have to hand out
`&mut`, and the consuming transitions are precisely what make an illegal move
unrepresentable.

That the thread is *necessary* rather than merely tidy comes from CPAL: `Capture` is
`!Send`, so the thread that opens a device must be the thread that keeps it and therefore
the thread that takes every later command about it. This is D8, locked as
[ADR-0005](adr/0005-concurrency-model.md). Two clauses of the plan's original D8 wording
changed on contact with the work: the above, and **elevated thread priority is not
implemented** - WP-05's 192 kHz soak showed no overruns at ordinary priority, so it stays
available for a platform that needs it rather than applied speculatively.

### Three things the build got wrong first

**The finished frame count.** `Stopped` took its position from the deck *before* the
stop, and finalizing flushes the part-filled block, so the transport reported a length
up to one block shorter than the row in the project. Fixed by giving `Deck` an
associated `frames(&Report)` function: only the deck knows what its own report means,
and by the time there is a report there is no deck to ask.

**When a capture is finished.** `capture-finished` was published when the transport was
*reset*, because that is where the report is yielded. An operator who stops a side and
walks away would never have been told what was recorded. It is now published on the stop,
and a test pins it to exactly one occurrence - the report lives in two places and
publishing it twice would have a UI catalog the side twice.

**The terminator.** `closed` was the last statement in the thread function, which means
it was not sent if the thread panicked, and a consumer blocked on `Events::next` would
have waited for ever. It is now published from a **drop guard**, so it survives a panic;
`Bus::publish` was already infallible, which is what makes that safe to do while
unwinding.

### The exit criterion, first half: the compile proofs

Six `compile_fail` doctests, one per illegal move, each **paired with the legal twin that
must still compile**. The pairing is not decoration. Measured this session: stable
rustdoc **ignores the error code** in a ```compile_fail,E0599``` fence - a doctest
annotated `E0599` passed while the code was actually failing with `E0308`. So
`compile_fail` proves only "this did not compile", which a typo satisfies. The proof was
then verified live: one illegal snippet was temporarily made legal, and the doctest
failed as it should. It is a live proof, not a decorative one.

### The exit criterion, second half: a full session from the CLI

```
$ vcw session side-a.vcw --script "arm,record,sleep 1,pause,sleep 0.3,poll,resume,sleep 0.6,stop,poll,reset,quit"
[  0.043] armed               armed on 48000 Hz, 2 ch, S32, shared into side-a.vcw
[  0.043] phase-change        idle -> armed
[  0.043] phase-change        armed -> recording
[  0.343] recording-position  12000 frames, 0.250 s
[  0.844] recording-position  36000 frames, 0.750 s
[  1.000] phase-change        recording -> paused
[  1.302] status              paused, 48000 frames
[  1.302] phase-change        paused -> recording
[  1.602] recording-position  60000 frames, 1.250 s
[  1.941] phase-change        recording -> stopped
[  1.941] capture-finished    capture 1 finalized: 76800 frames, 0 overrun(s), 0 underrun(s), 0 dropped, 0 error(s), bit-perfect no
[  1.941] status              stopped, 76800 frames
[  1.941] phase-change        stopped -> idle
[  1.941] closed              closed
```

One verb per line from stdin, or a whole session on one line with `--script`. Two verbs
are the driver's rather than the core's: `sleep <seconds>`, which is what makes a script
a session rather than a list, and `#` for a comment. `--json` emits one object per event
per line. There is deliberately **no prompt**: events arrive on their own thread whenever
the engine has something to say, and a prompt would be scribbled over by the next
position report, so the session echoes each command into the transcript instead and the
whole run reads back in order afterwards.

`crates/cli/tests/session_from_cli.rs` runs six of these through the **shipped binary**,
then re-opens the project and checks that the audio matches what the transcript claimed,
with every checksum recomputed. Including: a script that forgets to `stop` (the shutdown
finalizes the side rather than abandoning it), a verb with a typo in it (the run fails,
*after* the audio is safe), three commands issued out of turn (rejected, and the project
is left as it was found), and an arm that is thought better of (no capture row at all).

### What is verified, and what is not

Verified: the whole of §11's diagram walked in both directions; every step that is not in
the diagram illegal from every phase, checked exhaustively; a deck that refuses each of
its four operations, including a stop that fails; the clock discounting paused time; two
sides into one project; a device that cannot be opened leaving the transport idle; a
shutdown mid-capture finalizing rather than abandoning; two subscribers seeing an
identical stream; a panicking engine still closing the stream; and a full capture driven
through the binary with no frontend compiled. 277 tests, full gate green.

**Not verified: any platform but this one.** Every claim here is Linux x86_64. The
transport itself is platform-independent, but the simulated source is what most of the
tests drive, so what has *not* been exercised anywhere is the engine holding a real
`!Send` stream on Windows or macOS - which is precisely the case that motivated the
thread. `vcw session --device <id>` is the one-line way to check it on a rig with a
converter attached.

**Not attempted: re-entering the transport from a recovered project.** A project opened
with an unfinished capture in it is a state `vcw recover` reports and the transport knows
nothing about. Correct for now, since §15 asks recovery to *close* a capture rather than
resume one, but WP-16's "there is unfinished audio here" banner will have to decide what
the transport shows while it is up.

## Phase 1 - WP-08, the meters

Built 2026-09-25. `vcw-signal::meter` measures the levels; `vcw_audio::buffers::Tee`
carries the audio to it; `vcw-core::metering` is the worker between them. Exit criterion
met: verified against known-level test signals, and then verified again against a live
engine.

### Full scale is not 1.0, and that is the whole module

The finding that shaped everything else. In two's complement the largest positive `i16`
code is 32767, which decodes to 32767/32768 = **0.99997**, while the most negative is
-32768, which decodes to exactly **-1.0**. Full scale is asymmetric, and it is asymmetric
differently in each storage format.

A clip detector written the obvious way - `if sample.abs() >= 1.0` - therefore never
fires on integer input at all. It would be silent through an entire side pinned against
the top of the converter, which is exactly the fault a clip light exists to show. So
`full_scale(format)` returns a *pair*, and each sample is tested against the ceiling and
the floor separately.

One limitation is measured and documented rather than papered over: at 32 bits, `f32`'s
24-bit mantissa rounds the top few hundred `i32` codes to exactly 1.0, so clipping there
is detected a few codes early. That is a rounding error of about -0.00001 dB and it errs
towards reporting a clip that was within a hair of being one.

### The three measurements, and what each trades away

**Peak is since the last read.** Taking a snapshot resets it, so no transient can pass
between two polls unseen. The cost is that the number depends slightly on how often the
UI looks. The alternative - a peak that decays on its own clock - reads the same at every
poll rate and loses transients to do it, which is the wrong way round for a meter whose
job is to catch the one loud moment in a side.

**RMS is a true sliding window**, 16 buckets over 300 ms, advanced by sample count and
not by the reader. A per-snapshot mean would have been simpler and would have made RMS a
measurement of the UI's frame rate: 30 Hz and 60 Hz would read differently on identical
audio. `rms_does_not_depend_on_how_often_the_ui_looks` pins that, and
`the_rms_window_forgets_what_has_left_it` pins the other half - a loud passage that has
left the window is gone from the figure.

**The hold needle starts falling from the moment of the peak,** not from the moment the
signal stops, and falls at a rate in dB/s. This cost a test a correction: the first
expectation was out by exactly one bucket-step of 0.125 dB, because it had assumed the
hold clock started when the tone ended.

The clip latch stays lit until it is cleared, with a configurable n-consecutive-samples
rule for anyone who wants more evidence than a single sample.

### The fan-out sits after the ring, and the taps are lossy

§10 draws one distribution point feeding the writer, the meter, the waveform and the
detector. `Tee<S: PcmSource>` is that point. It wraps whatever the writer was going to
read from, copies each read into every tap, and is inserted at exactly one place -
`Recorder::open`, where the reader is handed to `persistence::spawn_on`.

That one place was chosen over the tempting one. A tap inside the CPAL callback would
have been closer to the source, but the simulated generator has no callback and no ring
at all, so a device-only fan-out would have left the UI-with-nothing-plugged-in case
unmetered - and being able to develop the whole application against it is §4.5. The
callback also keeps its guarantee untouched: three atomics and one memcpy, still proven
allocation-free by WP-04's counting-allocator test.

**Taps drop rather than block.** A meter worker that stalls loses audio it was only going
to average; a writer that stalls loses the record. The trade is stated in both
directions in the module doc: a stalled writer freezes the meter, and that is the
direction §10 requires. A tap counts the bytes it could not keep, so falling behind is
visible rather than silent.

### The meters run while armed, because §50 says so

"Set Level" comes before "Drop Needle". The meters are therefore live in `Armed`, which
cost nothing to arrange: WP-07 implemented `Armed` as a writer that is *running but
paused*, so the ring is already being drained and everything drained already goes past
the tap. The meter measures what the device is producing; the transport phase is not its
business.

`meter-update` publishes at 50 Hz, the middle of §17's 30-60. One refinement came out of
watching a real transcript: a tick that finds no new audio publishes **nothing**. Reading
a snapshot resets the peak, so a tick that woke a millisecond early was reporting silence
the stream never contained, and the needle flicked to the floor roughly once a second.
Genuine silence still reports correctly, because a quiet device sends zeros and zeros are
frames.

### What is verified

Thirteen known-level tests in `crates/signal/tests/known_levels.rs`, all against signals
whose level is known before the meter runs rather than recorded from it:

- sines at -0.5, -6 and -20 dBFS, in all five storage formats, read back to within
  0.02 dB, with RMS exactly 3.0103 dB below peak as a sine must be
- a constant reading the same peak and RMS; silence reading the floor in every format
- channels metered independently; RMS independent of poll rate; the window forgetting
  what has left it
- one sample at full scale latching, and staying latched; the top integer code clipping
  although it is not 1.0; the n-consecutive rule
- the hold needle falling at the rate it was given
- the five formats agreeing with each other on the same signal

Then the other half, which known levels cannot prove: that what reaches the meter *is*
the capture stream. `crates/core/tests/metering_live.rs` drives the real engine and
asserts the reported RMS is **-4.771 dBFS**. That figure is not a recorded observation -
the deterministic source is a hash, so its output is uniform over the code range, and the
RMS of a uniform distribution on [-1, 1) is 1/sqrt(3). A fan-out that dropped, duplicated
or reordered a byte would move it. The same file pins the meters running before `RECORD`,
no `meter-update` arriving after `capture-finished`, and a capture with the fan-out
attached still reporting zero dropped frames and zero overruns.

299 tests, full gate green.

### Cost

192 kHz stereo, the worst case the product supports: 10 s of audio metered in 0.377 s in
a debug build and **0.033 s in release**, about a third of one percent of a core. The
meter is not a thing to budget for.

### What is not verified

Linux x86_64 only, like everything above it. And the fan-out has been exercised against
the simulated source and the ALSA device on this machine, not against a converter running
for an hour - the lossy-tap behavior under sustained real load is the WP-05-style soak
that has not been run with meters attached.

## Phase 1 - WP-09, the waveform pyramid

Built 2026-09-25. `vcw-signal::waveform` renders; `vcw-project::waveform` reads;
`vcw-types::summary` holds the triplet both of them agree on. Exit criterion met, and
measured on a real 26-minute vinyl side rather than on a generated signal.

### The split, and why the query lives in the project crate

ADR-0003's rules that bite here are two: analysis must never reach a device, and only
one crate may open the project database. So the renderer knows nothing about SQLite - it
takes summaries and samples and returns columns - and every statement lives in
`vcw-project`, which is therefore allowed to depend on `vcw-signal`. `vcw-signal` depends
on `vcw-types` alone, so nothing circular is possible.

The triplet moved out of `vcw-project::persistence` into `vcw-types::summary` on the way,
because the writer computes it and the reader folds it and one definition is the only way
those two stay in agreement.

### RMS composes exactly, and Audacity's does not

`Summary::merge` weights by **true sample count**:
`sqrt((n1*r1^2 + n2*r2^2)/(n1+n2))`. That is exact, which is what makes the pyramid
honest: a column drawn from stored triplets is the same number the samples themselves
would have produced, so zooming out changes the resolution and not the answer.

Audacity weights its 64k level by block *capacity* instead of by the count actually in
the block, which makes its RMS slightly high on any short final block. Measured against
`/data2/vinyl_rips/simples_test.aup3` and pinned in
`weighting_by_capacity_instead_of_count_is_what_makes_audacity_high`, so the difference is
recorded rather than inherited by accident when import lands.

### `Summary64k` is dead weight for anything VCW records

The ladder is 1 frame, 256 frames, the block, and 65,536 frames - and at D3's 250 ms
block the last rung is *coarser* than the one below it: 65,536 frames against 12,000 at
48 kHz and 48,000 at 192 kHz. It also needs a blob parsed to reach the same rows the
block level already has as three scalar columns.

So it is never chosen for a capture VCW wrote. It is still *written*, for AUP4
compatibility (§49), and still readable, for imported Audacity blocks, which is the only
place it can ever be the right rung.
`the_64k_level_is_never_read_from_a_capture_we_wrote` is the test that keeps that true.

### The finding: a 192 KB blob makes three floats expensive

This is the part that was not predicted and that decided the schema.

A `sampleblocks` row at 24/192 carries 192 KB of audio, so with a 64 KiB page size it
occupies pages of its own and nothing else shares them. Reading `summin`, `summax` and
`sumrms` - twelve bytes - still costs a page fault per block. A 26-minute side is 12,528
blocks, so a full zoom-out is **784 MiB of page reads to obtain 150 KB of triplets**.
Cold on ext4 that measured **3.77 s**, against a requirement of sub-second, and no amount
of care in the renderer could have touched it.

Two covering indexes fix it by putting the coarse rungs somewhere the audio is not:

| Index | Columns | Size on a 2.3 GiB side |
|---|---|---|
| `sampleblocks_levels` | `blockid, summin, summax, sumrms` | 576 KiB, 0.02 % |
| `sampleblocks_summary256` | the same, plus `summary256` | 28 MiB, 1.2 % |

The query names them with `INDEXED BY`, which is deliberate in two ways. SQLite left to
itself prefers the integer primary key and produces the slow plan; and `INDEXED BY` is an
assertion rather than a hint, so if an index ever goes missing the query fails loudly
instead of quietly reverting to three seconds in front of a user.

The second index has to repeat the whole-block triplet as well - twelve bytes beside two
kilobytes - because the reader falls back to it for a block with no summary blob. Written
without those three columns it is a *lookup* index rather than a covering one, SQLite
fetches the row after all, and the entire 28 MiB buys nothing. That was measured, not
reasoned: the first version of the index made no difference at all, and
`EXPLAIN QUERY PLAN` said `SEARCH sb USING INDEX` where it now says `USING COVERING
INDEX`.

`the_coarse_levels_never_touch_a_row_that_holds_audio` locks it down **structurally**,
by asserting on the plan, not by timing anything. The difference it guards is two
hundred fold, and a timing test for it would still have been flaky.

There is no index for `summary64k`. Nothing VCW writes reads that rung, and the blocks
that do need it - imported ones - have no `capture_blocks` row and never reach this
query. If import makes it hot, that is the time to measure it.

### The measurement, on real music

A 26-minute 192 kHz stereo 32-bit side from `/data2/source_rips` was pushed through the
product writer onto `/data2` (ext4, SATA SSD): **300,627,479 frames, 12,528 blocks,
2.33 GiB**. Every read below had the page cache evicted first with
`posix_fadvise(DONTNEED)`, so these are cold numbers, and each draws **both** channels.

| Span | Width | Rung | Cold |
|---|---|---|---|
| whole side | 160 px | block | 21.8 ms |
| whole side | 1920 px | block | 17.6 ms |
| whole side | 4000 px | block | 17.0 ms |
| whole side | 8000 px | summary256 | 303.5 ms |
| 8 min | 1920 px | summary256 | 74.0 ms |
| 100 s | 1920 px | summary256 | 19.9 ms |
| 10 s | 1920 px | summary256 | 5.7 ms |
| 1 s | 1920 px | samples | 18.7 ms |
| 50 ms | 1920 px | samples | 6.2 ms |

Before the indexes the first row was 3,767 ms, the eight-minute span 1,347 ms and the
8000 px draw 4,170 ms. Worst case anywhere in the sweep is now 304 ms, for a whole-side
draw at a width no display can ask for.

The picture is worth having as well as the timings. At 160 columns the side shows its
track gaps as narrow notches and its lead-out as a single tall spike, peak 0.9332 on the
left and 0.9096 on the right - which is what a vinyl side looks like and is not what
uniform noise looks like. The generated source the tests use draws a featureless block,
correctly, and would have hidden any error that depended on real dynamics.

### Regeneration from PCM, on the same side

§19 asks for the pyramid to be regenerable from the audio, and a toy capture cannot
really test that. So the 26-minute side had every `summary256` and `summary64k` blob set
to `NULL` and `vcw waveform --rebuild` pointed at it: **12,528 of 12,528 blocks rebuilt
from the stored audio in 78.7 s**, reading all 2.3 GiB of PCM to do it.

The drawing that came out is **byte-identical** to the one the writer's own summaries
produced, at the block level and at the 256-frame level alike - 1,920 columns of a 100 s
span compared field by field. That is the claim worth making about a pyramid: it holds no
information the audio does not, so losing it costs time and nothing else.

### What §37 actually claims, restated

"Render cost independent of total length" is true and is easy to overclaim. Three
separate things, measured separately:

1. **The output is always exactly `pixels` columns.** Constant by construction, whatever
   the span.
2. **At any zoom coarse enough to reach the block level, the read is independent of
   sample count.** The same 10 s of audio costs 267 µs at 192 kHz and 292 µs at 48 kHz -
   four times the samples, no extra cost, no blobs opened - and a full zoom-out costs the
   same 17 to 22 ms at 160, 1920 and 4000 columns.
3. **At mid zoom the read is proportional to the samples in the span, never to the length
   of the recording the span came from.** The same 5 s drawn out of a 10 s capture and a
   200 s capture: 18.8 ms against 19.3 ms. Twenty times the recording for 2.5 % more
   time.

What is *not* claimed: that a wider span is free. It is not, and the table above shows
it climbing with span until the ladder steps up a rung, at which point it falls again.
A 500 s span at 1920 px costs 8 ms while a 100 s span costs 20 ms, because the wider one
reaches the block level and the narrower one does not.

### What it cost the capture path

Two more indexes to maintain on every commit. `blockid` is an autoincrement key, so both
inserts land at the end of their b-tree and neither rebalances. A five-minute real-time
24/192 soak: commit p50 6.2 ms, p95 17.3 ms, **p99 28.9 ms, max 42.5 ms** against the
250 ms block budget, peak WAL 5.06 MiB, zero loss and every one of 345,657,600 bytes
matched against what the source must have generated. File size grows 1.4 %.

**The 90-minute soak was re-run against the new schema on an idle machine and it
passes.** The two covering indexes cost the capture path nothing measurable.

| | With the indexes | Reference (pre-index) |
|---|---|---|
| real-time factor | 1.00001 | 1.00001 |
| commit p50 / p95 / p99 | 6.9 / 18.4 / **32.8** ms | - / - / **63.2** ms |
| commit max | **102.6 ms** | 102.3 ms |
| prepare p50 / max | 6.0 / 12.7 ms | - |
| peak WAL | 5.25 MiB | 4.57 MiB |
| overruns / underruns / dropped | 0 / 0 / 0 | 0 / 0 / 0 |
| `validate` | clean | clean |
| byte readback | all 6,220,938,240 | all of them |

The two numbers to look at are the maximum commit and the write-ahead log. **The worst
commit is 102.6 ms against the reference's 102.3 ms** - three tenths of a millisecond
apart over 21,601 commits, which is as close to "no effect" as a measurement of this
kind gets. The p99 is better rather than worse, at 32.8 ms against 63.2 ms, which is
machine state rather than the indexes helping; the honest reading of both together is
that maintaining two append-only b-trees on an autoincrement key disappears into the
noise of the commit the writer was already doing. **Peak WAL rose 15 %,** 4.57 MiB to
5.25 MiB, which is the indexes' own pages passing through the log and is the one cost
that is actually visible. It stays bounded and nowhere near a ceiling.

**The ring is not under-sized, and the earlier worry about it is closed.** A previous
attempt at this run recorded a 965 ms commit against a 1,000 ms ring and 28 overruns,
and raised the question of whether the ring is sized against the mean rather than the
tail. On an idle machine the worst commit in ninety minutes is 102.6 ms: ten times the
headroom, and the 965 ms stall was contention rather than anything the writer does.

That earlier attempt failed, **and the failure was mine** - I ran the full gate on the
same machine while a real-time soak was going, on the reasoning that a pass under
contention would be a stronger result. It is not a stronger test, it is a spoiled one,
and it cost ninety minutes and settled nothing. Kept at
`/data2/vcw-scratch/soak90-contended.log` because one thing in it is worth keeping: the
byte mismatch it reported was provably the dropped audio and not corruption. The
verifier found frame 195,888,000 of channel 0 holding `D9 90 24` where the source would
have produced `8D 94 7C`, and searching the generator over the next 300,000 frames finds
exactly one frame producing `D9 90 24` - frame 195,941,760, which is 53,760 later and
exactly the reported drop count. The writer stored what it was handed, in order,
unaltered; the ring lost 280 ms and everything after was shifted by it, with `validate`
clean saying the same from the checksum side. That is the failure mode a byte-for-byte
verifier exists to distinguish, working.

### The CLI

```sh
vcw waveform side-a.vcw --pixels 160 --rows 21
vcw waveform side-a.vcw --start 300 --end 400 --pixels 1920 --json
vcw waveform side-a.vcw --rebuild
```

It reports which rung it read and how long the read took, so every number above is
checkable on any machine without a UI. `--rebuild` recomputes the pyramid from the stored
PCM before drawing, by default only where a summary is missing; it never writes `samples`
and never touches a block with no `capture_blocks` row, so an imported Audacity project
cannot be rewritten by a redraw.

### Tests

329 in the workspace, full gate green. 11 unit tests on the renderer, including that the
answer has exactly as many columns as pixels were asked for, that the level chosen is the
coarsest that still fills every pixel, that a run straddling a column boundary is split
by how much falls each side, and that a backwards span is empty rather than enormous.
Nine integration tests on the reader, including that every level gives the same answer
for the same span, that the pyramid can be thrown away and rebuilt identically, and that
an unknown capture is an error rather than an empty picture. Four on the CLI verb. Two on
the query plan.

One test needed a genuine correction rather than a fixed expectation: the span-independence
test was reading *different audio* from its two captures, because the helper it used went
silent at the halfway point of whichever capture it was filling. A second helper whose
value is a function of frame index alone fixed it, and the first is now documented as
unusable for span comparisons.

### Fixed on the way past

`recovery::tests::a_log_left_behind_is_visible_before_anything_opens_the_project` was
flaky at about one run in twenty-five, and had been since WP-06 - confirmed by looping it
40 times in a worktree at `75123ab`, before any of this work. It asserted that a
checkpoint folds *exactly* the bytes an inspection saw, but `Project::open` stamps
`last_written_at` on its way in and can add a frame of its own, depending on the
checkpoint SQLite attempts when the writer's connection closes. The claim worth making is
that the checkpoint found everything the inspection did, so it is now a floor. 40 runs
clean.

### What is not verified

Linux x86_64 only, like everything above it. The measurements are from a SATA SSD with a
64 KiB page size; the Pi 5's SD card is where the `summary256` rung is most likely to
hurt, and that is the run to take before anyone adds a fourth rung on instinct. Nothing
publishes a waveform event yet either - §19's progressive build exists on the *write*
side, where the writer summarizes every block as it commits, but the read side is polled
rather than pushed, which is a WP-16 question about what the view wants.

## Phase 1 - WP-10, playback

Built 2026-09-26. `vcw-audio::playback` owns the output stream, `vcw-project::pcm` reads
the PCM back, `vcw-core::playback` is the transport, and `vcw play` drives all of it from
a script. Exit criterion met on both halves: **a gapless seek**, proved byte-for-byte in
CI with no sound card and measured at **median 19.8 ms** on a real device, and **a
bit-perfect path reported honestly**, which on this machine reads `bit-perfect playback,
confirmed against the OS` and on a converting path says so instead.

**Milestone M2, *it plays back*, is met.** The whole chain runs headless and was run
end to end on hardware for the record:

```sh
vcw session m2.vcw --script "arm, record, sleep 20, stop, quit" --rate 48000 --format s32
vcw waveform m2.vcw --pixels 100 --rows 11
vcw play m2.vcw --capture 1 --device alsa:hw:CARD=PCH,DEV=1 \
    --script "play, sleep 2, seek 15, sleep 2, skip-back, sleep 2, stop"
```

Capture, progressive waveform, playback, seek. 960,480 frames recorded with no loss,
288,000 frames played with 0 gaps across a seek and a skip.

### The four targets of §21 are one span

§21 asks for playback of the complete capture, a selected region, an individual track and
a boundary audition. They differ only in which frames they cover, so `Scope` resolves all
four to a `Span` and everything below it plays a span and knows nothing else. A boundary
audition is [`BOUNDARY_CONTEXT_SECONDS`] of 3 s each side of a frame, clamped, which is
the only one of the four that needed a number invented; §21 does not give one.

The transport reduces the same way. Six verbs - `PLAY PAUSE STOP SEEK SKIP FORWARD
SKIP BACK` - and the last three are all `seek` with the arithmetic done first. `SKIP` was
[`SKIP_SECONDS`] of 10 s here, with a note that once WP-13 recorded boundaries the skips
would become "next boundary" and "previous boundary", which is what they are for. **WP-16
did that**, and 10 s is now only the fallback for a side nothing has been analyzed from.

### Epoch-tagged chunks, not a byte ring

Capture's ring works because the producer is the callback. Playback inverts that, and a
ring inverts badly: the producer would be the feeder, which cannot clear a ring it does
not consume, so a seek would leave up to a second of the old position queued and the
listener would hear it. The queue is therefore chunks, each tagged with the epoch it was
filled in and the frame it starts at. A seek bumps the epoch; the callback discards every
chunk that does not match, unplayed, and recycles it.

Two things fall out of that for free. The position is **exact rather than inferred** -
the callback knows the frame number of the chunk in its hand, so nothing subtracts a
buffer depth it cannot see - and `drained` is per-epoch, so running out of audio at the
end of a span is the end of it while running out mid-span is an underrun, and the two
are never confused.

The chunks themselves are allocated once and circulate through two SPSC queues, full one
way and spent the other, exactly as capture's buffers do. `tests/rt_safety.rs` covers
`Source::on_data` under a counting allocator on four paths - the ordinary one, a seek, a
starved feeder and a stalled feeder - because the seek path is the one that tempts an
implementation into clearing a collection, and it runs *on the audio thread* by design.

### No resampler, and a capture plays at its own rate or not at all

Locked as [ADR-0006](adr/0006-playback-rate-policy.md). The rate is not a preference. A device that cannot do 192 kHz cannot play a 192 kHz
capture, and the answer is `Error::RateUnavailable` naming the rates the device does
offer, not a silently resampled side. The format is a preference, and the order is the
*opposite* of capture's: capture takes the best the device offers because a better
capture is strictly better, and playback takes the format that matches what is on disk
because anything else is a conversion.

`convert::natural` is what "matches" means: the device format a stored format would
rather be played in. Int24Padded maps to S24 rather than S32, because the bytes are the
same three bytes and the narrower stream is the one that can still be called
bit-perfect. It is lossless for every stored format, which is what lets the render path
be byte-exact.

### Two findings, both measured, both invisible from the code

**The queue depth is not a tuning knob, it is a correctness constraint.** The first live
run played at half speed with 16 underruns, while every counter except `underruns` said
the device was healthy. ALSA's own default buffer on this machine is **350 ms**; the
queue was `QUEUE_CHUNKS` of 20 ms, which is **160 ms**. A queue shallower than one
callback underruns on *every* callback and cannot be rescued by a faster feeder. So
playback now asks for a buffer it chose - [`TARGET_BUFFER_MILLIS`], four chunks, 80 ms -
and derives the queue from it; a backend that will not be told gets a
[`FALLBACK_QUEUE_MILLIS`] second-deep queue instead of an argument. On this device the
request is honored: `buffer 3840 frames, fixed`.

**A gapless seek needs the feeder to be holding an empty chunk when the seek lands.**
With the buffer fixed and the queue sized, five seeks on a real device still cost exactly
five underruns and 34,560 frames of silence - one buffer per seek - while the render path
cost none. The render path tops the queue up synchronously before each callback; a real
feeder does not. When a seek lands, every chunk in the queue is stale *and* every empty
chunk is in the queue, so the feeder has nothing to fill and cannot take one back out of
an SPSC queue it is the producer of. The callback then discards the lot in one pass,
finds nothing behind them and plays a buffer of silence.

The fix is a reserve: `Feeder::hold_back` keeps one callback's worth of chunks out of
ordinary filling, and the feeder spends them the moment it sees the epoch change. The
new position is queued *behind* the audio the seek invalidated, so the callback walks
past the stale chunks and keeps reading in the same pass. The feeder's idle sleeps became
interruptible at the same time, because a 50 ms sleep is 50 ms of a 80 ms budget.

After both: **0 underruns** across five seeks, three consecutive runs reporting
identical counters.

### The seek join latency, measured

| | |
|---|---|
| min | 11.7 ms |
| median | 19.8 ms |
| max | 20.4 ms |

Five seeks per run, three runs, on `alsa:hw:CARD=PCH,DEV=1` at 48 kHz S32 with a
3,840-frame buffer. The median is one chunk, which is the granularity the design chose,
and the ceiling the test enforces is 500 ms.

The first version of that measurement reported 0.00 ms and was worthless, which is worth
recording because it is the shape of mistake a latency test invites. It polled
`Player::position` after `Player::seek`, and `seek` *stores* the frame it asked for -
so the poll was reading the write it had just made and nothing the device had done. The
playhead reading the target immediately is right for a UI and useless for a measurement,
so the callback now records `Cursor::delivered`, the epoch it last copied audio out of.
A seek has joined when `delivered` catches up with `epoch`, and that is a fact about the
device rather than about the caller.

### The render path is why gaplessness is testable without a device

`playback::render` drives the same `Pump`, the same epoch-tagged queue and the same
`Source::on_data`, synchronously, and writes the audio frames to a file. Silence is
reported in `health` and never written. So a gapless seek becomes a byte comparison that
runs in CI: play 0-2 s, seek to 4 s, and the output must equal `whole[..2 s] ++
whole[4 s..]` with nothing repeated and nothing missing.

Cues are reported rather than rounded away. A cue fires at the first period boundary at
or after the frame it names, so `Rendered::applied` carries `{ verb, after, landed }` and
the test computes its expectation from the join that actually happened. That turned a
flaky assertion into a documented granularity, and gave the CLI something true to print.

### Fidelity is three-way, like capture's verdict

`Fidelity` is Confirmed, Refuted or Unconfirmed, and "nothing rules it out" is never a
pass. One refutation is unique to this side: **the samples were converted for the
device.** A 24-bit side played on a 32-bit stream sounds identical and is not
bit-perfect, and it says so. A render is never verified against hardware, because there
is no hardware under it.

### The CLI

```sh
vcw play side.vcw --capture 1                       # the whole capture
vcw play side.vcw --start 65 --end 130               # a region
vcw play side.vcw --track 3                          # one track
vcw play side.vcw --boundary 65.4                    # 3 s either side of a boundary
vcw play side.vcw --render out.raw --start 0 --end 5  # no device needed
vcw play side.vcw --script "play, sleep 2, seek 15, skip-back, stop" --json
```

`--script` is a comma-separated list of the six verbs plus `sleep <seconds>`; it opens
paused, so a script that never says `play` plays nothing, deliberately. Three events go
on the bus - `auditioning`, `playback-position` and `playback-finished` - and §35 names
neither, so the names follow its kebab-case convention.

### Tests

411 in the workspace, full gate green, `cargo deny` clean. 15 on the transport, four
through the shipped binary including the byte-exact gapless seek, four on the real-time
contract of the playback callback, two on the chunk reserve, and two `#[ignore]`d
hardware tests that measured the numbers above.

### What is not verified

Linux x86_64 and ALSA only, like everything above it. The device half was run on this
machine's S/PDIF output, chosen because the simulated source is full-scale noise and the
digital output has nothing plugged into it; `VCW_TEST_OUTPUT` overrides it. WASAPI in
exclusive mode, CoreAudio and AAudio are all untried for output, and the buffer
negotiation is exactly where they are most likely to differ - the fallback path exists
for them and has never run.

Nothing has played a 192 kHz side yet, and nothing has played for an hour. The reserve
fixes the seek that lands between callbacks; a seek storm has not been tried.

## Phase 1 - WP-11, the detection port

Built 2026-09-26. VRipr's three detectors, ported into `vcw-signal`, published as §24
observations rather than tracks, resolved into decisions, and reachable from the command
line at both ends of §22: live while the record turns, and again over the committed
side. Exit criterion met on both halves. **Parity: 97.6% to 99.7% of VRipr's boundaries
reproduced over all 595 snippets of the labeled corpus, every agreement at the
identical frame**, with the residue traced to one documented cause. **Provenance and
confidence: every boundary carries both, plus the measurements behind them**, and
`vcw detect --evidence` prints the lot.

**Milestone M3, *it finds tracks*, is met.** On a real 26-minute side:

```sh
vcw detect side-a.vcw --min-sources 2
```

```
  analysis   15658 window(s), 3 detector(s), 12.839 s
  silence          6 boundary/ies at   -40.0 dB, floor -
  spectral-change  6 boundary/ies at   -40.0 dB, floor -
  hmm             266 boundary/ies at   -40.0 dB, floor -
  showing    6 of 270 boundary/ies, those 2 or more detectors reported
     1  start      0.000 s  conf 1.00  silence+spectral-change (2)
     2  end      282.800 s  conf 0.53  silence+spectral-change (2)
     3  start    283.800 s  conf 1.00  silence+spectral-change+hmm (3)
     4  end      686.000 s  conf 0.53  silence+spectral-change (2)
     5  start    687.200 s  conf 1.00  silence+spectral-change+hmm (3)
     6  end     1561.400 s  conf 0.59  silence+spectral-change (2)
```

Three tracks, 4:43, 6:42 and 14:34. The 266 the HMM found on its own are the subject of
one of the findings below.

### The split VRipr does not have

VRipr's detectors take a file path and decode it with Symphonia. VCW's take frames,
because §22 asks for live analysis while the record is still turning and there is no
file to open. So `features::Windows` is the only thing that touches audio: it turns
capture bytes into `Frame { rms, flatness }`, and `silence`, `spectral` and `hmm` see
nothing else. The same extractor serves both passes, which is what makes the live answer
and the refine answer comparable rather than merely similar.

`Windows` carries a frame that straddles two calls instead of dropping it. A meter can
drop three bytes and be wrong by nothing anyone can hear; an extractor cannot, because a
dropped orphan sample shifts the window alignment for the rest of the side and moves
every boundary after it. That was a real defect, found by a test that pushed the same
audio in ragged chunks and in one go and demanded the same frames
(`frames_do_not_depend_on_how_the_audio_arrives`).

### Positions are frames, and that is where VRipr and VCW part company

VRipr works in seconds as `f64`. VCW works in frames, and `Region::seconds` exists for
printing. The difference shows up in exactly one place and it is worth the paragraph:
`merge_gaps` bridges gaps *shorter than* a limit, and VRipr computes the gap from
`index as f64 * window_secs`, so a gap of exactly eight 100 ms windows comes out as
0.7999999999999993 and gets bridged. VCW computes 0.8 and leaves it.

That single boundary condition accounts for most of the corpus disagreement. Forcing it
the other way was tried: the level detector rises from 99.52% to 99.86% and the HMM
*falls* from 98.01% to 96.22%, because VRipr's own answer depends on how the error
happened to accumulate in each snippet. Neither comparison reproduces it; only
reproducing the accumulation would, and that means giving up the frame arithmetic that
makes the live pass and the refine pass agree. A gap of exactly `min_silence_secs` is a
boundary, which is what the setting says it is.

### The adaptive floor can only find the groove if there is enough groove

`adaptive_floor` is VRipr's interpolated 3rd-percentile estimate, and the percentile is
the whole story: the estimate lands in the inter-track groove only if the groove is
*more* than 3% of the side. A tightly cut side is under 1% groove, and then the third
percentile sits inside the quietest music. Two tests were written against the wrong
premise before this was measured - they asked for a floor near the groove on traces that
were exactly 3% groove, and got -30.2 dB and -23.3 dB instead. The limitation is now
documented on the function, and it is why `adaptive_margin_db` is as wide as 12 dB.

### Contrast is a median, because a padded boundary straddles the transition

Boundary confidence comes from the contrast between the music before a boundary and the
gap after it, over five windows each way. Taking the mean failed a 50 dB test case at
44.8 dB, and the reason is not noise: the padding puts a window or two of the *other*
side inside the look-back range, and a vinyl pop drags a mean up on its own. A max/min
would fix both and bias every score upward, which is the one direction a confidence must
not be wrong in. The median fixes both and biases nothing.

### Smoothing costs the flatness detector two or three windows of position

`spectral::smooth` is VRipr's ±3-window rolling mean, 700 ms at the default window, and
it moves a boundary by two or three windows - pinned at window 302 becoming 303 in
`smoothing_costs_the_boundary_a_few_windows_and_that_is_the_trade`. The same blur
corrupts the measurement of the thing being scored: reading flatness at the boundary
understates the gap-to-music separation by a factor of two, so the score stands back
`SKIP = SMOOTHING + 1` windows. And the order of the pipeline matters more than it looks:
a 200 ms tonal pop inside a 1.2 s flatness gap erases the gap through the rolling mean
entirely, so the transient filter runs *before* gap-fill, not after.

### The HMM posterior saturates, so it is a veto and never a score

This is the session's headline finding and it changed the design. Forward-backward
returns a posterior of **1.0 for every boundary Viterbi commits to** - across a 60 s
fade, and at only 3 dB of real contrast. It has to: the emissions are fitted to the
quietest 15% and the loudest 40% *of the data being classified*, and a Gaussian
log-likelihood is quadratic, so whatever the side contains, the two states end up
separated in the model that was built from them. A posterior used as a confidence would
report total certainty about a boundary that is not there.

So `confidence = min(level_confidence, posterior)`: the posterior can only ever veto.
And it has one exception, because a capture edge has no transition to measure - the first
boundary of a side that begins in music scored 0.0002, which is the correct posterior for
a state change that never happened and a useless confidence for a boundary that is
certainly real. Inside `EDGE_WINDOWS` of either end the veto is skipped, and
`hmm.posterior_applies` records which way it went, so the evidence says whether the veto
was in force rather than leaving a reader to infer it.

Three other HMM premises were disproved by probes before they became tests: with
indifferent emissions a chain drifts to *Music*, not to the biased start state, because
`min_sound > min_silence` makes music the stickier of the two; a featureless side comes
back as one Music region rather than none; and posteriors sum to 2.015 rather than 2.000
for two boundaries, because some paths cross more than twice.

### The HMM over-segments a real side, faithfully

266 boundaries on the side above, against six from each of the other two. It is not a
defect in the port - the parity harness puts the HMM at 98.01% of VRipr's own answers,
and VRipr's HMM does the same thing - it is what a self-fitted two-state model does to a
record with quiet passages in it: the quietest 15% of a real side *is* music, so the gap
state gets fitted to quiet music and then finds it everywhere.

This is the case the resolver exists for. Every one of those 264 unsupported boundaries
comes back at confidence 0.50 with `agreement() == 1`, and the six a second detector
seconded come back at 0.53 to 1.00. `vcw detect --min-sources 2` is the same filter at
the command line, and WP-13 should not promote a boundary no second detector saw.

### Agreement is counted, never multiplied

`resolve` clusters observations within half a second of each other, per edge, and takes
the **maximum** confidence in a cluster rather than a noisy-OR. Three detectors reading
one level series and each reporting 0.5 are not three independent witnesses; they are one
measurement counted three times, and a noisy-OR would turn it into 0.875. The position
goes in the safe direction - earliest start, latest end - so a padding error clips
silence rather than music. A boundary a person placed fixes the position of its cluster,
cannot be merged away, and two user boundaries close together stay two boundaries (§24).

### The live pass is the refine pass with less audio

Not an approximation of it. The live worker keeps the whole feature trace - 15,000
windows for a 25-minute side, about 200 KB - and re-runs the detector on every drain,
which costs well under a millisecond. So a provisional marker is the final answer
computed from the audio that has arrived so far, and
`the_live_pass_and_the_refine_pass_agree` proves the two produce identical regions and
boundaries to the frame when one is fed ragged 7,331-byte chunks and the other the lot.

A marker is published only once it cannot move, which is `min_silence + gap_fill +
pre + post` behind the analyzed position: **1.2 s at the defaults**. A marker is never
retracted, and an adaptive live pass settles nothing at all - the threshold depends on
the whole side - which `Live::settled` reports honestly by returning 0.

The live pass is levels only: no FFT on a tap of the capture stream. The refine pass is
where the spectral extraction happens, once, with all three detectors reading its
frames. That buys a property worth having: a disagreement between two detectors on the
same side cannot be a disagreement about what they were looking at.

### Parity, measured

`crates/signal/tests/vripr_parity.rs`, `#[ignore]`d because it reads 294 MB from outside
the repo. `/data2/vripr_training` is 595 snippets VRipr cut from its own track tables -
16 s of mono 16 kHz audio centered on a boundary, peak-normalized, with a JSON sidecar
naming the kind. The reference is **VRipr's own detectors run over the same snippets**,
computed out of tree at `/data2/vcw-scratch/parity` from a verbatim copy of
`/data2/vripr/src/audio/mod.rs` and checked in as
`crates/signal/tests/fixtures/vripr_answers.jsonl`, so the reference outlives the other
repository.

| Detector | Fixed threshold | Adaptive | Snippets identical | Offset |
|---|---|---|---|---|
| `silence` | 99.52% | 97.63% | 98.66% / 95.29% | 0.000 s |
| `spectral-change` | 99.71% | 98.71% | 97.98% / 96.13% | 0.000 s |
| `hmm` | 98.01% | 98.01% | 93.95% | 0.000 s |

Every agreement is at the identical frame; the test asserts that, not a tolerance. The
HMM is the same under both thresholds because it never reads one - it fits its own. The
gate fails below 97% of VRipr's boundaries or 90% of snippets matching exactly.

### Agreement with the *labels* is low, and VRipr's is lower

The same harness scores both against what the sidecars say, and the numbers are
uncomfortable until you see the second column:

| Detector | VCW | VRipr | `mid` snippets left alone |
|---|---|---|---|
| `silence` | 17.1% | 16.3% | 99.5% |
| `spectral-change` | 11.3% | 10.8% | 99.5% |
| `hmm` | 30.9% | 30.7% | 82.7% |
| resolved, 2+ detectors | 12.3% | - | 99.5% |

VCW is a hair ahead of VRipr on every row, which is the only thing this comparison can
establish. The corpus is dominated by ambient and drone records whose tracks segue with
no silence at all, and the labels are the track table - which for those records came
from a release listing or from a person, not from any detector. The `.onnx` file sitting
in the corpus directory is the rest of the story: VRipr was training a learned detector
on this material precisely because its classical ones could not do it. §22 lists the
three that are required, and this is what they are worth on the hard cases; §22's
"expected track count, release durations, fingerprints, side topology" are the rest of
the answer, and they arrive with WP-12 and WP-13. The resolver already takes a boundary
from a release listing without a line of new code
(`a_boundary_from_a_release_listing_needs_no_new_code_to_be_heard`).

### Cost

A 26-minute side at 192 kHz: **12.8 s** for the refine pass in release, 15,658 windows,
one FFT each. That is 120x faster than the side plays, and it is dominated by the
extraction rather than the detectors. The live pass costs a 250 ms drain of a 2 s lossy
tap and a re-scan of at most 15,000 frames, and the capture it hangs off reports the
same zero dropped frames and zero overruns it did with only the meter attached
(`detection_costs_the_capture_nothing`).

### The CLI

`vcw detect <project>` runs the refine pass and prints the boundaries with their
provenance, confidence, agreement and - with `--evidence` - every measurement behind
them. `--threshold-db`, `--adaptive`, `--min-silence` and `--min-sound` reach the
detectors; `--min-sources` filters by agreement; `--json` gives a UI the same thing.
`vcw session --json` now renders `track-detected` as it happens, which is how the live
half is visible with nothing else running.

Nothing here writes. A boundary becomes a track in WP-13, and the tracks `vcw detect`
prints are labeled implied for that reason.

### Tests

496 in the workspace pass and 4 are `#[ignore]`d, the full gate is green, `cargo deny`
is clean and so is the doc leg. 77 of them are in `vcw-signal`'s lib covering the five
new modules, four are on the engine's detection path, three go through the shipped
binary, and one is the parity harness, which is `#[ignore]`d and was run.

### What is not verified

The parity figure is parity, not accuracy. Nothing here has been checked against a
boundary anyone confirmed by ear; the labeled corpus is VRipr's own reading of its own
records, and on the hardest third of it both implementations are mostly wrong together.

The live pass has only ever been fed the simulated source through the engine - uniform
noise, which has exactly one boundary. The live/refine agreement test uses real
synthesised material but drives `Live` directly rather than through a capture, so what
has never happened is a live pass over a real record with real gaps in it, on a real
device. That wants a turntable and a side, and it is the first thing to do with WP-11
when there is one to hand.

Adaptive mode has no live story worth the name, as above. And the HMM's over-segmentation
is reproduced rather than solved: the resolver makes it harmless, but §22's guided
detection - expected track count, release durations, side topology - is what would
actually fix it, and that is WP-12 and WP-13 work.

## Phase 1 - WP-12, metadata

Built 2026-09-26. `vcw-metadata`: a provider trait with Discogs and MusicBrainz behind
it, §32's genre normalization ported from VRipr, artwork fetching, an on-disk cache,
per-service rate limits, timeouts and cancellation - and, the part that shaped everything
else, §40's promise that the application stays fully usable with networking disabled.
**Exit criterion met on both halves:** the tests are fixture-backed and offline, and the
offline *build* is a gate leg.

```sh
vcw metadata search --artist Autechre --album Amber --provider musicbrainz --limit 5
```

```
  musicbrainz  2 candidate(s)
     1  Autechre - Amber   1994  Warp  WARPLP25   GB  2LP  [bd5b1270-...]
     2  Autechre - Amber   2014  Warp  WARPLP25R  GB  2LP  [1a9f1c33-...]
```

### Three layers, and the order between them is the design

```text
  Provider  (discogs::Discogs, musicbrainz::MusicBrainz)
     |  builds URLs, parses bodies, knows one service's grammar
     v
  Client    cache -> rate limit -> retry -> timeout -> cancel
     |
     v
  Transport (Offline | Recorded | Agent)   <- the only thing that can do I/O
```

A `Provider` knows one service and nothing about time, retries or the network. A
`Client` is the request path and owns all of that. A `Transport` is the only thing in the
crate that can perform I/O, which is what makes the offline promise checkable rather
than merely intended.

**Cache before rate limit**, and it is worth saying why, because the other order looks
just as reasonable: reading something already downloaded does not involve the service, so
a cached answer must not spend a slot. Spend one and a warm cache is *slower* than a cold
one, which is the opposite of a cache.

### Offline is a property of the build, not a flag

`Offline` is the default transport and refusing is an ordinary answer with an ordinary
message - `networking is disabled, so MusicBrainz was not contacted` - not an exception
for a caller to handle specially. The HTTP agent lives behind a `net` feature, so:

```sh
cargo test -p vcw-metadata --no-default-features   # 148 lib tests, no HTTP code compiled
```

That is a gate leg now. A build that cannot reach the network is not a mode the
application can be put into by mistake; it is a build in which the code that would do it
does not exist.

Two smaller decisions follow from the same instinct. **The clock is a trait**, so no test
in the crate sleeps: `TestClock` advances virtual time and the limiter's spacing and the
retry backoff are asserted as numbers rather than waited out. And `Recorded` replays real
captured payloads, so the parsers are tested against what the services actually sent
rather than against what I think they send.

### Rate limits are promises, and cancellation is honest about its granularity

Discogs allows 60 requests a minute authenticated, MusicBrainz one a second sustained,
and both ask for a self-identifying User-Agent. `Limiter::reserve` claims a slot so
concurrent callers queue rather than collide, and a caller that gives up calls `release`
so the slot is not wasted. Retries are conservative - 429, 5xx, timeout and unreachable
only - with geometric backoff, obeying `Retry-After` unless it asks for longer than
`max_backoff`.

Cancellation is cooperative, because there is no runtime here to cancel into. The token
is checked before each attempt and every 50 ms of any wait, so waiting is interruptible
immediately; a request already on the wire is bounded by its timeout instead. That is the
true behavior and the docs say so rather than implying something tidier.

### §39: the token goes in a header, because the URL is the cache key

VRipr put `&token=` in the Discogs query string. VCW puts it in an `Authorization`
header, and the reason is not stylistic: the URL is the cache key, the log line and the
thing a person pastes into a bug report. `Token` has no `Serialize`, no `Display` and a
`Debug` that prints a character count, credentials come from the environment only, and
`no_url_anywhere_in_a_discogs_exchange_carries_the_token` asserts it as a property of the
traffic rather than of the code that generates it.

One behavior worth recording because both answers are defensible: an offline build with
no token configured reports the *networking*, not the missing credential. It is the one
of the two that would change the outcome, and a build that cannot reach the network has
no business reading a credential at all.

### The genre port is held to VRipr's output, not to my reading of its code

`assets/genre.dat` is VRipr's file byte for byte: 639 mapping rows, 632 distinct keys.
The algorithm was reimplemented, so the way to know it is right is to compare answers,
not code. `tests/fixtures/vripr_genres.jsonl` is 1,819 answers recorded from a *verbatim*
copy of VRipr's `genre.rs` running out of tree at `/data2/vcw-scratch/genreparity`, and
every one is reproduced. Nothing in this repository carries a second implementation.

**That comparison found a latent nondeterminism VRipr shipped.** 22 keys collide under
case folding, and 5 of those collisions have *differing* answers. VRipr resolved them
with `HashMap::iter().find()`, so which answer `HARDROCK` got depended on the hash seed
for that run. VCW resolves by first spelling in file order, deterministically, with a
32-iteration stability test - and those 5 folds are excluded from the parity fixture,
because VRipr's answer there is not a fact about anything.

A related detail that only shows up on real data: a repeated key means whatever the
*last* row says, which is how a file appends a correction, and a fold has to be built in
a second pass because it must resolve to whatever that spelling *finally* means.

### Two findings that only a live query could produce

This is the entire reason eight `#[ignore]`d live tests exist. A fixture captured from a
wrong query is a wrong answer that passes forever.

**MusicBrainz `format:` matches the exact medium format name.** `format:vinyl` looks
obviously correct and returns nothing. MusicBrainz holds four distinct values - `Vinyl`
(37,643 releases), `12" Vinyl` (408,724), `7" Vinyl` (159,636) and `10" Vinyl` (10,888) -
so the filter has to name all four:

```text
artist:"Autechre" AND release:"Amber" AND (format:"Vinyl" OR format:"12\" Vinyl" OR format:"7\" Vinyl" OR format:"10\" Vinyl")
```

**A release's `genres` can be empty while its release-group's are populated**, so genre
extraction falls back to the group. MusicBrainz tags are lowercase by convention where
Discogs' are not, so the MB path title-cases before the §32 lookup - provider-local
presentation, deliberately not pushed down into `Genres::normalize`, which stays
VRipr-compatible for the parity fixture. And MusicBrainz returns no artwork URLs at all:
`cover-art-archive.front == true` means one exists at
`coverartarchive.org/release/<mbid>/front`, which the client then fetches uncached.

### Positions are guesses about a label, so nothing there may fail

`vcw_types::Position::from_str` takes the unambiguous form, `A1`, and nothing else, which
is right for a project file. A provider tracklist is not a project file: it carries
whatever the person who cataloged the record typed off the label. So the grammars live
in `vcw-metadata::positions`, out of `vcw-types`, and handle the letter-run convention
(`AA` is A2, not side AA), separators people add, heading rows that are not tracks at
all, and a wholly numeric tracklist split A/B at the medium's halfway point with the odd
extra on the first side. An unreadable position yields nothing and the track keeps its
title; a tracklist with one odd row is still worth showing.

The two services also disagree about letters: Discogs restarts at A on every medium,
MusicBrainz runs them straight through. `side_for` takes a letter at face value when it
is already at or past the medium's first side and offsets it otherwise.

Discogs has a related quirk worth naming: it reports the disc count in `formats[0].qty`
and then prints one flat tracklist. When the tracklist names more sides than `qty`
claims, `media_of` grows the list - believe the tracklist, it came off the label.

### The CLI

```sh
vcw metadata credentials                   # what is configured, never what it is
vcw metadata genres "HH; ambient techno; Mn"
vcw metadata search --artist Autechre --album Amber [--provider musicbrainz] [--json]
vcw metadata fetch bd5b1270-7468-47f0-9c9a-928199f9e4ad
vcw metadata search --offline --artist Autechre   # and see what refusing looks like
```

`fetch` guesses the provider from the shape of the id, an MBID being recognizable. The
offline report prints the per-provider detail and the JSON document either way, and only
then fails - an error line on its own loses the thing the caller asked for.

### Tests

153 lib, 3 genre-parity, 11 offline, 9 doc (one of them a `compile_fail` proving `Token`
cannot be serialized), and 8 live tests ignored by default. Workspace total is now
**681 passing, 0 failing, 12 ignored**, gate-green including `fmt`, `clippy -D warnings`,
`cargo deny`, the rustdoc leg and the new offline-build leg.

The live ones were run once, all 8 passing in 6.90 s, and the CLI read *Amber* back
correctly: 2 records, sides A to D, 11 tracks, side totals 18:50 / 20:36 / 13:47 / 21:10.

### Fixed on the way past

A race in the capture writer, found by the workspace gate rather than by anything in this
work package: `a_device_that_vanishes_leaves_everything_it_did_deliver` failed once under
load, recording a `Finalised` state for a capture whose device had been unplugged. The
writer thread finished the moment the ring's writing end went away and read whatever end
state had been set by then - but a caller cannot read a device's *final* counters until
it has released the device, and releasing it is what takes the ring's writing end with
it. So the correct result was routinely set just after the writer had already decided.
The window is microseconds on an idle machine and the gate found it on a busy one. The
writer now commits what it holds when the producer disappears and then waits for the
stop it is guaranteed to get, so the recorded end state is always the one the owner set;
`crates/cli/src/capture.rs` and `soak.rs` both had the vulnerable order, and both are now
correct without changing. `a_result_set_after_the_source_went_quiet_is_still_the_one_recorded`
pins it, and fails on the old code.

Also on the way past: three broken rustdoc links in `vcw-metadata` and a redundant one in
`vcw-types`, all found by the doc leg and none visible to clippy.

### What is not verified

Discogs' two live tests have never run, because `VCW_DISCOGS_TOKEN` is not set on this
machine; the Discogs parser is exercised against recorded fixtures only. AcoustId has a
rate limit configured and no provider behind it - that is WP-26. Nothing has been fetched
through a proxy, no artwork has been downloaded from Cover Art Archive outside a fixture,
and the cache has never been pointed at a filesystem that ran out of space. And, as
everywhere else in Phase 1, this is Linux x86_64 only.

## Phase 1 - WP-13, the vinyl data model and editing

Built 2026-09-26. Schema v2 and the whole editing surface: a release with its artwork,
sides with the captures behind them, track boundaries with the case for each one, and
tracks as the spans between them. Every §31 verb - add, move, delete, split, merge,
lock, renumber, reassign - plus adoption, which is the bridge from WP-11's detections to
rows. **Exit criterion met on both halves, each asserted by its own test file:** edits
never touch a committed block, and a locked boundary survives a second analysis pass.

```sh
vcw tracks side-a.vcw adopt --side A --min-sources 2 --dry-run
vcw tracks side-a.vcw list
```

```
  270 decision(s), 6 accepted, 264 turned down, 0 already settled (dry run)

  release    1 disc(s) claimed, 2 side(s) present, numbering alpha

  side A (disc 1, first face)  capture 1
    A1        0.000 -   282.800 s  (282.800 s)  The Rainbow [confirmed]
    A2      283.800 -   686.000 s  (402.200 s)  (untitled)

  side B (disc 1, second face)  capture 1
    B1      687.200 -  1561.400 s  (874.200 s)  (untitled)
```

That is the real 2.33 GiB side, and the three tracks are the three tracks on the record.

### A track is its two boundaries

`tracks` carries `start_boundary` and `end_boundary` and **no frame columns at all**.
The extent comes from the join, which is why moving a boundary moves whichever tracks it
bounds with nothing to cascade: there is no second copy of the position to update, and
therefore no second copy to be wrong. `tracks_have_no_frames_of_their_own` reads the
table's column names and fails if any of them contains *frame*, because caching the
extent is the obvious optimization and it is also the bug.

It decides the shape of everything above it. `split` writes two boundaries rather than
one shared one - `UNIQUE (start_boundary)` and `UNIQUE (end_boundary)` would forbid
sharing, and §33's export padding differs at the end of one track and the start of the
next anyway - and `merge` has to delete the right-hand row *before* the left claims its
end boundary, or that same constraint fires mid-statement.

### There is no `discs` table, and there should not be

`Side::disc()` is `index / 2 + 1`. A disc row would store a fact the side already
implies, and two places to store one fact is how they come to disagree, so
`crates/project/src/disc.rs` is an arithmetic view over `sides`: `Disc { number, first,
second }`, assembled on demand. `releases.discs` is the operator's *claim* and the side
rows are the reality, which makes `disc::missing()` the gap between them - the answer to
*what still needs recording*, in playing order, rather than a validation failure. A
two-disc release with three sides recorded is a normal Tuesday, not a broken project.

### A side is created on purpose, never implied

`side::ensure` is explicit. A capture arriving does not conjure side A, because inventing
one for an unnamed recording would quietly relabel a *mislabeled* recording instead of
leaving the question open, and the operator is the only one who knows which face went
under the needle. `sides.capture_id` is pointedly **not** unique: both faces may share
one take, which is what a single unattended recording of a whole record looks like, and
it is the case that makes `track::move_to_side` meaningful at all. A move to a side
holding *different* audio is refused with `DifferentCapture`, since a track's boundaries
are frames into its own side's capture and would otherwise point at audio that is not
the track.

### §24: what a lock binds, and what it does not

`Provenance::is_locked()` is true only for `User`. `move_boundary` and `delete_boundary`
refuse a locked row with `Error::BoundaryLocked`, and `move_boundary_forced` is the
operator's override - it moves the row *and* claims it as `User`, because a person who
overrides a lock is the new author of that position.

**`merge` is not blocked by a lock, and that is a decision.** A lock binds *analysis*,
not the person who set it, so joining two tracks across a locked boundary leaves the
boundary in place as a marker of where the join was rather than refusing the edit. The
doc comment and a test name both claimed refusal until the CLI exercise showed otherwise;
the behavior is right and the words were wrong, so
`merging_across_a_locked_boundary_keeps_the_boundary` now says what happens.

One consequence had to be paid for elsewhere. A boundary left inside a merged track would
be picked up by the next adoption pass and paired with the following free end, quietly
re-splitting what the operator had just joined, so `adopt::pair` excludes boundaries
*inside* an existing track as well as the two that bound it.

### The upsert had to be taught about attribution

A boundary write is an upsert on `(side_id, at_frame, edge)`, and the first version of it
let a detector landing on a locked frame overwrite that row's `provenance` and
`confidence`. §24 reserves the position *and* the credit:

```sql
confidence = CASE WHEN locked THEN confidence ELSE excluded.confidence END,
provenance = CASE WHEN locked THEN provenance ELSE excluded.provenance END,
sources    = excluded.sources,
evidence   = excluded.evidence,
locked     = locked OR excluded.locked,
```

`sources` and `evidence` update either way, because a detector agreeing with the operator
is worth recording; what it may not do is turn their boundary into a silence one.

### Promotion is a policy, and 2 is the number WP-11 chose

`adopt::Policy` is the judgment WP-11 deliberately declined to make: `min_sources`
defaults to **2**, so a boundary only one detector saw is kept as a row and never becomes
a track. On the real side that is the difference between 270 candidate boundaries and 6,
because the HMM fires at every quiet bar - which is VRipr's behavior faithfully ported,
not a defect. `min_confidence`, `tolerance`, `pair_tracks` and `min_track_frames` (2 s by
default) are the rest of it, and `--dry-run` reports the decision without writing, so the
figure can be argued from a record rather than from taste.

A full pass over the 2.33 GiB side takes **2 min 57 s** through the debug-built CLI,
which is a working figure and not a performance claim - nothing in WP-13 has been timed
in release, and the pass is WP-11's spectral extraction with a few hundred row writes
after it. An earlier note of 12.7 s for the same pass could not be reproduced today and
has been struck rather than explained.

Adoption lives in `vcw-core`, not `vcw-project`: ADR-0003 will not let the project layer
see `vcw-signal`. Its public functions take `&Project` rather than `&Connection` so that
`vcw-core` still does not link `rusqlite`.

### `already_locked` is zero, and that is the system working

The re-analysis loop is `adopt::observations` into `detection::refine` into
`adopt::adopt`, and `a_locked_boundary_survives_a_second_analysis_pass` runs all of it
with the second pass configured 6 dB more sensitive than the first, on purpose, so the
detectors genuinely disagree with what is stored.

The operator's boundary survives, and adoption's `already_locked` counter reports **zero**
skips while it does. That looked like a hole and is not: the resolver sees the operator's
boundary alongside the detectors' - `observations` is what hands it over - and merges
them into one decision that `Provenance::User` wins, so there was never a separate
detector decision for adoption to skip. §24 is honored one layer earlier than the
counter measures. `a_detector_landing_beside_a_locked_boundary_is_skipped` covers the
path that *does* increment it, with the observation withheld.

### Re-analysis must not grow the rows either

Found by reading the real side's evidence dump rather than by any test. Positions settle
after one pass because every write is an upsert, but the *case* for a boundary settled
only if handing a stored row back to the resolver is reversible, and it was not: the row
holds a decision, so each measurement in it already carries the name of the detector that
took it, while `resolve::attach` prefixes an observation's evidence with its provenance as
it absorbs it. Pass two therefore stored `hmm.hmm.at`, pass three
`hmm.hmm.hmm.at`, and one boundary on the real side was carrying 40-odd
measurements, most of them the same number under a longer name. Nothing was wrong with
the numbers and nothing downstream had broken; the column was simply growing with each
press of a button an operator is expected to press repeatedly while tuning a threshold.

`adopt::as_observation` now strips the row's own provenance back off on the way in and
drops exact duplicates, and the write deduplicates too, so the round trip is idempotent.
A reading that has *changed* between passes is still kept, because `resolve::attach` is
right that the pair of them is the only record that the boundary moved.
`re_analysis_does_not_grow_the_evidence_column` runs three identical passes, requires the
second and third to agree, and fails on any name that repeats a detector - it fails on
the old code at the third pass. Stripping *one* prefix would not have been enough:
peeling a single copy is a no-op on a name the old code had already doubled, so a project
analyzed before the fix would have kept `hmm.hmm.at` at a fixed depth forever. It peels
the whole run, and the real side's rows healed on the next pass: the boundary that was
carrying 40 measurements now carries 24, and the second pass over it wrote 8 boundaries
and created no tracks, which is idempotence on a real record rather than on a fixture.

One level of nesting survives on purpose, because it is not growth:
`spectral-change.silence.contrast_db` is the detector chain's own layering - the
spectral-change detector reports what the level detector saw underneath it - and it is a
fixed point under any number of passes.

### A side has no extent, and one take of both faces exposes it

Found by pointing the CLI at the real side rather than at a fixture. `sides.capture_id`
is not unique on purpose, so both faces can share one capture - a single unattended rip
of a whole record - but a side row stores no start or end frame. Detection runs on the
*capture*, so `adopt --side A` against a shared capture writes every boundary in the
whole take onto side A and pairs them into tracks there, including in the region the
operator means as side B. Side B then holds its own rows at the same frames, and the
listing looks like duplicated tracks.

No function is doing the wrong thing, and no test sees it, because the tests attach one
capture per side. Until a side carries a frame range the workaround is to adopt only the
side a shared capture is really about, or to record the faces separately. The fix is a
migration adding that range, with adoption clipping to it - and it is the same fact §21's
skip and §33's export will need when they have to know where a face ends. It is on no
work package's exit criterion, which is why it is written down here.

### Topology validation reports what is wrong, not what is unfinished

`validate` grew `check_topology`: `empty-track`, `overlapping-tracks`, `track-numbering`
and `boundaries-without-audio`. It deliberately does **not** report a boundary that
bounds no track. That is the normal state of a side which has been analyzed and not yet
edited - 264 of the real side's boundaries are exactly that - and a validator that cries
about the ordinary case trains people to ignore it.

### Two constraints that shaped an algorithm each

`UNIQUE (side_id, number)` means `renumber` cannot walk a side assigning 1, 2, 3 in
place: the first write collides with the row that already holds that number. It writes
negative numbers in one pass and the real ones in a second. And boundary order at a
shared frame - a track ending where the next begins - was relying on `'end'` sorting
before `'start'` by accident of collation, so it is now explicit:

```sql
ORDER BY at_frame, CASE edge WHEN 'end' THEN 0 ELSE 1 END
```

### Nullable means inherited

`tracks.artist`, `composer` and `comments` are nullable, and NULL means *take the
release's*. So `Update` has to distinguish three things, not two: `None` leaves a field
alone, `Some("...")` sets it, and `Some("")` clears it back to inherited.
`Record::artist_or(&release_artist)` is what readers use, so no caller has to remember
the rule.

### The CLI

```sh
vcw tracks side-a.vcw list [--boundaries] [--json]
vcw tracks side-a.vcw attach --side A --capture 1
vcw tracks side-a.vcw adopt --side A [--min-sources 2] [--dry-run]
vcw tracks side-a.vcw add --side A --start 0 --end 282.8
vcw tracks side-a.vcw split 1 --at 140.0
vcw tracks side-a.vcw merge 1 2
vcw tracks side-a.vcw set 1 --title "The Rainbow" --confirmed
vcw tracks side-a.vcw move 4 --to 283.9 [--force]
vcw tracks side-a.vcw lock 4 [--off]
vcw tracks side-a.vcw reassign 3 --to B
vcw release side-a.vcw set --artist "Talk Talk" --title "Spirit of Eden" --discs 1
vcw release side-a.vcw artwork front cover.jpg
vcw release side-a.vcw show [--json]
```

Times are seconds everywhere and rounded rather than truncated, because a boundary typed
as `282.8` should land on the frame the listing printed. Every verb above was run against
the real side, in sequence, and the project validated clean afterwards.

### Tests

46 new lib tests - 12 in `side`, 8 in `disc`, 21 in `track` and 5 in `release` - 6 more in
`tests/validation.rs`, 3 in `tests/edits_are_nondestructive.rs` and 5 in
`tests/reanalysis.rs`. Workspace total is **751 passing, 0 failing, 12 ignored**,
gate-green on `fmt`, `clippy -D warnings`, `cargo deny`, the rustdoc leg, the
offline-build leg and the parity leg.

The two exit-criterion files are worth describing, because they are the criterion rather
than a proxy for it:

- **`edits_are_nondestructive.rs`** fingerprints every byte of `sampleblocks` and
  `capture_blocks` - blob contents included, each of the six nullable summary columns
  with them - then runs 15 editing verbs through `unchanged()` and requires the
  fingerprint identical after each: add, split, merge, remove, update, lock, move,
  forced move, delete, renumber, reassign, relabel, detach, remove side. A clean
  `validate` with `verify_checksums: true` follows. A companion test mutates one block's
  samples with `zeroblob(length(samples))` and requires the fingerprint to **change**,
  because a hash that never moves proves nothing.
- **`reanalysis.rs`** is the loop above, and also pins idempotence - the same pass twice
  produces the same boundaries, the same tracks and now the same evidence.

### Fixed on the way past

- **The boundary upsert let a detector steal a locked row's attribution.** Described
  above; it was a §24 hole, found by writing the `ON CONFLICT` clause out and asking what
  each column should do.
- **`adopt::pair` deleted boundaries it had just written.** It created a track to mark a
  pair consumed and removed it again when the pair was too short, which took the
  boundaries with it. Rewritten as a single non-destructive timeline pass. A policy test
  that should have passed is what exposed it.
- **`release_artwork.width` and `height` were always NULL.** WP-12 wrote the rows and
  left the columns, so `put_artwork` now reads the dimensions out of the image header:
  PNG at a fixed offset, JPEG by walking the segment chain and skipping the three markers
  that are not frames, bounded against truncated input and against a zero segment length
  that would loop. Five tests.
- **`Error::DifferentCapture` had a run of stray spaces in its message**, and `Soak`'s
  doc comment in the CLI had been cut in half by `Metadata` being inserted into the
  middle of it. Both are the sort of thing only reading the output catches.

### What is not verified

No UI: every verb is reachable through `vcw tracks` and `vcw release` and nowhere else,
which is WP-15 and WP-16. The transport still skips a fixed 10 s - the boundaries to
skip to exist as rows now, but `vcw-core::playback` has not been taught to ask, which is
deliberate, since WP-16 decides whether a skip lands on a boundary or on the padded
track start §33 exports. Nothing has been exported, so the claim that a track's two
boundaries are enough for a splitter is WP-14's to test. §22's guided detection - using
an identified release's track count and durations to steer the detectors - is still not
built, and it is what would actually fix the hard cases. Alpha numbering is asserted on
synthetic sides up to disc 3 and has never met a real box set. And, as everywhere in
Phase 1, this is Linux x86_64 only.

## Phase 1 - WP-14, export

Built 2026-09-27. §33's last step: a finished project becomes files. A splitter that reads
committed blocks plus edit instructions and writes nothing back, WAV and FLAC writers that
stream, tags and artwork through lofty, and VRipr's naming templates ported token for
token. **Exit criterion met on both halves.** Bit-exact WAV extraction is asserted against
the source blocks in `crates/export/tests/from_a_project.rs`, and the tags are read back by
four pieces of somebody else's software - `ffprobe`, `flac`, `metaflac` and python
`mutagen` - in `crates/export/tests/third_party.rs`.

```sh
vcw export demo.vcw --into demoout --format flac
```

```
  into       demoout
  format     FLAC, template "{album_artist}/{album}/{tracknum} - {title}"
  tracks     2 file(s), 171990 frame(s)
  artwork    69 byte(s) of image/png, embedded and beside the files
    1/2    demoout/Kraftwerk/Trans-Europe Express/01 - Europe Endless.flac
    2/2    demoout/Kraftwerk/Trans-Europe Express/02 - Hall of Mirrors.flac
  wrote      2 file(s), 1 cover(s), 171990 frame(s), 0.6 MiB
```

Verbatim, from a 44.1 kHz 24-bit project on `/data2/vcw-scratch/wp14cli`. The
two file names would read `A1 - Europe Endless.flac` today: `{tracknum}` is the
position on the label now, which is what VRipr's was and what §29 keeps - see
*a two-sided record collided with itself* below for why. `flac -t`
reports `ok` on both files, `metaflac --show-bps` 24, and the directory holds
`folder.png` beside the two tracks.

### Plan, then run, because the collision is at track nine

`splitter::plan` resolves every track to a path, a span and a set of tags and writes
nothing; `splitter::run` produces the files. Everything knowable before the first byte is
checked in the first call: an unknown token in the template, two tracks that want the same
file, a file already on disk. An export is minutes of work over gigabytes, and a collision
found at track nine is found too late. `--dry-run` is then free, and so is the list a UI
wants to show before an operator commits to it.

The splitter does no arithmetic on blocks at all. `pcm::Reader` already reassembles the
per-channel blobs and hands out interleaved frames from any frame in the capture, so a
boundary that lands mid-block costs nothing and there is no second opinion about where a
block edge is. A track's audio is the span between its two boundaries, clamped to what was
committed, and nothing else: no fade, no lead-in, no gap trimming. Earlier notes in this
file mention a "padded track start §33 exports" - there is no padding in §33 and there is
none in the code. An exporter that quietly added 200 ms of run-in would make the
bit-exactness above untestable.

### The FLAC encoder had to be written streaming, and the header patched at the end

*This records WP-14's `flacenc` writer. `flac-codec` replaced it on 2026-10-06 and
streams natively, so the hand-patched `STREAMINFO` is gone; the three findings below are
why the replacement was checked against the same readers.*

`flacenc::encode_with_fixed_block_size` builds the whole stream in RAM, which is fine for
a track and not fine for a 2.33 GiB side. `Flac` uses the per-frame
`encode_fixed_size_frame` instead and patches `STREAMINFO` in `finish`, which is what the
reference encoder does. A `PADDING` block of 8 KiB goes in at `create` so the tagger can
write a `VORBIS_COMMENT` in place rather than rewriting the file.

**`STREAMINFO` must declare the nominal block size at both ends.** `update_frame_info`
honestly recorded the short final frame as the minimum, and min != max tells libFLAC the
stream is variably blocked - where frame headers carry sample numbers rather than frame
numbers. `flac -t` then warned once per frame that the numbering did not increase and that
the file might not be seekable, on a file whose audio frames were **byte-identical** to
the reference encoder's. Measured against `flac 1.5.0` on the same input: it declares
min = max = 4096 for a 48000-frame file and for a 300-frame one, where the only frame
there runs short. `set_block_sizes(block_frames, block_frames)` is the fix.

### The `fmt` chunk goes extensible above 16 bits, and a reader is why

The 46 WAVs in `/data2/source_rips` are all 32-bit stereo with a 16-byte `fmt` chunk and
format tag 1, so the corpus said plain PCM was fine at any width. Then `flac 1.5.0`,
reading a 24-bit file we had just written, said *"legacy WAVE file has format type 1 but
bits-per-sample=24"*. A warning from the reference encoder on our own output is not
something to ship, so the rule is now `WAVE_FORMAT_EXTENSIBLE` when `channels > 2 ||
bits > 16` and the plain 44-byte header otherwise. Both sides of that evidence are in the
module docs, because the corpus is still true: readers tolerate the other shape.

### What a container will not take, said out loud

- **WAV stops at 4 GiB.** RIFF sizes are 32-bit. A 30-minute side at 192 kHz in 32-bit
  stereo is 1.4 GiB, so this is reachable by a long unsplit side rather than by a track.
  Refused before the file is created, with FLAC named as the answer.
- **FLAC is an integer codec.** A float capture is legitimate - §8 allows it and Audacity
  produces it - and choosing how to bring it down to integers is a decision about headroom
  that belongs to a person. Refused by default, with WAV named *and* the setting that
  changes the answer named beside it.

**The two library caps are gone, and the float refusal is now a question rather than a
wall.** `flacenc` 0.5.1 stopped at 24 bits and 96 kHz, which were the library's limits and
neither the format's, and because a device negotiation takes the widest integer format on
offer that meant the ordinary S32 rip had no FLAC path at all. `flac-codec` replaced it on
2026-10-06: FLAC now carries up to 32-bit integer at any rate VCW records, and the rate
refusal no longer exists. Narrowing a float capture *silently* was considered and rejected
on measurement - a real 32-bit rip from `/data2/source_rips` uses the whole low byte (`OR`
of every low byte is `0xff`, max absolute value 2,092,715,264), so dropping eight bits is
not lossless and is not the exporter's decision - but the operator-chosen version of it
shipped with 0.1.2-alpha: three switches in `Settings > Export` and on `vcw export`,
`--narrow` (`refuse`, `24`, `32`), `--dither` (`tpdf`, `none`) and `--headroom` (dB).
`refuse` is the default, so an install nobody has configured behaves exactly as before.
24 bits is the width worth having: a float significand is 24 bits, so a full-scale sample
survives intact, and a 32-bit FLAC stream has no mid/side at all because the difference
channel needs one bit more than the samples. The dither generator is a xorshift64 seeded
per file from a constant, so §33's reproducibility claim survives it; anything still over
full scale after the headroom is clamped, not wrapped. `narrowed()` is keyed on
`carries()` rather than on FLAC, so the day a container learns floats it stops narrowing
with no edit.

### lofty 0.25 removed the freeform key, so the tags go in twice

VRipr wrote its vinyl-specific fields with `ItemKey::Unknown("DISCOGS_RELEASEID")`. In
lofty 0.25 `ItemKey` is a closed `Copy` enum with no such variant, so a freeform key
cannot be named through the generic tag at all. The mapped fields are still built once as
a `lofty::tag::Tag` - one list of assignments, not two - and it is then converted into the
container's own tag, where the freeform keys can be named: `VorbisComments` for FLAC,
where a key is just a key, and `Id3v2Tag` for WAV, where it is a `TXXX` frame.

Two conventions are ported deliberately. A multi-value field is one string split on `';'`
and written as **separate items**, so a player that understands multi-value shows two
artists and one that does not shows the first rather than showing punctuation. And where
the canonical Vorbis key differs from VRipr's, both go in: `LABEL` **and**
`ORGANIZATION`, `RELEASECOUNTRY` **and** `COUNTRY`. A reader looking for either finds it,
and a library built with the old tool keeps the shape it had.

VRipr wrote no embedded artwork at all - it dropped a `folder.jpg` beside the tracks, and
players that read embedded art showed nothing. The default here is `--artwork both`: one
image file per album directory *and* a `CoverFront` picture in every file. `metaflac
--export-picture-to` returns the bytes that went in.

One incidental finding worth knowing: lofty maps `ItemKey::EncoderSoftware` to the Vorbis
**vendor string**, not to an `ENCODER` comment. `metaflac --show-vendor-tag` prints
`VCW 0.1.0`; `--export-tags-to` does not mention it. That is the right place for it in a
FLAC file, and it is not where you would look.

### Tagging must not move the audio, and that is asserted per container

A tag write that shifts one sample has broken the bit-exactness the rest of the work
package proves. FLAC is checked by digest - the stream's MD5 is over the samples, so an
unchanged `metaflac --show-md5sum` plus a clean `flac -t` means only metadata moved - and
WAV by finding the `data` chunk before and after and comparing it byte for byte.

### The CLI

```sh
vcw export side-a.vcw --into ~/rips [--format flac|wav] [--template "{album_artist}/{album}/{tracknum} - {title}"]
vcw export side-a.vcw --into ~/rips --side A [--side B]
vcw export side-a.vcw --into ~/rips --artwork none|embed|folder|both
vcw export side-a.vcw --into ~/rips --dry-run [--json]
vcw export side-a.vcw --into ~/rips --overwrite
```

The project is opened **read-only**, because §33 says an export reads immutable blocks and
edit instructions: opening it writable would make a crash mid-export a risk to the one
thing in the project that cannot be redone. A line is printed as each file starts rather
than as it finishes, since the file being worked on is the useful thing to see.

### Tests

820 in the workspace, 12 ignored, 69 of them new here: 36 in the `vcw-export` lib
(15 `encoder`, 15 `naming`, 6 `tagging`), 14 in `tests/third_party.rs`, 13 in
`tests/from_a_project.rs` and 6 in `crates/cli/tests/export_from_cli.rs`.

The three files that carry the criterion:

- **`from_a_project.rs`** records a known pattern through `vcw-project`'s own capture
  writer, cuts it at frames that are deliberately not block-aligned, exports, and compares
  each file's data chunk against the exact slice of what went in - for `Int16`, `Int32` and
  four-channel `Int32`, where a stored frame and a WAV frame are the same bytes, so the
  comparison is an identity and not a transform. `Int24Padded` gets its own test for the
  dropped pad byte. A fourth test hands the reference decoder a FLAC export and compares
  what comes back.
- **`third_party.rs`** shells out to tools somebody else wrote and believes them over us:
  `ffprobe` on every stored format, `flac -t`, a `flac -d` round trip, `metaflac
  --show-md5sum` against the reference encoder's digest, `metaflac --export-tags-to` for
  every tag, `mutagen` for both containers, `sox` for the duration. A missing tool skips
  its own test, so this passes on a bare CI runner - and
  `at_least_one_verifier_is_installed` fails when *nothing* is available, because the gate
  can be unverified or it can be green, not both.
- **`export_from_cli.rs`** drives the whole thing through the shipped binary on a machine
  with nothing plugged in, and cross-checks the exported WAV against what `vcw play
  --render` renders over the same frames. They share `pcm::Reader` underneath, which is
  the point: there is meant to be exactly one way to get samples out of a project.

### Fixed on the way past

- **A provider's slash became a directory, in the original.** VRipr's
  `apply_path_template` substitutes first, then splits on `/` and sanitizes each segment,
  so a Discogs title of `AC/DC Medley` becomes a directory called `AC` containing a file
  called `DC Medley`, and its `sanitize_filename` maps the nine Windows-hostile
  characters without touching `..`, so a title of `..` climbs out of the output
  directory. The port does not inherit either: `Values::sanitized()` runs *before*
  substitution, which is what keeps the operator's `/` in `{album}/{title}` - a directory
  they asked for - apart from a provider's `/` inside a name. The per-segment pass
  afterwards is kept and is not redundant: it catches the hazards typed into the template
  itself.
- **Typo suggestions could not see a transposition.** This one is ours, not VRipr's:
  its threshold is `max(2, shorter/3)`, which accepts `titel` at a plain-Levenshtein
  distance of 2, and the port tightened the budget to 1 for a five-character token and so
  returned nothing. The distance is now Damerau-Levenshtein with adjacent transpositions,
  which scores the swap as one edit and keeps the tighter budget; `bitrate` still
  correctly gets no suggestion.
- **The extension came from the template, so there was none.** `plan` produced
  `01 - Europe Endless` with no `.flac`, caught by a test asserting the relative paths.
  Appended rather than set with `with_extension`, which would read `Symphony No. 5` as a
  file called `Symphony No` with an extension of ` 5` and replace it - there is a test for
  that too.
- **`vcw session --format` was silently ignored by the simulated source.** It was
  hard-coded to S32, so `--format s16` produced a 32-bit project and there was no way to
  make a FLAC-exportable capture without hardware. `Simulated::deterministic_as` honors
  it; the pattern generator already wrote at the stored width, so a simulated capture stays
  recomputable frame by frame in any format.
- **`Writer::Flac` was 472 bytes against `Wav`'s 64**, so every writer moved by value
  carried the larger. Boxed, with the reason written down.

### What is not verified

No UI: export is reachable through `vcw export` and nowhere else, which is WP-16. **MP3
and Ogg are not built** - §33 lists them as required initial formats and the plan puts them
in G3 behind D5's LGPL relink consequence, so this is a known deferral rather than an
oversight. A 32-bit project cannot be exported as FLAC, as above. Nothing has been
exported from the 2.33 GiB real side yet, so the streaming claim is asserted on tracks of
seconds rather than of minutes, and no export has been timed. `vcw play --track N` is
still only a label and does not resolve a track's span, which is why the CLI cross-check
asks for a region in seconds and then asserts the span it got. Linux x86_64 only, like
everything else in Phase 1.

## Phase 1 - WP-15, the Tauri 2 shell

Built 2026-09-27. §5's desktop shell, §35's command and event surface, and D9's generated
TypeScript with a drift check. Two new things in the tree: **`crates/contract`**, the
eleventh crate, which is the typed surface itself; and **`app/`**, a cargo workspace of its
own holding the Tauri binary and a React frontend. **Exit criterion met, and structurally
rather than by discipline** - `cargo tree --workspace` at the repository root cannot reach
Tauri, because the shell is not a member of that workspace.

### The exit criterion is a layout decision, not a lint

*Core crates have zero Tauri dependency (enforced in CI).* The `core-is-ui-free` job has
existed since WP-01: it walks `cargo tree --workspace` at the root and fails if `tauri`,
`wry`, `tao` or `webkit2gtk` appears anywhere in it. That check is only worth running
while the shell is outside the workspace, so `app/` is excluded from the root and carries
its own `Cargo.toml`, with `crates/*` as path dependencies. Put the shell inside and Tauri
is in the tree by construction, and the job has to be weakened to an allow-list of crates
it is willing to forgive - which is the moment the criterion stops meaning anything.

The second benefit is the four-target matrix. WebKitGTK is a Linux system library; the
product's own jobs should not have to install it to compile a crate that is glue.

The cost is paid honestly: root `fmt`, `clippy`, `test` and `doc` do not reach `app/`, so
the gate has four more legs (`appfmt`, `appclippy`, `apptest` and the frontend's
`uicheck`) and CI has two more jobs - **`shell`**, which installs the WebKitGTK stack and
runs fmt, clippy and tests in `app/src-tauri`, and **`bindings-are-current`**, which
regenerates the TypeScript and then runs `git diff --exit-code` over it. `app/Cargo.toml`
repeats the root's `[workspace.lints]` table verbatim: a rule that fired in one workspace
and not the other would make moving code between them an argument about lints.

### The contract is a core crate, because units are application behavior

`vcw-contract` depends on `vcw-core`, `vcw-project`, `vcw-audio`, `vcw-types` and
`serde` - and on nothing from Tauri. It holds three things §35 names:

- **Events.** `Wire` is every `vcw_core::Event`, flattened into one discriminated union
  tagged on `kind`, where the tag is exactly the string `Event::name` already returned.
  A test builds one of all fourteen core events and asserts that each maps to a `Wire`
  whose `kind` equals that name, so a new core event that falls through to the catch-all
  fails the build rather than arriving at the frontend as an unlabeled warning.
- **Commands.** `Request` is the eight verbs that change something, each parsed from JSON
  with refusals that name the field at fault (`Failure { code, message, field }`).
- **View models.** What a UI is given to draw, with the units already resolved: dBFS
  rather than amplitudes, seconds beside frames, a side letter rather than a side index,
  `A3` rather than a position index. Every one of those is a calculation, and §2 puts a
  calculation in the application layer - not in a React component where it becomes a
  second opinion the moment `vcw --json` prints the first one.

`contract::read` is the read path: `release`, `captures`, `sides` and `tracks`, each
taking a `&Connection` so a read-only handle serves them. `Track::of` needs a side letter,
a sample rate and a numbering scheme to produce one row, which is why it is a constructor
and not a `From` - a track row on its own cannot answer two of those three questions, and
a view model that guessed 44.1 kHz would put every duration on a 48 kHz project out by
nine percent. The test that covers it is named
`seconds_come_from_the_rate_and_not_a_guess`.

### D9: one generated file, and a test that fails on drift

`app/ui/src/bindings/vcw.d.ts` is 911 lines, 30 exported declarations, generated by
`bindings::typescript()` from ts-rs 12.0.1, committed, and rewritten with
`VCW_BLESS=1 cargo test -p vcw-contract --test bindings` - the same convention
`docs/SCHEMA.md` already uses for the schema dump. One file rather than ts-rs's
file-per-type export, for a reason that only shows up in the failure case: a directory of
generated files has to be compared entry by entry, and a *deleted* type is what that
comparison gets wrong. One file is one string comparison, and a removed type is a removed
block. The drift test reports the first differing line rather than the whole file.

Three things about ts-rs had to be found by probing it:

- **`u64` renders as `bigint` by default, which is wrong at run time.** `serde_json`
  writes a frame count as a JSON number and `JSON.parse` returns a double, so nothing that
  crosses this boundary is ever a `bigint`. A frontend typed that way cannot add two frame
  counts without a cast that lies in the other direction. `Config::new().with_large_int("number")`
  is the fix, and `nothing_generated_is_a_bigint` keeps it. The ceiling accepted is 2^53
  frames, which at 192 kHz is about a billion years.
- **`rename_all` renames variants; fields need `rename_all_fields` as well.** They are
  independent, and a `peak_db` survived a whole probe as snake_case with only the first set.
- **Every generated line ends in a space.** Stripped, because the first editor to save the
  file would otherwise produce a whitespace diff of all 911 lines, and
  `the_file_has_no_trailing_whitespace` asserts it.

`no_pcm_crosses_the_boundary` is the other test worth naming: it walks the generated field
names and fails on anything that would put samples through the IPC, which is S3's finding
turned into a rule rather than a memory.

### One event channel, and a thread per long command

The shell emits everything on a single webview event, `vcw://event`, carrying the whole
`Wire` union. Nothing is coalesced and nothing is binary: S3 measured the boundary as
free, the main-thread waveform draw as the cost, and `InvokeResponseBody::Raw` as a
pessimisation for frames this size. `pump.rs` spawns a forwarding thread per subscription
which stops when an emit fails or when `wire.is_last()` - the `closed` event - arrives, so
a closed window does not leave a thread reading a bus for ever.

A synchronous `#[tauri::command]` runs on the main thread, the one drawing the window, so
the two long commands spawn:

- **Export** plans on the command thread, which is deliberate: a bad naming template comes
  back as a refused command with a field name, and only then is a `vcw-export` thread
  started. Progress arrives per file, and an export ends in exactly one of
  `export-finished` or `export-failed`, because the command that starts it returns as soon
  as the thread exists. Those two and `export-progress` are the three
  `Wire` variants with no core event behind them.
- **Playback** gets a thread because `vcw_core::playback::Player` is not `Send` - it owns
  a `cpal::Stream`. The thread opens the player, owns it, takes `Verb`s over a channel and
  calls `player.tick()` on a 16 ms timeout, without which no playhead is published. A
  `stop` waits for the thread to join, because the next `play` opens the same device.

`library.rs` opens the project read-only per read and **closes it explicitly**, which is
what deletes the `-wal` and `-shm` sidecars; a handle dropped without one looks like a
crash to the next recovery check. `edit.rs` is the only command that opens it writable,
and it resolves the rate from the boundary's own side's capture before any arithmetic: a
marker at 12.5 s is 600,000 frames at 48 kHz and 551,250 at 44.1, and the wrong one moves
it half a second.

### The surface test reads the generated TypeScript

`main.rs` carries a `WIRED` list of six contract tags and a `NOT_WIRED` list of two, and
the test that checks them parses `"command": "..."` out of the generated `Request`
declaration rather than out of a hand-written list. A ninth verb added to the contract
therefore fails the shell's tests without anyone remembering that this file exists. Three
of the wired tags are served by a differently-named command, which is written down where
the list is: `seek` is one of six playback verbs and a command per verb would be five
functions that all call `player.apply`; `export` is served by `export_plan` and
`export_run` because §33 plans before it writes; `transport` carries the capture verbs for
the same reason. The read commands - `devices`, `tracks`, `waveform` and the rest - are
not in `Request` at all, because §35's list is the commands that change something.

Sixteen commands are registered. `devices` returns `Vec<Device>` rather than a `Result`:
a machine with one broken USB interface should still show the other three.

### The frontend is a smoke page, and that is all it is

`app/ui` is React 19 on Vite 7 with TypeScript strict plus `noUncheckedIndexedAccess`,
`exactOptionalPropertyTypes` and `verbatimModuleSyntax`. `src/api.ts` is the only file that
calls `invoke`, and `App.tsx` is a device table, transport buttons, a frames-and-peak
readout, a capture table and an event log. Its `describe(event: Wire)` switches over all
seventeen event kinds exhaustively, which is the whole argument for D9 in one function: a
new event kind is a compile error there rather than a silent gap in the log. `pnpm check`
passed first time against the generated types, which is the evidence that they are usable
and not merely current.

### Fixed on the way past

- **`custom-protocol` defaults to on.** Tauri decides "am I a dev build?" from that
  feature and not from the cargo profile, so a release binary built without it loads
  `devUrl` and lands on `about:blank`. This cost S3 an afternoon; the feature is in the
  default set and the reason is in the manifest.
- **`unreachable_pub` fires on every `pub` item in a binary crate**, which is correct -
  nothing outside can name them. Everything in `app/src-tauri/src` is `pub(crate)`.
- **An integration test cannot import a binary crate's items**, so the planned
  `tests/surface.rs` is a `#[cfg(test)] mod tests` inside `main.rs` instead.
- **The doc link `[parse_format]` was public documentation pointing at a private item**,
  caught by the `doc` leg. Both halves of one mapping now have the same visibility, which
  is the actual fix: `format_name` and `parse_format` are the wire spelling of a sample
  format in each direction, spelled out rather than derived from `SampleFormat`'s
  `Display`, so a rename in `vcw-types` cannot change what a frontend has to send.
- **`WIRED` was dead code in the binary**, because only the test reads it.
  `#[cfg(test)]`, since `generate_handler!` is the real wiring and a second copy compiled
  into the product would be a second answer to the same question.
- **`no_pcm_crosses_the_boundary` failed on `clippedSamples`** and on prose containing the
  word. A substring ban was the wrong test; it compares field names now, and when an
  export field called `bytes` tripped it, the field was renamed to `bytes_written` rather
  than the test weakened - it is named for what it counts either way.

### Tests

**861 passing in the workspace, 0 failing, 12 ignored**, of which 37 are new here: 33 in
`vcw-contract` (4 in the lib, 6 in `wire.rs`, 17 in `commands.rs`, 5 in `views.rs`, 5 in
`bindings.rs`) and 4 in the shell. The gate is eleven legs now - `fmt clippy test parity
offline deny doc` at the root, `appfmt appclippy apptest` in `app/src-tauri`, `uicheck` in
`app/ui` - and the em-dash sweep covers `.ts`, `.tsx`, `.css` and `.json` as well.

The shell ran on this desktop: the window opened, and the ALSA enumeration noise in its
log is the proof that the frontend loaded and completed an IPC round trip, because nothing
but the frontend's `devices()` call enumerates.

### What is not verified

**Nothing in the shell has been driven through a real capture by hand.** Arm, record,
stop, play and export are wired and tested as functions; the buttons have been clicked
only against an empty project. **No screenshot of the window was captured** -
`gnome-screenshot -w` fell back to X11 on a Wayland session and hung.

`search_metadata` and `select_release` are declared in the contract and **refused** with
the code `not-wired`, named in `NOT_WIRED` rather than left out, because they need a
provider client held across calls, a disk cache, a credential that §39 forbids storing in
the project, and cancellation for a search a person changes their mind about. WP-12 built
every piece of that except where the shell keeps them, and WP-16's metadata browser is
what decides that - it is a design question, not a morning's wiring. `waveform-update` and `fingerprint-match` are named in §35, and
they are **absent from `Wire` rather than declared in it** - an earlier version of this
sentence had that backwards. The pyramid is built by the writer and read on demand, so
there is nothing to push, and declaring the variant would publish a promise the core
cannot keep.

**A playback failure arrived as a `capture-warning`** with the code `playback-failed`,
because the open happens on a thread and the bus had no playback-refused event. It was a
wart and it was recorded as one; the fix was an event on the bus, not a workaround in the
shell. That event was built when Phase 1's loose ends were closed - see *Phase 1 -
closing the loose ends* below.

`bundle.active` is `false` and the icon is a 245-byte placeholder: packaging is WP-20 and
this shell is not something to hand anyone. `csp` is `null`, which is Tauri's development
default and has to be set before anything ships. The frontend is a smoke page and not
WP-16's UI. Linux x86_64 only, like everything else in Phase 1 - and the `shell` CI job
is Linux-only too, so the WebKitGTK stack is the only webview this has ever met.

## Phase 1 - WP-16, the React UI

Built 2026-09-27. §34's panels, §43's keyboard map and §44's workflows. The frontend is
now **4,486 lines of TypeScript and TSX across 22 files** - eleven panels under
`app/ui/src/panels/`, a root that owns the layout and nothing else, one store, one keymap,
two formatting helpers and two test files. The gate has a twelfth leg.

**The exit criterion was not met, and the test that was supposed to prove it could not
see why.** That sentence replaces the one that stood here until WP-16a, which claimed
both halves were met and asserted by tests. First light found otherwise: three of the
four row selections in the application could only be made with a mouse, so five of §44's
workflows could not be *started* from the keyboard, whatever the map said. See WP-16a
below. The claim is corrected rather than deleted because the way it came to be written
is the interesting part - the tests said what they were asked to say, and what they were
asked was the wrong question.

### The keyboard criterion needed three tests, and only two existed

*Every §44 workflow completable by keyboard alone.* WP-15 left two checks behind and they
looked like enough:

- `COVERAGE` in `keymap.ts` is a `Record<Workflow, readonly Action[]>` over the literal
  map, so a §44 workflow with no binding is a **type error in `pnpm check`** with no test
  runner involved;
- `keymap.test.ts` proves every chord is spellable by `keys.ts`, that no two bindings in
  one scope collide, and that the §43 defaults are the chords §43 names.

Between them they prove the map is *complete and consistent*. Neither can see whether a
binding does anything, and four of them did not: `arm`, `search-metadata`,
`choose-release` and `export` were in the map, passed both checks, and had no handler
behind them. So there is a third test, `wiring.test.ts`, and it is WP-16's exit criterion
written down:

- it finds every `useKeys(` call in every `.tsx` file by reading the sources through
  Vite's `import.meta.glob("./**/*.tsx", { query: "?raw" })`;
- it asserts that **every action in `BINDINGS` is handled in at least one of them**;
- and it asserts that **every §44 workflow has at least one handled action**, which is
  the criterion itself and not a proxy for it.

It was verified to fail before it was trusted: deleting Export's `useKeys` line makes it
name both the `exportRun` action and the `export` workflow. The first version of its
extractor was wrong in the quiet direction - it looked for a block ending in `\n  };`, so
a single-line `useKeys("metadata", { lookup: search, accept });` was invisible and the test
passed by not looking. It now counts brackets, and `handled()` matches both
`record: () => ...` and the shorthand `arm,`. A test that cannot fail is worse than no
test, which is why that paragraph is here rather than in a commit message.

### The map was unreachable, which no amount of handlers would have fixed

The second finding was structural. Every scoped binding - `t`, `a`, `d`, `l`, `Enter` -
requires its panel to be in front, and at the start of WP-16 **nothing could bring a panel
forward from the keyboard**. The criterion was unmeetable no matter how much was wired.
Eight bindings were added, all `workflow: "navigate"`, all `scope: "global"`:
`Ctrl+1`..`Ctrl+6` for the six panels, `Ctrl+D` for the diagnostics log and `Escape` to
dismiss. `Ctrl` and a digit rather than a bare digit, because a bare digit is the first
thing taken away the moment somebody types a catalog number into a field. The map is
now **32 actions covering all 20 of §44's workflows**.

### No business logic in TS is a review gate, so it was reviewed - and it failed twice

The contract does most of the work: a `Track` arrives carrying `A3`, its seconds and its
confidence, so a component has nothing to compute. But "nothing left to compute" has to be
true of every field a panel touches, and an audit of my own TypeScript found three places
where it was not. All three were moved into Rust rather than defended:

- **`Export.tsx` divided a frame count by 44,100.** A hard-coded rate in the one panel
  whose job is to be right about what gets written. The plan does not carry a rate, and a
  project with a 96 kHz side and a 48 kHz side makes *any* divisor wrong, so the panel now
  shows frames and says so.
- **`Waveform.tsx` multiplied and divided by `capture.rate`** to turn a playhead into a
  pixel and a column into a frame. `view::Waveform` gained `start_seconds` and
  `end_seconds`, so the panel now reads the span it was given.
- **`Tracks.tsx` computed `row.end - row.start`.** `view::Track` gained `seconds`, which
  is `end - start` over the rate, carried rather than left to the caller.

That is the criterion behaving as intended: it caught three unit conversions in a
frontend written by someone who knew the rule, which is roughly the rate at which they
appear when nobody is checking. `store.ts` states the line in its own header - everything
it holds is *a copy of the last thing Rust said* - with the one declared exception, the
bounded event log, argued rather than smuggled.

### The four deferrals, answered in code

Each of these was recorded by an earlier work package as a WP-16 question. None is
answered in prose only; each is a decision in a file with the argument beside it.

- **WP-09: the waveform is polled, not pushed.** `Waveform.tsx` measures its own column
  count with a `ResizeObserver` and asks for exactly that many. The argument is in its
  header: S3 found the IPC free and the main-thread draw expensive, the rows are on disk
  the instant the writer commits them, and a pushed `waveform-update` would deliver
  columns at the writer's rate rather than at the width the view happens to be. Nothing
  publishes that event today and now nothing needs to.
- **WP-11: a rejected boundary is shown.** `Tracks.tsx` lists it with its confidence and
  the detectors that agreed, dimmed and italic, not hidden. A person cannot promote what
  the picture does not show, and `Policy::min_sources = 2` is a policy that gets tuned -
  which it cannot be from a UI that renders only what survived it. The waveform draws it
  too, thin against the thick locked ones, so the distinction survives grayscale.
- **WP-07: a recovered capture is shown and never resumed.** `Capture.tsx` reports a
  `recovered` or `interrupted` capture with how much of it survived and offers nothing but
  play. Appending to a capture that stopped for a reason nobody has established is the one
  operation in the application that can lose a rip. The panel says "Nothing has been
  resumed or removed: play it, and decide", which is the honest state of it.
- **WP-13: a skip lands *on* the mark.** Implemented in `vcw-core::playback`, where that
  file's own constant doc had already predicted it. Detail below, because it turned out to
  be more than a UI opinion.

### WP-13's skip is now live, end to end

The deferral was "whether a skip lands on the boundary or somewhere inside the track", and
the answer is on it. The alternative - a second of lead-in so the needle drop is audible -
is wrong for the verb: a person skipping forward is looking for the top of a track, and
landing early means the first thing they hear is the end of the previous one.
`BOUNDARY_CONTEXT_SECONDS` exists for the other job, auditioning a join to judge whether
it is in the right place, and that is where context belongs.

Three pieces, because the core half alone would have been inert:

- **`vcw_project::track::edges_of_capture`** returns both ends of every track on every
  side a capture was recorded to, ascending and deduplicated. Both ends and not just the
  starts, so `SKIP BACK` from inside the last track lands at its top and `SKIP FORWARD`
  from there lands at its end rather than running to the end of the side. Deduplicated
  because adjacent tracks share a frame, and a duplicated mark is a skip that appears to
  do nothing. One flat list across both faces, because §21 lets one capture hold two and
  the needle does not stop at the join either.
- **`Audition::marks`**, sorted and deduplicated by its builder rather than trusted, with
  `Player::skip_forward` and `skip_back` landing on the next mark and falling back to
  `SKIP_SECONDS` when there are none - an unanalyzed side, which is the case the fixed
  step exists for. The `render` driver mirrors it exactly, because a render is how a skip
  is tested without a sound card and a driver that skipped differently would make that
  test worthless.
- **Both callers fill it.** `app/src-tauri/src/audition.rs` reads the edges in the same
  read-only open that resolves the scope, and `crates/cli/src/play.rs` does the same, so
  `vcw play --script "skip,skip"` moves between tracks. Nine new tests cover it: the
  no-marks fallback, landing on the next and previous mark, a mark under the playhead not
  counting in either direction, holding the key walking rather than sticking, the builder
  sorting and deduplicating, a render whose marks are deliberately *not* ten seconds apart
  so an implementation that ignored them could not land on them by accident, and the two
  project-level queries.

The marks handed in are the whole capture's, not the scope's, and that is deliberate:
`Player::seek` clamps into the span, so a skip out of a one-track audition lands at its own
end, and filtering in the shell would be the shell deciding twice what playback decides
once. One behavior is worth stating because it looks like a bug and is not: `SKIP BACK`
from inside a track lands on that track's top, and walking further back needs a second
press. That is what a transport's back button does.

### WP-15's open design question: the shell keeps none of it

Where the shell holds a provider client, a disk cache and a §39 credential was left to
WP-16. The answer is that it holds none of the three. `app/src-tauri/src/metadata.rs`
states it at the top and the code is short because of it: the client is built per call from
`Setup`, the cache is a directory under Tauri's own `app_cache_dir`, and the credential is
read from the environment at the call site and dropped when the command returns. The only
thing held across a call is a `Cancel` token, and only because a half-finished network
request has nowhere else to be recorded - a second search cancels the first rather than
racing it.

Both commands are `async` and do their work in `spawn_blocking`, because §40 boxes a
search at ten seconds and a synchronous Tauri command runs on the main thread, which would
mean a window frozen for ten seconds. `select_release` fetches first and opens the project
for writing only after the fetch returns, so a provider timing out cannot hold a write
lock on the project while it does. A request that names a provider overrides §39's on/off
switch, on the grounds that a person who clicked "ask Discogs" has said what they want more
recently than the settings panel did.

### The escape hatch is gone, which makes the test a requirement

WP-15 shipped a `NOT_WIRED` list and a `refused` command to answer for it, because a
command a frontend can send and nothing answers is worse than one that refuses out loud.
**All 17 commands in `Request` are now wired**, so `NOT_WIRED`, `fn refused` and
`Error::NotWired` have been deleted rather than left as an empty array. That turns
`every_command_in_the_contract_is_wired` from a reminder into a hard requirement: a verb
added to the contract now fails the suite until something in the shell honors it.

### What the panels are, and what each decides

- **`Transport.tsx`** is always mounted, below whichever panel is in front, and owns the
  ten transport actions. It takes the current capture and side as **props**: `App` decides
  which they are, so a `marker` keypress on an ambiguous project does nothing rather than
  guessing. §21 allows two faces on one capture, so "the first side with a capture" would
  put a marker on side A of a record cued to side B; the rule is one side or none.
- **`Meters.tsx`** draws percentage-width `div`s, not a canvas. S3's finding was that the
  main thread is the cost, and three nested divs per channel cost the compositor and not
  the main thread. The hold needle and the clip latch come from the event; nothing here
  decides when a clip has expired.
- **`Waveform.tsx`** fetches on resize and on the event that says the rows moved, and
  draws in a second effect so a fetch does not block a paint.
- **`Browser.tsx`** is §34's project list plus the three-field create helper the user
  asked for - artist, recording title, catalog number, none of them required, which is
  what `config::new_project` already accepts.
- **`Capture.tsx`** shows the device, the rate, the format, `problems`, and the negotiated
  format beside the enumerated one, because §7's divergence is the thing an operator needs
  to see before the needle drops.
- **`Tracks.tsx`** is the editor: detect, delete, nudge and rename. `NUDGE` is 0.05 s and
  a nudge sends `force: false`, so it can never break a lock - §24 refuses it and the
  refusal says why.
- **`Metadata.tsx`** seeds its four criteria from the release row and shows
  `Accepted.unmatched`/`unnamed` as a *result* rather than an error, because a release
  with fewer tracks than the side is a fact about the record.
- **`Export.tsx`** plans and then runs, in that order, which is §33's order.
- **`Settings.tsx`** writes every field whole. Credentials are shown as name, present,
  character count and variable - there is no command in the shell that returns one, which
  is a stronger guarantee than a masked input.
- **`Diagnostics.tsx`** is §42's log, newest first, filterable by kind, paged at 120.
- **`Help.tsx`** is generated from `BINDINGS` and shows the whole map, marking
  out-of-scope groups rather than hiding them - a key a person cannot find is the same as
  a key that does not exist.

### The twelfth gate leg

`uitest` (`pnpm test` in `app/ui`) is now a leg of its own, beside `uicheck`. The reason
is the finding above: `pnpm check` proves the frontend compiles against the generated
bindings and that the map covers §44, and it proved both of those while four workflows had
no handler. CI gained the matching step. The gate now runs twelve legs -
`fmt / clippy / test / parity / offline / deny / doc` at the root, `appfmt / appclippy /
apptest` in `app/src-tauri`, and `uicheck / uitest` in `app/ui` - at **910 passing, 0
failing, 12 ignored** in Rust and **16 passing** in the frontend, with the em-dash sweep
clean.

### What WP-16 leaves behind

- **The window had never been opened** when this was written. `pnpm build` succeeded -
  271 kB of JS, 6.15 kB of CSS, 49 modules - every command was tested as a function and
  every binding as a wire, but nothing had been driven by hand and no screenshot existed.
  That run was called the cheapest outstanding item in the plan and the only thing that
  would say what §44 actually feels like. It was both: see WP-16a.
- **A playback open failure still arrives as a `capture-warning`** coded
  `playback-failed`. Unchanged by WP-16 because the fix is an event on the bus, not a
  workaround in the shell. Closed later: see *Phase 1 - closing the loose ends*.
- **`waveform-update` and `fingerprint-match`** are declared in §35 and nothing produces
  them. The waveform decision above is why the first one is not missed.
- **A side still has no extent**, and the Transport's one-side-or-none rule is the honest
  consequence: on a capture holding two faces, the marker key does nothing until somebody
  says which face. The fix is a frame range on `sides`, as recorded under WP-13.
- **No macOS or Windows.** The frontend is WebKitGTK-only so far, which means the layout
  has met one engine and one font stack.

## Phase 1 - WP-16a, what first light found

Built 2026-09-27, straight after WP-16 and before WP-17, because §12's item 1 said to run
the thing before building more of it. The window opened, all eight panels drew real data,
and three defects came out that the 914-test gate had been green through. That is the
whole argument for the run, so it is worth being precise about why each one was invisible.

### The window works, which is the part that is easy to skip past

Before the defects: the Projects panel listed four real projects with their release,
catalog number, side and track counts, length and size; the waveform strip read **1,920
columns out of a 2.33 GiB capture** spanning 26:05.77 with the track boundaries drawn in;
the Capture panel reported `48000 Hz s32 x2 (unverified)`, which is S1's
never-trust-CPAL's-format-report surfacing honestly in the UI rather than being quietly
smoothed over; Settings read the library root back out of
`~/.config/dev.vcw.app/settings.json` and listed `discogs / no / VCW_DISCOGS_TOKEN`, so
§39's "credentials come from the environment and are never written to a project" is
visible to the operator; Metadata pre-filled from the stored release; and the Keys overlay
grouped every binding by scope with its §43 reference and a "(not this panel)" annotation.
None of that needed fixing. Screenshots are under `/data2/vcw-scratch/firstlight/`.

### A selection no key could move, which is the one that changed a claim

*Every §44 workflow completable by keyboard alone* (`PROJECT_PLAN.md:564`). Four lists in
the application draw a chosen row: the projects, the tracks, the boundaries, the release
candidates. Three of them could only be chosen by clicking:

| File | Line | What could not be selected |
|---|---|---|
| `Browser.tsx` | 167 | the project `Enter` opens |
| `Tracks.tsx` | 217 | the track `Enter` edits |
| `Tracks.tsx` | 318 | the boundary `Delete` and the nudges act on |
| `Metadata.tsx` | 193 | the candidate `Enter` accepts |

A `grep` for `Arrow`, `tabIndex` or `onKeyDown` across `app/ui/src/panels/` returned
nothing but Capture's two `focus()` calls onto native `<select>` elements. So `navigate`,
`edit-track-metadata`, `delete-marker`, `move-marker` and `choose-release` all had a
correct binding, a handler, a help-overlay entry - and no way to reach their own subject.

The keyboard map documented the hole in its own words, which is the detail worth keeping.
The overlay reads *"Open the selected project"*, *"Edit the selected track"*, *"Delete the
selected marker"*, *"Accept the selected release"*, and contains **no binding that selects
anything**. It was legible on screen, in the application, the first time anybody looked.

**Why `wiring.test.ts` passed.** It asserts that every action in `BINDINGS` is a key of
some `useKeys` handler object, and every one of them was. The test cannot see that a
handler reads a piece of state, nor that the only thing which wrote that state was
`onClick`. It answered "is this action handled" correctly; the criterion asks "can a
person complete this workflow", and those come apart exactly here. Three tests were
supposed to cover the criterion between them and all three were about *actions*.

So WP-16a adds a fourth, and this one is about *lists*:

```ts
const lists  = (file) => (SOURCES[file]?.match(/"selected"/g)  ?? []).length;
const movers = (file) => (SOURCES[file]?.match(/\bstep\(/g)     ?? []).length;
```

A panel that draws `n` selected rows must contain `n` calls to `step`, the shared mover.
Both sides are counted out of the source, so a fifth list added without a mover fails in
the gate rather than in a screenshot. It was mutation-checked by deleting Metadata's mover
and confirming the failure names the file: `./panels/Metadata.tsx: 1 list(s), 0 mover(s)`.
The `>= 3` guard beneath it exists so that renaming the `selected` CSS class makes the
test fail loudly instead of passing vacuously - which is the failure mode of every test
that reads source text, including the three it is joining.

**The bindings.** `ArrowUp` and `ArrowDown`, unmodified and scoped per panel: the global
map had already taken the horizontal pair for seeking, and a vertical arrow in a list can
only mean one thing. The tracks panel needs both its lists, so `Shift` picks the boundary
one, on the same argument §21's skip uses `Shift` for the coarser move. Eight bindings,
eight `COVERAGE` entries, and the help overlay picked them all up without being touched -
it walks the map.

**One mover, not four.** `app/ui/src/select.ts` holds a nine-line `step()` and seven
tests. It clamps rather than wrapping, because holding `ArrowDown` should come to rest on
the last row - a list that jumps back to the top is a list you cannot arrive at the bottom
of. With nothing selected the first press takes an end, which is what makes a panel
reachable from a standing start and is the entire defect in one sentence. A `current` that
is no longer in the list - a track that was deleted underneath the selection - is treated
as a fresh start rather than an error. It knows nothing about tracks, so it is not
business logic in TypeScript.

Verified by driving the running window: four `ArrowDown` presses and `Enter` opened the
fourth project; three `ArrowUp` presses walked the selection back up three rows, stepping
over the reason rows rather than onto them; in the Tracks panel two `ArrowDown` presses
chose `A2 Hall of Mirrors` while three `Shift+ArrowDown` presses chose the third boundary
independently, and the evidence pane followed. Every panel's selected row also scrolls
itself into view with `block: "nearest"`, because arrowing down a long library otherwise
walks the selection out of the viewport.

### The window can describe a project the shell does not have open

The worst of the three, and nothing in the UI said a word about it.

`transport::open_project` validated a path with `Project::open_read_only` and then
committed it. A pre-WP-13 project passes that check completely: it is a real `.vcw` with
the right `application_id`, and `identify()` tests `user_version` only for being *too
new*. So the path was committed, and then every §29 read against it failed with
`no such table: releases`, because in v1 there is no such table.

Those five reads were an unhandled `Promise.all` in `store.ts`. One rejection meant
`setProject` was never reached, so the window carried on showing the **previous** project
- its title, its track list, its boundaries, its waveform - while `shell.project` pointed
at the new one. The status line said `Ready.`

That is not a cosmetic divergence. Every edit verb resolves against the shell's path, so
`place_marker`, `split_track` or `export` would have gone to the file nobody was looking
at. On a library of real rips that is an edit to the wrong record, and the operator's only
clue would have been a title bar they had no reason to doubt.

Three changes close it:

- `Project::require_current_schema` (`crates/project/src/sqlite.rs`), raising a new
  `Error::SchemaNeedsUpgrade`. Deliberately **not** called by `open_read_only`: v1 is a
  schema this build reads perfectly well, and everything about a capture - the layout, the
  blocks, the waveform - comes out of it unchanged. Refusing at the door would break the
  readers that are entirely happy. It is asked by the readers that need a v2 table.
- `open_project` asks it, and **upgrades rather than refuses**. §16 says a newer version
  should upgrade an older project without destroying the original, `Project::open` does
  exactly that inside a transaction, and v1 to v2 adds three empty tables and touches no
  audio. The path is committed only once the reads are known to work, which is the
  invariant the bug was the absence of. The decision lives in `ensure_readable`, split out
  so it can be tested without a Tauri `State`.
- `store.ts` catches. A failed read clears to `NO_PROJECT` and puts the reason in the
  status line. An empty window with a reason on it is a bad outcome; a full window
  describing the wrong record is a worse one.

The root's own four-way read in `App.tsx` had the identical shape - one failure left the
devices, the settings, the credentials *and* the library on stale values silently - so it
now goes through `store.run`, which is what puts a reason on screen.

### An amber row of zeros with no reason on it

`browse::summarize`'s doc promises "the row a browser draws grayed out with a reason
beside it". What it actually had was `title={project.problem ?? project.path}` - a hover
tooltip. First light showed two amber rows reading `0 sides / 0 tracks / 0:00.00` against
files holding twenty and six seconds of audio, with no reason anywhere on screen, and no
reason at all reachable by somebody driving this by keyboard.

The reason is now a row of its own under the row it belongs to, spanning the table because
it is a sentence and not a cell. And it is a sentence about the project rather than about
SQLite: `contents` asks `require_current_schema` *before* the reads, so the row reads

> `/data2/vcw-firstlight/six-seconds.vcw was written by an older VCW (schema version 1,
> this build uses 2). Open it to upgrade it.`

rather than `no such table: releases`. Opening it then does what the row says. Verified
end to end: `twenty-seconds.vcw` and `six-seconds.vcw` both went from `user_version` 1 to
2 by four arrow presses and `Enter`, their amber rows healed, and their lengths filled in
as 0:20.01 and 0:06.02 where both had read 0:00.00. Sides and tracks stay at zero, which
is correct - a capture-only project has nothing analyzed in it yet.

Opening a project now also re-reads the library, through a new `onLibraryChanged` prop.
`store.reload` only re-reads the *open* project, so without it the row a person had just
fixed went on saying it needed fixing until something else refreshed the list.

### Two cosmetic ones, and what the second one was really about

The export panel's tickboxes sat a finger's width from their labels. The cause was
`label input { min-width: 18ch }`: a checkbox got a box eighteen characters wide with the
glyph drawn at its left edge. Worth recording because the first fix was **wrong** -
`padding: 0` on the checkbox, which is also true and changed nothing visible, and the
screenshot afterwards said so. The gap was measured off the frame at ~65 CSS pixels, which
is 18ch at 13px monospace, which named the real rule.

The `Into` field was too narrow for its own placeholder and clipped it mid-word, which
reads as a truncated value rather than a hint. It asks for the width with a class rather
than every text field being widened, because the metadata panel's four short fields fit on
one row as they are.

### What WP-16a leaves behind

- 914 Rust tests and 24 frontend tests, all twelve gate legs green.
- One new error variant, one new project method, one new frontend module, one new gate
  assertion, eight new bindings.
- The Metadata panel's arrows are covered by `select.ts`'s tests and by the wiring
  assertion, but were **not** exercised in the running window: reaching a candidate list
  means a live lookup against MusicBrainz or Discogs, and that is not a call to make
  unasked. The other three lists were driven by hand.
- **Nothing here was found by a test.** Every one of the five came out of opening the
  window and pressing keys, on a tree that was gate-green at 910 tests when the session
  started. That is the finding behind the findings, and it is the argument for putting
  WP-17's harness next rather than more features.

## Phase 1 - WP-17, the test corpus and soak harness

Built 2026-09-28. §41 lists twelve kinds of test and the work package's exit criterion is
"nightly CI job; regressions fail the build". Before writing anything, the existing
`vcw soak` was run against its own `--fast` flag to see what was already there. It failed,
which set the shape of the whole day: **every gate added here was run in anger before it
was wired to a verdict, and three of them found something.**

### `--fast` had never passed, and could not have

The flag's doc comment called it a smoke test. A first run of it reported **3,578,279
overruns, 1,717,573,920 dropped frames** and a byte mismatch at frame 48,000.

The cause is not a bug in the writer; it is the pace being unverifiable by construction.
`Pace::Fast` runs the simulated feeder flat out and the ring overruns almost immediately.
§10's contract says a full ring costs the **whole** callback, so the writer's frame index
stops agreeing with the source's: written frame *n* holds the sample the source generated
for some later frame, and `Simulated::expected_sample(frame)` is the wrong expectation for
every frame after the first drop.

The first fix considered was to make `Pace::Fast` apply back-pressure. That would have
been wrong, and grepping the eleven `Pace::Fast` sites is what showed it:
`capture_path.rs`'s `a_reader_that_falls_behind_overruns_instead_of_blocking_the_producer`
uses that pace *precisely because* it drops, and changing it would have disarmed the test
protecting §10 without failing anything. So the change is additive - a third pace:

- **`Pace::Metered`** waits for room instead of overrunning. No device behaves like this,
  and `Sink::on_data` must still never block, which is why the waiting is done on the
  feeding thread and never in the sink. It is the only pace that is both fast and
  verifiable: driven by the writer's own drain rate, nothing is dropped, the written frame
  index still equals the source frame index, and every byte can therefore be recomputed.
  It measures throughput and never latency.

`Capture::free_bytes()` is the accessor it needs, documented as not being for a callback's
use. `--fast` now means metered, and the output says "metered" rather than "fast" so the
distinction is visible in a log.

The paired test asserts both arms, because a metered run reporting no loss proves nothing
on its own - it is also what a machine fast enough to keep up would look like at
`Pace::Fast`. The claim is that the two paces *differ*, so the dropping arm has to drop
for the test to mean anything, and it is asserted rather than assumed.

### A device that goes silent was filed as a flawless capture

This is the defect of the day, and the fault harness found it within minutes of existing.

`--vanish-after` models R9's real shape: a USB interface that stops delivering **without
saying anything**. Not an error, not an empty callback - silence. Run it, and the capture
was recorded as `finalised`, with `is_clean()` true and all four counters at zero.

It is a structural hole, not an oversight. `Diagnostics` has exactly four counters -
`overruns`, `underruns`, `dropped_frames`, `stream_errors` - and every one of them counts
an event that *happened*. **None of them can describe data that never arrived.** Then
three separate call sites (`core::engine::ending`, `cli::capture`, `cli::soak`) derive the
capture's final state from `diagnostics.is_clean()` alone, so all three agree on the wrong
answer. An operator whose interface dropped out ten seconds into a side would have been
handed a ten-second project marked complete and flawless.

Fixed in the writer, which is the only component that is waiting for bytes and can
therefore notice their absence:

- `Config::stall_millis`, default 2,000. Zero disables it.
- `Progress::stalls` / `Progress::has_stalled()`, with a private `stalled_now` latch so
  an episode is counted once rather than on every poll for the rest of the run.
- `Outcome::stalls`, and a state **downgrade** at the single place the capture row is
  finished: a stalled run is written as `interrupted` whatever the caller asked for.

Overruled in the writer rather than returned for the caller to apply, because three
callers already computed this from `is_clean` and a fourth would have too. The clock starts
at spawn on purpose, so a device that opens and never delivers at all is caught as well. A
paused writer does not trip it, because a paused writer still drains a live device.

The test has a control arm at `stall_millis: 0` proving the old answer comes back, which
is what makes it a test of the watchdog rather than of the source.

### WAL contention: the instrument was the problem, and the WAL was ungated

§41 asks for a database contention test. The thing worth contending over is what a user
actually does during a recording - watch the waveform - so `cli::contend` runs reader
threads issuing the same `project::waveform` queries the window issues, at three zoom
levels, against a read-only connection opened on the live project.

The first run looked like a serious defect: **WAL peak 89.78 MiB on a 24-second capture**
against a 4 MiB budget, zero checkpoints completed, and the soak printed `pass`. The
mechanism is real and specific - a checkpoint needs every reader gone to reclaim WAL pages,
so continuous readers starve it - and it is invisible, because it shows up as a file size
and not as an error anywhere.

But four readers looping flat out issue about **11,000 queries a second**. That is not a
window, it is a fuzzer. So the readers were given a redraw rate and the question was
measured rather than argued:

| readers | redraws/s | WAL peak |
|---|---|---|
| 0 | - | 4.94 MiB |
| 4 | 10 | 4.94 MiB |
| 4 | 60 | 6.56 MiB |
| 1 | flat out | 10.88 MiB |
| 4 | flat out | 42.6 - 89.8 MiB |

At a realistic 60 Hz the overshoot is 1.6 MiB on a 4 MiB budget; at 10 Hz it is
unmeasurable. **Commit latency was unaffected in every arm** (p50 ~6 ms, max ~30 ms), which
is WAL mode delivering exactly what it promises: readers do not block the writer. So this
is not a product defect - and the day's second finding is the one that remains:

**nothing checked the WAL size at all.** A run that reached 90 MiB against a 4 MiB budget
printed "pass: zero loss, bounded WAL, every byte accounted for". `Outcome::wal_within
_budget` now gates it, at four times the configured budget by default, always true under
`Checkpoint::Never` because that policy grows the WAL by design. `--reader-hz 0`
reproduces the unbounded case on demand and is *expected* to fail the gate, which is what
makes it a demonstration rather than a claim.

### Memory growth is a number, then a gate

Nothing in `crates/` sampled RSS. `Growth` now reads `VmRSS` from `/proc/self/status` on
the progress tick that already exists - `/proc/self/statm` would have needed a page size,
which means either a `libc` dependency or an assumption that is wrong on aarch64 with
16 KiB pages.

The baseline is the first sample taken at least five seconds in, not the first sample:
resident size climbs while the binary faults in and SQLite allocates its page cache, and
measuring from zero would report a leak on every run. A run too short to have a baseline
reports that it has none rather than reporting zero growth.

The evidence for the threshold came off the real soak rather than out of the air. A
70-minute 24/192 real-time capture on media2026 measured **42,672 kB at 15 minutes and
43,224 kB at 70 minutes** - about **0.5 MiB an hour**, on a run writing 2 GB. That same
run went on to 90 minutes and was **still 43,224 kB at minute 85**, so the half-megabyte
is early settling and not a slope. The gate is
set at 32 MiB of growth, which is roughly sixty hours of headroom.

### File-backed capture, through a verb

`Pattern::File` existed and no verb reached it, which is why the feeder for real rips has
been an out-of-tree `realrip/` directory since S1. `vcw soak --from-file` retires it.

Two things had to change. `Pattern::File` was a whole-file reader, and a real rip is a
container: the audio starts 44 bytes in **at the earliest**, and what follows the audio is
metadata that would arrive as a burst of noise at every wrap. So it now carries a byte
range, clamped to the file's real size rather than trusting the container's declared
length - a rip that was cut short still declares the length it meant to have.

Finding that range is `cli::wavfile`, and skipping 44 bytes is right often enough to be
dangerous. A 24-bit file has a 40-byte `WAVE_FORMAT_EXTENSIBLE` `fmt `; a tagged rip
carries a `LIST` or `id3 ` chunk that can come **before** `data`. Guessing wrong offsets
every sample in the run and still verifies clean, because the verifier would be comparing
the wrong bytes against themselves. So the chunks are walked, the pad byte on an
odd-length chunk is honored, and `data` before `fmt ` resolves rather than failing - a
case that legal, rare, and one the first implementation got wrong until its test said so.

The file's rate, channel count and format **win over the flags**, because reinterpreting a
48 kHz 32-bit rip as 192 kHz 24-bit would produce a project that verified perfectly
against the wrong bytes. Every WAV in `/data2/source_rips` turns out to be 32-bit PCM at
48 kHz with format tag `0x0001` rather than `0xFFFE`, which the spec does not ask for
above 16 bits - so the reader accepts both tags. Refusing them would have refused the
corpus.

`verify()` grew a file arm, and it re-reads the file rather than trusting anything the run
produced: the feeder is a byte stream with a wrap, so stream byte *p* is file byte
`offset + p % length`, and a block starting at frame *f* starts at stream byte
`f * frame_bytes`. One read per block, not one per sample. Shifting the expected offset by
a single byte makes it fail at frame 0, which is how it was checked.

Two real rips have been through the path end to end: `Background_Memory Card.wav`
(32:24) and `Charlatan_Equinox.wav` (42:19), 162.9 MB and 99.6 MB verified byte for byte.

### The verifier has to stop at a loss, and that is the claim being made

`--starve-after` failed for a reason worth recording. A starved device delivers one empty
callback, and `source.rs` skips the generator while still advancing the frame index - which
is correct, because the audio for those frames **never existed**. The written stream is
contiguous while the source's index has a gap, so every frame after the starve is shifted
and the byte comparison reports a mismatch on all of them.

Comparing past a loss does not detect the loss - the loss is already counted as an
underrun. So a run that injected one verifies up to the loss and stops. That is not the
weaker check it looks like: "the damage was confined to the fault" is exactly the claim
being made about a fault run, and this is what makes it checkable rather than asserted.

### The harness, and the exit criterion

`scripts/soak-harness.sh` holds the legs and the reasons. One script rather than a list of
steps in a YAML file, so that the thing CI runs is the thing a person can run on the
machine where the failure happened; a harness only reachable through a workflow file is a
harness nobody reproduces. Every leg ends in `vcw soak`, which exits non-zero on its own
verdict, so the gates live in the binary and the script only chooses the runs.

`short` is eight legs in **1 minute 32 seconds**: `clean`, `contention`, the four faults,
a synthesised WAV, and the corpus when `VCW_RIP` names one. The synthesised WAV is written
by the script rather than committed, and it deliberately carries a `LIST` chunk in front
of its audio - a CI runner has no vinyl corpus, but the property being checked is that
whatever bytes the container holds come back out of the project unchanged, and synthesised
bytes test that as well as recorded ones do.

`nightly` is two real-time hours: one clean, one with four readers at 60 Hz. The pairing
is the point, because it is the only way to attribute a difference in WAL peak or commit
tail to the readers rather than to the runner.

`.github/workflows/ci.yml` gained `schedule: '0 4 * * *'` and two jobs - `soak` on every
push, `soak-nightly` on schedule and manual dispatch only. Both keep their logs as
artifacts `if: always()`, and both build `--release`, because a soak spends its time in the
deinterleave, the summaries and the CRC and a debug build of those changes what the run is
measuring. Every leg runs at 48 kHz: a 24/192 stereo capture is 1.15 MB/s and an hour of
it is 4 GB, which is most of a runner's free space, and none of the properties these legs
check depend on the rate.

The nightly's timing numbers are worth reading and not worth trusting. A hosted runner
shares its CPU and its disk, so a commit tail measured there says as much about the
neighbors as about the writer. The commit budget stays on because 250 ms against a ~25 ms
p99 leaves an order of magnitude of headroom - it takes a genuinely pathological runner to
breach it, and that is worth seeing too. **The real-time numbers of record come off the
rigs, not off CI.**

### The harness cannot pass by not testing

A fault run's pass condition is the *opposite* of a clean run's: the fault has to show up,
and then the damage has to stop at it. That inversion is easy to get wrong in a way that
makes everything green, so it was checked from both ends:

- `--starve-after 99999`, past the end of the run, correctly **FAILS** with
  `NO UNDERRUN COUNTED`, and the harness script exits 1 with the reason visible.
- `if false && pace == Pace::Metered` makes the metered test fail with "a metered source
  overran; it is supposed to wait for room".
- Faking 100 MiB of growth flips a passing run to `FAIL` with the memory line showing it.
- Shifting the file verifier's offset by one byte fails at frame 0.

Every one of those was run, reverted, and re-confirmed green.

### Storage, on something other than this box's SATA disk

media2026 (Ryzen 9 5900XT, NVMe) ran the same configuration this machine has been measured
on all along. Identical 0.15-minute metered soaks, 48 kHz S24 stereo:

| | rtf | commit p50 / p99 / max | prepare p50 / max |
|---|---|---|---|
| dev box (SATA) | 32.5 | 4.6 / 24.0 / 42.1 ms | 1.5 / 4.9 ms |
| media2026 (NVMe) | 48.3 | 3.7 / 9.1 / 15.9 ms | 0.3 / 0.6 ms |

The 90-minute 24/192 real-time run on it passed, and it is the longest verified capture
the project has:

```
  ran         5400.0 s wall, 5400.1 s of audio, real-time factor 1.00001
  written     1036815360 frames, 43202 blocks, 5.79 GiB of samples in 21601 commits
  commit      p50 4.3 ms, p95 9.3 ms, p99 11.7 ms, max 17.0 ms, budget 250 ms
  prepare     p50 1.3 ms, max 2.6 ms (deinterleave, summaries, crc)
  wal         peak 5.25 MiB, 0 writer checkpoint(s)
  counters    0 overruns, 0 underruns, 0 dropped frames, 0 stream errors
  validate    clean
  bytes       every one of 6220892160 matches what the source generated
  verdict     pass: zero loss, bounded WAL, every byte accounted for
```

**The worst commit was 17.0 ms and it was already 17.0 ms at minute 15**, so the
distribution is stationary over the whole run rather than degrading; against **102.6 ms**
on this box's SATA disk that is a sixfold difference in the tail and no difference at all
in the verdict. The WAL held 5.25 MiB across ninety minutes without the writer ever
needing a checkpoint of its own, and **6.2 GB of samples were verified byte for byte** -
the largest readback the project has done by two orders of magnitude. D3's 250 ms budget
is not close to being the binding constraint on either disk.

## Phase 1 - the CI repair, 2026-09-28

**CI had never been green.** Every one of the twenty runs GitHub still holds, back to
2026-09-25, failed - while the local gate was green on every one of those commits. The two
disagreed for three days and nothing said so, because nobody looked: the gate was the
thing being trusted and CI was a notification nobody had opened.

That is the finding. The five causes underneath it are almost incidental by comparison,
but three of them were real defects and two were shipped the day before.

### The local gate could not see what CI sees

This box was on stable 1.94.1. CI runs `dtolnay/rust-toolchain@stable`, which was 1.98.1 -
four releases and about six months of clippy lints ahead. A lint added in that window
cannot fail a local run by construction, so `chunks_exact_to_as_chunks` sat unseen here
while it turned CI red on every push.

The fix is a leg, not a one-line edit. **`toolchain` is now leg zero** and fails when
`rustup check` reports stable behind, with the note that CI runs the newer one; when
`rustup check` cannot reach the network it reports `SKIPPED` rather than blocking a commit
offline. Two more legs close the rest of the gap between the gate and CI's job list:
**`msrv`** (`cargo +1.90 check --workspace --all-targets`, mirroring CI's own MSRV job, so
an API newer than the promise fails here first) and **`spikes`**, which had no local
counterpart at all.

**Leg zero earned its place immediately.** Once the toolchain was current, clippy found
**four** lint sites rather than the one CI had reported - `crates/types/src/summary.rs`,
`crates/export/src/encoder.rs`, `crates/export/tests/from_a_project.rs`,
`crates/metadata/src/musicbrainz.rs` and `crates/contract/src/browse.rs`. CI stops at the
first crate that fails to compile, so its log named `vcw-types` and hid the other three
crates completely. **A red CI tells you less than a green local gate, and the fix for that
is not to read the log harder.** Two of the five are `as_chunks`, which also dropped an
intermediate copy; two are `sort_by_key(Reverse(..))`, same stable order.

### The memory gate was live on one platform in three

`Growth` reads `VmRSS` from `/proc/self/status`. On Windows and macOS that read simply
failed, `sampled` stayed false, no baseline was ever taken, and `within` returned true -
so `--max-growth-mib 32` reported "not measured" and the run printed **pass**. WP-17's
memory gate existed on Linux and was silently absent on the other two Tier 1 platforms.

This is the exact failure the WP-17 section argues against, shipped inside WP-17, one day
later. Writing down "a gate that has only ever been seen green is indistinguishable from a
gate that cannot go red" does not make the next gate honest.

The fix refuses rather than pretends. `resident_bytes` is `#[cfg(target_os = "linux")]`
with an explicit `None` arm, and **`check_growth_gate` makes an unmeasurable gate an
error**, naming `--max-growth-mib 0` as the remedy:

```
this platform cannot report resident memory, so --max-growth-mib 32 cannot be
honored. Pass --max-growth-mib 0 to soak without the memory gate; every other
check still applies.
```

It is checked before the project is created, like the input file, so a run that cannot
honor what it was asked costs nothing to find out. `Growth::sample_at` splits the reading
from the policy, which is what lets the baseline logic be tested on a machine with no
procfs - and that test failing on Windows and macOS is how the whole hole surfaced.
`a_memory_gate_the_platform_cannot_honor_is_refused` covers all three cases: refused when
unmeasurable, allowed when not requested, allowed when measurable.

**Unverified:** the refusal has never executed on Windows or macOS. It is unit-tested and
CI will be the first to run it.

### The export verifiers were absent, and would have stayed absent on Windows

`at_least_one_verifier_is_installed` failed on both Linux runners. **The test was right.**
It exists so that the thirteen tests which read our exports back with somebody else's
decoder cannot silently skip, and the runners had no `flac` and no `ffmpeg`. CI now
installs them per platform.

Fixing that exposed a second defect in the same file. `tool()` joined a bare name onto
each `PATH` entry, so on Windows it would never find `ffprobe.exe` however many verifiers
were installed - **every third-party check in that suite was unreachable on Windows**, and
the only test that would have said so is the one that fails when none is found. Same shape
as the memory gate: a helper that works on the developer's platform and quietly disables a
whole class of verification elsewhere. It now tries the `.exe` suffix. The Windows step
installs ffmpeg alone, since it carries `ffprobe` and one package is one thing that can go
wrong.

Windows had been failing before it ever reached these tests - `cargo test` stops at the
first failing binary, and `vcw-cli`'s came first - so this one was queued behind the
memory gate and invisible until it was fixed.

### The soak job was red by construction

Every correctness claim passed on the runner: zero loss, every byte verified, WAL bounded,
memory flat, 3,438 reader queries covered. The job failed on **commit latency alone** -
736.1 ms against a 250 ms budget on the `clean` leg.

That number is the runner, not the writer. `PROJECT_PLAN.md` already said so before the
first run: "a hosted runner shares its CPU and its disk, so the nightly's endurance and
correctness results stand and its commit tail does not." **The caveat was written and the
job was left asserting the thing the caveat denies.** Documenting a limitation is not the
same as handling it.

`vcw soak --ignore-commit-budget` reports the latency without gating on it, and the report
line says so where nobody can miss it:

```
commit      p50 3.5 ms, p95 8.6 ms, p99 15.4 ms, max 49.0 ms, budget 5 ms (NOT GATED, so this is not a timing claim)
```

The JSON carries `"gated": false` beside the percentiles for the same reason. It is the
only optional gate in the verdict: loss, byte fidelity, WAL bound, memory and reader
health all still apply on a shared box, because none of them depend on who else is using
the disk. `VCW_SHARED=1` turns it on, both CI soak jobs set it, and the same variable also
sets `--max-growth-mib 0` on non-Linux, where the gate would now be refused.

Checked from both ends, which is the standing rule: at a 5 ms block budget the same run
**fails** with the gate on (52.0 ms, exit 1) and passes with it off, labeled.

### The spikes job had never passed on a clean checkout

`tauri::generate_context!` resolves `frontendDist` at compile time and panics if the
directory is missing. The IPC spike's `dist/` is a build artifact and gitignored, so it
exists on the machine that built it and nowhere else. Both CI and the `spikes` gate leg
now write a placeholder `index.html` first; nothing there runs the window. A fresh clone
needs the same `mkdir`, which is now the gate's job rather than folklore.

### What this changes about the gate

Fifteen legs, and the rule behind the list is new: **the gate must have a counterpart for
every CI job.** It had eleven against CI's thirteen, and the two missing ones were exactly
where the rot was - the toolchain that decides which lints exist, and the spikes workspace
that only compiles where it was built. The em-dash sweep and the test tally are unchanged.

A green gate now costs one `rustup` check more than it did and means considerably more
than it did.

## Phase 1 - what the first green-path CI run found, 2026-09-28

The CI repair went in at `e7cd249` and the run that followed it was still red, which is
the point of it. Everything the repair had aimed at worked: the export verifiers
installed on all four runners, `soak harness (short)` passed with `VCW_SHARED=1` where it
had failed on commit latency, `spikes still compile` passed on a clean checkout, and
`fmt + clippy`, `msrv 1.90`, `cargo deny`, `core has no UI dependency` and
`TypeScript matches Rust` were all green. What failed was `cargo test --workspace` on
three of the four platforms, each for a different reason, and only `linux-x86_64` - the
platform this is developed on - passed.

**Four defects, two root causes, and the shape of both is the one this project keeps
finding: a number calibrated on the dev box, asserted everywhere.**

### Windows could never have run a debug build of this binary

`test (windows-x86_64)` failed three `detect_from_cli` tests with the same message, and
the message was the spawned child's rather than the test's:
`thread 'main' has overflowed its stack`.

Windows reserves **1 MiB** for a process's main thread against 8 MiB on Linux and macOS,
and the size lives in the executable header rather than being asked for at run time. A
debug `vcw --version` wants between **1.0 and 1.5 MiB before it has parsed an argument**,
because clap's derive expands an `augment_subcommands` function per subcommand enum that
builds every `Command` and every `Arg` as a local, and unoptimized they are all live at
once. `gdb` puts the fault in `augment_subcommands`, five frames under `main`, with
nothing of ours in between.

Reproduced here in one command, which is the part worth keeping: `ulimit -s 1024` is
Windows' ceiling on a machine that is not Windows, and the same binary that runs at
`8192` aborts at `1024`. A release build of the same commit runs in **256 KiB**.

So this was never a defect in a shipped binary, because releases are what ship. It was
that **no integration test that spawns this binary could pass on Windows**, and because
cargo stops at the first failing target, the rest of the CLI suite never ran there at
all. Three tests were visible; the true count was unknown and still is.

`main` now runs the CLI on a thread with a stack it asks for, 8 MiB, which is what the
other two platforms hand a main thread anyway. A thread stack is mmapped, so it answers
to neither the executable header nor `RLIMIT_STACK` - which is why this fix is testable
where raising the header with the MSVC linker's `/STACK:` would not have been.
`crates/cli/tests/startup.rs` is the gate: parsing and a real subcommand under a
1024 KiB stack, plus `the_shell_really_does_lower_the_stack`, which fails if `sh` ignores
the limit rather than letting the other two pass on 8 MiB of headroom. Verified from both
ends - `main.rs` reverted, both tests fail quoting the overflow, restored, both pass.

### The recovery test charged process start-up to recovery loss

`test (linux-aarch64)` failed one test:
`recovered only 0.750 s of a 2.626 s capture; the floor allows 1.000 s of loss`.

`kill_at` started its clock at `spawn` and allowed `BLOCK_MILLIS + 750 ms`, the 750 ms
being start-up slack "for a Pi booting this off an SD card". On a cold `ubuntu-24.04-arm`
runner start-up was about **1.9 s**, so a test about commit granularity was measuring
process launch.

The clock now starts when the writer says it is capturing - `vcw soak` prints its banner
once `persistence::spawn` has returned - so there is no start-up term left to guess at.
The allowance is **one commit block, 250 ms**, which makes the assertion *tighter* than
the one that was failing rather than looser: the measured shortfall on this host is
0.126 s. The bound is two-sided, because the ring is already filling while the project is
created and the banner comes after that, so the stored audio can begin up to a ring's
worth before this clock starts. That is a ceiling on overstatement, not on loss, and
S1's conclusion that the ring is not part of the loss is untouched.

### The test was piping the child's stderr and throwing it away

`test (macos-aarch64)` failed three tests, and all three accusations were wrong:
`a killed writer should leave a hot log, found Sidecars { wal_bytes: 0, shm_bytes: 0 }`,
`no hot log to dry-run against`, and `no such table: capture_blocks`.

`start()` set `Stdio::piped()` on stderr and never read it. `Child::kill()` succeeds on a
process that has already exited and `status.success()` is false either way, so
**a writer that died during startup was indistinguishable from one that was killed** -
and the assertions then blamed the writer for the sidecars it never got as far as
creating. There is now a `still_running` check before every kill and `stderr_of` on every
failure path. Verified by pointing a project at `/proc`: the assertion now reads
`unable to open database file` instead of accusing the writer.

**This makes macOS legible, it does not fix macOS.** What `vcw soak` does wrong on
`macos-latest` is still unknown, and the next run is what will say.

### A test that could pass by doing nothing

`a_capture_killed_before_it_committed_anything_still_recovers` killed 120 ms after spawn
and returned early if the project did not exist yet, on the reasoning that a machine too
fast for the case under test should not fail. On the runners it is not a fast machine, it
is a slow one, and the early return is a silent skip that asserts nothing - the same
shape as WP-17's memory gate reporting "not measured" beside "pass". It now kills on the
readiness signal, which is after `Project::create` and a full block before the first
commit, and asserts the project exists rather than shrugging.

### Where that leaves it

Gate green at **931 tests**, three of them new, zero em dashes. Two of the four findings
are fixed outright, one is a measurement that was wrong and is now right, and one is a
diagnosis tool rather than a fix. Nothing here says the next CI run is green: Windows only
ever showed its first failing test target, and macOS has not yet been asked the question
in a form that produces an answer.

## Phase 1 - WP-20, Audacity import

Built 2026-09-28. Spike S5 had already decoded the format; this is the port, and the
thing it exists to prove is not the parser. It is that **an Audacity project becomes an
ordinary VCW capture**, so that every verb built between WP-04 and WP-14 works on twenty
years of somebody else's rips without being told where the audio came from. The work
package was taken next for exactly that reason: signing it off exercises most of the
toolchain in one pass.

Four steps, in order: the grammar in Rust, a fixture shrinker so CI has real Audacity
bytes to read, the landing, and the round trip back out to a tagged file.

### `vcw-import`, the twelfth crate

The clean-room constraint from S5 holds: the grammar was derived from file bytes and no
Audacity source was consulted, and the port was made from the spike's notes and the
Python probe, not from anything upstream.

The reading half is `sniff` (which version, from `user_version` and not from the
extension), `doc` (the dictionary and the record stream), `model` (elements to clips,
labels and tags), `read` (the two together) and `audit` (every block reference resolved
against `sampleblocks`). All 30 corpus projects parse with **every byte of `project.doc`
consumed and zero dangling block references**, and each one is diffed against
`spikes/aup-format-probe/probe.py --json`: the oracle is an independent implementation of
the same grammar, so a disagreement means one of the two is wrong and neither gets the
benefit of the doubt.

The refusals are part of the deliverable rather than a fallback. Tags `00 09 0B 0D 0E`
never occur in 30 real files, so they are rejected rather than skipped; a file with no
`application_id` is refused from its header rather than from a failed query, because the
message a user sees for "this is not an Audacity project" should say that.

### The fixture shrinker, which deletes rather than writes

CI cannot hold a 271 MB project, and a fixture written here would prove only that our
encoder agrees with our decoder - which is the mistake the corpus tests exist to avoid.
So `fixture` shrinks a real project by **deleting byte slices**: the dictionary, the
element structure and every attribute except the four it has to rewrite are bytes
Audacity produced. `clips.aup3` is 68 KB, `clips.aup4` 72 KB and `rate-trap.aup3` 320 KB,
and between them they carry both page sizes in use, both generations, a shared sample
block, AUP4's `0x10` thumbnail blob and the rate trap. Five tests read them in CI.

What a fixture cannot carry is audio - a repository is no place for somebody's commercial
vinyl - so the sample bytes are zeros and the corpus tests cover decoding against the
real files.

### The landing: re-blocked, not adopted

This was the decision of the work package. The obvious implementation is to adopt
Audacity's `sampleblocks` rows into `capture_blocks` and copy nothing: the blocks are
already 1 MiB of mono PCM and already immutable, which is what the schema wants.

It does not work, and the reason is a trim. `waveclip/@offset` is the **sequence origin,
not the audible start** - the audible span is `[offset + trimLeft, offset + numsamples /
rate - trimRight]` - so a clip's first audible sample almost never falls on a
262,144-sample boundary. An adopted block would need a per-block sample offset that
`capture_blocks` has no column for and `validate()` no way to check, and every reader in
the project - playback, export, the waveform pyramid, recovery - would need a special
case for audio that came in rather than was recorded.

So import **re-blocks the assembled timeline through `persistence::Writer`**, the same
writer a live capture uses. It costs one copy of the audio at import time and buys
structural identity: an imported capture is a capture. The three comments in
`vcw-project` that had anticipated adopt-in-place were reconciled rather than left in
tension, and `capture_blocks.blockid` now records the argument in place.

Four things the timeline assembler has to get right, all of them asserted:

- **Offset is the sequence origin.** Trims are rounded to samples, not truncated, and
  document order is not even offset order.
- **Gaps are load-bearing silence.** `capture_blocks.sequence` is documented contiguous,
  and closing a gap would slide every label off the audio it names. So a gap is written
  as zeros, which is silence in all three stored formats.
- **No sample is converted.** D4 holds on the way in: `StorageFormat::Int24Padded`
  (`0x00040001`, four bytes) keeps Audacity's own 24-bit layout rather than narrowing it.
- **Blocks are shared, so they are read and not moved.** One 3-way shared block in the
  clip-split project is read three times, and a test says so.

`CaptureMode::Imported` was added rather than picking a plausible default. The other
three variants all answer "how did VCW ask for this device", which an import never did;
recording it as `Shared` or `Native` would be the sort of default that later reads as a
measurement, and the capture row is the one place a reader looks to find out where the
audio came from. It cannot claim bit-perfect, because unknown provenance is reported as
"no" rather than inherited.

Labels become locked user boundaries with titles, tags become the release - `ALBUM`,
`ARTIST`, `YEAR`, `GENRE` through §32's normalization, `COMMENTS` - and every tag is also
kept verbatim under `import.tag.*` so nothing is lost by not having been mapped. What is
deliberately dropped is editor state: gain, pan, mute, solo and envelopes describe how
Audacity was set up to play a project, not what is on the record.

### Two defects the tests found, neither of them in the parser

**A `Timeline` trusted an invariant its input type does not carry.** Clips arrive sorted
when `Project::from_events` parses a document, because the parser sorts them - but
`model::Project` is an ordinary struct a caller can build by hand, and the landing tests
do. The assembler had a `debug_assert!` where it needed a sort and a refusal; a
hand-built project read the wrong clip rather than saying so. It now sorts, and refuses
overlapping clips by name and time.

**A label over deleted audio.** `rate-trap.aup3`'s three labels all sit past the end of
the shrunken audio, which is also the ordinary shape of a project somebody deleted a clip
from and kept the labels of. Landing them would have produced tracks pointing at
nothing. A label beginning at or past the end of the audio is now **reported and
skipped** - reported, because a silent drop is how a user loses track names without
finding out - and one that merely overruns is clipped.

### What the corpus says

Two `#[ignore]`d tests run against the real files. The first lands two projects and then
checks, for every clip on every channel, the head and tail of its audible span against
**the source `sampleblocks` row read directly** at the frame the document puts it at, then
checks the inter-clip gaps are silent. The oracle is the source file, not our reader: a
trim off by one, a block stitched at the wrong offset or a swapped channel all move audio
relative to the timeline, which is what it measures. The second lands both generations of
`simples_test` and compares frames, format, channels and a CRC32 of the whole assembled
audio - the AUP3 to AUP4 conversion is lossless on the audio layer, and this is the
assertion that says so.

Landed projects validate clean with no findings.

### The round trip, which is the exit criterion

`vcw import` has two modes, and the default is to do the work - the opposite of
`vcw recover`, because an import creates a new file and destroys nothing. It refuses a
destination that exists, and `--dry-run` reads the document, audits the blocks and prints
what would land in about a second without writing a byte, which matters on a 612 MB
source.

The dry run always prints both rates when they disagree, which they do in 22 of the 25
corpus projects:

```text
  audio       48000 Hz, 2 ch, Float32
              project/@rate says 192000 Hz; that is an editor preference and is ignored
```

Driven by hand on a real 612 MB rip: imported in 2m04s (debug build), 79,141,433 frames a
channel across 13,192 blocks, three tracks from labels, two tags. `vcw tracks list` and
`vcw release show` then read it with no knowledge of its origin, and `vcw export` wrote
574.2 MiB of tagged WAV in 9.7 s. Asking for FLAC instead produced the refusal it should:
float32 is not narrowable to an integer codec without somebody deciding about headroom,
and 24 of the 25 rips are float32 - so the message names WAV, and arrives before the
first file rather than after half an album.

The exit criterion itself is a test rather than a demonstration.
`both_generations_of_one_project_export_as_the_same_audio` imports `simples_test.aup3`
and `simples_test.aup4`, puts each through `vcw tracks` and `vcw release set`, exports
both as WAV and compares the two sets of files **byte for byte in the audio chunk**,
streamed a megabyte at a time because a side is 419 MB and a test that needs a gigabyte
resident is a test that fails on the Pi. Tags are read back out of the written bytes by
hand rather than with `lofty`, because a reader that shares the writer's idea of the
format cannot catch the writer being wrong about it.

#### What is verified, and what is not

- **Verified:** all 30 corpus projects parse, byte-complete, against an independent
  oracle; real vinyl audio at 24-bit and float32, with trims, gaps and shared blocks,
  lands where the document says, checked against the source blocks; both generations of
  one project land and export as identical audio; an imported project validates clean and
  is read by `tracks`, `release` and `export` unchanged; the rate trap is honored all the
  way into a `.vcw`; the refusals for no audio, mixed rates, overlapping clips, a
  fractional rate, an unknown tag, a non-Audacity file, an existing destination and
  float32-to-FLAC all fire.
- **Not verified:** nothing has imported a project written by an Audacity older than
  3.7.x, because the corpus does not contain one. Import timing is a debug-build
  observation on one machine and is not a budget. `TRACKNUMBER` is read and deliberately
  not mapped, because §32 numbers tracks from their position on the side. And a side still
  has no frame extent, so an import that actually holds two faces in one capture lands as
  one side - the same gap WP-13 recorded.

Tests: 40 lib, 5 fixture, 10 landing and 6 corpus in `vcw-import`, plus 6 for the verb in
`vcw-cli`. Seven of those are `#[ignore]`d and read `/data2/vinyl_rips`, which is the
user's own irreplaceable audio and is opened `mode=ro` every time.

---

## Phase 1 - WP-18, the documents and the diagnostic bundle

Built 2026-09-28. Five deliverables: the format specification (already written at WP-02
and now load-bearing), a third-party reader that proves it, the supported project API,
the user guide, and `vcw bundle`. §42 and §49.

The theme of the work package is that **a document is a claim, and a claim wants a
test**. Four of the five defects found in this pass were in things I had just written
and believed.

### The exit criterion, made permanent

"A third-party tool can read a project using the spec alone" is not a thing that can be
asserted once. `crates/project/tests/third_party_spec.rs` drives `tools/vcw-read.py`
over projects the product wrote and requires the two implementations to agree on what
the project holds, on the bytes of every one of the five storage formats, on where a
track starts and stops, on a damaged block, and on refusing what it should not read.
Two implementations of one document, rather than a document compared with the code that
generated it.

Six deliberate mutations of the Python reader were needed to establish that the five
tests can fail, and the fourth one found that **the half-open-span test could not**: the
mutation "read a span as inclusive" still passed, because the only track being checked
ended at the end of the capture and `read_frames` clamps. The first of two adjacent
tracks has room to be wrong, and now both are checked.

If no Python 3 is on the machine the test panics rather than skipping. A test that turns
itself off would report that the specification is readable on a host where nothing had
read it.

### `vcw bundle`, and building around a negative

§42 asks for a diagnostic bundle that reports version, OS, backend, device
configuration, project integrity and capture errors **without including recorded audio**.
The hard clause is the last one, so the tests are the negatives: a searchable ASCII
marker is written as the capture's PCM and must appear nowhere in the document, raw or
hex; no string value may exceed 4,096 characters; no track title, album, artist or
project path may appear; credentials appear by presence and character count only. Six
mutations of `bundle.rs` proved each of those can fail.

`a_bundle_carries_no_audio` was the second assertion in this pass that could not fail,
and for two reasons at once. Blocks are stored **per channel**, so the marker written
into an interleaved buffer was de-interleaved into alternating halves and never appeared
in a blob; and a 200-frame fixture was under the 4,096-character guard anyway. The
fixture now builds two per-channel streams and interleaves from them at 24,000 frames.

Everything about a project in the bundle is a shape or a count. The table row counts are
read from `sqlite_master` rather than from a list, because the first draft asked for a
`waveform_blocks` that does not exist and reported it as `null` - which reads like an
empty table rather than like a bug in the bundle.

It started at 7.7 MB. Summarizing the device survey took it to 593 KB, digesting each
`capability_fingerprint` to 16 hex characters took it to 128 KB, and compressing long
channel lists to a min, a max and a count took it to **99 KB**, or 2.6 KB with
`--no-devices`. `--all-devices` still writes the whole survey for the case where the
question is about one device's capabilities.

A bundle of a project that will not open is still a bundle: the open failure is recorded
and everything else in the document survives. That is the case the verb exists for.

### Logging, at the seams that diagnose

`tracing` is now a dependency of seven crates and the shell, with the subscriber
installed only by the two binaries, a `--log` flag, a `VCW_LOG` filter that takes a
target each, and `warn` as the default so the JSON-emitting verbs stay pipeable. Logs go
to stderr, never stdout, because several verbs emit JSON documents and a log line in the
middle of one makes it unparseable.

§42's "routine audio callbacks shall not log" is enforced by
`crates/audio/tests/no_logging_on_the_audio_thread.rs`, which reads the source of the
four modules the callback runs through and the bodies of both `on_data` entry points and
fails on any of ten macros. The third test in that file requires the rest of the crate
to log at least four times: a rule that is satisfied by doing nothing is not a rule. The
one logging call on a callback is the host error callback, which is not routine and sits
in code that already allocates, and says so in a comment.

Credentials stay out of the log structurally rather than carefully:
`vcw_metadata::agent::without_query` strips a URL's query string before any log line, so
a provider that one day wants its key as `?token=` does not put it in every line of a
file people paste into bug reports. Import logs `labels_skipped.len()` and not the
labels, because a skipped label is a track title.

### The two documents, and what checking them found

`docs/PROJECT-API.md` is §49's supported surface - read, validate, recover, migrate -
with `crates/project/examples/read_a_project.rs` beside it as a compiled companion that
the gate builds. Verifying the prose against the code immediately found `Plan::Salvage`,
which does not exist; the variants are `DryRun`, `Commit` and `Repair`.

`docs/USER-GUIDE.md` is §50's workflow from both ends, and its two tables are generated
rather than transcribed. `app/src-tauri/tests/the_user_guide_lists_every_key.rs` parses
`app/ui/src/keymap.ts` and requires the guide to list every chord, label, scope and §43
suggestion, with no extras. It is a Rust test reading a TypeScript file because vite's
`server.fs.allow` denies reading `docs/` from the UI tests, and loosening a desktop
application's dev-server allowlist to make a documentation check possible is a poor
trade.

`crates/cli/tests/the_user_guide_names_real_commands.rs` checks every `vcw ...` line in
the guide against the binary's own `--help`: every subcommand name, every flag name, and
whether a documented line stopped short of a subcommand that is required. It found three
wrong command lines in the first draft - `vcw export --out` where the flag is `--into`,
`vcw tracks --split 3 --at N` where `split` is a subcommand taking two positionals, and
`vcw metadata release <id>` where the verb is `fetch`. All three read perfectly well.
Structural rather than executed, deliberately: running every documented line would reach
a metadata provider over the network and open an audio device.

### The recovery floor, repaired on the way past

The first full gate run of this work package failed `kill_and_recover`'s random-kill
test at 3.255 s of clock against 3.000 s of recovered audio, over a floor that allows
one 250 ms commit block. It was not recovery. The simulated source is paced by a clock
it does not own, and on a host running the rest of the gate it falls behind real time, so
a floor that treats wall-clock seconds as seconds of audio fails by a few milliseconds
for reasons that have nothing to do with the thing under test.

The bound is now three checks that can each carry a different part of the claim. A block
is committed whole or not at all and a killed writer never flushes the part-filled one,
so **the recovered frame count is an exact multiple of the block** whatever the scheduler
did - that is the timing-free assertion, and it is the one that actually says "commit
granularity". The ceiling stays as it was. The floor is taken against
`ran_for * rtf`, the pacing the writer itself last reported, which is the audio that
existed rather than the time that passed: on an idle host rtf is 0.9998-something and the
bound is the tight one the fifty-kill run established, and on a starved host it relaxes
by exactly the amount of audio that was never made. `--every 1` guarantees a progress
line lands, and its absence is a failure rather than a skipped check.

Three mutations confirmed the three: losing one extra block, keeping a partial block, and
inventing two and a half seconds each fail a different one.

### Where it stands

The gate is green across all fifteen legs at **1013 tests**, up from 991. New files:
`crates/project/tests/third_party_spec.rs`, `crates/project/examples/read_a_project.rs`,
`crates/cli/src/bundle.rs`, `crates/cli/src/logging.rs`,
`crates/cli/tests/bundle_from_cli.rs`,
`crates/cli/tests/the_user_guide_names_real_commands.rs`,
`crates/audio/tests/no_logging_on_the_audio_thread.rs`,
`app/src-tauri/tests/the_user_guide_lists_every_key.rs`, `docs/PROJECT-API.md` and
`docs/USER-GUIDE.md`.

## Phase 1 - WP-19, packaging and release

Built 2026-09-28. The work package ships what the previous eighteen built, and
the thing worth recording is that **shipping it is the first exercise that runs
the product the way a stranger will**. Every leg of the gate runs code from a
source tree; a package runs a copy of a build artifact, from a path nobody
chose, against a configuration file. Three of the five findings in this pass
could not have come from anywhere else, and one of them was not in the product
at all.

### What a package contains

The shell was already bundled by `cargo tauri build`. Two things were missing
from it and both matter to a person who installs rather than builds.

The **CLI travels inside the package**. Every workflow in `docs/USER-GUIDE.md`
is a `vcw ...` line, `vcw bundle` is what somebody is asked to send when
something breaks, and until this pass the only way to get either was to install
Rust and compile. It ships as a Tauri `externalBin` sidecar, which wants a file
named `binaries/vcw-<target-triple>`; `tools/stage-cli.sh` is what builds and
names it, adding `--target` only when the triple is not the host and `.exe` for
a Windows one. The deb puts it on `PATH` as `/usr/bin/vcw`, so the guide's
command lines work on an installed system, which is the only state in which
anybody will read them.

The **debug sections came out**. Measured, because the difference is not small:
`debug = 1` in `app/Cargo.toml` produced a 95 MB binary, a 25.7 MB deb and a
104 MB AppImage. `debug = 0` with `strip = "debuginfo"` leaves **16.4 MB**, an
**11.2 MB deb that now also carries the CLI**, and an **88.6 MB AppImage** whose
remaining bulk is WebKitGTK and its dependencies rather than anything of ours.
Stripping the debug sections keeps the symbol table, so a panic backtrace still
names its frames and loses only the line numbers. The CLI keeps its line tables
for a local build and gives them up only for a shipped one, which is what the
new `[profile.ship]` in the root manifest is for: the soak reads backtraces, and
a release profile that is also the shipping profile makes those two needs fight.

Beside those, the small obligations: `libasound2` declared once rather than
twice, because the deb bundler appends its own `Depends` and a duplicate is a
lintian error; the license installed as `/usr/share/doc/vcw/copyright` through
`deb.files`, which is the only mechanism that puts an arbitrary file in a deb;
a real icon set rendered from a real SVG by `tools/make-icons.sh`; `bundle.active`
true and a `csp` that is no longer `null`, which were the three items WP-16 left
queued here.

One trap for anybody building a package by hand: `cargo tauri build` must be run
from **`app/src-tauri`**, which is what the CI job does. From `app/` the CLI
finds the nearest `package.json`, decides the app directory is `app/ui`, and
runs `beforeBuildCommand` from there, so `pnpm --dir ui build` looks for
`app/ui/ui` and the build stops before it compiles anything.

### A four-minute build, checked by eight fast tests

Packaging configuration fails at build time, and the build is four minutes on
this machine and twenty across the matrix in CI. That is the wrong feedback loop
for a JSON file, so `app/src-tauri/tests/the_bundle_ships_what_a_user_needs.rs`
reads `tauri.conf.json` and asks the questions the bundler would: does every
file the configuration names exist, is the CLI actually staged and named for a
triple, is every Tier 1 platform in `targets`, is the identifier one macOS can
use, is the `Depends` list free of what the bundler adds anyway, is the window
locked down, and is the version one number in one place.

Ten deliberate mutations confirmed nine of them. The tenth is the interesting
one: pointing `icon` at a file that does not exist **passes the test and fails
the compile**, because `generate_context!` opens every icon at macro-expansion
time and panics with the path. The loop over icons stays, because it carries
`licenseFile` and the `deb.files` sources too, and the test now says in a
comment that the compiler is the real guard for that one entry.

### The repository was hiding files the build needs

`.gitignore` contained `/tools`, left over from VRipr training material that has
since moved out of the repository. It was ignoring three files that are not
training material:

* `tools/vcw-read.py`, which **is WP-18's exit criterion**. The independent
  reader that proves the format is open was not in the repository.
* `tools/verify-release.py`, which the CI workflow names five times.
* `tools/make-icons.sh`, which is how the icon set is regenerated.

No gate leg could see this, and none ever would have: every one of them runs
against the working tree, where the files are present. A fresh clone would have
failed the `package` job at the checksum step and the `test` leg at the
third-party spec, and the error would have named a missing file rather than a
stale ignore rule.

`crates/cli/tests/every_file_the_build_needs_is_tracked.rs` is the guard. It
collects every path the CI workflow names and every path a Rust source reaches
for, then asks git, for each one, **which rule ignores it** rather than merely
whether one does. That distinction is the whole design: `dist/` and
`gen/schemas/` are supposed to be ignored, because CI builds them, so the test
holds an allowlist of build-output rules and fails on anything else. It checks
"exists and is not ignored" rather than "is tracked", so a new file that has not
been committed yet is legal. Three findings came out of writing it: a path in a
header comment pointing at `/data2/vripr` made `git check-ignore` exit 128, a
deliberately built spike `dist/` needed the allowlist, and `check-ignore -v`
reports the last **matching** rule, which may be a negation, so `!**/tests/fixtures/**`
was read as "ignored" when it means the opposite.

### First run, on the only platform this machine can speak for

The deb was extracted the way an install lays it out, and the result is a
first-run capture that the operating system agrees with: `/usr/bin/vcw` from the
package enumerated the devices, armed one, recorded **589824 frames at 96 kHz
S32 with 0 overruns, 0 underruns and 0 dropped frames**, and `vcw recover
--verify` read every block back clean. That is WP-19's exit criterion for Linux
x86_64, and it is met.

The AppImage launches and draws the real interface. Under the desktop session it
then ignored every synthetic keystroke and mouse click, which looked exactly
like a packaged-build input bug and was not: the session is Wayland, XTEST is
dropped, and `xdotool getwindowfocus` returns the compositor's guard window.
Re-run under `Xvfb`, the same artifact is fully interactive. The finding is
about the harness, and filing it as a product defect would have cost a day.

### Two product defects, found by clicking the package

Both are in the frontend, both were invisible to 1013 tests, and both needed the
application to be running rather than reasoned about.

**A project created in the window was missing from the list until a restart.**
`Browser.tsx` called `store.reload()` after creating a project and not
`onLibraryChanged()`. They are not the same thing: the first re-reads the project
the shell has open, the second re-reads the directory it came from. Creating a
project changes both. The project was open, on disk, and absent from the list it
was created in.

**The first option in the device picker said "Host default" and armed a tone
generator.** `Arm.device: null` means the simulated source, which is how the
transport is driven with nothing plugged in, and the panel offered it under a
label that promises the opposite. A first run therefore drew full-scale meters
from synthetic audio and filed a capture against device `simulated source`. The
option now says **"Simulated source (no device)"**, and a new `defaultInput()`
picks the host's own default input when nothing is pinned, so the honest choice
is also the default one.

Each fix was confirmed in the artifact, not just in the tree: the project-list
one in the rebuild that followed it, and the device one in a rebuilt AppImage
driven under `Xvfb`, whose device field opens on the one device the host calls
its default input while the shipped frontend bundle contains the new label and
no longer contains the old one. That screenshot also shows something to leave
alone deliberately. The
device it selects is named **"Default ALSA Output (currently PipeWire Media
Server)"**, which reads wrong in a capture panel and is correct: `alsa:default`
reports `is_default_input: true` with `input.supported: true`, and the name is
ALSA's own description of the PCM rather than anything VCW composed. Renaming
somebody else's device in our interface would be worse than the confusion.

Both defects are held by tests, and the first of them is **the frontend's first
rendered test**: `Browser.test.tsx`
mounts the panel with `createRoot`, submits the form and counts the calls, which
is not a question source text can answer. `vite.config.ts` now includes
`src/**/*.test.tsx` for exactly that reason.

### The checksum tool, and what a checksum says

`tools/verify-release.py` writes and checks a coreutils-format `SHA256SUMS`,
standard library only and Python 3.8 upwards, because the one moment it is needed
is on a machine where nothing is installed yet. `crates/cli/tests/release_checksums.rs`
runs it: the easy half is the file that verifies, and the seven tests spend their
time on the tampered artifact, the download that never finished, the artifact
nobody signed up for, the pinned digest given out of band, and the sums file the
tool cannot parse, which is an error rather than a reassuring summary over a
file it never looked at.

Run against the real artifacts it reported **eighty unrelated files as
unlisted**, because the sums file was not beside them and the sweep looked at
every file in its directory. It now looks only at things shaped like a release,
and the epilog says out loud that names resolve beside the sums file, which is
where it differs from `sha256sum -c`.

The tool's own documentation carries the limit: a matching digest says the bytes
are the bytes the release was built from, and nothing whatever about who built
them. Anybody who can replace a download can replace the checksums beside it.
That is what the signing steps are for, and `--expect` is how to pin a digest to
one you were told through another channel.

### Signing, and what happens without a certificate

The `package` job signs on macOS and Windows when the repository has the
secrets, and **builds anyway when it does not**, because a release that cannot
be built without a certificate is a release nobody can reproduce. What it must
not do is produce an unsigned artifact quietly, so both branches write a line
into the job summary naming which happened, including what the user will see:
Gatekeeper refusing the bundle, or SmartScreen warning about the installer. The
updater's key is handled the same way and is used only if it is set.

Release notes come from `CHANGELOG.md`, which is new and written for the person
installing rather than the person committing: every entry is something somebody
can see. A tag cut before the Unreleased section is renamed still carries its
notes, and a tag cut after it does not carry them twice. The notes always end
with how to verify the download.

### What this work package cannot close from here

The exit criterion asks for a clean install and a first-run capture on **every**
Tier 1 platform. **Linux x86_64 is done and evidenced above. The rest needs the
rigs.** Linux aarch64, Windows and macOS are built by the `package` matrix and
have never been installed by hand, which is the only thing that counts here, and
the macOS half of the criterion (bundle builds, non-device tests pass) is a CI
claim that CI has not yet made, because **CI has still not been green**.

Two open questions belong to the platforms rather than to the code. **Where the
sidecar lands in an MSI and in a `.app` is unknown**, and neither directory is on
`PATH`, so `vcw` from a Windows or macOS install may need a shim or a documented
full path rather than the `/usr/bin/vcw` the deb gets for free. And
`tauri-build` warns that `STATIC_VCRUNTIME is deprecated; use
build.windows.staticVCRuntime`, which is harmless today and will not be.
### Where it stands

The gate is green across all fifteen legs at **1031 tests**, up from 1013. New
files: `app/src-tauri/tests/the_bundle_ships_what_a_user_needs.rs`,
`crates/cli/tests/every_file_the_build_needs_is_tracked.rs`,
`crates/cli/tests/release_checksums.rs`, `app/ui/src/panels/Browser.test.tsx`,
`app/ui/src/panels/Capture.test.tsx`, `tools/stage-cli.sh`, `CHANGELOG.md`,
`app/.cargo/config.toml`, the icon set rendered from
`app/src-tauri/icons/icon.svg`, and three files that existed but were hidden by
`.gitignore`: `tools/vcw-read.py`, `tools/verify-release.py` and
`tools/make-icons.sh`.

Artifacts on this machine: `VCW_0.1.0_amd64.deb` at 11.2 MB and
`VCW_0.1.0_amd64.AppImage` at 88.6 MB, both in `app/target/release/bundle/` and
both rebuilt after the last fix.

## Phase 1 - closing the loose ends

Built 2026-09-29, after every work package in Phase 1 was. This section is the
things that were carried as "recorded and not fixed" - and the first CI run that
was read properly, which turned out to be the same subject: every finding below
is something a green gate had been asserting for days.

### A playback refusal is now an event, not a warning

Four of the sections above carry the same paragraph. **A playback open failure
arrives as a `capture-warning` coded `playback-failed`, because the open happens
on a thread and the bus has no playback-refused event; the fix is an event on the
bus, not a workaround in the shell.** It was written under WP-15, repeated under
WP-16, and repeated again in `## Next up`. It is now wrong, which is the only
reason this is worth a section: it was the cheapest item left in Phase 1 and it
had survived three work packages.

`Event::Denied` is that event. The name is `playback-refused` on the wire, and it
is **a variant of its own rather than a warning because a refusal is terminal**:
no `Auditioning` came before it and no `Ended` will follow it, so a consumer that
put its transport into a playing state when it asked has nothing else coming to
take it out again. That is the whole argument, and it is why the same failure
*during* playback stays a warning - a verb that fails mid-audition is a mishap in
something that is still running and will still end.

The path is the one the contract's drift tests lay out, and it is worth listing
because it is the shape of every future event: the variant in
`vcw_core::events::Event` with a `name()` arm and a `Display` arm, the wire
variant in `crates/contract/src/event.rs` with its `kind()` and its `From<&Event>`
mapping, the variant count in `crates/contract/tests/wire.rs` raised from 14 to
15, the regenerated `app/ui/src/bindings/vcw.d.ts`, a `detail()` arm in
`crates/cli/src/session.rs` so `--json` carries the fields, the reducer case in
`app/ui/src/store.ts` and the log line in `app/ui/src/describe.ts`. Two of those
are compile errors rather than choices: the frontend's `fold` and `describe`
switches are exhaustive over the union, so a new kind stops the build until
somebody decides what the UI does with it. That is D9 earning its keep.

`Scope::label()` is the small new thing in `vcw-core`. `Scope::describe` answers
in seconds, which needs a `Layout`, which needs the project open - and **the one
moment that is not available is the moment most worth reporting**, because an
audition refused because the project could not be read has no rate to divide by.
So the label answers in frames, and the refusal says `the region 0-480000 in
frames of capture 7: unable to open database file` rather than nothing.

The reducer does two things and both matter: the transport goes back to not
playing, because this event is the only thing that will ever say so, and the
reason is shown, because "the device will not play 96 kHz" is a sentence somebody
can act on. A device that cannot play a capture's rate is refused rather than
resampled (§9, §21), so that sentence is the product working as specified and the
UI had no way to say it.

**Four mutations, four assertions.** Publishing a warning instead of the refusal
fails the shell's new test with the old event quoted in the message; publishing
anything after the refusal fails the "nothing follows it" assertion; removing the
reducer's `playing: false` fails the store test; and removing the reason from the
message fails the test that reads it. **The fifth mutation found a test of my
own.** A reducer test that folds a refusal into the initial state and asserts the
transport is not playing passes with the reducer's reset removed - nothing is
playing in `NOTHING` either, so it asserted the starting state rather than the
fold. It is deleted, and the finding is a comment in `store.test.ts` where the
next person to write one will read it.

### The memory gate was reading a page cache as a leak

The nightly's `hour-contended` leg failed on one line: `memory baseline 19.9
MiB, peak 59.1 MiB, end 56.8 MiB, grew 39.1 MiB of 32 allowed`. Everything else
about that hour was perfect - zero loss, every one of 1,036,846,080 bytes
verified, a bounded WAL, a real-time factor of 1.00000, and four readers making
679,478 queries while it ran.

`cache_size = -32_000` in `crates/project/src/sqlite.rs` is 31.25 MiB of SQLite
page cache, and SQLite never gives it back. It fills over the first minutes of a
run and then stops, which is the shape a bounded cache has and a leak does not.
The baseline cannot absorb it: the baseline is taken one `Growth::SETTLE` in,
which is five seconds, long enough for the binary to fault in and far too early
for a cache that fills over minutes. So the gate was counting a fixed
entitlement as growth, and doing exactly what it was written to do.

**Two hypotheses died on the way to the fix, both of them mine.** The first was
that the four readers were the whole of it. Two six-minute runs on the dev box,
same audio, `--max-growth-mib 0` so the run reports rather than judges, one
variable:

| readers | baseline | peak     | grew     | rtf     |
|---------|----------|----------|----------|---------|
| 0       | 14.5 MiB | 41.7 MiB | 27.2 MiB | 1.00013 |
| 4       | 22.8 MiB | 52.0 MiB | 29.2 MiB | 1.00013 |

The uncontended run grew 27.2 MiB with one connection open. So it is the
*writer's* cache, and the readers are 2 MiB between them. The second hypothesis
followed from the first correction and was wrong in the other direction: I had
already rewritten the allowance as `limit + per_connection * (1 + readers)`,
counting every connection as entitled to the full 31.25 MiB, which allows 187
MiB on a four-reader run and would let a real leak through. The four-reader
measurement is what refuted it.

**Why a reader is cheap.** `Project::open_read_only` does not call
`apply_connection_pragmas`. It sets `foreign_keys` and nothing else, so a
read-only connection runs on SQLite's compiled-in `cache_size` of 2000 KiB while
the appending writer holds fifteen times that. That is worth restating as an
open question rather than a finding, because it is the wrong way round: the
writer appends and rarely re-reads a page, and the connection that would use a
large cache is the one drawing waveforms out of 64 KiB blob pages (WP-09's
finding). Whether the writer needs 31.25 MiB at all - and whether a reader
should be given it - is a tuning question with an `rss` measurement attached,
and it matters most on the Pi 5, where the soak is queued and the RAM is not
free. Nothing here changes either number.

So the gate now states the entitlement instead of tripping over it:
`--max-growth-mib` is a leak margin *on top of* the page cache the run's
connections may fill, which is the writer's `PAGE_CACHE_KIB` (now exported from
`vcw-project` rather than an inline literal) plus 2 MiB for each reader. The
report prints the sum and the parts, so the gate is checkable from its own
output: `grew 1.8 MiB of 72 allowed (32 plus the 40 MiB of page cache this run's
connections may fill)`. The nightly's 39.1 MiB passes that with room, a 300 MiB
leak fails it, and a unit test pins the arithmetic so a changed `cache_size` is
a red test rather than a quietly different gate. One of those assertions exists
only to make the 187 MiB mistake impossible to make twice.

**And the flat gate was never as strong as it looked.** 27.2 MiB of cache
against a 32 MiB budget leaves under 5 MiB, so on that gate a verdict turned on
where the baseline happened to land. The 90-minute soak passed it because the
run plateaus at about 42 MiB and then grows half a MiB an hour, so an hour-long
leg spends most of its length flat. The capture path's memory is still flat -
that is the WP-17 finding and it stands - but the number the old gate was
reading was mostly a cache filling up.

### A two-sided record collided with itself

The first export of a real project never got to the audio. It refused at the
plan, as §33 says it should, with the message that named the defect:

```
Error: tracks 2 and 2 both export to .../Spirit of Eden/02 -.flac
```

`track::Record::number` is the number *within its side* - §29's alpha position,
the number printed on the label - so on any record with a B side it repeats.
Side A's second track and side B's second track are both `2`, and the export was
using that number for two things it does not fit: the naming template's
`{tracknum}`, where it made every untitled side-B track collide with a side-A
track, and the tag's track number, where it put two tracks numbered 2 on one
album. The message could not even name the two tracks, because both their names
were `2`.

**VRipr had this right and the port lost it.** `{tracknum}` is a VRipr token, and
in VRipr it expanded to the alpha position: `format!("{}{}", dt.side, dt.number)`
in `src/metadata/mod.rs`, so the default template - the same string in both
products - wrote `A1 - The Rainbow`. §29 says VRipr's alpha numbering is
retained, and now it is:

* `{tracknum}` renders through the release's `Numbering`: `A1` under `Alpha`,
  which is the default, and the running number across the release under
  `Numeric`. `naming::expand` already passed a non-numeric track number through
  untouched, so the machinery was waiting for it.
* `{position}` and the `VINYL_POSITION` tag are the alpha position **whatever
  the numbering is**. That is a second small fix in the same place: under
  `Numeric` they used to render `6`, which threw away the one piece of
  provenance a vinyl rip has that a CD rip does not.
* The tag's track number is the track's one-based position **within its disc**,
  counting across that disc's sides, because that is what every player assumes
  a track number means. Not the release-wide sequence: on a two-disc set,
  disc 2's first track is 1 again, and `disc_number` is what separates them.
* A collision now names the tracks by position: `tracks A2 and B2 both export
  to ...`.

Both ends proved, on a test that builds a two-sided project with four untitled
tracks and one capture behind both faces. Putting `record.number` back into the
template fails it on the file names; putting it back into the tag fails it with
`[Some(1), Some(2), Some(1), Some(2)]` against `[Some(1), Some(2), Some(3),
Some(4)]`. Nothing in the suite had caught it because every export test until
now used one side.

### The first export of a real side, timed

§37 asks for it and nothing had ever measured it. The subject is the first-light
capture in `/data2/vcw-firstlight/real-side-a.vcw`: **192 kHz, 2 channels,
Int32, 300,627,479 frames**, 26 minutes 6 seconds of a real side in a 2.33 GiB
project, laid out as four tracks over 299,366,400 frames.

**FLAC refused it, up front and by name:**

```
Int32 audio cannot be written as FLAC: the FLAC format allows 32-bit samples but
flacenc 0.5.1 stops at 24, and narrowing 32 bits to 24 loses signal. Export this
one as WAV.
```

That is the refusal WP-14 wrote for exactly this case, arriving on the first real
project put in front of it, and the output directory was still empty afterwards.
It is worth stating what it means in practice: **a 32-bit capture is a WAV-only
capture**, and the default export format cannot carry it at all.

**WAV, the same side, release build, `/data2`:**

| | |
|---|---|
| Written | 4 files and 1 cover, 299,366,400 frames, 2284.0 MiB |
| Wall clock | **28.36 s** |
| CPU | 5.49 s user, 8.56 s system, 49% of one core |
| Rate | 80.5 MiB/s, **55x real time** |
| Peak RSS | **832 MiB** |

So an hour-long side is about a minute, and the job is I/O bound rather than
CPU bound at half a core.

**The peak resident figure is the finding.** 832 MiB for a 2.3 GiB export is not
a buffer anybody chose: sampling `VmRSS` every second through the run gives
12 MB flat with three spikes in it, of 600, 247 and 834 MiB - one per track, and
each one the size of the track it belongs to. `lofty`'s `Id3v2Tag::save_to_path`
rewrites a RIFF file in memory to insert the `id3 ` chunk, so **tagging a WAV
costs the file's own size in RAM**, after the audio has been written perfectly
and streamed through a 64 KiB buffer.

The FLAC path does not do this, and the difference was measured rather than
assumed. A 24/96 Int24Packed project of 398,088,000 frames - 4 hours 36 minutes
of audio, 1377.8 MiB of FLAC out - took **1m58.71s** at 91% of a core (105.15 s
of it user time, so this one is CPU bound where the WAV was I/O bound) and
peaked at **13.1 MiB of resident memory**, with a 630 KB PNG cover embedded. That
cover is eighty times the 8192 bytes of padding the encoder leaves for a tag, so
it forces the rewrite the padding exists to avoid, and `metaflac` confirms the
picture went in. 140x real time, and memory flat. So lofty streams a FLAC
rewrite and buffers a RIFF one.

What that costs: §37 asks for bounded memory use, and WAV export is the one path
in the product where it does not hold. A WAV track may be just under 4 GiB
before the container's own ceiling refuses it, and tagging one would ask for
4 GiB of RAM - which is the whole machine on a 4 GB Pi. **Recorded, not fixed:**
the remedy is to stop asking lofty to write the container and write the `id3 `
chunk ourselves, appended after the data chunk with only the RIFF size to
correct, which is the shape the FLAC writer already uses and would make the
memory flat for every format. That changes the bytes of every exported WAV, so
it wants third-party verification beside it rather than a footnote in a change
about something else.

### Sub-second waveform latency, measured

§37 asks for four things about a capture in progress - *responsive UI during
capture*, *sub-second waveform latency*, *real-time-feeling meters*, *bounded
memory use* - and until now the soak measured one of them, *bounded memory*,
and that only since WP-17. It counted the readers' queries and timed them, which
says the window can ask a question while the writer works and says nothing at
all about whether the answer is current. A reader answering in one millisecond
out of a picture ten seconds stale passes every gate the soak had.

**The measurement.** A reader thread now reads `Counters::frames()` - the newest
frame the device has handed to the ring - immediately *before* it asks
`Shape::of` for the newest frame a read-only connection can draw, and records
the difference in microseconds at the capture rate. In that order on purpose:
the query costs a fraction of a millisecond against a lag of hundreds, and
reading the device's clock first means the instrument can only ever understate
itself. What the figure contains is the whole path from stylus to drawable
pixel except the drawing: the ring dwell, the commit interval, and SQLite's
visibility rules. What it does not contain is the webview, which is deliberate
and is dealt with below.

**Two minutes, 96 kHz, Int24Packed, four readers, real-time pace, `/data2`:**

```
readers     4 reader(s), 26499 queries, 58934802240 frames covered,
            p50 1.0 ms, p99 4.2 ms, max 14.3 ms
waveform    behind the device by p50 130 ms, p99 250 ms, max 300 ms
            over 26499 redraw(s), budget 1000 ms (§37)
```

**The lag is the commit interval and nothing else**, which is the result worth
having. Blocks are 250 ms and a batch is one, so a block becomes visible when
it commits; a reader sampling uniformly between two commits sees a mean of half
an interval, and 130 ms is half of 250. The p99 is one whole interval and the
maximum is one interval plus a commit's own 50 ms. There is no queue anywhere in
this path, no accumulating backlog, and no term that grows with the length of
the capture - the same 250 ms at two minutes as at two hours, because `Shape::of`
reads the pyramid's top rung and not the samples (WP-09's finding, and §37's
*waveform rendering independent of total sample count* is the same claim from
the other side).

So **sub-second waveform latency holds with a factor of four in hand, and the
budget is now a gate**: `--waveform-budget-millis`, default 1000, gating the
p99 rather than the maximum. That choice is the one judgment in here. One
redraw in a hundred arriving a second late is a product that feels slow; one
redraw held up behind a checkpoint is not, and the maximum is printed beside the
verdict either way so nothing is hidden by the choice.

**Proved at both ends, and the failing end is the more interesting one.** Since
the lag *is* the commit interval, raising the interval is the regression: the
same run with `--batch-blocks 8` commits every 2 s and reports `behind the
device by p50 1010 ms, p99 2000 ms, max 2030 ms`, and fails. Everything else
about that run passed - zero loss, every byte verified, the commit budget (which
scales with the batch, to 2000 ms) comfortably met, memory flat, the WAL bounded,
and the readers still answering at a p50 of 0.8 ms. **A batch of eight is a
perfectly healthy writer with an unusable window**, and before this change the
soak would have called it a pass. That is the whole argument for the gate.

The harness agrees at a quarter of the length: `short`'s `contention` leg, 15
seconds with four readers, reports `p50 130 ms, p99 270 ms, max 350 ms over 3138
redraw(s)` and passes, and all eight legs of `short` are green including the
corpus leg fed from a real rip. The same p50 to the millisecond at an eighth of
the redraws is what a figure that is structurally the commit interval looks
like.

**A trap the gate walked into, and the harness would have found a year from
now.** `--fast` runs the source at a metered pace: throttled so nothing drops,
but still 28x real time. A reader on that run measures 1250 ms of lag and it
means nothing - the frames between the device and the drawable end are a ratio
of production speed, not a duration anybody experiences, and at 28x a 45 ms
commit looks like 1.25 s. The first version of this gate would have failed every
metered leg that ever gained a `--readers`, for the same reason `--fast` cannot
make a commit-latency claim and the byte verifier cannot run at `Pace::Fast`.
So the gate is off at a metered pace, the figure is still printed, and the line
carries the reason: `not gated: at a metered pace this is production speed, not
staleness`. The unit test that pins it asserts the exemption rather than the
comment. What saved this from shipping is that the eight-leg harness is a
different instrument from the unit tests: `short` passed, and reading *why* the
one leg with readers was the only one making the claim is what turned it up.

**What a FAIL says now.** That run also showed up something the report had been
getting away with: the verdict printed a bare `FAIL` and left the reader to diff
twenty lines against a green run. Ten conditions can fail a soak, so each one
names itself - `FAIL - the waveform fell further behind the device than §37
allows` - and the names go into `--json` as `broke` too, because a nightly's
JSON is the only record of a red leg once the log has scrolled.

**And `--json` was exiting zero on a failing soak.** Found while checking that
the new key came out where it should: the JSON arm printed its payload and
returned `Ok(())` unconditionally, so a run with `"passed": false` in its own
output handed the shell a success. Nothing in the repository drives the soak
that way - the harness reads the human-readable report - which is exactly why it
survived: the flag exists for scripts, and the only consumer that would have
found it is one nobody has written yet. It now exits with the same status as the
report it replaces, and prints the payload first so the JSON is still there to
read.

**Which half of "responsive UI during capture" this is**, and the finding that
came out of asking. The measured half is the Rust half: everything from the
device callback to a drawable summary, inside 300 ms at the worst. The other
half belongs to `app/ui/src/panels/Waveform.tsx`, and reading it to write this
section turned up something the figures above make newly relevant.

**The panel is polled, and it does not poll during a capture.** WP-16 settled
pushed-versus-polled and the argument is in the component's own header: a redraw
is driven by the four things that change the picture - the capture, the channel
and range, the panel width, and a project generation counter - and by nothing
else, with `capture-finished` bumping the generation so the last block appears
without an event that carries pixels. The counter is bumped by exactly four
event kinds - `capture-finished`, `track-detected`, `detection-finished`,
`export-finished` - and `recording-position`, which arrives throughout a
capture, is deliberately not one of them, because a re-read per meter tick is
the mistake S3's spike warned about. That decision is sound and the reasons for
it still hold. What it means, stated plainly for the first time, is that **a
waveform does not grow while a record is being recorded**: the freshness measured
above is what the engine can serve, not what a person currently sees. The
component says as much - *what polling costs is freshness during a live capture,
and that cost is paid by the meters instead* - and it is a reasonable reading of
§37 that the meters are what *real-time-feeling* refers to and the waveform is
what you look at afterwards to place a boundary.

It is not the only reading, and the cost of the other one is now known rather
than guessed. A live-growing waveform means a timer while the phase is
`recording`, and the numbers say what that would cost: the query is p99 4.2 ms
in Rust, the picture would be 130-300 ms behind, and S3 measured a full-canvas
main-thread redraw of a 1400x220 waveform at **29% of the main thread sustained
at 60 Hz**, which is about 5 ms of main thread per redraw - so **2 Hz is
roughly 1% of the main thread for a picture a quarter of a second old**. That is
cheap enough that "the waveform does not move while you record" should be a
decision somebody makes on purpose rather than a consequence of the dependency
list of an effect. **Recorded, not changed**: it is a product judgment about
what a recording screen should do, it belongs beside whether the panel
auto-scrolls and where the playhead sits during capture, and this change is
about measurement.

S3's other results apply unchanged and are worth restating because they bound
the answer either way: delivery across the Tauri boundary is ~10 ms and the
boundary itself is free, an `OffscreenCanvas` worker costs **0.5%** against the
main thread's 29%, and the worker was the only configuration that never missed
a frame budget. A panel redrawing a few times a second does not need the worker;
one that follows a capture at 60 Hz would.

**What is still unmeasured**, stated plainly rather than implied: the meters
(§37's *real-time-feeling* is not a number and needs one), the webview's own RSS
over a long session (S3 measured ~1.46 MiB/min with no plateau - risk R8, and
the only figure here that looks like a problem), and all of it on anything but
x86_64 Linux. The Pi 5 is where a 250 ms commit interval is least likely to
survive, and its soak is queued.

On the harness: `VCW_SHARED=1` turns this gate off alongside the commit tail,
and the same argument applies verbatim - a hosted runner's timing measures the
runner. The figure is still measured and printed there, with `not gated` on the
end of the line, because a number from a runner is worth having even when it
cannot be allowed to fail a build.

### The two events §35 names and nothing publishes, decided

`waveform-update` and `fingerprint-match` have been carried as a gap since
WP-15, in three documents and in a comment in every one of the four places a new
event has to be added. The gap was never a defect - both are *absent* from the
wire union rather than declared and dead, so the frontend's exhaustive `switch`
cannot be written against them and nothing promises them - but "recorded as a
gap" in four files is not the same as a decision in one.

**`waveform-update` was already decided, in the one place that had to live with
it.** `Waveform.tsx`'s header settled pushed-versus-polled under WP-16 and
argued it properly: a pushed waveform means the engine deciding how wide the
panel is and how many pixels a peak column covers, which are facts only the
browser has. What was missing was not the decision but the evidence, and the
measurement above supplies it: the poll costs **p50 1.0 ms and p99 4.2 ms** with
four readers competing for the same database during a 96 kHz capture, and
returns a picture at most **300 ms** stale. An event could not make that picture
newer, because the lag is the commit interval and is the same for a push as for
a poll. So the event would buy nothing and cost the thing §35's other events are
careful about: a second place where the state of the capture lives. Every event
in the union either carries something the frontend cannot compute
(`meter-update`) or says *the project changed, read it again*
(`track-detected`). A waveform notification is the second kind, and
`track-detected` already is it.

**So it is refused rather than deferred**, and that is now written where the
next person looks rather than only in a React component: `vcw-contract`'s
`event` module documentation carries the figures, `vcw-core`'s says the variant
will not arrive, and `store.ts` says why no case handles it. The alternative
reading - that §35's list is a checklist of things to publish - would have added
a 60 Hz event to a pyramid that answers in a millisecond.

**`fingerprint-match` stays open because the thing behind it does not exist.**
Fingerprinting is Phase 2, the event's payload is a decision about what a match
*is* (§26's regions, a confidence, a provider), and inventing the type now would
be inventing the feature. It is the one §35 name that is still a gap, and it is
recorded as Phase 2's rather than as an omission.

### The gate was green because of a file nobody tracks

`WP-19` made `bundle.externalBin` name the CLI, so `tauri-build` now refuses to
run while `app/src-tauri/binaries/vcw-<triple>` is missing. That directory is in
`.gitignore`, correctly - it holds a 10 MB build artifact. The dev box has had a
real one in it since the first package build, so `appfmt`, `appclippy` and
`apptest` all passed here, and **the same three steps failed on every CI run for
two days** with `resource path binaries/vcw-x86_64-unknown-linux-gnu doesn't
exist`. A fresh clone would have failed identically.

This is the third time the same lesson has been paid for, and the first time it
was a *missing* file rather than a present one: a green gate has to mean a green
CI, and anything the gate needs that the repository does not carry is a hole in
that claim. Both ends now write the same one-line placeholder - a text file, not
a binary, because nothing in either place executes it - and the real staging is
`tools/stage-cli.sh` in the `package` job, which is the job that must fail if
staging breaks. The failure was reproduced on this machine by moving the staged
binary aside, and the placeholder was watched to fix it.

### WP-17's memory gate stopped four recovery tests on two platforms

`vcw soak` defaults to a 32 MiB growth gate and **refuses to start where resident
memory cannot be read**, which is every platform without a procfs. That refusal
is right, and it is WP-17's own finding: a gate that cannot fail is worse than no
gate, because the report reads the same whether it held or was never applied.

What it also did was stop `kill_and_recover.rs`, whose writer is `vcw soak`, on
Windows and macOS - all four tests, before the first frame, with the refusal text
as the failure. **Recovery has never been exercised on either platform**, and the
reason is a flag this file never passed. It passes `--max-growth-mib 0` now,
explicitly and with the reason beside it, because a kill-and-recover test has
nothing to say about memory.

### A harness that closes a pipe under a running child

`recovery_reports_before_it_writes` calls `wait_until_recording` and discards
what it returns, which drops the receiver, which ends the thread draining the
child's stdout, which closes the read end of the pipe. The writer prints a
progress line every second; the next one fails; and **`println!` panics on a
failed write**, so the child died with `failed printing to stdout: Broken pipe
(os error 32)` and the test blamed it for exiting on its own.

A race, and the dev box wins it: the kill at 1.8 s usually lands before the
second progress line at 2 s. A loaded runner does not, which is why this was red
on `linux-x86_64` and on `linux-aarch64` and green here. The drain thread now
keeps draining after the receiver has gone, which is the property a harness owes
a child it is going to kill later.

It leaves a product question open rather than closed: `vcw soak ... | head -3`
panics for the same reason, because Rust ignores `SIGPIPE` and turns the failed
write into a panic in whichever thread printed. That is a real defect in a CLI
and it is **not** what CI hit - CI hit the harness - so it is recorded here and
not fixed in the same change.

### What a byte audit means once the device has dropped something

`a_capture_killed_at_a_random_point_recovers_every_time` failed on
`linux-x86_64` with `channel 0 frame 48000 is not what the device produced`, and
the frame number is the whole diagnosis: 48000 frames at 48 kHz is exactly the
1000 ms ring, so the ring had filled and one callback had been discarded. The
product says what happens next in as many words, in `Pace::Fast`'s own doc
comment: an overrun discards a whole callback while the source's frame index
moves on, so stored frame *n* holds the sample the source produced for some later
frame, and **every frame after a gap mismatches**. `soak --starve-after` already
verifies only up to the fault for exactly this reason. The audit in the test did
not know any of it.

It does now, and it is a stronger check than it was. On a mismatch it searches
for the shift rather than being told it: the delta that puts the source's audio
back under the stored bytes for sixteen consecutive frames is the size of the
gap, and each sample is a 32-bit function of its own frame index, so sixteen of
them agreeing is not a coincidence anybody has to argue about. A mismatch that no
gap up to five seconds explains is still a failure. So the claim survives the
loss instead of being suspended by it: the audio either side of a gap is exactly
what the source produced, at a named offset, nothing was invented and nothing was
moved, and **the gap itself is named in the output** rather than tolerated
silently. The channels have to agree about it too, because a callback carries
every channel, and a gap in one of them and not the other is something no
overrun can produce.

Both ends were proved on purpose. `--starve-after 1.0` puts one empty callback
into the run, and the audit reports `Gap { at: 48000, frames: 480 }` - one
callback, 10 ms, at exactly one second - on every iteration, and passes.
Corrupting a single byte at frame 24000 fails with the diagnosis printed: stored
`[46, 5E, D1, D0]`, the source's `[47, 5E, D1, D0]`, and no gap that explains it.

**And it found something.** The first version of the rule that goes with the gap
was `dropped_frames > 0`: a capture that lost audio must not be filed as a
flawless one, §15's requirement and WP-17's finding. It failed on the starved
run, with `Diagnostics { overruns: 0, underruns: 1, dropped_frames: 0 }` - which
is the two counters dividing the loss honestly, and only one of them able to
count it. A ring overrun discards a callback we were handed, so the frames are
known and counted. A device that delivers an empty callback has told us nothing
about what it skipped, so `underruns` is the only true thing to say. The rule is
`!is_clean()`, which is what §15 actually asks, and it holds in both cases.

Then it found the second thing: the same run, killed at 1.76 s, with a lost
callback at 1.00 s, and **four zeros in the row**. That is the writer's
`diagnostics_millis` interval, 2000 ms by default, and it is a documented
decision rather than a defect - persisting the counters costs an fsync, doing it
on every commit would be eight a second, and `capture_diagnostics.updated_at` is
what recovery reads so it can report the staleness instead of hiding it. The
assertion now allows exactly that interval, taken from `Config::default()` rather
than written down again, plus the commit block. The consequence is worth stating
plainly: **a capture killed within two seconds of its first fault reads as
clean**, and the timestamp beside the counters is the only thing that says
otherwise.

## Phase 2 - WP-25, the two lossy formats

**Taken out of order, 2026-10-04**, because WP-14's export code was open and the only
thing holding MP3 and Ogg in G3 was D5's licensing question. That question was half the
size it looked: `cargo info` disagrees with D5 on both crates. `mp3lame-encoder` and
`mp3lame-sys` declare **LGPL-3.0**, not the LGPL-2.1 D5 recorded - libmp3lame's own
`COPYING` is the GNU *Library* GPL v2 "or any later version", and the wrapper exercises
the later-version option, which is why `deny.toml` needs a version of the license the
`chromaprint-next` exception does not cover. And `vorbis_rs`, `aotuv_lancer_vorbis_sys`
and `ogg_next_sys` are all **BSD-3-Clause**, not LGPL: **Ogg export carries no copyleft
obligation at all.** Both corrections are now in D5 itself, in the notices and in
`deny.toml`'s comment, because a license recorded wrong in a plan is the kind of error
that gets read twice and checked never.

### Features that exist for the license, not for the size

Both formats are **default-on cargo features**. R4's pre-committed mitigation was
"optional cargo features; ship FLAC/WAV only in MVP", and default-off would fail §33,
which names MP3 and OGG as initial export formats - a build that cannot write them does
not meet the requirement. So the feature is not there to make the binary smaller. It is
there so that a redistributor who cannot carry the LGPL-3.0 obligation can drop MP3
with `cargo build --release --no-default-features --features ogg` instead of forking the
tree.

`Container::Mp3` and `Container::OggVorbis` exist in **every** build regardless. That
keeps the contract parser, the ts-rs bindings, the CLI's `--format` and the panel's
select identical across feature combinations, and a build without the encoder answers
`--format mp3` with a sentence saying it was compiled without it - which is a better
answer than not knowing the word. The refusal arrives from `Writer::vet` at plan time,
so it costs nothing and lands before a directory is made.

### What the encoders needed that the lossless ones did not

Everything goes through `f32`, because both libraries want float. `fan_out`
de-interleaves a stored frame into planar buffers once per chunk, scaling by a power of
two so full-scale negative is exactly -1.0 rather than a hair past it, where libvorbis
clips. Three quality levels rather than a bitrate: `transparent`, `high` and `compact`,
mapping to LAME V0/V2/V5 and Vorbis q8/q6/q3. The **name** is what travels - through the
settings file, the JSON command, the CLI flag and the report - because a VBR stream does
not record which `-V` made it, so the run log is the only place the setting survives.
The quality is a **no-op on the lossless containers rather than a refusal**, so the panel
can hold one value while the format changes under it and nothing has to be cleared.

**MP3 refuses a rate MPEG never defined.** Nine rates across MPEG-1, 2 and 2.5, nothing
above 48 kHz. Measured against the corpus: 54 of the 59 rips in `/data2/source_rips` are
at 44.1 or 48 kHz and go through untouched; the five at 192 kHz are refused by name.
Resampling is the way past it and VCW does not do it, for the same reason WP-14 refuses
to dither a 32-bit capture into FLAC - an anti-alias filter is a choice with an audible
result on someone else's record. Ogg refuses almost nothing, and is the only container
besides WAV that takes a `Float32` capture, which makes it the lossy answer for exactly
the rips MP3 and FLAC both turn away.

**No new tagging code was needed.** Two backends already cover four containers, because
what differs is the tag format and not the codec: ID3v2 is what a RIFF reader and an MP3
reader both look for, Vorbis comments are what FLAC and Ogg both carry natively, and
lofty puts each in the right place for the file it is handed. The embedded cover goes
into an Ogg as a base64 `METADATA_BLOCK_PICTURE` without a line of new code.

### Four API traps, each found by a test

`InterleavedPcm` hardcodes `len()/2`, so it is **stereo-only** and a mono track has to go
through `MonoPcm`. `vorbis_rs` has **no empty-block guard** and passes the sample count
straight to `vorbis_analysis_wrote`, where zero is libvorbis's end-of-stream signal - so
an empty write would truncate the stream rather than do nothing. `mp3lame-encoder`'s
`std` feature is **not default**, and without it `BuildError` and `EncodeError` do not
implement `std::error::Error`. And the **Xing/LAME VBR header is a placeholder frame**
emitted inside the first encode call's output, patched at the end by seeking to byte
zero; removing that patch on purpose left a file every player opens and reports the
wrong length for - three seconds of audio came back as **2.83 s**.

### Two defects in existing code, surfaced by testing the new code

**`Report::bytes` was taken before the tagger ran.** It came from `Writer::finish`,
which returns before `tagging::write` opens the file, so every export under-reported its
own size by the size of its tags - and an embedded cover is not close to free: a 4 MB
sleeve scan across a ten-track side is 40 MB missing from the one number a person checks
against the disk space they just used up. It is now measured from the filesystem after
tagging, which also makes the number true for all four containers rather than for the
two whose writers happened to be exact.

**`Error::Unencodable`'s reason could not name what it was refusing.** It was a
`&'static str`, so the MP3 refusal could describe MPEG's rate table but not say which
rate the capture is at - leaving the reader to work out which of the nine listed numbers
theirs is not, from a spec the exporter was already holding. It is a `Cow` now, and three
refusals name the rate or the channel count they are looking at.

### Verified by readers we did not write

For a lossy codec that is the only kind of verification there is: our own decoder would
only prove we are consistently wrong. `ffprobe` on codec, rate, channels and duration
for every rate each container takes, including 192 kHz Ogg. `ogginfo` with **no
warnings**, which checks the Ogg page structure rather than just decoding what it can.
`sox` reading a **1 kHz left, 3 kHz right** tone back out of each container through
`ffmpeg`'s channel split, because a planar fan-out is exactly the kind of code that
swaps two channels and still produces a file every player happily plays - with FLAC as a
control, so the tone generator and the two measuring tools are trusted before any claim
is made about the encoders. Python `mutagen` on ID3v2 in an MP3 and on the
`METADATA_BLOCK_PICTURE` in an Ogg. And one check with no tool in it at all: the three
quality levels must write files of **decreasing size**, because every step between the
panel's select and the encoder's builder could drop the setting and still produce a
playable file at the default.

**Both ends of every new check were proven.** Swapping the fan-out failed the channel
test and left the FLAC control green. Dropping the quality argument failed the size
test with `transparent wrote 27017 bytes and high wrote 27017`. Removing the VBR patch
failed three tests and nothing else. The UI's plan-dropping test was verified the same
way, by deleting the `setPlan(null)` it exists for.

### Where it stands

`cargo test`, `cargo clippy --all-targets -- -D warnings` and the full export suite pass
in **all four feature combinations** - neither feature, each alone, and both. Nothing
has been committed. **MP3 has no cue-sheet path** and never will; if a whole-side
archival deliverable gets a cue sheet it will be FLAC's native `CUESHEET` block, which
is still an open question. And nothing in the gate previously built `vcw-export`
without its default features - so the escape hatch in the notices now has a gate leg
and a CI job of its own, `features` and `export-features`, which build all three of the
combinations everything else misses.

### First light on real records

Both writers were then run against whole records from `/data2/vcw-firstlight`, which is
the part no fixture covers.

**`boc-side-a.vcw` as MP3 V2** - 48 kHz, 17 tracks, 60:11 of audio across four sides.
107.8 MiB in **56.7 s wall, 49 MB peak RSS**, so 64x realtime and flat memory. Every
one of the 17 durations matched the plan **to the millisecond** under `ffprobe`, which
is the Xing header doing on an hour of real audio what the unit test only proved on
three seconds. `mutagen` read back `TIT2`/`TALB`/`TPE1`/`TPE2`/`TRCK`/`TPOS`/`TDRC`/
`TCON`/`TSSE`, seven `TXXX` frames and a 1.69 MB `APIC` in each file, with the album's
typographic apostrophe intact and `TRCK` correctly disc-relative (`1/9` on A1, `8/8` on
D3) rather than side-relative.

That export is also the first real measure of the byte-accounting defect this work
package fixed: 17 files x 1,692,604 bytes of embedded JPEG is **27.4 MiB of the 107.8
MiB reported**. Taking `Writer::finish`'s number, as `run` used to, would have claimed
about 80 MiB - a 25% under-report of the space a person just used up.

**`real-side-a.vcw` as Ogg Vorbis q6** - a genuine 192 kHz `Int32` capture, 7 tracks,
52 minutes. 88.8 MiB in **2:00, 61 MB peak RSS**. `ogginfo` found **zero warnings in
all seven files**, each at 192 kHz with the full Vorbis comment set, both `GENRE`
fields, and the cover parsed as a real `Picture: 3 (Cover (front))` block out of
`METADATA_BLOCK_PICTURE`. The same project refused MP3 by name - *"this capture is at
192000 Hz"* - and wrote nothing, exit 1, no output directory created.

Two incidental confirmations. Side B's tracks point at **the same frame spans as side
A** in that project, so A2 and B2 came out byte-identical in length while A1 and B1
differ by the 21 bytes of their `VINYL_POSITION` tag: `sides-have-no-extent` in the
wild, and evidence that the Vorbis path is deterministic.

### What first light found that the suite could not

**The refusals were giving stale advice, and nothing could see it.** FLAC's three
refusals all ended *"Export this one as WAV"*, written before Ogg Vorbis existed. That
192 kHz capture is exactly the case: FLAC refuses it, and **Ogg takes it** - it had
just taken it - but the message never said so. The sentence was never *wrong*, which is
why it survived: WAV really does carry a 192 kHz `Int32` capture. It was *incomplete*,
and the only test on that wording asserted `contains("FLAC or WAV")`, which agrees with
whatever it said yesterday.

Correcting the messages one at a time did not work. Each correction introduced the
next defect, and the test caught all of them: offering FLAC above 96 kHz, then offering
FLAC with a caveat ("takes 88.2 and 96 kHz but no higher" - at 176.4 kHz that is a
second refusal), then offering WAV for a side too long for a 32-bit RIFF size. **Four
distinct drifts in one family of eight sentences**, so the sentences were the problem.

**The advice is now generated, not written.** `encoder::alternatives(refused, spec)`
asks every other container whether it would carry *this* capture and composes the
clause from the answers - "Export this one as WAV to keep it lossless, or as Ogg Vorbis
for a smaller file" - and every refusal in the crate ends with it. All four drifts
become impossible by construction, and a fifth container appears in every message that
should mention it on the day it is added.

That needed one structural change. Each limit check was split into a `*_why` returning
the **reason only** and a thin wrapper that appends the advice, because `alternatives`
has to ask the limits and the limits have to call `alternatives` - so the two halves
cannot be the same function. `carries` is built on the `*_why` predicates, and
`the_cheap_predicate_agrees_with_the_real_refusal` holds that seam shut across five
formats, six rates (including 0) and three lengths.

`carries` judges whether a container *could* carry the audio, not whether this build
has the encoder: `NoEncoder` counts as yes. Otherwise the prose a person reads would
depend on the cargo features and the `features` leg would disagree with the default
build about what the messages should say.

**Both ends were proven, and two attempts were not good enough.** The first version of
the advice test only checked that a *named* container works, and **passed on the
shipped wording** - restoring all three original FLAC messages left it green, because
"Export this one as WAV" was never wrong. Adding the group rule failed them with
`FLAC refuses it and names [Wav, Flac], but says nothing about [OggVorbis(High)]`. The
second mistake was worse and quieter: a test-local `carries` helper that wrapped
`Writer::vet` **shadowed the real one**, which made the equivalence test a tautology -
breaking `carries` left it green. Removing the shadow, it fails. With the generator in
place the advice test can no longer be defeated by editing a string, so its end is
proven by breaking `alternatives` instead: dropping the lossy branch fails it with
`says nothing about [Mp3(High), OggVorbis(High)], which would carry it`.

One existing test was holding a defect in place and had to be rewritten:
`a_wav_too_big_for_riff_is_refused_before_the_file_is_made` asserted the message
contained the word `"FLAC"` for a 192 kHz 32-bit spec - a container that refuses that
capture on both rate and depth. It now asserts Ogg Vorbis is named **and FLAC is not**.

`USER-GUIDE.md` was wrong in the same way twice: the four-formats table claimed FLAC
takes "any rate" when `flacenc` stops at 96 kHz, and *"A 32-bit capture can only leave
as WAV"* had been false since the day Ogg landed.

### A track with no title is called `Untitled`

The second thing first light found was cosmetic and immediately visible: six of the
seven tracks on the 192 kHz project had no title, so the default template
`{album_artist}/{album}/{tracknum} - {title}` produced `A2 -.ogg` - a separator
standing with nothing behind it. The capture was fine and the tags were right; the
file names were simply unpresentable.

`naming::expand` now substitutes `naming::UNTITLED` for an absent `{title}`, the way
it already substituted `00` for an absent `{tracknum}` three lines above. Three
decisions are worth recording.

**Only the file name, never the tag.** `splitter::values` feeds `naming::path_for`
alone; `Tags` is built separately from the same `track::Record`. So an untitled track
gets `A2 - Untitled.ogg` on disk and keeps an **empty title tag**, because a blank
title is the truth about the record and a made-up word in a library is worse than a
blank field.

**Before substitution, not after.** Once `{title}` has been replaced there is no way
to tell a title that is absent from one that is genuinely blank, and `trim()` means a
title of one space - which a real provider row supplied - counts as absent.

**A bracket group still wins.** `name_the_untitled` tracks `[` depth and substitutes
only at depth zero, so `[{title}]` keeps the existing *only if there is one* meaning.
Proved by mutation: removing the depth check fails two tests, dropping the `trim()`
fails two, and not substituting at all fails five.

Two of my own premises about `[...]` were wrong and the tests corrected both. A group
collapses only when its contents **trim to empty**, so `[ - {title}]` leaves `[ - ]`
behind: the bracket form was never a workaround for the dangling separator. And a
group that is *not* empty **keeps its brackets as literal text** - `[{year}] {title}`
has always given `[1994] Desire Lines`. The first draft of the `USER-GUIDE.md`
paragraph offered `{tracknum}[ {title}]` as the way to get `A1.flac`; the test written
to pin that advice showed it also gives `A2[ Sunshine Recorder]` on the same record.
The guide now says plainly that no template drops the separator on the untitled tracks
only, and the test pins **both** halves of the bracket rule so the paragraph cannot go
stale.

`from_a_project.rs` had a test asserting `["A1 -.flac", "A2 -.flac", "B1 -.flac",
"B2 -.flac"]` - the four names that came off the real two-sided record. It now asserts
the `Untitled` form. The two other places quoting `02 -.flac` are historical accounts
of a refusal that happened, and were left as written.

### A dry run now prints the paths it resolved

Verifying the `Untitled` change found the next thing: `--dry-run` printed four counts
and no names. Its own help said it resolves *"every path, every tag, every frame
count"*, and `--json` carried all of them, but the text output showed none - so the
only way to see what a naming template had done was to run a real export and look at
the directory afterwards, which is the thing the flag exists to avoid. Checking seven
file names cost a two-minute 192 kHz Ogg encode.

`cli::export::print_paths` now prints them, numbered, **relative to `--into`** - the
relative part is what the template produced, and `--into` is on the line above - plus a
`cover` line per directory, because one `folder.png` per album is a fact about the
layout that a template argument is usually about.

The real run's progress lines were made relative too, so a dry run is **the same text**
the run prints, line for line, and the two can be compared by eye. That is the property
`a_dry_run_prints_the_paths_the_run_will_write` asserts: it runs both and compares the
numbered lines, not just that some name appeared. Proved by mutation three ways -
dropping the listing, and making either side print absolute paths, each fails it. On a
real project the two outputs differ only in the last line.

**Parallel export is on hold** by instruction. The measurement that prompted the
question stands in the record above: the Ogg run spent 2:00 of wall clock on one core.

### Captures now record what equalization they arrived with

§51 was drafted this session and specifies playback equalization curves for Phase 3.
Almost all of it can wait. One part cannot: **which curve the hardware already
applied** is a fact about the capture that is gone the moment the capture ends.
Nothing in the audio distinguishes a flat transfer from an RIAA one with enough
confidence to act on, and a project full of captures that say nothing can never be
equalized without a guess. So the provenance field ships now, years before the curves
it exists for.

`CaptureEq` is three cases - `Flat`, `Riaa`, `Unknown` - and `Unknown` is the
`#[default]`. Deliberately not `Riaa`, even though an RIAA phono stage is
overwhelmingly the common case: a plausible default is exactly what a later processing
chain would read as a measurement. §51 already says an `Unknown` capture is refused
rather than assumed, and that only works if `Unknown` means nobody said.

Schema v3 is one statement:

```sql
ALTER TABLE captures ADD COLUMN capture_eq TEXT NOT NULL DEFAULT 'unknown';
```

`FORMAT_VERSION` stays 1 - nothing an existing column means has changed. `ADD COLUMN`
with a `NOT NULL DEFAULT` is metadata-only in SQLite: it rewrites no rows, so the
migration is free on the 2.33 GiB test project and can run on open rather than being
offered as a job. `the_equalization_migration_only_adds_a_column` pins that by
forbidding every other verb in the DDL, and
`an_older_project_gains_the_equalization_column_as_unknown` builds a database at v2,
writes the insert a v2 build would have written, upgrades, and checks both halves: the
column reads `unknown` and the row's frames are untouched.

The path runs `--capture-eq` / **Settings -> Audio -> Equalization on input** ->
`Arm.eq` -> `Setup.eq` -> `CaptureInfo::with_eq` -> the column, and a typo is
**refused** at the contract rather than defaulted: somebody who typed `raia` believes
they have recorded RIAA. The settings home is the right one because the field
describes the operator's preamp, which does not change between records - asking once is
asking as often as the answer changes.

Two generators had to learn the new shape. `doc.rs` parsed `CREATE` only and panicked
on the `ALTER`; it now folds an added column onto the table that created it, last,
which is where SQLite puts it - and `every_documented_column_exists_with_the_type_it_claims`
already cross-checked column order against `PRAGMA table_info`, so the fold is verified
against a real database rather than against the parser's own opinion. `tools/vcw-read.py`
refused a v3 file outright; it now reads v3 and reports the curve, which is the §49
claim under test: a third party can read what VCW writes, including the parts VCW added
after the spec was published.

Nine mutations, all caught: dropping the column from the `SELECT`, making the default
`Riaa`, letting a typo become `Unknown`, parsing the CLI flag and discarding it,
folding the documented column at the front, removing the DDL comment, skipping
migration 3, and - on the frontend - the Capture panel dropping the setting or
assuming `riaa`. That last pair matters most: the panel is the only thing that can put
the field on an `arm` from the window, and a panel that silently dropped it would make
every GUI capture permanently `Unknown`.

## Phase 2 - WP-28, the About dialog

**2026-10-04.** The compliance gap WP-25 opened is closed. Shipping `mp3lame-sys`
compiles **libmp3lame into the binary** under LGPL-3.0, inside a product whose own code
is MIT. The paperwork was already done and correct - `THIRD-PARTY-NOTICES.md` names the
component and the obligation, `LICENSE-LGPL-3.0` and `LICENSE-GPL-3.0` are in the tree,
`deny.toml` carries the allowance - but **nothing in the running application pointed at
any of it**. Someone who installed the `.deb` and never opened the repository was told
nothing, and the shell wrote its own version to stderr at startup and showed it to
nobody.

The dialog now carries, in that order of importance: the version and build identity,
the MIT line for VCW itself with the copyright holder, the third-party components with
the LGPL-3.0 one named and its source offer, and the credits.

**Nothing in it is prose.** Every field is read from the crate that owns the fact:
`CARGO_PKG_VERSION`, `CARGO_PKG_LICENSE`, `CARGO_PKG_AUTHORS` and
`CARGO_PKG_REPOSITORY` from the manifest, `SCHEMA_VERSION` and `FORMAT_VERSION` from
`vcw-project`, SQLite's own run-time version string, and the notices from
`vcw_export::notices`. That last one is the whole design. A build compiled without
`mp3` **has no LGPL component to declare**, so the one place the question can be
answered is the crate the cargo features belong to - `cfg!(feature = "mp3")` written in
`app/src-tauri` asks about the *shell's* features, and written in a webview cannot ask
at all. `Container::compiled_in` answers it on the variant rather than on
`feature()`'s string, so a fifth container cannot compile until it says which way it
goes, and `Container::notice` hangs the license facts off the same enum. The two are
deliberately separate: what obligation a container *would* bring is a fact about the
container, and which ones this build *does* bring is `notices()`' filter - which is
what lets the test check the filter rather than check itself.

This is the rule `encoder::alternatives` arrived at the hard way in WP-25, applied
before the drift rather than after it. Refusal advice written as a string literal
drifted four separate ways in one work package. A notice is worse when it is wrong: it
is either a claim about a license that does not apply or silence about one that does.

The exit criterion is a test and not a screenshot, and it can fail on purpose:
**the notices the binary reports agree with the features it was compiled with**,
asserted in `vcw-export` so the gate's `features` leg runs it in all four combinations.
Removing the `compiled_in` filter passes in the default build - every container is
compiled in there - and fails in the other three, which is precisely why the leg exists.
Making `compiled_in` lie about MP3 fails both the new test and WP-25's own
`a_build_without_an_encoder_refuses_at_plan_time_and_not_at_write_time`, which now
cross-checks the accessor against an independently derived answer.

**The CLI carries the same obligation and now says so.** `vcw doctor` prints the license
list from the same generator, because the command-line binary is redistributed in the
same package and links the same encoders; a person who only ever runs the CLI should not
have to open the repository to find out their copy contains LGPL-3.0 object code. The
test for it asserts the printed text against `notices()`' own answer rather than against
a hard-coded "LGPL-3.0" - `vcw-cli` declares no features of its own, so a
`cfg!(feature = "mp3")` written there would have been a check that could never fail.

Three mutations, all caught: `notices()` without its feature filter (fails in three of
four combinations), `compiled_in` returning `true` for MP3 unconditionally, and
`doctor` dropping the relink offer. On the frontend, rendering the relink paragraph
unconditionally fails `makes no relink offer when nothing linked is copyleft` - both
directions matter, because a build with no copyleft component that printed the offer
anyway would be claiming an obligation it does not have.

The credits are the one part of the dialog that is written out, because there is no
fact to derive them from.

Skipped: a keyboard chord for the dialog. It is a header button and `Escape` closes it,
and §44's workflows do not include reading a license. Add one if it is ever wanted.

### What first light found in it, and the two things it can now open

The dialog was built green and looked wrong, which is the `first-light` lesson again:
the tests say the panel renders what it was handed, and only a screenshot says where it
landed on the glass.

**The Library's sticky header painted through every overlay in the application.**
`.rows th` is `position: sticky` with a `z-index` of its own and `.overlay` had none, so
the table header's text appeared *inside* the dialog and hid its first line. The About
masthead is simply where it was first noticed - the keyboard map and every other overlay
had the same defect. `.overlay` now carries `z-index: 10`.

**The panel is a fixed head over a scrolling body**, so `Close (Esc)` cannot scroll out
of reach of a mouse: `.overlay-box.about` is a column flex box and `.about-scroll` takes
`overflow-y: auto` with `min-height: 0`, which is the part that makes a flex child
actually scroll rather than grow.

**Two CSS measures were wrong, and the second was invisible without the first fix.**
`.about-group p` carried a 60ch measure, which read as a ragged strip down the middle of
a panel whose tables run full width; the override written as a bare `.about-relink`
never applied, because `.about-group p` is the more specific selector. The measure is
gone rather than overridden, the prose is justified, and the art now sits *beside* the
build table in a two-track grid (`auto minmax(0, 1fr)`) rather than above it - which,
with the product name across the top as the masthead, brought the whole dialog inside
one panel height with nothing left to scroll to.

**The dialog is also the product's only outward door**, and no URL crosses the boundary
to open it. `library::support(page)` takes a *name* - `"coffee"` or `"shirts"` - and
looks it up in a two-entry `PAGES` table in Rust, so the webview can ask for a page and
cannot ask for an address. No new dependency and no capability change: the platform's
own handler (`xdg-open`, `open`, `cmd /C start`) is spawned and deliberately not reaped,
because `status()` would block the window while a browser starts. A page nobody offers
is refused with a message that **generates** the list of ones that exist, which is
`encoder::alternatives`' rule applied a third time; the frontend shows that refusal
rather than looking like a dead button. Proven end to end, once, by clicking it on a
real display: the page opened in the author's own browser.

## Phase 2 - WP-21, fingerprinting off the capture stream

**2026-10-04.** §25's two halves are built: the live worker that fingerprints candidate
regions while the record turns, and the offline path that fingerprints any span of
committed audio. `chromaprint-next 0.1.0` is in the graph as the dependency of record,
which makes **LGPL-2.1-or-later** the second relink obligation in the binary after
libmp3lame's LGPL-3.0, and `deny.toml`'s exception for it is no longer commented out.

**`vcw_fingerprint::chromaprint::Builder` is the whole of the algorithm's surface**, and
its one real job is frame alignment. S4 found that `AudioProcessor::consume` only
`debug_assert!`s that it was handed whole frames, so a release build fed a part-frame
swaps the channels from that point on and reports nothing - and the thing feeding it is
a **tap**, which hands over whatever happened to be in the ring when it was read. So
`push` takes bytes in any quantity and carries a part-frame to the next call. The test
is the shape of the proof: the same eight seconds fed in chunks of 3, 777, 4,099 and
65,536 bytes must produce one identical fingerprint, where 3 is smaller than a frame
and 777 and 4,099 are not multiples of one. Removing the carry buffer fails it.

**Measured, because the first version of the tests asserted a number that was wrong:**
the algorithm emits **8.08 sub-fingerprints a second after a 2.65 s warm-up**. Five
seconds of audio gives 19 items, not the ~40 a flat rate predicts. That number is now in
a comment rather than in anybody's head, and it is also why a region shorter than about
three seconds is **refused** rather than returned as an empty fingerprint that would
match everything: `Error::TooShort` names the frame count it was given.

**The live worker is `vcw_core::fingerprinting::Fingerprints`**, a third consumer of
§10's tee in the shape of `detection::Detectors`, and it is the only worker in the
engine that **listens** as well as reads: its regions come from `Event::Detected` on the
bus, which is what makes §25's "progressively rather than repeatedly fingerprinting the
entire recording" true without the detector knowing anything about fingerprints. It is
spawned *before* `Detectors` for one reason - a subscriber that appears after a publish
has missed it - and stopped *after* it in `halt`, because the detector's last publish is
what closes the last region it can see.

Three decisions in it are worth having written down:

- **A region starts late and that is fine.** The detector announces a boundary only once
  nothing later can move it, about 1.2 s after the fact, by which time the tap has handed
  that audio over and forgotten it. So a region's audio begins at the worker's cursor and
  `Region` carries both numbers. S4 measured the cost: region-boundary error is bounded
  at **~0.064 BER against 0.47-0.49 for unrelated audio**, so a late start cannot break a
  match, and buffering 1.2 s of every stream to avoid it would cost 6.5 MiB at 192 kHz
  for nothing.
- **A holed region is thrown away, not published.** §10's rule is that no consumer may
  cost the recording a sample, so the tap is lossy. A meter that misses 200 ms shows a
  stale needle; a fingerprint that misses 200 ms is **shifted from that point on** and
  matches nothing, while looking exactly like one that works. `Tap::dropped_bytes` is
  read when a region opens and again when it closes, and a region that lost audio is
  counted in `Fingerprinted::holed`. Defeating that check fails
  `a_region_the_tap_lost_audio_from_is_thrown_away_rather_than_published`.
- **Nothing is persisted.** There is no schema change in WP-21. A fingerprint is evidence
  for a lookup, and there is no lookup until WP-22; re-fingerprinting committed audio
  costs 0.6% of a core (S4), so a cache with nothing to serve would be a migration
  nobody can use. The regions are held in the `Recorder` the way `detected` is.

**The exit criterion is S4's claim, asserted rather than quoted:** the fingerprint a
region produces off the tap is **bit for bit** the fingerprint the same audio produces
offline, in `crates/core/tests/fingerprint_live.rs`. The test makes timing irrelevant
instead of tuning it - the tap is sized to hold the whole stream so no scheduling
accident can drop audio, and audio is pumped only after the boundary is published so the
cursor cannot have moved - which leaves the worker's logic as the only thing it can fail
on. A second test proves an end boundary closes a region and that nothing after it is
inside, by fingerprinting the exact prefix the region reports and comparing; making
`Edge::End` not close fails it with "the region ran 768000 frames past the 384000 the
boundary allowed".

**`vcw fingerprint` is the offline twin, and §4.5's reason for it.** It takes a span
(`--from`, `--length`) or every track the detectors imply (`--tracks`), prints items,
the simhash and the base64, and writes nothing. Driven on a real ten-minute side - 600 s
of a 48 kHz 32-bit rip pushed through the writer - it found what no test had: **the
implied-track pairing left the first region open to the end of the side**. `vcw detect`
pairs a start with the next end and tolerates two starts in a row, which is honest for a
list a person reads; for fingerprinting it meant region 1 was 0 s to 600 s and overlapped
all thirteen regions after it, at 4,825 items of fingerprint for nothing. A start now
closes whatever was open, which is the live worker's rule, and the regions came out
contiguous: fourteen of them, three refused as too short to say anything, the longest
1,581 items over 198 s. Real music also confirmed the rate - 4,825 items over 600 s is
8.04 a second against the 8.08 measured in the unit tests.

Skipped: the live regions have no CLI surface, because §35 declares no event for them -
`fingerprint-match` is a *match*, which is WP-22. They are reachable from
`Recorder::fingerprinted()` and covered by the integration test. Skipped too: a cap on
the offline path. The live worker stops a region at 120 s, which is `fpcalc`'s default
and AcoustID's indexing length, but a span the operator asked for is a span they get.

## Phase 2 - WP-22, the fingerprint lookup

**2026-10-04.** A fingerprint now has somewhere to go. `vcw_metadata::AcoustId` posts
one to `api.acoustid.org/v2/lookup` and hands back the recordings it matched, each with
the pressings it appears on; `MusicBrainz::recording` fetches the other half for a match
that came back as nothing but an MBID. `vcw fingerprint --identify` drives it on a real
side. Nothing is persisted: §35's `fingerprint-match` event and a table for a chosen
match both wait on the evidence resolver, because choosing among candidates is not the
lookup's job.

**It landed in `vcw-metadata`, not in `vcw-fingerprint` where the plan put it.** A lookup
is a provider request before it is anything to do with audio: it needs the rate limiter,
the retry policy, the cache, the credential rule and the offline mode that crate already
has, and `vcw-fingerprint` would have had to grow a second copy of all of it to host a
37-line stub. The stub is deleted and `crates/fingerprint/src/lib.rs` carries a section
saying where it went, so a reader following the plan finds it. `AcoustId` is deliberately
*not* a `Provider`: that trait searches with words and answers with releases, and this
one searches with audio and answers with recordings.

**POST, decided by measuring.** A fingerprint is about 28 base64 characters per second of
audio, which was measured off the real corpus rather than assumed:

| Audio | Fingerprint | In a form body |
| --- | --- | --- |
| 120 s | 3,303 chars | fits a request line |
| 300 s | 8,476 chars | at the 8 KB default limit |
| 600 s | 16,884 chars | twice over it |

So a five-minute track is already at the request-line limit a default server enforces,
and a whole side is well past it. `net::Request` grew a body and the `Transport` trait
kept one method: a request with a body is a POST and one without is a GET, which is the
only distinction either provider needs, and both are read-only. The credential travels
in the body, so `Request`'s `Debug` prints `body_bytes` and never the body.
`acoustid_fingerprint_400s.txt` is a real 400 s fingerprint kept as a fixture, and a
test fails if it is ever replaced with something that would fit in a URL.

**The `meta` separator is a space, and getting it wrong fails silently.** AcoustID
documents `meta=recordings+releases+tracks`. In a form body `+` *is* a space, so sending
a literal plus means `%2B`, and AcoustID then reads `recordings%2Breleases%2Btracks` as
one unknown name, answers `{"status": "ok"}` with the matches present, and omits every
piece of metadata. No error, no warning, no clue. Measured against the live service:

| Separator sent | Recordings returned |
| --- | --- |
| `%2B` (a literal plus) | 0 |
| `%2C` (a comma) | 0 |
| `+` (a space on the wire) | 1, with 13 releases |
| `%20` | 1, with 13 releases |

`META` is therefore the string `"recordings releases tracks"`, which `encode` turns into
`%20`. A unit test asserts the encoded form contains no `%2B`, and a live test asserts
the service still reads it that way, because this is the kind of thing a provider changes
without telling anyone.

**Every provider error message in VCW has been empty, and nobody noticed.** A live test
with no API key was expected to report what AcoustID says about a bad key. It reported
`AcoustID returned HTTP 400: ` with nothing after the colon. The service does send a body
- `{"error": {"code": 4, "message": "invalid API key"}, "status": "error"}` - and `ureq`
3 was throwing it away: `http_status_as_error` defaults to true, which turns any 4xx or
5xx into `Error::StatusCode(u16)` and discards the response. One config call fixes it for
all three providers, and the status still reaches the client, which is what lets it
decide a 429 is worth retrying. A second fix was needed for AcoustID specifically, which
puts its refusals in a 200 as often as in a 400: `refined` re-reads an HTTP error's body
as AcoustID JSON, so a bad key is `Error::Rejected` rather than a bare status, and
`Error::Refused` is a new variant for a refusal that arrived inside a success. A caller
that trusted the status code would have read a refusal as "no match found", which is the
one wrong answer available.

**First light, and the finding that matters for the product: a vinyl transfer matches
AcoustID, and the first conclusion written here that it did not was wrong.** The wrong
version is left described because the way it was wrong is the lesson. `vcw fingerprint
--identify` on a real side returned `no match at AcoustID`, and a sweep through the C
reference `fpcalc` 1.6.0 agreed: nine 120 s windows over *Tomorrow's Harvest*, the same
side at three positions by seven speeds, and six more records at three windows each -
**48 lookups, zero results** - against a digital mp3 control that scored 0.974 through
the identical path. The conclusion drawn from that was that a record is a different
master for a different medium and does not match a digital one.

Challenged on it, the missing control turned out to be in the *method*. Every one of
those 48 lookups declared the **window's** length as the `duration` parameter and started
at an arbitrary point mid-side. Both are fatal, and neither is documented:

| fact | measurement |
| --- | --- |
| `duration` is a hard pre-filter, not a hint | One track's fingerprint matched declared as 170 to 183 s and returned **zero results** at 166 s and at 190 s. The same bytes scored 0 declared as 120 s and **0.974** declared as the track's true 371 s. |
| The fingerprint must begin at the track's start | 0 s of offset scored 0.867, 5 s scored 0.868, 10 s scored 0.833 and **11 s scored nothing at all**. |
| The audio itself can be short | 30 s from the track start scored 0.840 and 60 s scored 0.867, against 0.868 for the whole track. |
| A vinyl transfer does match | **0.64 to 0.88** across the tracks of a real rip. |

So those zeros were guaranteed whatever the audio was. Redone track-aligned with real
durations, the vinyl rip of *Tomorrow's Harvest* identifies `Boards of Canada - Gemini`
at **0.867**, and 9 of its 17 tracks come back correctly named. A digital source of the
same material scores 0.96 to 1.00: one real library album, a 37-track Ultravox box,
returned **37/37 at 0.956 to 1.000**, including the rarities disc.

S4's finding is not in conflict with this and neither replaces the other: boundary error
costs at most 0.064 BER in *fingerprint* terms, and AcoustID's index lookup is not a BER
comparison. A good fingerprint of the wrong eleven seconds is still a good fingerprint
and still finds nothing.

**Our implementation was the first suspect and it is cleared.** `chromaprint-next` is a
pure-Rust port, so index compatibility was an assumption nothing tested - the bit-for-bit
exit criterion compares our output against our *own* output. A digital mp3 through the
whole VCW chain returns **0.97 with both vinyl pressings named**, so the port matches the
index when the audio is in it, and ffmpeg's chromaprint muxer was checked byte-identical
to `fpcalc` on the same window.

**What does remain a real gap is ours, and it is the layout.** Over the corpus at
`vcw fingerprint --tracks --min-sources 1`, 2002 regions across 21 rips have a **median
length of 5.6 s**, 70% are under 10 s, and only 3.4% are the 120 to 480 s a real track
occupies. The agreement threshold does not rescue it: on one ten-minute three-track side,
1 source gives 16 regions with a 5 s median, 2 gives a single 134 s region spanning all
three tracks, and 3 gives `no track boundaries were agreed`. The fingerprint verb
defaults to 1 while `adopt` defaults to 2, and neither value produces a track. Since
identification needs the start within 10 s and the duration within 7 s, `--identify` on
a default `--tracks` run is asking about fragments that cannot be answered, and
`seconds_of` in `fingerprint.rs` declares each region's own length, which is the wrong
number for anything that is not a whole track.

**The Vinyl Streamer is not a counter-example either way.** That Raspberry Pi project
fingerprints with Chromaprint **locally**: a record is taught once by playing it and
importing the metadata from Discogs by hand, and later plays match against its own local
database. Vinyl against your own earlier rip of the same pressing is an easier problem
than vinyl against a digital master, and it is one VCW could usefully answer (*have I
ripped this side before?*) as a Phase 3 idea.

One caveat recorded rather than resolved: a probe at 469.0 s returned nothing where
469.7 s scored 0.851 on the same track. A sub-second shift should not matter at a 10 s
tolerance, and chromaprint's ~0.124 s frame step is the obvious suspect, since the index
needs exact subfingerprint hashes. Unverified and parked.

The consequence for the design is in `docs/design/WP-23-identification-resolver.md`: the
catalog number a person reads off the label stays the strongest evidence VCW has, and
AcoustID is the fallback for the records that cannot be found by name plus the
*confirmation* for the ones that can. The populated fixture was still captured by
`trackid` rather than by fingerprint (recorded in `tests/fixtures/README.md`, with why).

**Sixteen legs green on this box at Rust 1.99.0, 1,130 Rust tests and 138 frontend
tests** - the whole gate, including `spikes`, `appclippy` and `apptest`, which do not run
on media2026. The bench box was busy with the corpus soak, and the split exists for
resources rather than capability; the one leg it is still needed for is a judgment call
about whether `rustup check` works here, and it did.

**Leg zero was lying, which is why it is leg zero.** `rustup check` exits **100** when an
update is available, and the leg tested its exit status first, so it printed
`toolchain SKIPPED (rustup check could not reach the network)` on exactly the outcome it
exists to catch: stable at 1.98.1 against CI's newest stable, 1.99.0. The update line was
sitting in a variable nobody read. It now reads the output for the finding and the status
only to tell a real failure from one, and the full gate was re-run on 1.99.0 before any
of the above was claimed green.

**The `features` leg earned its place.** `pub mod acoustid;` was inserted one line above
`pub mod agent;` and took the `#[cfg(feature = "net")]` that belonged to it, leaving the
only module that mentions `ureq` compiled unconditionally. Everything built and all
1,096 tests passed; `cargo check -p vcw-metadata --no-default-features` was the only
thing that failed, which is exactly the §40 regression that leg exists to catch.
`acoustid` itself needs no feature: it holds a `Transport` and does not know what kind.

**A read defect in schema v3, found by accident and fixed.** Driving the corpus work
turned up `no such column: c.capture_eq` on a project written before this morning's
migration. `85acd44` added the column and every read site in `session.rs` selected it by
name, so **every pre-v3 project failed to open for reading**, through all sixteen of
those sites - `fingerprint`, `detect`, `waveform`, `export`, the lot. `open_read_only`
could not catch it: it rejects a *newer* schema and accepts an older one, which is the
right policy and exactly why the caller has to read tolerantly. The select now asks
whether the column exists and the row parser already defaulted it to `Unknown`, which is
what a v1 project honestly knows about its equalization. The regression test builds a v1
fixture by winding the migrations back, asserts the fixture really is older before it
asserts anything about the read, and was mutation-proved: with the column check forced
true it fails with the original error.

## Phase 2 - WP-23, the identification resolver

**The design document came first, and then had to be rewritten.** The plan says the
resolver "deserves a design document before code", so one was written from the AcoustID
measurements above:
[`docs/design/WP-23-identification-resolver.md`](design/WP-23-identification-resolver.md).
Its first version had identification *discovering* the record from audio, because the
detectors cannot supply a layout and the measurements showed audio identification works.
That was the wrong end of the problem, and the correction came from the owner of the
workflow rather than from a measurement:

> when we start a new project we'll prompt the user for some basic information, artist,
> title, catalog number, mono/stereo, and whether an RIAA EQ will be applied on playback
> and on the exported files. So up front we'll have some pretty solid information to make
> informed decisions as the rip workflow progresses. we need only get into the acoustid
> weeds if we cannot find the release on discogs via catalog lookup, or artist title
> combination

Which is right, and it makes the hard part cheap. A project starts from a record in
somebody's hand, so four of the five things setup asks for are release identity stated by
a person looking at the object, arriving before the first sample does. The resolver's job
is not to guess what the record is; it is to turn a stated identity into a specific
**pressing**, confirm the audio is consistent with it, and lay the side out. Artist and
title identify a work, a catalog number identifies a pressing, and §28 asks VCW to tell
pressings apart.

`Lookup::ORDER` is that policy as code, and it is a pure function over the evidence so
the escalation order is asserted by a test rather than emerging from the order somebody
wrote the calls in: Discogs by catalog number, Discogs by name, MusicBrainz by name,
and only then the audio. AcoustID is last for three measured reasons - about a third of
`/data2/source_rips` does not resolve at MusicBrainz from artist and album at all, a text
lookup is one request where identification from audio alone cost 63 to 138, and §26's
rule that automatic identification never silently replaces what a person confirmed is
structural if the typed facts are the *first* evidence rather than a late tie-breaker.
`is_possible` keeps §40 honest too: a catalog lookup with no catalog number is not a
cheap failure, it is a request that cannot succeed.

**Four modules, and the split is the design.** `evidence` collects `(source, fact)` pairs
and judges nothing, because the same fact arrives from several places and disagrees with
itself - a person types `CHRH 1296`, Discogs says `CHRH1296`, MusicBrainz has no
catalog number at all, and that is three states rather than one field. `candidate`
gives one release's account of them. `confidence` holds every weight in one table.
`resolver` picks one, or declines to.

**The three-way verdict is load-bearing.** Silent is not a small disagreement. A release
with *no* catalog number is merely unsupported by the number on somebody's sleeve,
while a release with a *different* one is contradicted and must lose to a candidate with
less evidence in its favor; fold those together and the better-documented database loses
every time it is honest. An identified recording, by contrast, can agree but **never**
disagree: AcoustID's release lists come from digital submissions and routinely omit a
vinyl pressing entirely, so a candidate missing from the list is unmentioned and not
excluded.

**The thresholds were set by a failing test, which is the right way round.** A candidate
agreeing on artist, title and track count scored 0.45 against a `LIKELY` floor of 0.50,
so the ordinary result of a text search was being discarded instead of shortlisted. The
floor now sits at exactly what artist-plus-title is worth, because "right album, unknown
pressing" is a question to put to a person. Deliberate consequences of the same table: a
catalog number alone reaches `LIKELY` and not `CERTAIN`, since a number can be mistyped
into a search box; identified recordings accumulate (§26 requires it) but are capped,
because ten confirmed tracks prove the audio is this album and say nothing about which
pressing; and nothing short of a catalog match can reach `CERTAIN` at all.

**Declining is a result, not a fallback.** Three outcomes and no fourth. The failure mode
worth designing against is not "could not identify the record", which is ordinary and
recoverable; it is a plausible wrong pressing written silently over a catalog number
somebody read off the label. So a tie asks (`Doubt::TooClose` - two pressings of one
record agree about artist, title and track count equally well), a contradiction asks
(`Doubt::ContradictsAPerson`, which can never resolve however high the score), and the
shortlist stops at five because past that a question becomes a search result. The reason
is reported from the agreements rather than from the score, since §26's "evidence-based
rather than a single-match decision" is only true if the evidence can be printed.

26 tests and a doctest, all green, clippy clean. **Not done and called out rather than
smuggled in:** the provider calls are not wired - `Lookup` says what to ask and nothing
asks it yet.

## Phase 2 - schema v4, the two intents the setup prompt asks for

The two setup facts that had nowhere to live now have one. Both are booleans on the one
release row, which is the project header:

```sql
ALTER TABLE releases ADD COLUMN is_mono INTEGER NOT NULL DEFAULT 0;
ALTER TABLE releases ADD COLUMN riaa_eq INTEGER NOT NULL DEFAULT 0;
```

**Why they are asked and the other three fields are offered.** Artist, title and
catalog number are a head start: leave them blank and identification finds them. These
two are not findable by anything. A mono groove transferred with a stereo cartridge gives
two channels that are *nearly* identical and never exactly so, which is a measurement no
threshold survives; and an equalization curve leaves no trace in the audio it was applied
to, which is the whole reason §51 requires `captures.capture_eq` recorded from the first
release that can capture at all. Unticked is an answer rather than a gap, so they are
`bool` and not `Option<bool>` on the wire.

**`riaa_eq` is not `capture_eq`, and the two are easy to confuse.** `captures.capture_eq`
is per capture and records what the signal *already carried* when it reached the sound
card. `releases.riaa_eq` is per project and is what the operator wants done about it.
The first is the input to the decision; the second is the decision.

**Neither one touches the capture path.** §9 governs what lands and §51 says equalization
is "a non-destructive stored decision ... applied on playback, render and export". The
mono fold is the same kind of decision: the stereo capture of a mono record stays stereo
in the project, and the sum happens on the way out. That is what makes both flags free to
change your mind about, and it is why a mono rip is still a stereo file in the `.vcw`.

**What the migration cost, against the five-places checklist.** Four of the five were
already paid for by v3: `doc.rs` parses `ALTER TABLE ... ADD COLUMN` since then and folded
both columns onto `releases` with no change, `SCHEMA.md` regenerated from the `--`
comments above the DDL, `tools/vcw-read.py` wanted `4` in `SUPPORTED_USER_VERSIONS`, and
the shell's `wind_back_to_v1` needed nothing because it drops the whole `releases` table.
The fifth cost the usual: `release::load` names the two columns, so it asks
`has_intents()` first for the same reason `session::select` asks `has_capture_eq()` -
`open_read_only` does not migrate, and a bare `SELECT is_mono` would have answered
`no such column` to the export planner, the track numbering and the library listing
rather than to the one caller that wanted the flag. One `migrations.rs` assertion of the
applied list was rewritten as `(3..=SCHEMA_VERSION)`, so the next migration does not
break a test that is about a v2 row.

`identity::accept` carries both over from the existing row, beside `composer` and
`comments` and for a stronger reason: no provider reports them, and §26 says
identification does not get to silently undo what a person stated.

**Reachable from both ends.** `vcw release set --mono true --riaa false`, printed by
`vcw release show` and in its `--json`; two checkboxes on the window's new-project form,
whose seed condition now counts a tick as "something was given" - a person who ticks mono
and types nothing else had been having it dropped. 1,161 Rust tests, 138 frontend, all
sixteen gate legs green.

**Not done, and this is the whole of what the flags do today: they are recorded.** The
export does not yet fold to mono and nothing applies a curve. The fold is a transform
in `splitter::cut`, which currently moves interleaved bytes from the reader straight to
the writer without decoding them, so it needs the sum at -6 dB in each of the four
storage formats, a halved `Spec::channels`, and the four containers checked. The curve
is Phase 3 and §51 wants rather more than a boolean: nine named curves, per side and
overridable per track, each within ±0.5 dB of its published source. The boolean buys
RIAA, which is every record cut after 1954; widening it to a curve reference is another
additive migration when the curves ship.

## 0.1.1-alpha, the first release

**Published 2026-10-06 at `8b51808`:**
<https://github.com/shunte88/vcw/releases/tag/v0.1.1-alpha>. Eight assets, marked as
a pre-release: `.deb` and `.AppImage` for x86_64 and aarch64, an `.msi`, a macOS
`.app.tar.gz` and `.dmg`, and a `SHA256SUMS` recomputed in the release job rather
than concatenated from four platforms' own.

**A draft keeps a placeholder tag until it is published.** The release job ends with
`draft: true`, which is right: a tag builds the packages and stages a release but puts
nothing in front of anybody. What is easy to miss is that the draft it leaves carries
`tag_name: untagged-<hex>` rather than the tag that triggered it, so flipping
`draft=false` publishes a release on the placeholder. It did, for a few seconds, until
the `tag_name` was patched. Publish by setting the tag and the flag together. And
`prerelease: true` in the workflow applies to the *next* tag; an existing draft needs
the same PATCH.

The tag is the thing to be careful with, because `tags: ['v*']`
is the only trigger that turns the packaging job into a public GitHub release with
four platform builds and a `SHA256SUMS` behind it, and `package` has no `needs:` -
it publishes whatever the tag points at whether or not the test matrix is green. So
the tag waited nine commits for a green `main`, and the ones it waited for are the
interesting part of this release.

**Eight CI rounds, and only one of them was a bug in the product.** The rest were
tests making claims about the machine, on a hosted runner that could not meet them,
and each round hid the next because cargo stops at the first test target that fails.
In order:

- *A meter rate and a capture state, on macOS and Windows.* `VCW_SHARED=1` already
  meant "this machine is not ours" in the soak harness; it now gates the claims that
  are about a scheduler rather than about VCW. How many meter snapshots arrive in a
  window, how much audio a wall-clock window produced, and `Finalised` against
  `Interrupted` - which is exactly `!diagnostics.is_clean()`, so a starved runner
  reporting it is §38 working. What a snapshot *contains* still gates everywhere.
- *A CRLF checkout.* `app/ui/src/bindings/vcw.d.ts` and `docs/SCHEMA.md` are compared
  byte for byte against what their generators emit, and the generators emit LF.
  `.gitattributes` pins the working tree, repository-wide rather than two patterns.
  The diff message said "out of date at line 1630: committed `<end of file>`,
  generated `<end of file>`" because `str::lines()` cannot see a line ending; it now
  says which.
- *Real-time captures racing each other.* Seven test files start captures at the
  speed of a record, and libtest started them all at once: a Windows runner lost 2400
  frames to five ring overruns. `alone()`, a mutex per test binary, because cargo
  already runs the binaries one at a time. `Setup::simulated` reaches
  `Pace::RealTime`, so every live `Engine` test is one of these.
- *A path wearing the wrong separator.* A template's `/` is a separator wherever it
  is typed and the path it becomes wears the platform's. The expectation converts
  with `MAIN_SEPARATOR_STR`, not the result: a backslash is a legal character in a
  Unix file name.
- *A race the test created on purpose.* `a_simulated_capture_can_never_be_called_bit_perfect`
  needs a clean run before its refusal means anything, and at `Pace::Fast` the
  producer outrunning a descheduled reader is the design. `Pace::Metered` waits for
  room, so the precondition is a promise.
- *A test that hung instead of failing.* This one was worth the chase. A fixed 150 ms
  window was false on Windows, where a 1 ms sleep is 15.6 ms; the assertion panicked,
  and the unwind went into `Drop for Handle`, which joins the writer thread. Stopping
  a writer drains its source to the last byte by design, the test's source never runs
  dry, and the join waited for a loop that would never end. Nineteen minutes of
  silence, twice, because a job's log blob does not exist until the job ends.

**A hung job tells you nothing, so make it talk.** The writer test now publishes a
stage as it advances and a watchdog thread reports the last one reached and aborts.
It has to write to the `Stderr` handle rather than through `eprintln!`, because
libtest's output capture is inherited by spawned threads and an aborting process
never hands the captured buffer back. That one line - `stuck at stage 1 - finished
["spawn"], next is pause took effect` - is what turned a week of guessing into a
reproduction on this machine. `timeout-minutes: 25` on the test job so the next one
is twenty-five minutes rather than six hours.

**The version lives in four places and a test says so.** The root workspace's
`workspace.package.version` plus the eleven path-dependency pins beside it,
`app/Cargo.toml`, `app/src-tauri/tauri.conf.json` and `app/ui/package.json`.
`the_version_is_one_number_in_one_place` compares the Tauri config against
`CARGO_PKG_VERSION` as the crate actually resolved it, so the config and the binary
cannot drift; nothing checks `package.json`, which is the one to remember by hand.
The spikes keep their own `0.1.0` and are deliberately left alone - they are finished
evidence in their own workspace, not shipped code.

**The changelog heading is a machine-read string.** The release job does
`version="${GITHUB_REF_NAME#v}"` and then an awk with an exact `$0 == "## $version"`
match, falling back to `## Unreleased` only if that finds nothing. So the section for
this release is spelled `## 0.1.1-alpha` with no date and no brackets on the heading
line; the date went on a line of its own underneath. Keep a Changelog's usual
`## [0.1.1] - 2026-10-05` would have silently fallen through to the Unreleased branch.
`the_version_is_one_number_in_one_place` also wants the changelog to name either
`## Unreleased` or the declared version, and `## 0.1.1-alpha` contains `0.1.1`, so
renaming the section rather than adding an empty Unreleased beside it is enough.

**What the release carries beyond the bump.** The repo-wide US spelling sweep, 206
files in the main pass and 27 stragglers; `crates/contract/build.rs`, which stamps
`VCW_BUILT` and repaints `assets/version.svg`; the `built` field on `About`, shown on
the splash and in the About dialog; the compact VU dials; and the README swap, with the
development README moved to `docs/README.md`.

**Two spelling carve-outs, both deliberate.** `finalised` stays, 108 occurrences of it:
it is a value in `captures.state`, parsed in `vcw-types`, and crossing the wire as
`CaptureStateName`, so changing it is a schema v5 migration rather than a spelling fix.
The column carries no CHECK constraint - `validate()` is what rejects an unknown state -
so the migration would be a row rewrite rather than a table rebuild, which is cheaper
than it sounds and still not worth doing. And the `ALIASES` table in `crates/export/src/naming.rs`
keeps `catalogue`, `catalogue_number` and `organisation` alongside the US rows, because
that table is not VCW's prose - it is what other tools and other people's templates call
these fields, and a VRipr template reading `{catalogue}` has to get a suggestion rather
than a spelling lesson. The sweep had flattened those rows into duplicates of the US
ones, deleting the tolerance without failing a test; they were restored by hand and two
asserts added. Worth knowing for the next sweep: a `(?<![A-Za-z])catalogue(?![A-Za-z])`
boundary silently misses `CatalogueAtDiscogs`, so grep the stems afterwards rather than
trusting the pass.

**`build.rs` at the repository root is dead and still committed.** The root manifest is
virtual - `[workspace]`, no `[package]` - and Cargo only runs a build script for a
package, so it has never executed once. `tools/version-badge.sh` is superseded by
`crates/contract/build.rs`. Neither is referenced by CI, the gate, `scripts/` or the
README. Both should be deleted; the deletion is outstanding.

**Gate before the tag:** sixteen legs green, 1173 Rust tests passed, 0 failed, 23
ignored, 155 frontend tests passed, exit 0.

## 0.1.2-alpha, the float capture gets a FLAC path

**Published 2026-10-06 at `64cf18c`:**
<https://github.com/shunte88/vcw/releases/tag/v0.1.2-alpha>. Eight assets again, marked
as a pre-release. Two things in it, one of them the reason for cutting it.

**`flac-codec` replaced `flacenc`.** The old encoder stopped at 24 bits and 96 kHz, both
its limits and neither the format's, and because a device negotiation takes the widest
integer format on offer that meant the *ordinary* rip - S32 - had no FLAC path at all,
with 192 kHz ruled out beside it. §8 requires 192 kHz and §33 requires FLAC, so that was
a real gap in the requirement rather than a theoretical one. Both ceilings are gone, the
rate refusal with them. `the_flac_library_really_does_stop_where_we_say_it_does` - the
test written to fail the day either cap lifted - was turned around rather than deleted:
it now asserts that 32-bit at 192 kHz opens, and that 33 bits and a rate past 2^20 still
do not, so the day the *format's* own limits are the binding ones `Flac::why` owes a
sentence and this is what notices. The replacement streams natively, so WP-14's
hand-patched `STREAMINFO` is gone too; it was checked against the same third-party
readers that found the three WP-14 findings.

**A float capture can now be narrowed, by somebody who decides to.** This is the half
that needed a decision rather than a library. FLAC is an integer codec and an imported
Audacity project is float, so the export was refused by name. Narrowing it *silently* was
rejected at WP-14 on measurement - a real 32-bit rip from `/data2/source_rips` uses the
whole low byte - and that still stands. What shipped is the operator-chosen version:
**three switches answering three separate questions**, not one switch that guesses.

| CLI | `Settings > Export` | Values |
| --- | --- | --- |
| `--narrow` | Narrow to | `refuse` (default), `24`, `32` |
| `--dither` | Dither | `tpdf` (default), `none` |
| `--headroom` | Headroom (dB) | `0` (default) to `60` |

`refuse` is the default, so **an install nobody has configured behaves exactly as it did**
and the refusal is unchanged except that it now names the setting that changes the answer.
Four things about the shape of it are worth keeping:

- **24 is the width worth having, and the code does not say so.** A float significand is
  24 bits, so that is where a full-scale sample survives the trip intact; it is also the
  widest FLAC stream that can still use one channel to predict the other, because the
  difference channel needs one bit more than the samples and the format stops at 32. A
  32-bit stereo FLAC has no mid/side at all - `-0` and `-1` come out byte-identical. The
  reasoning is in `docs/USER-GUIDE.md` under *Narrowing a float capture*, because it is a
  thing a person chooses and not a thing the code decides.
- **`narrowed()` is keyed on `carries()`, not on FLAC.** It narrows only where
  `spec.is_float() && !carries(container, &spec)`, so WAV never narrows, Ogg never
  narrows, and the day a container learns floats it stops narrowing with no edit. Same
  discipline as `alternatives()`, and `narrowing_applies_only_where_the_container_needs_it`
  asserts it rather than trusting it.
- **Dither is reproducible on purpose.** §33 says an export is reproducible from blocks
  plus edit instructions, which a random generator would quietly break, so it is a
  xorshift64 seeded per file from a constant. `dither_is_noise_and_is_the_same_noise_every_time`
  is the check. Triangular, a little under one LSB: ~4.8 dB of noise floor traded for
  rounding error that no longer tracks the music.
- **Over full scale is clamped, not wrapped.** A float capture can sit above full scale
  without clipping and an integer file cannot, which is what `--headroom` is for; what
  survives the attenuation still gets clamped, because a wrap turns a peak into a crack.

**The Windows CI flake was in the test, not the engine.** `transport_session` settled its
post-pause frame count with a 200 ms sleep against a **250 ms** commit interval, so it
sampled mid-flush: a runner read 24,000 frames and 34,080 half a second later and reported
*"the transport recorded while paused"*. Replaced with a convergence poll that returns as
soon as the count stops moving and panics if it never does. The general form is that a
fixed sleep waiting on a flush has to outlast the interval it is waiting for, and if it
does not name that interval nobody can tell by reading it.

**Documentation that had gone stale with the encoder swap was corrected with it**: the
`README.md` format table, `docs/USER-GUIDE.md`'s format table and its float paragraph, and
the *What a container will not take, said out loud* section above, which still named
`flacenc` 0.5.1's two caps as live limits.

**Gate before the tag:** sixteen legs green, 1185 Rust tests passed, 0 failed, 23 ignored,
156 frontend tests passed, exit 0. **One round of CI, not eight:** 12 jobs green on `main`
at `64cf18c` first, then the tag, then 17 jobs including four `package` legs, then publish.
The draft carried `tag_name: v0.1.2-alpha` already, unlike 0.1.1-alpha's - the difference
was not measured, but 0.1.1's tag had been deleted and re-pushed eight times under an
existing draft and this one was pushed once onto a green head, so read the draft's
`tag_name` before publishing rather than assuming either way.

## 0.1.3-alpha, the window says more, in fewer places

**Published 2026-10-07 at `d0e6a5a`:**
<https://github.com/shunte88/vcw/releases/tag/v0.1.3-alpha>. Eight assets, marked as a
pre-release. Nothing in it changes a byte that gets recorded or exported. It is the UI
change list the previous section said was the user's to write -
`/data2/vcw_ui_modifications_10062026.txt`, thirteen items - plus eleven findings from a
read of the window beside it, and one real defect that only CI could see.

**About stopped printing addresses and started opening them.** The logo is now the way to
the source, each third-party component has a globe beside it that opens that component's
source, and the URL column those addresses used to sit in is gone - it had been invisible
for some time behind a measure that outranked it, which is the specificity trap already
written up in the WP-28 notes. All of it goes through the same `support(page)` table in
the shell: the webview names a page and never holds a URL, so `PAGES` grew one entry and
`addresses()` already generated the rest. `Ctrl+I` opens the dialog, which had a button
and no key.

**One new fact is derived rather than typed, which is the whole discipline of that
dialog.** About now carries a sentence saying what VCW is, and it is a new
`[workspace.metadata.vcw] description` in the root manifest read by `About::current`,
with `the_description_is_what_the_project_says_it_is` checking the literal against the
manifest. The alternative was a paragraph in a `.tsx` file that would drift from the
repository's own description the first time either changed.

**`Settings > Metadata` reads as switches, and each one is explained where it sits.** It
was three tickboxes and one paragraph underneath all of them. The question that prompted
the change was whether *Allow network lookups* is redundant beside the two catalog
switches, and it is not: it selects the transport, so it is the only one of the three
that stops an AcoustID lookup or a cover-art download. Deriving it from
`musicbrainz || discogs` would let a both-off configuration still reach the network. The
panel now says so in the sentence under the switch rather than leaving it to be
rediscovered. Beside it, *Get Discogs API Token* opens the page that generates one, the
genre map field describes what a genre map is, and `Credentials` explains why a variable
you exported can read as not set - a desktop launcher starts VCW from the session
environment rather than from your shell, which is the usual cause and has a different fix
on each platform.

**The event log was missing the events a bug report is about.** A refused command was
shown in the status bar and then forgotten, so the panel a report is copied out of never
saw it. `Store.run` now takes the command's name and writes a `command-refused` line on
failure, and the Tracks verbs pass theirs, so the log reads `detect in idle: ...` rather
than `a command in idle: ...`. Rows stay one line each, because a table whose rows are as
tall as their longest sentence is not scannable - and double-click opens one in full,
wrapped, which is the thing worth copying. The filter no longer takes focus when the
overlay opens: it did, and that made the status bar's standing offer of *"? for the
keyboard map"* a lie for as long as the log was up, because a field with focus is correct
to take a literal `?`.

**The library and Tracks say what they do not know.** A project VCW cannot read showed
`0 sides, 0 tracks, 0:00`, which is a statement about a record; it now shows dashes, which
is a statement about VCW, with the reason spelled out on the selected row only rather than
on every amber row at once. The first project is selected when the list loads. Tracks
points at the Library with no project open and at Capture with no capture, instead of
always talking about detection, keeps Detect disabled until there is a capture to detect
in, and draws the waveform stage only when there is audio for it.

**One review finding was resolved as no change.** `Ctrl+1..6` select panels and `Ctrl+D`,
`?` and `Ctrl+I` open overlays; the log being on the second list rather than the first is
not an inconsistency, because the log is an overlay and not a panel.

**Six of the fixes were found by looking at screenshots, and no test in the tree could
have caught any of them.** `currentColor` inside an `<img>` resolves against the SVG's own
document, which inherits no color, so both new marks painted black on a dark sheet and
were simply absent - the fix is a `<span>` with `mask-image` and
`background-color: currentColor`. `fieldset` in this sheet is
`display: flex; flex-wrap: wrap`, so a group of three switches and two text fields laid
itself out on one line across a 1600px pane. `fieldset .hint { order: 1 }`, which is right
for a packed row, collected every explanation at the foot of the panel so the sentence
about Discogs sat three paragraphs below the Discogs switch under a field about genres.
The switches had no measure and flew to the right window edge. The scales mark took the
heading's `--dim` and read as a smudge. And `.about-scroll` had no gutter, so every
wrapped line was read through the scrollbar. `/data2/vcw-scratch/firstlight.sh` is now a
reusable keyboard-driven Xvfb harness that shoots thirteen panels, which also exercises
§44's claim that no workflow needs the mouse.

**The gate was 16/16 green before every push, and `main` still went red three times.**
That is the useful part of this release. Four CI rounds, none of them about the release:

- **A real defect, found only because Windows is slow where Linux is fast.**
  `vcw session --script 'arm,record,sleep 3,stop'` produced a **1.37 s** side on a Windows
  runner. Arming is where the device is opened, and `arm` sent its command and moved
  straight on - so `record` queued behind an arm still in progress and the sleep that was
  meant to *be* the recording spent 1.6 s of itself waiting for WASAPI to open a stream.
  ALSA opens in 21 ms, which is why this had been invisible here since WP-06. `arm` now
  waits for `Event::Armed` on a condvar the printer thread sets, bounded at ten seconds
  and released at once by `Rejected | Refused | Denied` so the three illegal verbs in
  `record,pause,stop,arm,record` still cost the script nothing. The printer is the only
  consumer of the event stream by design, so nothing subscribes twice. **Three Windows
  failures across two crates were this one defect** - a pyramid rung, a span's end frame
  and a dropped-frame count - and all three were consistent with "the side is shorter than
  it was asked for", which was the whole bug. The first attempt waited in `record` for the
  first `Position { frames > 0 }` instead; that works, but the first position arrives one
  commit interval after audio starts, so every scripted side grew by 300 ms and broke a
  pause test whose discriminator is only 0.4 s wide.
- **A test that raced for its own precondition.**
  `a_region_the_tap_lost_audio_from_is_thrown_away_rather_than_published` gave the tap half
  a second of ring for eight seconds of audio and expected the overflow to hole the region.
  On an M-series runner the worker drained the ring as fast as the pump loop filled it,
  `holed` stayed 0, and a test about what happens to a holed region failed for want of a
  hole. The tap is now 16 KiB, smaller than one 64 KiB write, so the loss happens inside a
  single call before any consumer gets a turn. Both ends checked: with a tap big enough to
  hold the stream the assertion fails as it should.
- **A test asserting on a boundary.** The zoom test asked for 12 columns, which is exactly
  the count at which the block rung stops fitting a 3 s side. Widths now sit with margin
  inside a rung. A test that only passes when a real-time capture lands on an exact frame
  count will go red for a reason it is not about.
- **A loss claim a borrowed machine cannot make.** `detection_costs_the_capture_nothing`
  records 800 ms at 96 kHz with two taps on the fan-out, the most demanding live capture in
  the suite, and asserts nothing was lost. A Windows runner lost 259,200 frames inside it,
  nearly four times the audio the capture contains - a writer thread that was never
  scheduled rather than a tap that cost anything. The claim is comparative and a stalled
  machine loses the same frames with one tap as with two, so it is behind `shared()` now
  with an `eprintln`. The frame count stays ungated: however badly the runner behaved, the
  detectors must not have stopped a side committing.

**Gate before the tag:** sixteen legs green, 1187 Rust tests passed, 0 failed, 23 ignored,
165 frontend tests passed, exit 0. Then 12 jobs green on `main` at `d0e6a5a`, then the tag,
then 17 jobs including four `package` legs, then publish. The draft carried
`tag_name: v0.1.3-alpha` already, as 0.1.2-alpha's had, so publishing was one
`-F draft=false`.

**One step was missing from the release recipe and is now in it.** Three tracked files are
generated from the version - `assets/version.svg`, `Cargo.lock` and `app/Cargo.lock` - so
a bump has to be built before it is committed, or the release commit claims a version its
own lockfiles disagree with.


## 0.2.0-alpha, the window does what you tell it

**Published 2026-10-08 at `4829f76`:**
<https://github.com/shunte88/vcw/releases/tag/v0.2.0-alpha>. Eight assets, marked as a
pre-release. Nothing in it changes a byte that gets recorded. It is the second UI change
list - `/data2/vcw_ui_modifications_10082026.txt`, seven items - plus the findings from a
read of the window beside it, and the first release where what the window looks like and
how an export is named are the operator's to set rather than mine.

**Buttons can be icons instead of words, and the word survives the switch.**
*Settings > Appearance > Buttons* changes the panel tabs along the top and the transport
along the bottom together, and gives both rows back about a third of their width. Every
button keeps its name: the tooltip still reads `Record (r)`, and `.btn-label` is clipped
off the page rather than taken out of it, so a screen reader still hears the word. Stored
in `localStorage` beside the interface scale and the meter style, because none of the
three is a fact about the record. Six glyphs were drawn for this release in the existing
language - open line art, `#4f8cc9`, 2.7663 wide, on the same 73.768 box - and
`Face.test.tsx` now reads the stylesheet as text and insists that every glyph a component
asks for is one the stylesheet can draw, which is a class of bug no type can catch: `Face`
takes a string, so a seventh tab added without its rule compiles, renders, and shows an
empty 16 px gap.

**The Save button is gone and Settings save themselves.** Change anything and it is
written a moment after you stop; while the write is outstanding the panel says `Saving`
beside its title. The acknowledgement is derived rather than remembered, and that is not
a stylistic preference: `store.run` swallows a refusal into the status bar and resolves
either way, so a flag set in `.then` would read "Saved" over a write the core had
refused. The word goes when the stored settings catch up with the draft, and if the write
never lands it stays, beside the reason.

**That feature shipped silently doing nothing, and 174 passing tests all agreed it
worked.** `useStore()` returns a new object literal on every render and the window
re-renders at meter rate, so an effect listing `store` in its dependencies re-runs about
sixty times a second - and its cleanup cancelled the 600 ms save timer every time. The
panel said `Saving...` forever and `settings.json` was never touched. **No test could see
it, because a test renders when something changes and the window renders always.** The
first replacement test passed on the broken code for the same reason; it only
discriminates now because it keeps re-rendering right through the debounce, with a store
object rebuilt each pass and a settings object that holds still. The fix holds the write
in a `useRef` refreshed by a dependency-free effect, so the timer depends on the two
values that are actually a reason to save again. This is `first-light-finds-what-tests-cannot`
a second time, and it is the reason every release now gets an hour in the running window.

**Track numbers have three spellings, and none of them is written back to the project.**
*Settings > Export > Track numbers* chooses the position on the label (`A1`, `B2`) or a
plain number, and *Counted* then chooses whether the number restarts on each side or runs
across the sides of one disc. `vcw export --numbering` takes the same choice. It applies
to the export in hand only: exporting once as a sequence does not renumber the record,
because the number on the label is a fact about the record and the number in a file name
is a fact about this export.

**FLAC narrows a float capture at 24 bits instead of refusing it.** 24 is where an `f32`
significand survives intact, with a triangular dither and no attenuation, so the default
is the honest one rather than the cautious one. Settings still changes the width, the
dither and the headroom, and can be set back to refusing.

**The webview stopped keeping a second copy of every glyph.** `app/ui/public/` held a
duplicate of each icon that had already drifted from `assets/`. Vite inlines a small SVG
referenced by a relative `url()` straight into the stylesheet as a data URI, in dev and in
build alike, so the duplication bought nothing and is deleted. `tools/tidy-glyph.py`
strips the Inkscape editor state - window size, zoom, two dozen inapplicable style
properties - that was shipping inside those data URIs, and touches no geometry.

**A licensing gap was written down here, and there was never one.** Twelve of the icons
were recorded as borrowed art with no license, on the strength of a comment the drawing
editor writes into every file it saves. The author drew all of them. The entry was
retracted on 2026-10-09 - see that day's section - and `THIRD-PARTY-NOTICES.md` carries
no open item.

**`main` went red on macOS after the release commit, on a test nothing in 0.2 touches.**
`a_region_the_tap_lost_audio_from_is_thrown_away_rather_than_published` again, and the
second time it has been the subject of a release note. `d0e6a5a` had made the *loss*
deterministic - a 16 KiB tap cannot accept a 64 KiB write on any machine - and left the
*ordering* to a sleep: the test published a boundary and slept 300 ms before flooding the
tap, against a worker that drains the bus every 250 ms. A 50 ms margin on a parked thread.
Lose that race and the region opens after the audio it was meant to lose has already gone,
which is not a hole in a region but loss outside one, correctly not counted. The same
50 ms sat in front of every boundary in the file. **The fix is to ask the worker rather
than to guess at it:** `Fingerprints` reports how many boundaries it has acted on -
counted after the region exists, so a caller waiting on it is waiting for an open region -
and how many frames it has taken off the tap, which is the cursor a region actually closes
at. Both sleeps became waits and no assertion in the file depends on a duration any more.
Both ends checked: with the worker's cadence slowed eight times, the sleeping version
fails on two of the three boundaries and the waiting version passes.

**The gate earned its keep twice in this release.** `doc` caught an intra-doc link from
the new public documentation to a private const - `-D rustdoc::private_intra_doc_links`,
a one-word fix, and a red CI job that never happened. And the split holds: 13 legs on
media2026, the 3 that need GTK headers run here, 1154 Rust tests passed, 0 failed, 23
ignored, 174 frontend. Then 12 jobs green on `main` at `4829f76`, then the tag, then 17
jobs including four `package` legs, then publish.


## Next up

**Where to pick up.** **Every work package in Phase 1 is built and committed**, and so
are the two Phase 2 packages taken out of order: WP-25's lossy encoders and schema v3's
`capture_eq` at `85acd44`. WP-28's About dialog and WP-21's fingerprinting followed at
`2297178`, WP-22's lookup at `75424f2`, and WP-23's resolver with schema v4 at `b96b440`.
**The tree is clean and everything described above is committed.** `v0.2.0-alpha` is
tagged at `4829f76`, pushed, packaged and published - see the section above it - and
`main` is at the tag with nothing past it.

**0.3 has three things on it, and one of them is a licensing question before it is a
coding one.** The second UI change list shipped whole in 0.2.0-alpha, so what is waiting
is: **i18n** - a language select in *Settings > Appearance* defaulting to US English and
translations submitted against a TOML template, which is an extraction job across 23
`.tsx` files and about 272 `format!` sites rather than a file-format one; **AIFF and AAC
export**, where AIFF is ordinary work - big-endian PCM, an 80-bit IEEE 754 extended float
in the `COMM` chunk, and a check that lofty will tag it - and **AAC cannot be started
until it is settled how it gets encoded at all**, because the usual encoder is Fraunhofer
FDK, whose license is not OSI-approved and is not MIT-compatible, and VCW ships binaries
on four platforms; and **the twelve glyphs still to be redrawn**. The
darker variant of the expanded log row is deferred with them. The README was read and
passed on 2026-10-06, and the question
of whether `"finalised"` earns a schema v5 migration to `"finalized"` was closed the
same day - it stays as it is. The reason that survives is the carve-out above: the
spelling rule exempts identifiers, this is one as well as a stored value, and the
migration would rewrite a column in every existing `.vcw`.

**One reason given for that decision was wrong, and is withdrawn.** I said nothing
rendered the raw state name to a user. `Capture.tsx:253` prints `{row.state}` straight
into the Captures table's State column, and `:262` puts it in a sentence, so `finalised`
is on screen whenever a project is open - confirmed in a screenshot of the published
AppImage on 2026-10-06. The grep that missed it searched the frontend for the literal,
and the frontend never names the value; it just prints what the wire sends. The decision
to leave the spelling alone stands on the user's instruction, but it should be read
knowing the word is user-visible, which is the one real argument the other way.

**WP-22 is built** (above): the lookup, the recording fetch, `--identify`, and the
measurement that says alignment rather than audio quality is what decides whether a
record can be identified. It deliberately persists nothing and publishes no §35
`fingerprint-match`, because both need decisions WP-23 owns.

**WP-23's evidence model, weights and resolver exist, and schema v4 has landed the two
setup facts.** What is left on the package is a single item: **the three provider lookups
behind `Lookup`**. It says what to ask, in what order, and nothing asks it yet;
`vcw-metadata` already holds all three providers, so this is a walk of `Lookup::ORDER`
through `next_lookup`, folding each answer into `Observed` and each candidate into a
`Claim`, stopping when `resolve` returns `Outcome::Resolved`. The setup prompt itself is
done in the window, minus the fields that do not exist yet.

**Two known defects, neither fixed, both deliberate.** `seconds_of` in
`crates/cli/src/fingerprint.rs` declares each region's own length as the AcoustID
duration, which is the wrong number for any region that is not a whole track, so
`--identify` on a `--tracks` run mostly asks questions that cannot be answered. And
`releases.is_mono` is recorded and read by nothing: the export does not fold to mono yet,
which is the piece of the processing chain the flag is waiting on.

**The gate now runs in two places, and WP-21's final run was split across both.**
media2026 takes thirteen of the sixteen legs, including the `toolchain` check this box
can never make; `spikes`, `appclippy` and `apptest` stay here, because the bench box has
no GTK or dbus development headers and no sudo to add them. `gate.sh` takes `VCW_ROOT`
for the second tree. The WP-21 result: **thirteen legs green on media2026 at 1071 Rust
tests and 135 frontend tests**, the three local-only legs green here, doctests green
here, and the em-dash sweep clean - all sixteen covered, on an rsync verified identical
by checksum before the run.

Two things came out of the split. The first is a real regression the single-box gate had
been calibrating away, and it is written up above. The second is smaller: the frontend
count silently disappeared from the tally on the bench box, because vitest colorizes
its summary there even off a tty and the tally's `sed` matched the uncolored line only.
The escapes are now stripped and a missing count prints `UNKNOWN` instead of nothing, so
the tally cannot quietly lose half of itself again.

**Phase 1's loose ends are closed.** The pile was three items: the
playback-refused event, a decision on the two unemitted events, and the first
real export timing beside §37's figures for a capture in progress. All three
are done and each one found something: the refusal was terminal by design and
the reducer had to say so, the export collided a two-sided record with itself
and showed that tagging a WAV costs the file's own size in RAM, and the
waveform-latency work found that the soak had been calling a two-second-stale
window a pass. The one item deliberately left open is the WAV tagging memory
cost, which changes the bytes of every exported WAV and wants verification of
its own.

**CI has still not been green, and the reasons are now known rather than
guessed.** Five things were red on the WP-19 push and on the nightly, and **all
five are fixed in the tree**: the shell job's missing CLI sidecar, WP-17's
memory gate stopping four recovery tests on Windows and macOS, a harness that
closed a pipe under a running child, a byte audit that did not know what a
dropped callback does to frame numbering, and a memory gate reading a bounded
page cache as a leak. Each was explained and reproduced or measured locally
before it was touched; none was fixed by retrying the job. What no local run can
answer is whether Windows and macOS have more to say about recovery, because
those four kill tests have never run there.

**All four `package` jobs passed on the 2026-09-29 nightly**: macOS on Apple
silicon in 8m14s, Windows in 24m43s, Linux x86_64 in 14m34s and Linux aarch64 in
11m46s. That is new, and it is half of WP-19's exit criterion arriving without a
rig: **the bundles build on every Tier 1 platform**. The half that is still
outstanding is that nobody has installed three of them. `release` has still
never run, because it only runs on a tag.

**Installed and run on Linux x86_64, by hand, from the deb** - device
enumeration, a 589824-frame capture at 96 kHz S32 with no losses, and a clean
`recover --verify`. **Windows, macOS and Linux aarch64 have never been
installed.** Where the CLI sidecar lands in an MSI and in a `.app` is an open
question, and neither directory is on `PATH`.

**WP-17 is committed at `b3b6e02`, the CI repair at `e7cd249` and `93a0ab8`.**
Its sections above are worth reading before anything else: it found two product
defects rather than harness gaps - a device that goes silent was being filed as a
flawless capture, and nothing checked the WAL size at all - and the first CI run
on the repaired workflow found four more, three of them on platforms nothing
local can reach.

The gate is **sixteen legs** -
`toolchain / fmt / clippy / test / parity / offline / features / deny / doc / msrv /
spikes` at the root, `appfmt / appclippy / apptest` in `app/src-tauri` and
`uicheck / uitest` in `app/ui` - and `/data2/vcw-scratch/gate.sh` is the durable copy of
the script. `features` arrived with WP-25 and builds the three `vcw-export` feature
combinations no other leg touches. There is a seventeenth thing to run that is not in
it: `scripts/soak-harness.sh short <dir>` is
**1m48s** for seven legs and is what CI runs on every push, and `VCW_RIP` pointed at a
WAV from `/data2/source_rips` adds an eighth, the corpus leg, for another 16 s.

**First light has been run**, which is what the item below used to ask for, and it is the
reason WP-16a exists. It cost about an hour and found five things in a tree that was
gate-green at 910 tests, two of which no test in the repository could have caught by
construction. The library it ran against is `/data2/vcw-firstlight` - four projects, one
of them a 2.33 GiB real side - and `~/.config/dev.vcw.app/settings.json` points at it.
Both pre-WP-13 projects in it have now been upgraded to v2 by being opened, so re-running
the schema part of that exercise needs a fresh v1 copy.

**The single most valuable thing left is still not code, and half of it is now done.**
WP-16 closed the last of Phase 1's UI work, which means every stage of both §50's chain
and §44's workflows exists. The §44 half has been run - that is first light, above, and
it cost an hour and found five defects. **§50's chain has still never been run once from
end to end**, and M4 asks for exactly that: the CLI chain on a real record with a tagged
FLAC at the end. It needs no new code and it needs the turntable, so it belongs at the
rig, and it is the only remaining thing that will say whether the links hold when nobody
is stopping between them.

**`WP-20`, the Audacity import, is done.** It was the largest item left in Phase 1 and
the only remaining one that added a capability rather than describing or shipping what
already exists, and it was taken next because it was the best-prepared work in the
repository: S5 had decoded both generations against 30 real projects and written the
traps down rather than leaving them to be rediscovered. Two of them still cost something
to honor - `waveclip/@offset` turned out to be the sequence origin, which is what
decided the landing mechanism, and a label over deleted audio had to be reported rather
than landed. Its section above has the account.

What it leaves open is small and recorded there: no project older than Audacity 3.7.x has
been imported because the corpus has none, `TRACKNUMBER` is deliberately unmapped, and a
side still has no frame extent, so a project that really holds two faces in one capture
lands as one side.

**WP-18 and WP-19 are built**, and the three things that were queued for WP-19 are
settled: `bundle.active` is on, the icon is generated from an SVG, and `csp` is set. What
was queued and is still open is whether the Tauri binary has a Windows stack ceiling of
its own. It does not use clap, so it does not share the CLI's frame, but nothing has
measured it.

**What WP-16 leaves behind.** Its section above has the list; two items carry forward,
the playback refusal having been built. **Nothing has been driven through a real capture
by hand** - the window compiles, every command is tested as a function and every
keybinding is tested as a wire, and first light drove all twenty workflows against a
library, but arm-record-stop-play-export has never been clicked against a turntable.
And the frontend has met **one browser engine and one font stack**, so the layout is
unproven on macOS and Windows.

**What WP-14 leaves behind.** §33 lists MP3 and Ogg as required initial formats and
neither is built - they are in G3 because D5's encoders extend the LGPL relink obligation,
and whether they ship as optional cargo features is a licensing call rather than a coding
one. **A default capture cannot be exported as FLAC**: a device negotiation takes the
widest integer format on offer, here S32, and `flacenc` 0.5.1 stops at 24 bits, with a
96 kHz cap beside it that §8's 192 kHz requirement walks straight into.
`vcw session --format s24` sidesteps both for testing; the real answers are a
32-bit-capable encoder through bindings, at the cost of D5's pure-Rust choice, or an
explicit operator-chosen dither, and `the_flac_library_really_does_stop_where_we_say_it_does`
fails the day either cap is lifted. Nothing has been exported from the 2.33 GiB real side
yet, so the streaming design is asserted on tracks of seconds, and **no export has been
timed** - which is the figure a progress bar needs to mean anything.

**§50's chain is now complete behind CLI verbs, and has never been run in one pass.**
Connect, select album, set level, drop needle, record, flip, record, review, correct,
export: each link exists and each has been exercised against the real side in isolation.
M4 asks for the whole workflow end to end from the CLI, so the run itself is what is
outstanding, not any part of it.

**WP-13's skip is done.** `SKIP FORWARD` and `SKIP BACK` land on the next and previous
track edge, falling back to ten seconds only on a side nothing has been analyzed from.
`track::edges_of_capture` is the query, `Audition::marks` carries the frames, and both the
shell and the CLI fill it - detail in the WP-16 section above. An earlier version of this
paragraph said a skip might land on "the padded track start §33 exports" - **there is no
padding in §33 and none in the exporter**, which cuts exactly the span between two
boundaries; the phrase was invented here and is corrected rather than left to be read as
a requirement.

**The promotion floor is a policy, and policies get tuned.** `Policy::min_sources` is 2,
which is WP-11's over-segmentation finding turned into code, and on the real side it is
the difference between 270 candidate boundaries and 6. It is still a blunt rule: a genuine
quiet passage only the level detector catches is turned down by the same arithmetic that
turns down the HMM's noise. Every boundary keeps its `confidence` and `sources` precisely
so a UI can show what was rejected, and `vcw tracks ... adopt --dry-run` is how the
number gets argued from a record rather than from taste.

**§22's guided detection is the piece of the requirement that is genuinely missing.**
Expected track count, release durations, fingerprints and side topology are all available
now - WP-12 fetches them, WP-13 stores them - and nothing uses them to steer a detector.
The resolver already accepts such a boundary; something has to produce one. It is the
highest-value unbuilt thing in the detection path, and it is not on any work package's
exit criterion.

**A side still has no extent**, which only matters when both faces share one capture -
and that is exactly what an unattended rip of a whole record produces. Adoption then
writes the whole take's boundaries onto whichever side it was pointed at. The workaround
is to adopt one side or to record the faces separately; the fix is a frame range on
`sides`, which §21's skip and §33's export will both want anyway. Detail in the WP-13
section above.

One thing WP-09 deliberately left undone and did not need: **the waveform is read, not
pushed**, and WP-16 decided to keep it that way. The writer summarizes every block as it
commits, so the rows are there the instant they land; `Waveform.tsx` measures its own
width and asks for exactly that many columns, which a pushed event could not have done.
Nothing publishes `waveform-update` and nothing needs to.

Still open on WP-03, WP-04 and WP-10, and all for the same reason: **Windows and
macOS.** The device matrix is reported on one OS, the OS format verifier exists for
Linux/ALSA only, and playback's buffer negotiation has only ever met one backend. On the
other two the verdict degrades to `Unconfirmed` rather than to a false pass, which is the
right failure, but neither work package can close on it. WP-05 through WP-13 inherit the
same gap by being untried there at all.

WP-05 and WP-06 have the same shape of gap: the 90-minute soak and the kill suite have
both run on x86_64/ext4 and nowhere else. WP-09's two new indexes put the soak back in
scope on this machine, and **that re-run is done and passes**: commit max 102.6 ms
against the pre-index 102.3 ms, zero loss, every one of 6,220,938,240 bytes matched. The
x86_64/ext4 gap is closed for the new schema; the Pi 5 on SD and on NVMe, and Windows,
are the runs that remain, and they need no new code - `vcw soak` and
`cargo test -p vcw-cli -- --ignored` are the harnesses.

Two measurement jobs stay queued and can run on the machine's own time: S3's
`cpu-matrix.sh`, and S3's two R8 isolation soaks. **D3's firmed-config soak is no
longer among them** - WP-05's exit soak is that run, with the product code rather than
the spike harness.

## Housekeeping

- All of Phase 0 is committed: the spikes, the CPAL 0.18 upgrade and the `.vcw` rename
  at `a28fd85`, the S3 IPC bench at `096a8a0`, and S4, S5 and the AUP4 delta at
  `19dd459`. WP-01 is committed at `cd8e445`, WP-02 at `acb8835`, WP-03 at `941981a`,
  WP-04 at `95f1f52`, WP-05 at `358c44a`, WP-06 at `b2a517b` and WP-07 at `051a648`.
  WP-08 is committed at `75123ab`, WP-09 at `ae9b6d8` and `807d097`, WP-10 at `91ba45f`,
  WP-11 at `35fc89d`, WP-12 and WP-13 together at `96438ff` and `b549eab`, WP-14 at
  `fc8436f`, and WP-14's documentation with the whole of WP-15 at `25ed2fd` - which is
  where `crates/contract/`, `app/` and [ADR-0007](adr/0007-rust-typescript-contract.md)
  land, along with the two new CI jobs. WP-13 is the second
  change to touch the schema - v2, the vinyl data model - so `docs/SCHEMA.md` went with
  it. WP-09 is the
  first change since WP-02 to touch the schema, so `docs/SCHEMA.md` was regenerated with
  it; regenerate with `VCW_BLESS=1 cargo test -p vcw-project --test schema_doc` whenever
  the schema moves, or `the_committed_document_matches_the_schema` fails. The schema has
  moved twice more since: **v3** adds `captures.capture_eq` at `85acd44` and **v4** adds
  `releases.is_mono` and `releases.riaa_eq` at `b96b440`. A migration is never only a
  migration - `doc.rs`'s DDL parser, the generated `SCHEMA.md`, `tools/vcw-read.py`'s
  `SUPPORTED_USER_VERSIONS`, the shell's `wind_back_to_v1` fixture and any `SELECT`
  naming the new column all have a say, and the last of those is the one that bites:
  `open_read_only` does not migrate, so a query naming a column unconditionally breaks
  every older project at once. Both `session::select` and `release::load` ask whether
  the column is there before naming it.
- **The gate is sixteen legs now** (twelve until the CI repair added `toolchain`, `msrv`
  and `spikes`, and sixteen since WP-25 added `features`), because the shell is a
  workspace of its own and the
  root's legs cannot see it: `fmt clippy test parity offline features deny doc` at the
  repository
  root, `appfmt appclippy apptest` in `app/src-tauri`, and **both** `uicheck`
  (`pnpm check`) and `uitest` (`pnpm test`) in `app/ui` - WP-16a split those two apart,
  because `pnpm check` proves the frontend compiles and cannot prove it behaves.
  `/data2/vcw-scratch/gate.sh` runs them all, tallies `gate-test.log` and
  `gate-apptest.log` together, and sweeps the changed files for em dashes - a sweep that
  now covers `.ts`, `.tsx`, `.css` and `.json` as well. The two new legs earned their
  keep on the first run: `doc` found public documentation in the new crate linking to a
  private item, and `appclippy` found a constant that only its own test reads.
  Building the shell needs the frontend built first, since `tauri-build` fails when
  `frontendDist` is missing.
- **The soak harness is not one of the twelve, and should not be.** `scripts/soak-harness.sh
  short <dir>` is 1m48s of real captures - seven of them, or eight and 2m04s with the
  corpus leg - and it is what CI runs on every push. It belongs next to the gate rather than inside it, because it needs a writable
  directory with a couple of gigabytes free and the gate needs nothing but the repository.
  `VCW_RIP=/data2/source_rips/<a>.wav` adds the corpus leg. `nightly` instead of `short`
  is two real-time hours and is for the scheduled job or an idle machine.
- **`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` is part of the gate.**
  It had never been run, and it found ten broken doc links across four crates that
  `clippy -D warnings` does not see: private items linked from public docs
  (`GAP_SHARE`, `sizing`), a link to a crate `vcw-signal` does not depend on, a file
  path written as an intra-doc link, a module and a function called `validate` needing
  `mod@` to disambiguate, and `crate::schema::markdown` for what is really
  `crate::doc::markdown`. It also found a real defect: the doc comment on
  `playback::choose` had been cut in half by a `const` inserted into the middle of it,
  so half the paragraph was documenting `TARGET_BUFFER_MILLIS` and `choose` began
  mid-sentence. Both halves are reunited. Run the doc leg with the others from now on.
- **`/data2/vcw-scratch/parity/`** is the A/B reference generator for WP-11: a scratch
  crate holding a verbatim copy of `/data2/vripr/src/audio/mod.rs`, run over
  `/data2/vripr_training` to produce `vripr-answers.jsonl`. Its output is checked in at
  `crates/signal/tests/fixtures/vripr_answers.jsonl`; the crate itself is not, so that
  nothing in the repository carries a second copy of VRipr's algorithm. Regenerate with
  `cargo run --release -- /data2/vripr_training vripr-answers.jsonl` from that directory
  if the reference ever needs rebuilding, and expect the parity floors in
  `tests/vripr_parity.rs` to need re-reading against the new figures.
- **`/data2/vcw-scratch/genreparity/`** is the same trick as `parity/`, for WP-12: a
  scratch crate holding a byte-verbatim copy of VRipr's `src/metadata/genre.rs` and its
  `assets/genre.dat`, reading genre strings on stdin and writing JSONL. Its output is
  checked in at `crates/metadata/tests/fixtures/vripr_genres.jsonl` (1,819 answers, md5
  `045fc5b24f800f5854f4a15419777cd5` over three runs); the crate is not, so the
  repository holds one implementation of the algorithm and one recorded set of answers.
  `inputs.txt` is the 1,819 query strings, and regenerating is
  `cargo run --release < inputs.txt > vripr_genres.jsonl`.
- **`/data2/vcw-scratch/`** is the measurement bench for WP-09 and WP-10, none of it in
  the repository
  and all of it disposable. `realrip/` is a throwaway crate that pushes a headerless WAV
  through `persistence::Writer` at full tilt - there is no CLI path for that yet, and it
  is how the 26-minute side got into a project. `evict.py` is the two-line
  `posix_fadvise(DONTNEED)` cache-evictor every cold number was taken with; without it
  the readings are RAM readings. `side-a.vcw` is the 2.33 GiB real side, `soak90.log` the
  passing 90-minute run and `soak90-contended.log` the spoiled one. `play/` holds WP-10's
  six-second project and the `.raw` renders taken off it by hand; `m2/m2.vcw` is the
  twenty-second capture the M2 chain above was demonstrated on.
- **`/data2/vcw-scratch/wp13-cli.vcw`** is a 2.5 GiB copy of `side-a.vcw` that WP-13's
  whole editing sequence was run against: adopt, set, split, merge, re-adopt, reassign,
  release set and artwork, then a clean `validate`. It is disposable, and it is also the
  project the evidence round-trip defect was found on, so it now carries both the old
  compounded names and the healed ones. Delete it when the disk is wanted; a fresh copy
  of `side-a.vcw` reproduces it in one `cp` and two adopt passes.
- **`/data2/vcw-scratch/wp14/`** and **`/data2/vcw-scratch/wp14cli/`** are WP-14's bench,
  neither in the repository and both disposable. `wp14/` holds the hand-written WAV and
  FLAC probes the encoder's header decisions were argued from, including the pair that
  `flac 1.5.0` was asked to compare against its own output - `ref.flac` and `ref2.flac`
  are the reference encoder's, `probe.flac` ours, and the `STREAMINFO` block-size finding
  is the difference between them. `wp14cli/` holds the projects the shipped binary was
  driven against: `side.vcw` (S32, two tracks, a release and artwork), `s16.vcw` and
  `s24.vcw` proving `vcw session --format` now reaches the simulated source, `cover.png`,
  and the `out/`, `wav/` and `flac24/` export trees. Reproduce any of it with
  `vcw session --script "arm,record,sleep N,stop"` and a few `vcw tracks` calls; nothing
  there took longer than a second to make.
- **`/data2/vcw_soak/`** holds what is left of the WP-05 soak: `wp05.log`, the run
  transcript quoted above, and `live.vcw`, the four-second hardware capture. The 5.94
  GiB `wp05.vcw` has been deleted, as have the two three-minute WAL-pair projects.
  `rec/demo.vcw` is the killed capture quoted in the WP-06 section. Nothing here is in
  the repository and all of it is disposable.
- **`/data2/source_rips`** is the source-audio corpus S4 ran against: 62 real vinyl rips,
  71 GB, 48 kHz and 192 kHz 32-bit WAV plus 24-bit FLAC. Distinct from
  `/data2/vinyl_rips`, which holds the 30 *projects* S5 used - 25 AUP3 and 5 AUP4, the
  latter five being conversions of AUP3 projects already in the set. Several titles
  appear in both corpora, which will make a good import round-trip test at WP-20, and
  the five matched pairs are the AUP3/AUP4 equivalence fixture.
- `.bench/` holds **~8 GB** of scratch databases - the 8.4 GB soak artifact plus older
  `.vripr`-suffixed files from before the rename. All gitignored and safe to delete; the
  S1 and S2 numbers are recorded here and in the spike write-ups.
- **The first entry in `deny.toml`'s advisory ignore list arrived with WP-14.**
  RUSTSEC-2024-0436 marks `paste` unmaintained and archived, with no safe upgrade because
  the author retired the crate rather than patched it. It reaches us through `lofty` and
  it is a proc macro: it runs at build time, emits identifiers, and no line of it is
  linked into the binary. The exposure is a build-time dependency that will not receive
  fixes, on a crate that has never had a fix to receive. The entry says all of that in
  place, and says to check whether the dependency has gone the next time lofty moves - an
  ignore nobody re-reads is how a real advisory gets through later.
- Licensing today: MIT core, cpal Apache-2.0 as an ordinary dependency, and **one live
  copyleft obligation**: `mp3lame-encoder` and `mp3lame-sys` are LGPL-3.0 and arrived
  with WP-25. `deny.toml` names them as exceptions, so any *other* copyleft crate that
  reaches the graph still fails the build, and `THIRD-PARTY-NOTICES.md` carries the
  relink notice under LGPL-3.0 §4 with `LICENSE-LGPL-3.0` and `LICENSE-GPL-3.0` beside
  it. The MP3 encoder is behind a default-on cargo feature so a redistributor who cannot
  carry that can drop it. Ogg Vorbis is BSD-3-Clause and adds nothing.
  `chromaprint-next` added the LGPL-2.1-or-later obligation when WP-21 put it in the
  graph; its `deny.toml` exception is live, and unlike the encoders it is behind no cargo
  feature, because §25 is not optional and a build without it could not identify
  anything.
- **Stay current on CPAL.** Two blocking defects and the device-id API all landed within
  two minor releases; pinning 0.16 had already cost us a fork.

## 2026-10-09, AIFF, the glyphs, and the language a refusal comes out in

Three things off the 0.3 list, none of them committed yet.

### AIFF export, end to end

`Container::Aiff` is the fifth container, written by hand in
`crates/export/src/encoder.rs` rather than taken from a crate: the WAV writer next to it
is hand-rolled for the same reason, and no new dependency means nothing new for
`deny.toml` or the notices. FORM/AIFF with an 18-byte `COMM` and an `SSND`, a fixed
54-byte header, big-endian samples, and the sample rate as an 80-bit IEEE 754 extended
float. The three sizes are patched in at `finish`, the odd-length pad belongs to FORM's
size and not to `SSND`'s, and `Aiff::CEILING` is `u32::MAX` minus the header, which is
the same 4 GiB ceiling WAV has for the same reason - so `TooLargeForWav` became
`TooLarge` and names its container.

Float is refused rather than silently narrowed, exactly as FLAC is, and routed into the
same `Width` machinery. `alternatives()` caught the first attempt: the refusal named no
container that would actually take the capture, because `Aiff::vet` built its error
without the generated advice clause.

Three readers agree. `ffprobe` reports `pcm_s16be`, `pcm_s24be` and `pcm_s32be` with the
right rate, channels and `duration_ts`; `ffmpeg` transcodes an AIFF back to a 24-bit WAV
and the samples compare byte for byte; and a real CLI export of `two-tracks-tagged.vcw`
wrote two tagged `.aiff` files with an embedded PNG cover that `ffprobe` reads back
whole. Both ends were proven: removing the byte-swap loop fails the ffmpeg round trip
and the tagging test, and notably does **not** fail the ffprobe test, which is why the
round trip exists.

`Container::ALL` is the single source of truth, so `tag_cases()` picked AIFF up on its
own and the frontend's `formats.test.ts` holds the two panels' option lists to the same
list.

### The glyphs

Eight supplied glyphs tidied through `tools/tidy-glyph.py`, given the VCW MIT header,
and three new ones drawn for the waveform zoom row. **All twenty-one glyphs are the
author's own work and MIT with the rest of the source**, which took three corrections to
get written down - the last paragraph of this section is the one worth reading.

**`library.svg` took two passes, and the reason is worth keeping.** The first drawing
was three overlapping records built as a white filled backing plate with blue grooves
over it, and it rendered as a solid blob: **a `mask-image` reads coverage and nothing
else**, so a white shape over a blue one has the same alpha as either and every
colour-based distinction disappears. Redrawing the circles as strokes restored the
grooves and produced an illegible ball of wool at 16 px, so the old books-on-a-shelf
glyph was kept and the problem reported. The replacement supplied the same day is the
same three records drawn as a single-colour fill with the grooves cut out of it, which
is what a mask can carry: verified at 16 px and 24 px in the real tab row under Xvfb,
busier than its neighbours but legible and distinct from `capture`.

**The attribution was wrong three times in two days, each time by reading it out of the
files.** First the `Generator: SVG Repo Mixer Tools` comment was taken as provenance: it
is the editor's signature, written into everything saved out of it, and that editor is
where these are drawn. Then, with that corrected, `capture` and `export` were counted as
borrowed because their headers said `Adapted from SVG Repo` - a clause that had ridden
along from a save-as over a file carrying it. Then ten more stayed on the borrowed list
for the same reason, until the author said plainly that every glyph in the set is
theirs.

So: **nothing inside a glyph file records who drew it.** Not the generator comment, not
the header - a header that disagrees with the author is a stale header, not a provenance
record. `tools/tidy-glyph.py` has lost its `--adapted` flag and the credit line it
wrote, `THIRD-PARTY-NOTICES.md` says all twenty-one are VCW's own with no open item, and
the only third-party art left in `assets/` is `bmc-red-button.svg`, which is Buy Me a
Coffee's own button and appears in `docs/README.md` rather than in the binary.

A drift test between the headers and the notices would have passed on every one of the
three bugs, because the two agreed with each other and were wrong together. That is why
there is not one: the error was in the record, not in drift between two copies of it.

### i18n, the path rather than the extraction

`vcw-i18n` is a new crate holding a TOML catalog, and `i18n/en-US.toml` is the source
language, compiled in with `include_str!` because a VCW that cannot find its own English
cannot print the error saying so.

**The hash is the point.** Every entry in a translation records `source = <FNV-1a of the
English it was made from>`, so `Catalog::stale_against` can report an entry that is
complete, well-formed and confidently out of date - the failure a flat `key = "string"`
map cannot see. FNV rather than `DefaultHasher`, which is documented as unstable across
releases. `vcw doctor --i18n` prints the directory to put a catalog in and the hash of
every key.

The slice built was the one the plan asked for: not the file format, but one string
travelling from a Rust refusal to a window in another language.

- `Aiff::why` and `Flac::why` return `vcw_i18n::t("export.aiff.float")` and friends -
  keys, not sentences, resolved inside `Display`.
- `crates/export/tests/in_another_language.rs` proves a `thiserror` `Display` comes out
  as `NOPE ZORBLAX QUUX` in locale `zz-ZZ`, with English still underneath for the
  untranslated FLAC key and for the generated `alternatives` clause.
- `Settings.language` is a top-level field, not a machine-local one, because the CLI
  prints the same sentences and has no local storage to read.
- *Settings > Appearance* has a Language select, defaulting to US English, built from
  whatever catalogs are in `i18n/` beside the settings file. Names come from
  `Intl.DisplayNames`, so a submitted `pt-BR.toml` appears as *Português (Brasil)*
  without anybody adding a line to a TypeScript table.
- `vcw_i18n::user_dir()` is the single rule for where that directory is, and both the
  shell and the CLI call it. The shell derived it from Tauri's `app_config_dir` first;
  two rules for one path is how a person's catalog ends up somewhere their settings file
  is not. The shell logs the directory at startup and `doctor --i18n` prints it.
- `vcw --lang pt-BR`, and with no flag the CLI reads `language` out of the settings file
  the window writes - the one thing it takes from that file, because a command that
  behaves differently depending on a GUI's saved state is a command nobody can script,
  and a language changes no behavior at all.

Verified in the real window under Xvfb: the menu reads *American English* and *Português
(Brasil)* with a catalog dropped into the config directory and no code change. Verified
on the real CLI: `vcw export --format aiff --narrow refuse` on a Float32 project prints
its refusal in Portuguese, with the alternatives clause still in English because nobody
has translated it - which is the documented partial-translation behavior rather than a
bug.

`every_language_this_repository_ships_is_current` walks `i18n/*.toml` and fails on a
short, stale or orphaned entry. It runs zero times today, and that is the point: the
check has to be in before the first translation arrives, or the first one merges
unchecked and the second is measured against it. Confirmed to fail on purpose with a
stale `pt-BR.toml` dropped in.

**What is left of i18n is the extraction, which is the bulk.** Four keys are in the
catalog. The window's own English - 23 `.tsx` files - and the remaining ~272 `format!`
sites in the crates are untouched, and the architectural question behind them is open:
catalog lookups inside `Display`, as this slice does, or errors carrying a key and
arguments resolved at the presentation layer. The first worked without touching
`thiserror` and is what the four keys use.

### Where this leaves 0.3

AAC is still the licensing question it was, and is still unstarted. `oxideav-aac` is
MIT, pure Rust and an AAC-LC encoder, which would dodge FDK entirely - first published
2026-04-17, so it needs burn-in on media2026 before it earns a place beside the others.
The glyph set is finished and the attribution is settled; the darker expanded-log
variant is unchanged.

---

## 2026-10-09, the first outside report, and a verb to answer it

Nothing was built today. What changed is the shape of 0.3, on the strength of one
message from somebody who does not have the repository.

**Somebody put VCW on piCorePlayer and most of it worked.** LMS on a Raspberry Pi,
TinyCore underneath, a filesystem that lives in RAM, and a turntable already wired to
it for playing records out to Squeezebox. They unpacked the `linux-aarch64` `.deb`
under `/home/tc` - no installer, no packages, the layout taken apart by hand - and used
`vcw` to record a side, search for the tracks and write FLAC. All of that worked. Then
they went looking for the window, found `localhost:5173` in the configuration, and
found nothing listening on it.

They were right twice. 5173 is `devUrl` and the Vite port, present in the build
configuration and absent from a release build, which opens no socket at all; and
installing libwebkit would not have rescued it either. The measurement is the argument:

| | packages linked | installed size |
| --- | --- | --- |
| `vcw` | 3 beyond libc - `libasound2t64`, `libc6`, `libgcc-s1` | 12.2 MB |
| `vcw-app` | **135** | **259 MB** |

`libwebkit2gtk-4.1-0` alone is 95 MB and `libjavascriptcoregtk-4.1-0` another 32. On a
desktop that is a shrug. On a host whose filesystem *is* memory it is the machine.

**The seam for an answer turns out to already exist, and §2 is why.**
`app/ui/src/api.ts` is the only file in the frontend that touches Tauri: 38 functions
over `invoke`, one subscription over `listen`, one `Wire` union on one event name. Its
header says as much and the grep agrees. §2 required that nothing in it decide anything
on the grounds that *"a second frontend would have to make the same choices"* - and the
second frontend has now been asked for by name. So the transport is small: the same
commands over one HTTP endpoint, an event stream, `app/ui/dist` served statically, that
one module swapped.

**The transport is not the work.** The shell trusts its caller absolutely and is right
to - the caller is a window owned by the same user on the same machine. The same 38
commands on a LAN socket read and write arbitrary paths, open audio devices and start
exports, on a box that is also running a music server. So §52 makes loopback the
default, charges an environment-supplied secret for any other address, roots the path
browser that replaces `tauri-plugin-dialog`'s native chooser, and keeps the whole thing
behind a build feature that is off until the verb is invoked. Audio auditions on the
host rather than in the browser, which for this deployment is correct and has to be
said rather than left to look like a fault.

**Written down as §52, and pulled into 0.3 rather than left in Phase 3.** The reason
for jumping the queue is not that it is cheap. Everything else in §46 adds capability
for people who already have a window; this gives a window to somebody who has none, and
the request came from use rather than from speculation. The estimate stays blank on
purpose: the command surface is enumerated and mechanical, the authentication model is
a decision nobody has taken, and a number against the second would be invented. Take
the auth decision first and the rest is transcription.

**Two documents now say the thing that was only ever true implicitly.** `README.md` and
`docs/USER-GUIDE.md` both state that the application is a native window and not a web
page, that there is no port, and that the reason the command line runs where the window
cannot is the 259 MB. Both give the interim answer - capture on the small machine,
*copy* the `.vcw` to a desktop, and do not open it over NFS or SMB, because SQLite's
locking is not dependable there and a project is not a thing to lose.

### Where this leaves 0.3

Three items. The **i18n extraction**, which is the bulk and wants its `Display`-versus-
arguments decision settled before the volume work rather than after it. **AAC**, still a
licensing question before a coding one. And now **`vcw serve`**. The glyphs, AIFF and the
i18n path are done and pushed.
