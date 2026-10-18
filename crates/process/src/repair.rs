/*
 *  repair.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The repair stage: stored defect rows applied to audio on the way out (WP-30 §8 step 5).
 *
 * MIT License
 *
 * Copyright (c) 2026 Stue Hunter
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */

//! The repair stage (WP-30 §2, §4, §8 step 5).
//!
//! A repair stage is not a filter. A filter *is* its parameters and can be
//! re-derived from a row; a repair is a stored list of spans plus a cheap
//! deterministic fix, because detection is expensive, heuristic and sometimes
//! wrong, and a person has to be able to veto one click without vetoing the rest.
//! That is why the defects live in a table and nothing is ever written back to the
//! sample blocks: §4.1 keeps the captured PCM immutable and this module computes
//! the repair every time audio leaves the project.
//!
//! **Context is the whole problem.** Interpolating a gap needs samples either side
//! of it, and a buffer handed to the chain has nothing before its first frame and
//! nothing after its last. WP-30 §4 names the two honest answers - widen the read
//! and discard the margin, or leave straddling defects alone and say so - and says
//! the first repair stage should take the first. So it does: [`Repairs::context`]
//! states how far either side the stage needs to see, the two call sites read that
//! much extra and hand it over as [`crate::Context`], and the stage repairs a span
//! using audio the caller will not emit from this buffer. The same mechanism covers
//! both edges a span has - the track's first frame and every chunk boundary inside
//! it - because to the stage they are the same question.
//!
//! The interpolator is a cubic Hermite across the gap with Catmull-Rom tangents,
//! which needs exactly two good samples each side. Two properties earn it the job.
//! It is **exact on a straight line**, which is what makes the test below able to
//! name a right answer for every repaired sample rather than eyeballing a plot.
//! And it is **bounded**: each tangent is measured across the whole gap rather than
//! from the slope of the last two samples, so it behaves like a chord and cannot
//! run away on a long span. A per-sample slope extrapolated over a few hundred
//! frames diverges, and an overshoot is a worse artifact than the click it
//! replaced.
//!
//! ponytail: Catmull-Rom is the cheap rung. The published method is an
//! autoregressive interpolation (Janssen) that re-fits the model the detector
//! already fitted and predicts into the gap from both directions. It would read
//! `order` samples of context rather than two, which is a bigger
//! [`Repairs::context`] and nothing else: the callers derive their read width from
//! that number, so the upgrade does not reach them. Do it when a listener says the
//! gaps are audible, not before.

use vcw_types::{Repair, StorageFormat};

use crate::Context;

/// Good samples needed either side of a gap to interpolate across it.
///
/// Two, because that is what Catmull-Rom reads. Named rather than written as a
/// literal because [`Repairs::context`] is derived from it and the callers are
/// derived from that, so the number has exactly one home.
const EDGE_FRAMES: u64 = 2;

/// The longest gap that is interpolated rather than muted, in milliseconds.
///
/// Two reasons for a cap and they agree on roughly this size. Musically, a curve
/// drawn between two samples 10 ms apart is invention rather than repair, and
/// silence is the more honest failure. Mechanically, this is what bounds how far
/// either side the callers have to read: without a cap a single pathological row
/// would make every chunk read the whole span around it.
const MAX_GAP_MILLIS: u64 = 10;

/// One span of damage, as the stage needs it.
///
/// Deliberately not `vcw_project::defect::Defect`: this crate sits below the one
/// that stores the rows, and the three fields a repair acts on are the whole of
/// what it needs. The caller converts, which also puts the "is this audible"
/// filter in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Damage {
    /// First damaged frame, counted from the start of the capture.
    pub start: u64,
    /// One past the last. Exclusive, so the length is a subtraction.
    pub end: u64,
    /// Which channels are damaged, as a bitmask: bit 0 is channel 0.
    pub channels: u32,
    /// What to do about it. [`Repair::None`] rows never reach here.
    pub repair: Repair,
}

/// The stored defect list, applied to buffers as they go past.
///
/// `Clone` because an export clones the chain per file, and `PartialEq` because a
/// plan holding one still has to compare.
#[derive(Debug, Clone, PartialEq)]
pub struct Repairs {
    /// Audible damage only, sorted by start frame then end.
    spans: Vec<Damage>,
    /// How many channels a frame holds, so a bitmask can be walked.
    channels: usize,
    /// The gap cap in frames, from [`MAX_GAP_MILLIS`] at the capture's rate.
    max_gap: u64,
    /// Index of the first span that may still be ahead of the read position.
    ///
    /// The two callers walk a capture forwards, so this turns what would be a
    /// scan of every row per buffer into an amortized step. A seek rewinds it,
    /// which is what [`Repairs::reset`] is for.
    cursor: usize,
}

impl Repairs {
    /// Builds the stage, or `None` when nothing audible is left.
    ///
    /// `None` rather than an empty stage so that [`crate::Chain::is_empty`] - and
    /// through it §9's bit-perfect refutation - is told the truth about a side
    /// whose every defect has been vetoed. A list of vetoes is not processing.
    #[must_use]
    pub fn new(mut spans: Vec<Damage>, channels: u16, rate_hz: u32) -> Option<Self> {
        spans
            .retain(|span| span.repair.is_audible() && span.end > span.start && span.channels != 0);
        if spans.is_empty() {
            return None;
        }
        spans.sort_unstable_by_key(|span| (span.start, span.end));
        Some(Self {
            spans,
            channels: channels as usize,
            max_gap: (u64::from(rate_hz) * MAX_GAP_MILLIS / 1_000).max(1),
            cursor: 0,
        })
    }

    /// How many frames either side of a buffer this stage has to be able to see.
    ///
    /// The contract the callers read. A span no longer than the gap cap is
    /// interpolated, and the furthest it can reach outside a buffer it overlaps is
    /// its own length plus the samples the interpolator reads past its edge.
    #[must_use]
    pub const fn context(&self) -> u64 {
        self.max_gap + EDGE_FRAMES
    }

    /// How many spans are in force. Reported in the export tags (WP-30 §5).
    #[must_use]
    pub fn count(&self) -> usize {
        self.spans.len()
    }

    /// Rewinds the cursor, for a seek.
    pub const fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Repairs every span overlapping `bytes`, reading context from either side.
    ///
    /// Writes only into `bytes`. A span wider than the buffer is repaired in
    /// pieces across successive calls and gets the same answer each time, because
    /// both its endpoints are read from the window rather than from the part of it
    /// being emitted.
    pub fn apply(
        &mut self,
        bytes: &mut [u8],
        context: Context<'_>,
        format: StorageFormat,
        at_frame: u64,
    ) {
        let frame_bytes = format.bytes_per_sample() * self.channels;
        if frame_bytes == 0 {
            return;
        }
        let frames = (bytes.len() / frame_bytes) as u64;
        if frames == 0 {
            return;
        }
        let window = Bounds::new(&context, bytes.len(), frame_bytes, at_frame);
        let end = at_frame + frames;

        // Everything whose last frame is behind us is behind us for good: the
        // callers only ever move forwards without a `reset`.
        while self
            .spans
            .get(self.cursor)
            .is_some_and(|span| span.end <= at_frame)
        {
            self.cursor += 1;
        }

        for index in self.cursor..self.spans.len() {
            let span = self.spans[index];
            if span.start >= end {
                break;
            }
            if span.end <= at_frame {
                continue;
            }
            self.repair(span, bytes, &context, &window, format, at_frame, frames);
        }
    }

    /// One span, on every channel its mask names.
    #[expect(
        clippy::too_many_arguments,
        reason = "the window is six numbers and three slices; naming them in a struct \
                  that exists for one call is more indirection than the call is worth"
    )]
    fn repair(
        &self,
        span: Damage,
        bytes: &mut [u8],
        context: &Context<'_>,
        window: &Bounds,
        format: StorageFormat,
        at_frame: u64,
        frames: u64,
    ) {
        let width = format.bytes_per_sample();
        let length = span.end - span.start;
        // A span longer than the cap is muted whatever the row says, and the
        // decision is a property of the span rather than of the buffer it landed
        // in - otherwise the same damage would be interpolated in one chunk and
        // muted in the next, and the seam between the two would be the artifact.
        let interpolate = matches!(span.repair, Repair::Interpolate) && length <= self.max_gap;

        // Endpoints first and for every channel, because reading them borrows the
        // buffer that the writing is about to borrow mutably.
        let mut ends = [None::<[f32; 4]>; 32];
        if interpolate {
            let reader = Reader {
                before: context.before,
                mid: bytes,
                after: context.after,
                window,
                channels: self.channels,
                format,
            };
            for (channel, end) in ends.iter_mut().enumerate().take(self.channels.min(32)) {
                if span.channels & (1 << channel) == 0 {
                    continue;
                }
                // Four good samples: two before the gap and two after it. Any of
                // them missing means the damage runs off the end of what was
                // recorded, where there is nothing to interpolate from and silence
                // is the honest answer.
                *end = match (
                    reader.get(span.start.wrapping_sub(2), channel),
                    reader.get(span.start.wrapping_sub(1), channel),
                    reader.get(span.end, channel),
                    reader.get(span.end + 1, channel),
                ) {
                    (Some(a), Some(b), Some(c), Some(d)) => Some([a, b, c, d]),
                    _ => None,
                };
            }
        }

        let first = span.start.max(at_frame);
        let last = span.end.min(at_frame + frames);
        for channel in 0..self.channels.min(32) {
            if span.channels & (1 << channel) == 0 {
                continue;
            }
            let ends = ends[channel];
            for frame in first..last {
                let value = match ends {
                    // t walks the gap's interior: the endpoints themselves are
                    // good samples at t = 0 and t = 1, so the first damaged frame
                    // is one step in and the last is one step short.
                    // The gap is the interval between the last good sample before
                    // the damage and the first good one after it, so it is one
                    // frame longer than the damage and the first damaged frame is
                    // one step into it.
                    Some(points) => quantize(
                        hermite(points, (length + 1) as f64, (frame - span.start + 1) as f64),
                        points,
                    ),
                    None => 0.0,
                };
                let offset =
                    ((frame - at_frame) as usize) * self.channels * width + channel * width;
                format.encode_sample(value, &mut bytes[offset..offset + width]);
            }
        }
    }
}

/// Where the three slices of the window start, in capture frames.
struct Bounds {
    /// First frame of `before`, which is also the first frame of the window.
    start: u64,
    /// First frame of `bytes`.
    mid: u64,
    /// First frame of `after`.
    after: u64,
    /// One past the last frame of the window.
    end: u64,
}

impl Bounds {
    fn new(context: &Context<'_>, mid_bytes: usize, frame_bytes: usize, at_frame: u64) -> Self {
        let before = (context.before.len() / frame_bytes) as u64;
        let mid = (mid_bytes / frame_bytes) as u64;
        let after = (context.after.len() / frame_bytes) as u64;
        Self {
            start: at_frame.saturating_sub(before),
            mid: at_frame,
            after: at_frame + mid,
            end: at_frame + mid + after,
        }
    }
}

/// Reads one sample out of the window, whichever of the three slices holds it.
struct Reader<'a> {
    before: &'a [u8],
    mid: &'a [u8],
    after: &'a [u8],
    window: &'a Bounds,
    channels: usize,
    format: StorageFormat,
}

impl Reader<'_> {
    /// `None` outside the window, which the caller reads as "cannot interpolate".
    fn get(&self, frame: u64, channel: usize) -> Option<f32> {
        if frame < self.window.start || frame >= self.window.end {
            return None;
        }
        let (slice, base) = if frame < self.window.mid {
            (self.before, self.window.start)
        } else if frame < self.window.after {
            (self.mid, self.window.mid)
        } else {
            (self.after, self.window.after)
        };
        let index = ((frame - base) as usize) * self.channels + channel;
        self.format.decode_sample(slice, index)
    }
}

/// A cubic Hermite step into a gap of `gap` frames, `step` frames in.
///
/// `points` are the four good samples around the damage, in frame order: the two
/// before it and the two after it. The middle pair are the endpoints the curve
/// joins; the outer pair only set the tangents.
///
/// The tangents are Catmull-Rom's, which is the detail the whole thing turns on:
/// each is measured between the samples either side of its endpoint, a span of
/// `gap + 1` frames, so it is a chord slope rather than a local one. Scale it by
/// the gap, as Hermite does, and the result stays the size of the step between the
/// endpoints however long the gap gets. A tangent taken from the last two samples
/// would be a per-sample slope, and multiplying that by a few hundred frames is how
/// an interpolator ends up louder than the click.
///
/// Rounds a repaired sample onto the grid the audio around it sits on.
///
/// A capture does not always use all of the width it is stored in: a 16-bit rip
/// padded into an `Int32` file has sixteen zero bits under every sample, and a
/// 24-bit one has eight. An interpolator that ignores that writes a value between
/// two steps the converter can never have produced, which is a claim of precision
/// the capture does not have - the same claim §9's narrowing switches exist to stop
/// an operator making by accident.
///
/// It is audible as nothing and visible as file size. FLAC codes a block of samples
/// with a common run of low zero bits as that many fewer bits per sample, so one
/// repaired sample off the grid costs the *whole* block its wasted-bits coding: on a
/// 16-in-32 side, 16 KiB per block touched, which measured as a 12% larger file over
/// a dozen clicks. Snapping the output costs one multiply and one round.
///
/// The grid is read off the four endpoints rather than declared, because nothing in
/// the project records it and the audio either side of the damage is the only
/// evidence there is. Zero carries no evidence - every grid divides it - so zeros are
/// skipped, and four of them mean the repair is silence anyway.
fn quantize(value: f32, points: [f32; 4]) -> f32 {
    /// The exponent of the largest power of two `x` is an exact multiple of.
    fn grid(x: f32) -> Option<i32> {
        let bits = x.to_bits();
        let exponent = ((bits >> 23) & 0xff) as i32;
        // Zero and subnormals carry no usable grid, and neither does a NaN.
        if exponent == 0 || exponent == 0xff {
            return None;
        }
        let mantissa = bits & 0x007f_ffff;
        let zeros = if mantissa == 0 {
            23
        } else {
            mantissa.trailing_zeros() as i32
        };
        Some(exponent - 127 - 23 + zeros)
    }

    let Some(shift) = points.iter().filter_map(|p| grid(*p)).min() else {
        return value;
    };
    // `exp2` of a shift read off real samples cannot overflow: it came from one.
    let step = f64::from(shift).exp2();
    (f64::from(value) / step).round().mul_add(step, 0.0) as f32
}

/// Evaluated in `f64` because a 32-bit sample has more significant bits than an
/// `f32` holds and the cubic multiplies them together.
fn hermite(points: [f32; 4], gap: f64, step: f64) -> f32 {
    let (y0, y1, y2, y3) = (
        f64::from(points[0]),
        f64::from(points[1]),
        f64::from(points[2]),
        f64::from(points[3]),
    );
    let m1 = (y2 - y0) / (gap + 1.0);
    let m2 = (y3 - y1) / (gap + 1.0);
    let u = step / gap;
    let (uu, uuu) = (u * u, u * u * u);
    let h00 = 2.0f64.mul_add(uuu, -(3.0 * uu)) + 1.0;
    let h10 = 2.0f64.mul_add(-uu, uuu) + u;
    let h01 = (-2.0f64).mul_add(uuu, 3.0 * uu);
    let h11 = uuu - uu;
    let value = (gap * m2).mul_add(h11, (gap * m1).mul_add(h10, h00.mul_add(y1, h01 * y2)));
    value as f32
}

#[cfg(test)]
mod tests {
    use super::{Damage, Repairs};
    use crate::Context;
    use vcw_types::{Repair, StorageFormat};

    const RATE: u32 = 48_000;
    const CHANNELS: usize = 2;

    /// A steady ramp on both channels, so a repaired sample is checkable by eye:
    /// anything the interpolator writes should land back on the line.
    fn ramp(frames: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        for frame in 0..frames {
            for channel in 0..CHANNELS {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the fixture is short and the values are small by construction"
                )]
                let value = (frame as i16).wrapping_mul(16) + channel as i16;
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    fn sample(bytes: &[u8], frame: u64, channel: usize) -> i16 {
        let at = (frame as usize * CHANNELS + channel) * 2;
        i16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn damage(start: u64, end: u64, channels: u32, repair: Repair) -> Damage {
        Damage {
            start,
            end,
            channels,
            repair,
        }
    }

    /// A gap on one channel is filled from the audio either side, and the other
    /// channel is not touched.
    ///
    /// The ramp makes the claim exact rather than approximate: Catmull-Rom over
    /// four collinear control points reproduces the line, so every repaired sample
    /// has a known right answer and a repair that merely looked plausible would
    /// fail. The untouched channel is the other half of the test - a vinyl click is
    /// damage to one groove wall far more often than both, and repairing the good
    /// wall throws away half a good stereo image.
    #[test]
    fn a_gap_is_filled_from_the_audio_either_side() {
        let mut bytes = ramp(64);
        let clean = bytes.clone();
        // Wreck frames 30..34 on the left channel only.
        for frame in 30..34u64 {
            let at = (frame as usize * CHANNELS) * 2;
            bytes[at..at + 2].copy_from_slice(&i16::MAX.to_le_bytes());
        }
        assert_ne!(bytes, clean, "the fixture has to be damaged to be repaired");

        let mut repairs = Repairs::new(
            vec![damage(30, 34, 0b01, Repair::Interpolate)],
            CHANNELS as u16,
            RATE,
        )
        .expect("one audible span is a stage");
        repairs.apply(&mut bytes, Context::NONE, StorageFormat::Int16, 0);

        for frame in 30..34u64 {
            assert_eq!(
                sample(&bytes, frame, 0),
                sample(&clean, frame, 0),
                "frame {frame} was not put back on the line"
            );
        }
        assert_eq!(bytes, clean, "something outside the span moved");
    }

    /// The same damage gets the same repair however the buffer is chopped up.
    ///
    /// This is the whole of WP-30 §4's straddling decision made runnable. The span
    /// is placed deliberately across the seam between two buffers, and the only
    /// reason the second buffer can repair its half correctly is that the caller
    /// handed over the frames either side. Drop the context and this test fails,
    /// which is the point of having it.
    #[test]
    fn a_span_across_a_buffer_edge_is_repaired_the_same() {
        let clean = ramp(64);
        let mut whole = clean.clone();
        for frame in 30..34u64 {
            let at = (frame as usize * CHANNELS) * 2;
            whole[at..at + 2].copy_from_slice(&i16::MAX.to_le_bytes());
        }
        let damaged = whole.clone();

        let spans = vec![damage(30, 34, 0b01, Repair::Interpolate)];
        let mut once = Repairs::new(spans.clone(), CHANNELS as u16, RATE).expect("a stage");
        once.apply(&mut whole, Context::NONE, StorageFormat::Int16, 0);

        // Now the same audio in two halves, cut at frame 32 - inside the damage.
        let frame_bytes = CHANNELS * 2;
        let cut = 32 * frame_bytes;
        let mut split = damaged.clone();
        let mut staged = Repairs::new(spans, CHANNELS as u16, RATE).expect("a stage");
        {
            let (first, rest) = split.split_at_mut(cut);
            staged.apply(
                first,
                Context {
                    before: &[],
                    after: rest,
                },
                StorageFormat::Int16,
                0,
            );
        }
        {
            let (head, second) = split.split_at_mut(cut);
            staged.apply(
                second,
                Context {
                    before: head,
                    after: &[],
                },
                StorageFormat::Int16,
                32,
            );
        }
        assert_eq!(
            split, whole,
            "the seam changed the answer, so the context is not reaching the stage"
        );
        assert_eq!(whole, clean, "and the answer was wrong in both");
    }

    /// Mute needs no context, a veto is not a stage, and a gap past the cap is
    /// muted rather than interpolated across.
    ///
    /// The three degradations in one test because each is one assertion and they
    /// share a fixture. The cap matters most: it is what bounds how far either
    /// side the callers have to read, so a span that ignored it would make every
    /// buffer in the file read the whole span around it.
    #[test]
    fn a_repair_degrades_rather_than_invents() {
        assert!(
            Repairs::new(
                vec![damage(10, 20, 0b11, Repair::None)],
                CHANNELS as u16,
                RATE
            )
            .is_none(),
            "a side whose every defect is vetoed is a side nothing is done to"
        );

        let mut bytes = ramp(64);
        let mut repairs = Repairs::new(
            vec![damage(10, 14, 0b11, Repair::Mute)],
            CHANNELS as u16,
            RATE,
        )
        .expect("a stage");
        repairs.apply(&mut bytes, Context::NONE, StorageFormat::Int16, 0);
        for frame in 10..14u64 {
            for channel in 0..CHANNELS {
                assert_eq!(sample(&bytes, frame, channel), 0, "frame {frame} not muted");
            }
        }

        // The cap is ten milliseconds, so at 1 kHz it is ten frames.
        let mut long = ramp(64);
        let mut capped = Repairs::new(
            vec![damage(10, 30, 0b01, Repair::Interpolate)],
            CHANNELS as u16,
            1_000,
        )
        .expect("a stage");
        assert_eq!(capped.context(), 12, "the cap sets what the callers read");
        capped.apply(&mut long, Context::NONE, StorageFormat::Int16, 0);
        for frame in 10..30u64 {
            assert_eq!(
                sample(&long, frame, 0),
                0,
                "a gap past the cap should be muted, not drawn across"
            );
        }
    }
    /// A repair does not write precision the capture does not have.
    ///
    /// A 16-bit rip padded into an `Int32` file has sixteen zero bits under every
    /// sample. An interpolator that ignores that writes values between two steps
    /// the converter could never have produced, and FLAC then loses its wasted-bits
    /// coding for every block one of them lands in - measured at 12% on a real side
    /// before this was here, which is how a silent problem became a visible one.
    ///
    /// The fixture is a sine rounded onto the grid, and both halves of that matter.
    /// A line snaps to any grid holding its endpoints, and a low-order polynomial
    /// interpolates to rationals with small denominators that often land on one by
    /// luck, so either would pass without the rounding and prove nothing.
    #[test]
    fn a_repair_writes_no_more_precision_than_the_capture_has() {
        const STEP: i32 = 1 << 16;
        let shape = |frame: i32| {
            (f64::from(frame) * 0.37)
                .sin()
                .mul_add(12_000.0, 0.0)
                .round() as i32
                * STEP
        };
        let mut bytes = Vec::new();
        for frame in 0..64i32 {
            let value = shape(frame);
            for _ in 0..CHANNELS {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        assert!(
            bytes.as_chunks::<4>().0.iter().all(|s| {
                let v = i32::from_le_bytes(*s);
                v == 0 || v.trailing_zeros() >= 16
            }),
            "the fixture has to be on the grid to begin with"
        );

        let mut repairs = Repairs::new(
            vec![damage(30, 36, 0b11, Repair::Interpolate)],
            CHANNELS as u16,
            RATE,
        )
        .expect("one audible span");
        repairs.apply(&mut bytes, Context::NONE, StorageFormat::Int32, 0);

        let mut moved = 0;
        for frame in 30..36 {
            for channel in 0..CHANNELS {
                let at = (frame * CHANNELS + channel) * 4;
                let value =
                    i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
                assert_eq!(
                    value % STEP,
                    0,
                    "frame {frame} channel {channel} was written off the capture's own grid"
                );
                let original = shape(frame as i32);
                if value != original {
                    moved += 1;
                }
            }
        }
        assert!(
            moved > 0,
            "nothing was repaired, so the grid claim is vacuous"
        );
    }
}
