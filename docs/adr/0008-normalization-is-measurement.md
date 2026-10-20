# ADR-0008: Normalization is a measurement, not a gain

**Status:** Accepted 2026-10-20.
**Requirements:** §9, §33, §51 · **Work package:** 0.4.

## Context

A record is cut at whatever level the mastering engineer chose, and a transfer is made at
whatever level the operator set on the preamp. Two sides of the same album can land 6 dB
apart, and two albums in the same library can land 12 dB apart. A person who exports a
shelf of vinyl and puts it on a player gets a collection that needs the volume control
between every track. That is the problem normalization exists to solve, and it is a real
one: it is the single most common complaint about a home-made library.

There are two ways to solve it and they are not variations on each other.

**Change the samples.** Measure the loudness, multiply every sample by the difference, and
write the result. The file now plays at a predictable level in anything, including
hardware that has never heard of a loudness tag.

**Write the measurement down.** Measure the loudness, write the number into the file's
tags, and leave the audio alone. A player that understands the tag applies the gain at
playback. A player that does not plays the file as it was.

The first one is not available to VCW. §9's bit-perfect claim is that the audio leaving an
export is the audio the converter produced, put through the repairs and the curve the
operator asked for and nothing else. WP-14's exit criterion is a byte comparison that
proves it. An applied gain would make that test fail, and the honest way to describe the
result would be that VCW is a tool that quietly changes your transfers - which is the
opposite of what an archival tool is for.

## Decision

**VCW measures loudness and never applies it.**

1. **ReplayGain 2.0 tags, four of them per file**: track gain, track peak, album gain,
   album peak. Gated integrated loudness to EBU R 128, referenced to **-18 LUFS**, with a
   true peak so a player applying the gain can tell in advance that it would clip.

2. **The measurement is taken on the bytes that reach the file**, immediately before the
   writer is handed them: after the repair stage, after §51's playback curve, after a fold
   to mono, after a narrowing to 16 or 24 bits. What is described is what is in the file,
   not what is in the project.

3. **Album gain is one gate over the whole side**, not the mean of the track gains. The
   R 128 gate discards blocks below a relative threshold, so a quiet track contributes
   fewer surviving blocks than a loud one and an average is simply a different number.
   This is why `splitter::run` cuts every file before it tags any of them.

4. **On by default**, with `--no-replaygain` on the command line and a tick in the Export
   panel. A tag a player ignores costs nothing; a library exported without one plays
   unevenly, and the person who wanted it is unlikely to know the setting exists.

5. **Silence gets no tag.** A gated loudness of `-inf` would become an infinite gain.

## What this rules out

- **A normalize-on-export switch that multiplies samples**, at any reference, with any
  limiter. Not a deferral: a build of VCW that writes an altered sample on an export path
  has broken the claim the rest of the tool is built to make.
- **ReplayGain 1.0's 89 dB reference.** The two land about 5 dB apart on real material, so
  a library holding both sounds uneven in exactly the way the tag exists to prevent.
- **Writing the number into the project.** The measurement describes an exported file at
  the width and curve that export chose. Another export at another width is another
  number, and storing one of them would make the project claim something about audio it
  does not hold.
- **A `Chain` stage.** WP-30's chain is for things that change samples. A meter that
  reported its findings by sitting in the signal path would be a filter that does nothing,
  and `Chain::is_empty` - which is what §9's refutation asks - would stop meaning what it
  means.

## Why this is the same answer as the rumble filter

§51's IEC rumble filter is parked for the same reason in the other direction: it is a
change to the samples that a person has to ask for, by name, knowing what it does. The
rule both decisions come from is that **VCW does not improve a transfer behind the
operator's back.** A repair is asked for per defect. A curve is asked for by name, and an
unstated curve means silence rather than a guess. Loudness is written down rather than
imposed. In every case the operator can get the other behavior from another tool, applied
to a file VCW handed over unmodified - and cannot get the unmodified file back from a tool
that decided for them.

## Evidence

- **EBU Tech 3341 case 1 passes**: a 1 kHz sine at -23 dBFS measures -23.0 LUFS, which
  against a -18 LUFS reference is +5.00 dB. That single number checks the meter, the
  reference and the sign at once.
- **The gate is not an average, measured rather than assumed.** A 20 s sine at -23 dBFS
  and a 20 s sine at -43 dBFS give an album gain within 0.1 dB of the loud track's own,
  because the quiet track's blocks fall below the relative threshold. The mean is 10 dB
  away.
- **Measuring moves no samples**, asserted directly: the same side exported twice, once
  measured and once not, has byte-identical audio in both files. WP-14's existing
  bit-exactness tests also run with measurement on, which is the same proof taken from the
  other end.
- **The meter costs 145x real time at 192 kHz**, measured on a two-side project of
  3123 s of 192 kHz stereo. Measured against unmeasured: FLAC 95.18 s against 73.59 s,
  WAV 45.79 s against 24.18 s. The delta is +21.6 s either way, because the work is per
  sample and has nothing to do with the encoder - which also means it is +29% of a FLAC
  export and +90% of a WAV one. True peak oversampling is most of it. A 20 minute side at
  96 kHz costs about 4 s. Peak RSS rose 8.4 MB for seven simultaneous meters.
- **lofty 0.25 reaches every container we write** from one `ItemKey`: Vorbis comments in
  FLAC and Ogg, ID3v2 `TXXX` frames in WAV, AIFF and MP3, and an iTunes freeform atom in
  ALAC. Verified by `ffprobe`, which did not write them.
- **`ebur128` 0.1.10 is MIT** with four small dependencies, and has the album mode:
  `loudness_global_multiple` gates across a set of meters, which is point 3 above and is
  the part that would have been wrong if it had been written here.
