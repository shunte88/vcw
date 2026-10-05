# ADR-0006: Playback rate and format policy

**Status:** Accepted 2026-09-26.
**Requirements:** §9, §21, §38 · **Work package:** WP-10 · **Decision:** not in the plan's
D-table; locked by the work.

## Context

§9 forbids the *capture* path from resampling, converting formats, mixing channels or
touching gain, and says so in a list. §21 says nothing of the kind about playback. It
asks for six transport operations, four audition targets, and that "native/bit-perfect
playback should be available where supported" - which is permission for a fallback, not
a description of one.

So playback had to decide what happens when the stored audio and the output device do
not agree. There are two axes and they are not symmetric:

- **The rate.** A 192 kHz side on a device that only does 48 kHz cannot be played
  without resampling, and a resampler is a signal processing decision with an audible
  result, a quality/cost tradeoff and no correct default.
- **The format.** A 24-bit side on a 32-bit stream is a shift. It is exactly
  reversible, costs nothing measurable, and sounds identical - but it is still a
  conversion, and a converted path cannot be called bit-perfect.

One more constraint comes from the archive's own purpose. A vinyl rip is captured once
and kept; the reason §9 is as strict as it is, is that the capture is the artifact and
everything else is a view of it. Playback is how an operator decides whether the capture
is good. A playback path that quietly changed the audio would be a poor instrument for
that judgment, whatever it sounded like.

## Decision

**No resampler anywhere in VCW. A capture plays at its own rate or it does not play.
Formats may be converted, losslessly only, and the conversion is always reported.**

Concretely, as built in `vcw-audio::playback` and `vcw-audio::convert`:

| | Policy | On a mismatch |
|---|---|---|
| Sample rate | Never converted | `Error::RateUnavailable`, naming the rates the device does offer |
| Channel count | Never mixed or spread | Refused the same way |
| Sample format | Converted, losslessly | Played, and the conversion is named in the `auditioning` event and the fidelity verdict |

The format preference order is the **opposite** of capture's, and deliberately so.
Capture takes the best the device offers, because a better capture is strictly better.
Playback takes the format that matches what is already on disk, because anything else is
a conversion: `convert::natural` gives the device format a stored format would rather be
played in, and maps padded 24-bit to S24 rather than S32 because the bytes are the same
three bytes and the narrower stream is the one that can still be bit-perfect.

Conversion goes through **left-justified `i32`** as its pivot, so widening and narrowing
are shifts with no rounding and no arithmetic. Padded 24-bit is measured as ±2^23 and
scaled by 256 on the way in. `natural` is lossless for every storage format VCW writes,
which is what lets the render path be byte-exact against the stored blocks.

`Fidelity` is three-way - Confirmed, Refuted, Unconfirmed - like capture's `verdict`,
and one of its refutations exists only on this side: **the samples were converted for
the device.** "Nothing rules it out" is never reported as a pass.

## What this rules out

- **A resampler behind a quality setting.** Rejected: it makes the answer to "does this
  capture sound right" depend on a preference the operator has probably forgotten
  setting. If VCW ever needs sample-rate conversion it belongs in export (§33), where
  the output is a new artifact that says what it is, and not in the monitoring path.
- **Playing a 192 kHz side through the system mixer at 48 kHz because it is available.**
  Rejected: that is precisely the silent conversion S1's finding 1 was about, arriving
  from the other direction. A refusal that names the device's real rates is more useful
  than audio that plays and cannot be trusted.
- **Choosing the widest format the device offers.** Rejected: it converts every side
  that is not already that wide, for no benefit, and costs the bit-perfect claim on all
  of them.
- **Reporting bit-perfect on the strength of the API's own success.** Already ruled out
  for capture by §9 and `verify::against`; playback uses the same verifier and inherits
  the same rule.

## Consequences

- A device that cannot play a capture is a **refusal with a reason**, not a fallback.
  `crates/core/tests/playback_live.rs::a_rate_the_device_cannot_play_is_refused` is the
  test; `vcw play` prints the rates on offer.
- The render path (`vcw play --render`) can be compared byte-for-byte against the stored
  blocks, because `natural` loses nothing. That is what makes a gapless seek testable in
  CI with no sound card, and it would not be possible if playback were allowed to
  convert on its own initiative.
- **Export, not playback, is where rate conversion will be asked for.** §33 will
  eventually want 44.1 kHz FLAC from a 96 kHz side, and that is a different decision with
  a different answer: an export produces a new file that documents its own provenance,
  where a monitor produces a judgment about an existing one.
- The 44.1/88.2 family and the 48/96/192 family are therefore **not interchangeable** on
  output. A converter that does only one family can only audition captures from that
  family, which is a property of the hardware rather than of VCW, and is reported as
  such.
