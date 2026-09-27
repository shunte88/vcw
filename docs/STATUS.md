# VCW - project status

**As of:** 2026-09-27
**Phase:** 1 is underway - WP-01 through WP-16 are built, plus WP-16a, all on Linux x86_64 only.
All five Phase 0 spikes returned verdicts on their primary platform; gate G0 remains
open on hardware coverage, WP-05's soak settled D3's firmed-config run, **WP-06 closes
milestone M1, *it records*,** WP-07 locks D8, WP-08 adds the meters and the §10 fan-out
they read through, WP-09 draws the waveform - and cost the schema two covering
indexes to do it in milliseconds rather than seconds - **WP-10 closes milestone M2,
*it plays back*,** with a seek that joins in a median 19.8 ms on hardware and byte-exactly
in CI, and **WP-11 closes milestone M3, *it finds tracks*,** at 97.6% to 99.7% parity
with VRipr over 595 labelled snippets, exact to the frame, and **WP-12 makes §40's
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
**Branch:** `main` at `260fcdd` (WP-16), with WP-16a uncommitted in the working tree.

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
| **S2** | Can SQLite absorb sustained 24/192 and survive a kill? | **Yes on x86_64/SSD**, 90-minute soak passed. Other platforms open. |
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
was the plug layer's fiction: a request for 48 kHz / 2 ch / I32 was reported as honoured
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
tag-length grammar desynchronises within a few records, so 30/30 clean parses is evidence
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
   against the labelled corpus on a machine with no audio stack.
3. **The Phase 0 spikes leave the product workspace.** Finished evidence, not shipped
   code. A Linux-only CI job keeps them compiling so `docs/spikes/` stays reproducible,
   while the product's four-target matrix stays about the product.
4. **D2 locked** ([ADR-0002](adr/0002-sqlite-binding.md)): `rusqlite` with `bundled`.
   The bundling is not convenience - §15 makes recovery a correctness requirement and
   recovery depends on WAL semantics that vary across the SQLite versions distributions
   ship. A recovery test that passes in CI and fails on a Pi because the OS shipped an
   older SQLite is not a test.
5. **D7 and D10 locked** ([ADR-0004](adr/0004-licence-and-toolchain.md)). The two
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
- **Licence gates**: `deny.toml` with the permissive allowlist, `THIRD-PARTY-NOTICES.md`
  rewritten for VCW's actual dependency set, and `LICENSE-LGPL-2.1` ported. D7 and D10
  locked in [ADR-0004](adr/0004-licence-and-toolchain.md).
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
  shapes and compares bytes. It also proves a read-only open honours the `-wal` - the
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
| `types/format.rs` | `StorageFormat::decode_sample` - one stored sample to a normalised `f32`, with the scaling **measured against the corpus**, not assumed |
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

**The summary pyramid was measured, not ported.** The spike's summariser scaled every
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
  group, the WAL ceiling honoured in pages derived from bytes, progress visible while
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
unrun, and S2's other open questions stay open: disk-full behaviour, induced fsync
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
sidecars. That is the right behaviour - the log is committed data and folding it in is
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
stop, and finalising flushes the part-filled block, so the transport reported a length
up to one block shorter than the row in the project. Fixed by giving `Deck` an
associated `frames(&Report)` function: only the deck knows what its own report means,
and by the time there is a report there is no deck to ask.

**When a capture is finished.** `capture-finished` was published when the transport was
*reset*, because that is where the report is yielded. An operator who stops a side and
walks away would never have been told what was recorded. It is now published on the stop,
and a test pins it to exactly one occurrence - the report lives in two places and
publishing it twice would have a UI catalogue the side twice.

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
[  1.941] capture-finished    capture 1 finalised: 76800 frames, 0 overrun(s), 0 underrun(s), 0 dropped, 0 error(s), bit-perfect no
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
finalises the side rather than abandoning it), a verb with a typo in it (the run fails,
*after* the audio is safe), three commands issued out of turn (rejected, and the project
is left as it was found), and an arm that is thought better of (no capture row at all).

### What is verified, and what is not

Verified: the whole of §11's diagram walked in both directions; every step that is not in
the diagram illegal from every phase, checked exhaustively; a deck that refuses each of
its four operations, including a stop that fails; the clock discounting paused time; two
sides into one project; a device that cannot be opened leaving the transport idle; a
shutdown mid-capture finalising rather than abandoning; two subscribers seeing an
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
for an hour - the lossy-tap behaviour under sustained real load is the WP-05-style soak
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
side, where the writer summarises every block as it commits, but the read side is polled
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
did that**, and 10 s is now only the fallback for a side nothing has been analysed from.

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
request is honoured: `buffer 3840 frames, fixed`.

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
reproduced over all 595 snippets of the labelled corpus, every agreement at the
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
pre + post` behind the analysed position: **1.2 s at the defaults**. A marker is never
retracted, and an adaptive live pass settles nothing at all - the threshold depends on
the whole side - which `Live::settled` reports honestly by returning 0.

The live pass is levels only: no FFT on a tap of the capture stream. The refine pass is
where the spectral extraction happens, once, with all three detectors reading its
frames. That buys a property worth having: a disagreement between two detectors on the
same side cannot be a disagreement about what they were looking at.

### Parity, measured

`crates/signal/tests/vripr_parity.rs`, `#[ignore]`d because it reads 294 MB from outside
the repo. `/data2/vripr_training` is 595 snippets VRipr cut from its own track tables -
16 s of mono 16 kHz audio centred on a boundary, peak-normalised, with a JSON sidecar
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
prints are labelled implied for that reason.

### Tests

496 in the workspace pass and 4 are `#[ignore]`d, the full gate is green, `cargo deny`
is clean and so is the doc leg. 77 of them are in `vcw-signal`'s lib covering the five
new modules, four are on the engine's detection path, three go through the shipped
binary, and one is the parity harness, which is `#[ignore]`d and was run.

### What is not verified

The parity figure is parity, not accuracy. Nothing here has been checked against a
boundary anyone confirmed by ear; the labelled corpus is VRipr's own reading of its own
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
it, §32's genre normalisation ported from VRipr, artwork fetching, an on-disk cache,
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
true behaviour and the docs say so rather than implying something tidier.

### §39: the token goes in a header, because the URL is the cache key

VRipr put `&token=` in the Discogs query string. VCW puts it in an `Authorization`
header, and the reason is not stylistic: the URL is the cache key, the log line and the
thing a person pastes into a bug report. `Token` has no `Serialize`, no `Display` and a
`Debug` that prints a character count, credentials come from the environment only, and
`no_url_anywhere_in_a_discogs_exchange_carries_the_token` asserts it as a property of the
traffic rather than of the code that generates it.

One behaviour worth recording because both answers are defensible: an offline build with
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
presentation, deliberately not pushed down into `Genres::normalise`, which stays
VRipr-compatible for the parity fixture. And MusicBrainz returns no artwork URLs at all:
`cover-art-archive.front == true` means one exists at
`coverartarchive.org/release/<mbid>/front`, which the client then fetches uncached.

### Positions are guesses about a label, so nothing there may fail

`vcw_types::Position::from_str` takes the unambiguous form, `A1`, and nothing else, which
is right for a project file. A provider tracklist is not a project file: it carries
whatever the person who catalogued the record typed off the label. So the grammars live
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

`fetch` guesses the provider from the shape of the id, an MBID being recognisable. The
offline report prints the per-provider detail and the JSON document either way, and only
then fails - an error line on its own loses the thing the caller asked for.

### Tests

153 lib, 3 genre-parity, 11 offline, 9 doc (one of them a `compile_fail` proving `Token`
cannot be serialised), and 8 live tests ignored by default. Workspace total is now
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
extent is the obvious optimisation and it is also the bug.

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
one for an unnamed recording would quietly relabel a *mislabelled* recording instead of
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
the behaviour is right and the words were wrong, so
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

`adopt::Policy` is the judgement WP-11 deliberately declined to make: `min_sources`
defaults to **2**, so a boundary only one detector saw is kept as a row and never becomes
a track. On the real side that is the difference between 270 candidate boundaries and 6,
because the HMM fires at every quiet bar - which is VRipr's behaviour faithfully ported,
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
detector decision for adoption to skip. §24 is honoured one layer earlier than the
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
analysed before the fix would have kept `hmm.hmm.at` at a fixed depth forever. It peels
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
bounds no track. That is the normal state of a side which has been analysed and not yet
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

Verbatim, from a 44.1 kHz 24-bit project on `/data2/vcw-scratch/wp14cli`. `flac -t`
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
  produces it - and choosing how to dither it is a decision about headroom that belongs to
  a person. Refused, with WAV named.
- **`flacenc` 0.5.1 stops at 24 bits and 96 kHz.** Both are the library's limits and
  neither is the format's: FLAC allows 32 bits and 655350 Hz. §8 requires 192 kHz and §33
  requires FLAC, so **this is a real gap in the requirement and not a theoretical one**,
  and `the_flac_library_really_does_stop_where_we_say_it_does` fails the day either cap is
  lifted so the refusal can be deleted.

The 24-bit cap has teeth, because `vcw session` takes the widest integer format a device
offers and that is S32 on this machine: **a default capture cannot be exported as FLAC.**
Narrowing it silently was considered and rejected on measurement - a real 32-bit rip from
`/data2/source_rips` uses the whole low byte (`OR` of every low byte is `0xff`, max
absolute value 2,092,715,264), so dropping eight bits is not lossless and is not the
exporter's decision. `vcw session --format s24` produces a project FLAC will take, WAV
takes any of them, and the remedies for the general case are a 32-bit-capable encoder
(libFLAC 1.4+ through bindings, at the cost of D5's pure-Rust choice) or an explicit,
operator-chosen dither. Both are bigger than WP-14.

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
  `apply_path_template` substitutes first, then splits on `/` and sanitises each segment,
  so a Discogs title of `AC/DC Medley` becomes a directory called `AC` containing a file
  called `DC Medley`, and its `sanitize_filename` maps the nine Windows-hostile
  characters without touching `..`, so a title of `..` climbs out of the output
  directory. The port does not inherit either: `Values::sanitised()` runs *before*
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
  make a FLAC-exportable capture without hardware. `Simulated::deterministic_as` honours
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

### The contract is a core crate, because units are application behaviour

`vcw-contract` depends on `vcw-core`, `vcw-project`, `vcw-audio`, `vcw-types` and
`serde` - and on nothing from Tauri. It holds three things §35 names:

- **Events.** `Wire` is every `vcw_core::Event`, flattened into one discriminated union
  tagged on `kind`, where the tag is exactly the string `Event::name` already returned.
  A test builds one of all fourteen core events and asserts that each maps to a `Wire`
  whose `kind` equals that name, so a new core event that falls through to the catch-all
  fails the build rather than arriving at the frontend as an unlabelled warning.
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
what decides that - it is a design question, not a morning's wiring. `waveform-update` and `fingerprint-match` are declared in `Wire` and
nothing produces them yet.

**A playback failure arrives as a `capture-warning`** with the code `playback-failed`,
because the open happens on a thread and the bus has no playback-refused event. It is a
wart and it is recorded as one; the fix is an event on the bus, not a workaround in the
shell.

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
thing taken away the moment somebody types a catalogue number into a field. The map is
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
  too, thin against the thick locked ones, so the distinction survives greyscale.
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
  `SKIP_SECONDS` when there are none - an unanalysed side, which is the case the fixed
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
once. One behaviour is worth stating because it looks like a bug and is not: `SKIP BACK`
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
added to the contract now fails the suite until something in the shell honours it.

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
  asked for - artist, recording title, catalogue number, none of them required, which is
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
  workaround in the shell.
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
catalogue number, side and track counts, length and size; the waveform strip read **1,920
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

`browse::summarise`'s doc promises "the row a browser draws greyed out with a reason
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
is correct - a capture-only project has nothing analysed in it yet.

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

## Next up

**Where to pick up.** WP-16 is committed at `260fcdd`. **WP-16a is finished and
gate-green but not committed** - it is the working tree, and it corrects WP-16's exit
criterion rather than adding to it, so read its section above before anything else. The
last full gate ran twelve legs -
`fmt / clippy / test / parity / offline / deny / doc` at the root, `appfmt / appclippy /
apptest` in `app/src-tauri` and `uicheck / uitest` in `app/ui` - at **914 passing, 0
failing, 12 ignored** in Rust and 24 passing in the frontend, and the em-dash sweep over
the changed files reports zero. `/data2/vcw-scratch/gate.sh` is the durable copy of the
script.

**First light has been run**, which is what the item below used to ask for, and it is the
reason WP-16a exists. It cost about an hour and found five things in a tree that was
gate-green at 910 tests, two of which no test in the repository could have caught by
construction. The library it ran against is `/data2/vcw-firstlight` - four projects, one
of them a 2.33 GiB real side - and `~/.config/dev.vcw.app/settings.json` points at it.
Both pre-WP-13 projects in it have now been upgraded to v2 by being opened, so re-running
the schema part of that exercise needs a fresh v1 copy.

**The single most valuable thing left is not code.** WP-16 closed the last of Phase 1's
UI work, which means every stage of both §50's chain and §44's workflows now exists - and
**neither has been run once from end to end**. M4 asks for the CLI chain; the window has
never been opened at all. Both runs are cheap, need no new code, and are the only things
that will say whether the parts hold together when nobody is stopping between steps.

**`WP-17`, the nightly soak and QA harness, is next** at weight 7 - the thing that turns
a run that passed once into a run that cannot quietly regress. It is small beside WP-16,
and the order matters: a harness written before the workflows existed would have tested
the parts it could reach.

**What WP-16 leaves behind.** Its section above has the list; three items carry forward.
**Nothing has been driven through a real capture by hand and no screenshot exists** - the
window compiles, every command is tested as a function and every keybinding is tested as
a wire, but arm-record-stop-play-export has never been clicked. **A playback open failure
still arrives as a `capture-warning`** coded `playback-failed`, because the open happens
on a thread and the bus has no playback-refused event; the fix is an event on the bus.
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
track edge, falling back to ten seconds only on a side nothing has been analysed from.
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
pushed**, and WP-16 decided to keep it that way. The writer summarises every block as it
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
  the schema moves, or `the_committed_document_matches_the_schema` fails.
- **The gate is eleven legs now**, because the shell is a workspace of its own and the
  root's legs cannot see it: `fmt clippy test parity offline deny doc` at the repository
  root, `appfmt appclippy apptest` in `app/src-tauri`, and `uicheck` (`pnpm check`) in
  `app/ui`. `/tmp/gate.sh` runs all eleven, tallies `gate-test.log` and
  `gate-apptest.log` together, and sweeps the changed files for em dashes - a sweep that
  now covers `.ts`, `.tsx`, `.css` and `.json` as well. The two new legs earned their
  keep on the first run: `doc` found public documentation in the new crate linking to a
  private item, and `appclippy` found a constant that only its own test reads.
  Building the shell needs the frontend built first, since `tauri-build` fails when
  `frontendDist` is missing.
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
- `.bench/` holds **~8 GB** of scratch databases - the 8.4 GB soak artefact plus older
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
- Licensing today: MIT core, cpal Apache-2.0 as an ordinary dependency.
  `chromaprint-next` adds an LGPL-2.1-or-later relink obligation at Phase 2.
- **Stay current on CPAL.** Two blocking defects and the device-id API all landed within
  two minor releases; pinning 0.16 had already cost us a fork.
