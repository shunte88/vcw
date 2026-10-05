# S4 - chromaprint-next streaming fingerprints

**Status:** COMPLETE on Linux/x86_64. Acceptance met, and met with more margin than asked for.
**Date:** 2026-09-24
**Requirement:** REQUIREMENTS §25, §26, §46 · **Plan:** PROJECT_PLAN §4 (S4)
**Artifact:** [`spikes/fingerprint-stream`](../../spikes/fingerprint-stream)
**Corpus:** `/data2/source_rips` - 62 real vinyl rips, 71 GB, 48 kHz and 192 kHz 32-bit WAV plus 24-bit FLAC

## Question

The plan asked one thing: feed live-shaped PCM chunks through
`Fingerprinter::feed()` and assert the fingerprint equals the offline
fingerprint of the same region. §25 needs that so it can fingerprint
**candidate regions progressively** instead of re-fingerprinting the whole
recording, and §46 needs the fingerprint worker to be incapable of starving
capture.

Chunk equivalence turned out to be the easy half. The harder question, which
§25 also depends on and the plan did not name, is what happens when the
detector puts the region boundary in slightly the wrong place - because it
will.

## Answer

**Yes, and the boundary does not have to be accurate.**

Chunk shape is irrelevant: every shape tried, down to feeding one frame at a
time, reproduces the offline fingerprint **bit for bit**. Region boundary error
costs at most **0.064 BER** - reached at half a sub-fingerprint step, 62 ms -
and that is the worst case, not a tail. For scale, unrelated audio scores
**0.47–0.49** and an MP3 320k round-trip costs **0.0009**. The fingerprint
worker costs **0.79 MiB** per stream and, on a whole 192 kHz side read off disk,
**0.6% of one core** - of which most is the disk read; the fingerprinting alone
is 0.06%.

The consequences for Phase 2 are concrete:

- Region fingerprinting can be driven straight off the capture stream at the
  capture rate. No pre-decimation, no staging file, no re-fingerprint pass
  after boundary refinement.
- The detector does not need sample-accurate boundaries for identification's
  sake. It needs them for *editing*, which is a different requirement with a
  different tolerance.
- Eight candidate regions can be open at once for **6.3 MiB** and, by arithmetic,
  under 5% of a core. Measured, eight was cheaper than that, because they share
  the one read of the stream.

## Test 1 - chunk shape invariance *(the plan's acceptance criterion)*

120 s from 60 s in, `Algorithm::Test2` (the AcoustID default), fed as the
offline one-shot and then as each chunk shape. Shapes were chosen to hit the
boundaries that actually exist in the capture path: the ALSA period and buffer
sizes S1 measured, S2's 250 ms commit block, chromaprint's internal 32768-sample
mono buffer, a prime size, and a ragged sequence standing in for uneven ring
drains.

```
$ fingerprint-stream chunk "/data2/source_rips/King Buffalo_Repeater.wav"
  region      120.0 s from 60.0 s (23040000 frames @ 192000 Hz, 2 ch)
  offline     948 sub-fingerprints in 90 ms  [38885268 2b88e618 2ac8ae18 2a48b929]

  chunk shape                          items  identical       BER    elapsed
  64 frames                              948        yes  0.000000       95 ms
  256 frames                             948        yes  0.000000       92 ms
  997 frames (prime)                     948        yes  0.000000      108 ms
  4096 frames                            948        yes  0.000000       80 ms
  16384 frames (ALSA period)             948        yes  0.000000       77 ms
  32768 frames (ALSA buffer)             948        yes  0.000000       74 ms
  32769 frames (buffer + 1)              948        yes  0.000000       81 ms
  48000 frames (250 ms block)            948        yes  0.000000       96 ms
  ragged 1..8192 frames                  948        yes  0.000000       85 ms

  PASS: every chunk shape reproduces the offline fingerprint exactly
```

The pathological shape - **one frame per `feed()` call**, 3.84 M calls for a
20 s region - is also bit-identical, and costs only 4× the one-shot:

```
$ fingerprint-stream chunk "/data2/source_rips/King Buffalo_Repeater.wav" --len 20
  1 frame                                140        yes  0.000000       83 ms
  4096 frames                            140        yes  0.000000       19 ms
```

Same result at 48 kHz (`Manitoba_Jacknuggeted EP.wav`, 10/10 shapes exact).

**Why it holds, not just that it holds.** `AudioProcessor::consume` accumulates
into a fixed 32768-sample mono buffer and only resamples when that buffer fills,
so the resampler always sees the same input in the same 32768-sample units
regardless of how the caller split the feed. `finish()` flushes the remainder.
Nothing in the chain is sensitive to call granularity. This is a structural
property of the crate, not a coincidence of these inputs - but it is worth
re-asserting in CI, because it is exactly the kind of property an optimization
could quietly break - so it is. `cargo test -p fingerprint-stream` asserts chunk
invariance, instance independence and the unrelated-audio baseline against a
deterministic synthetic signal, in 0.5 s, with no corpus on disk:

```
running 3 tests
test fp::tests::instances_are_independent ... ok
test fp::tests::ber_of_unrelated_signals_is_near_a_coin_flip ... ok
test fp::tests::chunk_shape_does_not_change_the_fingerprint ... ok
```

The corpus runs in this report are the evidence; those tests are the ratchet.

### One contract the API does not enforce

`feed()` takes `&[i16]` and `AudioProcessor::consume` checks frame alignment
with `debug_assert!`, not `assert!`. A release build handed a partial frame
will silently transpose the channel interleave for the rest of the stream and
produce a plausible, wrong fingerprint. **VCW's fingerprint worker must
guarantee whole frames at the type level** rather than rely on the callback
happening to deliver them. `fp::streamed` in the spike takes chunk sizes in
frames for this reason.

## Test 2 - region boundary sensitivity *(the finding that matters)*

The reference is the region cut exactly where the detector would ideally cut
it. Each row moves the start by some number of frames and re-fingerprints the
same duration. `BER unshifted` is what a naive elementwise comparison sees;
`BER best` is the best over ±64 sub-fingerprint shifts, which is what a matcher
that aligns before scoring sees.

One sub-fingerprint covers `1365/11025 = 0.1238 s` (8.08 items/s).

```
$ fingerprint-stream align "/data2/source_rips/King Buffalo_Repeater.wav"
    start offset       = ms   items BER unshifted   BER best  shift
               1        0.0     948      0.000000   0.000000      0
              64        0.3     948      0.000626   0.000626      0
            2971       15.5     948      0.016581   0.016581      0
           11885       61.9     948      0.064049   0.064049      0
           23771      123.8     948      0.124011   0.000000     -1
          190168      990.5     948      0.355584   0.000033     -8
          192000     1000.0     948      0.357364   0.010239     -8
          960000     5000.0     948      0.382483   0.049009    -40
          -11885      -61.9     948      0.066917   0.064117      1
          -23771     -123.8     948      0.124143   0.000033      1
         -192000    -1000.0     948      0.357133   0.010605      8
```

Three recordings across both rates agree to three decimal places on every row
that matters:

| start offset | 192 kHz *Repeater* | 48 kHz *Agor* | 48 kHz *Jacknuggeted* |
|---|---|---|---|
| ½ step (61.9 ms) | 0.0640 | 0.0641 | 0.0620 |
| 1 step (123.8 ms) | 0.0000 | 0.0001 | 0.0000 |
| 8 steps (990 ms) | 0.0000 | 0.0003 | 0.0001 |
| 1000 ms (8.07 steps) | 0.0102 | 0.0116 | 0.0095 |
| 5000 ms (40.4 steps) | 0.0490 | 0.0496 | 0.0500 |

*(all columns are `BER best`)*

**The model.** A region start error decomposes into a whole number of
sub-fingerprint steps plus a remainder. The whole-step part is free - it is a
pure shift, and the matcher absorbs it completely. Only the **sub-step
remainder** costs anything, and its cost depends on the remainder alone, not on
how large the total error was: a 5-second error (40.4 steps) costs 0.049, less
than a 62-millisecond error (0.5 steps) costing 0.064, because 0.4 of a step is
closer to alignment than 0.5 of one.

So the penalty is **bounded at ~0.064 regardless of how wrong the boundary is**.
That is the number Phase 2 should design against.

## Test 3 - capture rate

Does the rate we happen to capture at change the fingerprint? It matters
because AcoustID's stored fingerprints were submitted from whatever rate the
submitter had, which we do not control.

```
$ fingerprint-stream rate "/data2/source_rips/King Buffalo_Repeater.wav"
        rate   items BER vs native   BER best  shift
      192000     948             -          -      -
       96000     948      0.000000   0.000000      0
       48000     948      0.000000   0.000000      0
       44100     948      0.000000   0.000000      0

$ fingerprint-stream rate "/data2/source_rips/Side B Raw WAV.wav"
       96000     948      0.000198   0.000198      0
       48000     948      0.000165   0.000165      0
       44100     948      0.000165   0.000165      0
```

Zero on one recording, ≤0.0002 on the other - the same magnitude as the
single-bit resampler noise in Test 5. **Rate is a non-issue.** Fingerprint off
the capture stream at whatever rate the user chose; do not decimate first.

## Test 4 - throughput and headroom

```
$ fingerprint-stream throughput "/data2/source_rips/King Buffalo_Repeater.wav" --len 300
  region      300.0 s of audio @ 192000 Hz 2 ch
  shape                                wall      Melem/s  RT factor   headroom
  one shot                           0.190 s        302.9    1577.8x      0.06%
  48000 frames (250 ms block)        0.181 s        318.9    1660.8x      0.06%
  4096 frames                        0.177 s        325.9    1697.6x      0.06%
  ragged 1..8192 frames              0.176 s        327.2    1704.0x      0.06%
```

48 kHz is 3579–4116× real time. Chunk shape has no meaningful effect on cost,
so the worker can drain the ring in whatever units the ring gives it.

## Test 5 - cross-check against the C reference

The crate claims bit-identical output to C. That claim is load-bearing for
AcoustID match rates, so it gets checked here rather than taken on trust -
the same discipline that made S1 worth running.

`fpcalc` reads raw PCM, so both implementations can be handed **byte-identical
input** and the decode path eliminated entirely. Two cases: (A) the native
capture rate, so both libraries resample internally; (B) pre-resampled to
11025 Hz mono by `sox`, so neither library's resampler runs and only the
FFT/chroma/classifier stages are under test.

```
$ fingerprint-stream reference "/data2/source_rips/Side B Raw WAV.wav"
  fpcalc version 1.6.0 (FFmpeg Lavc62.11.100 Lavf62.3.100 SwR6.1.100)

  case                                            mine  fpcalc  identical       BER  items≠  bits≠
  A: native rate, both libraries resample          948     948         NO  0.000198       6      6
      #301 a0efd91b^a0efd90b  #325 a16bd6cb^a16bf6cb  #326 a12deecb^a129eecb
      #337 e26d2d36^e26d2d3e  #596 76aa3816^76aa381e  #750 9fad2c1a^9ead2c1a
  B: pre-resampled to 11025 Hz mono, neither does  948     948        yes  0.000000       0      0
```

Nine recordings at both rates:

| recording | rate | case A | case B |
|---|---|---|---|
| Manitoba - Jacknuggeted EP | 48k | exact | exact |
| Pole - Fading | 48k | exact | exact |
| Klaus Schulze - Deus Arrakis | 48k | exact | exact |
| Koreless - Agor | 48k | 5 bits / 0.000165 | exact |
| Nils Frahm - Encores One | 48k | 2 bits / 0.000066 | exact |
| King Buffalo - Repeater | 192k | exact | exact |
| King Buffalo - Demo 10th | 192k | 5 bits / 0.000165 | exact |
| Side A Raw | 192k | 2 bits / 0.000066 | exact |
| Side B Raw | 192k | 6 bits / 0.000198 | exact |

**The post-resample pipeline is bit-identical to C on every recording tested,
9 for 9.** With resampling in the path, 5 of 9 differ - always by **single
isolated bit flips**, 2 to 6 of 30,336 bits, at both rates. That is the
signature of a classifier sitting exactly on its threshold and being tipped by a
±1 LSB difference in the resampled stream, not of a structural divergence.

**Cause, as far as this spike establishes it.** `ldd` shows the packaged
`fpcalc` (Ubuntu `libchromaprint-tools 1.6.0-2build1`) links
`libswresample.so.6` and `libsoxr.so.0`, so that build does not use
chromaprint's bundled `av_resample` - which is what chromaprint-next ports.
Case B isolates the difference to the resample stage conclusively, since B
shares every other stage with A and is exact.

I tried to close the loop by letting FFmpeg's swresample do the downmix and
resample and then fingerprinting *that* with chromaprint-next (case C, in the
tool). It did **not** converge on fpcalc's answer either - 1 to 6 bits, in
different places. So the `ffmpeg` CLI's resampler configuration is not
chromaprint's either, and the hypothesis is **not confirmed**: all that is
established is that a resampler difference exists and that it is confined to
±1 LSB effects. Settling it properly needs libchromaprint built from source
with its bundled resampler, which this spike did not do.

**Why that is acceptable and not a loose end.** Test 6 puts the number in
context: 0.0002 is **five times smaller than a 320 kbps MP3 round-trip**, which
is a perturbation AcoustID handles as a matter of routine. It is ~2400× below
unrelated audio. It cannot change a match outcome. The finding is recorded
because it contradicts a blanket reading of the crate's "bit-identical" claim -
the claim holds against libchromaprint built from its own tree, not against
every distro build of `fpcalc` - and because that distinction would be
expensive to rediscover from a failing test later.

## Test 6 - what the BER axis actually means

Every other number in this report is unreadable without a scale, so the scale
is measured rather than assumed. Reference is the 24-bit source truncated to
i16 and fed at the native rate.

```
$ fingerprint-stream scale "/data2/source_rips/Manitoba_Jacknuggeted EP.wav" \
                           "/data2/source_rips/Klaus Schulze_Deus Arrakis.wav"
  perturbation                                            BER   BER best  shift
  round to nearest instead of truncate               0.000000   0.000000      0
  gain -6 dB                                         0.000000   0.000000      0
  gain -20 dB                                        0.000000   0.000000      0
  gain +3 dB                                         0.000000   0.000000      0
  MP3 320k round-trip                                0.000857   0.000857      0
  MP3 128k round-trip                                0.004714   0.004714      0
  same record, region 300 s later                    0.449400   0.441144     39
  unrelated record (Klaus Schulze_Deus Arrakis.wav)  0.484705   0.468578    -37
```

The ladder, smallest to largest:

| perturbation | BER |
|---|---|
| i16 narrowing: truncate vs round to nearest | **0.000000** |
| gain, −20 dB to +3 dB | **0.000000** |
| C-reference resampler difference (Test 5) | 0.0002 |
| capture rate, 192k vs 44.1k (Test 3) | 0.0002 |
| MP3 320k round-trip | 0.0009 |
| MP3 128k round-trip | 0.0047 |
| **worst region boundary error (Test 2)** | **0.064** |
| unrelated audio | 0.47–0.49 |

Two of these are decisions VCW owns, and both are now settled:

- **Sample narrowing is free.** A plain `>> 16` and a proper round-to-nearest
  give the identical fingerprint. The worker does not need dither or careful
  rounding for the fingerprint's sake.
- **Level is free.** −20 dB to +3 dB is invisible, because chroma
  normalization removes it. This matters for vinyl specifically: cartridge,
  preamp gain and pressing level vary between plays and between users, and none
  of it reaches the fingerprint.

## Test 7 - the worker as it will actually run

Streams a whole side off disk in 250 ms blocks, holding one block at a time -
no region resident, which is how the real worker behaves and which the Test 4
numbers do not show because they hold the region in RAM.

```
$ fingerprint-stream live "/data2/source_rips/King Buffalo_Repeater.wav"
  audio       1349.3 s (259073318 frames) @ 192000 Hz 2 ch
  block       250 ms = 48000 frames = 0.18 MiB of i16
  blocks      5398
  items       10877 sub-fingerprints (42 KiB)
  wall        8.72 s  (155x real time, 0.646% of one core)
  feed time   p50 0.25 ms · p99 0.71 ms · max 1.20 ms  (budget 250 ms/block)
  disk read   p50 1.45 ms · p99 2.30 ms · max 21.64 ms
  peak RSS    6.5 MiB (was 3.7 MiB before the stream)

$ fingerprint-stream live "/data2/source_rips/Klaus Schulze_Deus Arrakis.wav"
  audio       4538.5 s @ 48000 Hz 2 ch
  blocks      18154
  items       36635 sub-fingerprints (143 KiB)
  wall        7.73 s  (587x real time, 0.170% of one core)
  feed time   p50 0.01 ms · p99 0.33 ms · max 0.66 ms  (budget 250 ms/block)
  peak RSS    6.4 MiB
```

A 22-minute 192 kHz side and a 75-minute 48 kHz one both run in **6.5 MiB**,
with `feed()` taking **0.3% of its 250 ms block budget at p99**. The 155×
figure is lower than Test 4's 1578× because disk reads dominate it (1.45 ms
read against 0.25 ms of fingerprinting per block) - the fingerprinting itself
did not get slower.

Fingerprint accumulation is bounded in practice: a whole side is 42–143 KiB,
and §25 fingerprints regions rather than whole sides, so this is the
pessimistic case.

### Concurrent regions

Eight independent `Fingerprinter`s over the same blocks, as several candidate
regions would be:

```
$ fingerprint-stream live "/data2/source_rips/King Buffalo_Repeater.wav" \
                          --instances 8 --len 600
  feed time   p50 1.03 ms · p99 1.99 ms · max 4.80 ms  (all instances; budget 250 ms/block)
  peak RSS    10.1 MiB (was 3.8 MiB before the stream; 0.79 MiB per instance)
  agreement   all instances produced the identical fingerprint
```

Cost is linear and tiny: **0.79 MiB per open region**, all eight combined
taking 2 ms of a 250 ms budget. Bit-for-bit agreement across instances also
rules out shared mutable state, which is what makes it safe to run several on
one worker thread or to move one between threads.

## Dependency decision

`chromaprint-next 0.1.0` **from crates.io** is the dependency of record. This
applies the S1/CPAL lesson directly: the vendored CPAL fork hid how stale the
pin had become, and deleting it was what surfaced that both defects had been
fixed upstream. No `[patch.crates-io]`, no path copy.

The local checkout at `/data2/chromaprint-next` sits two commits ahead of
0.1.0, both SIMD work (`Add SIMD-accelerated FFT window application`, `Add
SIMD-accelerated stereo-to-mono mixing`). Those are exactly the kind of commits
that could change output, so they were checked rather than assumed: two
throwaway crates outside the workspace, one on the registry version and one on
the path, fingerprinting the same fixture.

```
published 0.1.0:  705 AQACwUn2aNkG_thzQEPzLLh1_EEokUdT5zhPo01zNGlD4VtQ3Zjy5QO949hJ...
local HEAD:       705 AQACwUn2aNkG_thzQEPzLLh1_EEokUdT5zhPo01zNGlD4VtQ3Zjy5QO949hJ...
diff: IDENTICAL
```

**The SIMD work is fingerprint-neutral.** The policy, matching CPAL's: track
the released crate; if we find a defect, fix it in a `shunte88` fork of record
and upstream it, rather than carrying a local path copy.

### Licensing

This is the first LGPL contact point. `chromaprint-next` is `MIT AND
LGPL-2.1-or-later` - the LGPL part is the `av_resample` port in
`src/audio/resample.rs`, derived from FFmpeg. Nothing changes today because the
spike is not shipped, but when the crate lands in Phase 2 the workspace license
becomes `MIT AND LGPL-2.1-or-later` and the relink obligation attaches, as
PROJECT_PLAN already anticipates. `THIRD-PARTY-NOTICES.md` and
`LICENSE-LGPL-2.1` are still to be ported in WP-01.

## Conclusions

1. **The plan's acceptance criterion is met.** Streamed and offline fingerprints
   are bit-identical for every chunk shape, down to one frame per call, at 48 kHz
   and 192 kHz.
2. **§25's progressive-region approach is confirmed and is cheaper than it
   looked.** The boundary penalty is bounded at ~0.064 BER - 7.3× better than
   unrelated audio - so no re-fingerprint pass is needed after the detector
   refines a boundary.
3. **§46's isolation requirement is satisfied by a wide margin.** 0.79 MiB and
   0.6% of one core per stream, `feed()` at 0.3% of its block budget at p99.
   The fingerprint worker cannot starve capture.
4. **Fingerprint from the capture stream at the capture rate.** Rate, gain and
   sample-narrowing are all measurably free.
5. **chromaprint-next matches the C reference where it counts** - the whole
   pipeline after the resampler, exactly, on 9 of 9 recordings. A resampler
   difference against the distro `fpcalc` exists and is bounded at 0.0002,
   five times below an MP3 320k round-trip.

## Honest limits

- **Linux/x86_64 only.** Pure Rust with one FFT dependency, so no
  platform-specific risk is expected - but S1 is the standing reminder that
  "expected" is not "measured". Re-run on Pi 5 (aarch64) and Windows with the
  rest of the Tier 1 matrix. The SIMD paths in the local checkout are NEON and
  x86 specific and deserve the same cross-check on aarch64 that was done here.
- **No AcoustID lookup.** Everything here is fingerprint *generation* and
  self-consistency. Whether these fingerprints actually resolve against
  AcoustID's database - the §26/§27 question - is untested, needs an API key,
  and is Phase 2 work. The BER ladder says a match should not be lost to
  anything VCW does, but that is an argument, not a measurement.
- **The C-reference resampler difference is characterised, not root-caused.**
  See Test 5. Bounded, immaterial, open.
- **Test2 only.** The other four algorithm variants are untested; AcoustID uses
  Test2 and VCW has no reason to use another.
- **Region boundaries were synthesised, not detected.** The offsets in Test 2
  are what a detector might plausibly be wrong by, chosen to straddle the
  sub-fingerprint step. Real detector error distribution arrives with WP-11.
- **The frame-alignment contract is unguarded in release builds.** Noted in
  Test 1; it is the one way to misuse this API and get a plausible wrong answer.
