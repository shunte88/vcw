/*
 *  span.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A half-open span of frames, which is what every audition target reduces to (§21).
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

//! A half-open span of frames, which is what every audition target reduces to (§21).
//!
//! §21 names four things playback has to be able to play - the complete capture, a
//! selected region, an individual track, and a boundary audition - and they are not
//! four mechanisms. Each one is a pair of frame numbers, so the transport takes a
//! [`Span`] and the *caller* is what differs: a region comes from a selection, a
//! track from the vinyl data model, a boundary from a marker and a pad either side.
//!
//! Half-open, `start` inclusive and `end` exclusive, matching the block queries in
//! `vcw-project`: it makes two adjacent spans tile without an off-by-one and makes
//! an empty span expressible, which a closed interval cannot do.

use serde::{Deserialize, Serialize};

use crate::rate::SampleRate;

/// A half-open span of frames: `start` is played, `end` is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// First frame, inclusive.
    pub start: u64,
    /// One past the last frame.
    pub end: u64,
}

impl Span {
    /// The span that plays nothing.
    pub const EMPTY: Self = Self { start: 0, end: 0 };

    /// A span from two frame numbers, in either order.
    ///
    /// Reversed input is sorted rather than refused, because the usual source of
    /// it is a drag selection made right to left and there is only one span it
    /// could possibly mean.
    #[must_use]
    pub const fn new(start: u64, end: u64) -> Self {
        if end < start {
            Self {
                start: end,
                end: start,
            }
        } else {
            Self { start, end }
        }
    }

    /// A span covering a whole capture of `frames` frames.
    #[must_use]
    pub const fn whole(frames: u64) -> Self {
        Self {
            start: 0,
            end: frames,
        }
    }

    /// A boundary audition: `before` frames leading up to `center`, `after` past it.
    ///
    /// Saturating at zero rather than wrapping, so auditioning a boundary near the
    /// start of a side gives a short span instead of one that begins near `u64::MAX`.
    #[must_use]
    pub const fn around(center: u64, before: u64, after: u64) -> Self {
        Self {
            start: center.saturating_sub(before),
            end: center.saturating_add(after),
        }
    }

    /// How many frames this span plays.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Whether it plays nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// Whether a frame falls inside it.
    #[must_use]
    pub const fn contains(&self, frame: u64) -> bool {
        frame >= self.start && frame < self.end
    }

    /// Trims the span to a capture's length.
    ///
    /// A span entirely past the end collapses to empty at the limit rather than
    /// inverting, which is what makes "play from the end" stop instead of refuse.
    #[must_use]
    pub const fn clamp_to(self, frames: u64) -> Self {
        let start = if self.start > frames {
            frames
        } else {
            self.start
        };
        let end = if self.end > frames { frames } else { self.end };
        Self {
            start,
            end: if end < start { start } else { end },
        }
    }

    /// How long the span lasts at a given rate.
    #[must_use]
    pub fn seconds(&self, rate: SampleRate) -> f64 {
        if rate.hz() == 0 {
            return 0.0;
        }
        self.frames() as f64 / f64::from(rate.hz())
    }

    /// A span named in seconds, which is how a command line and a UI both name one.
    ///
    /// Negative input floors at zero: it is a coordinate, not a duration, and a
    /// caller asking for one second before the needle dropped means the start.
    #[must_use]
    pub fn from_seconds(rate: SampleRate, start: f64, end: f64) -> Self {
        Self::new(frames_at(rate, start), frames_at(rate, end))
    }
}

/// Seconds to frames, floored at zero and saturating at the top.
#[must_use]
pub fn frames_at(rate: SampleRate, seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    let frames = seconds * f64::from(rate.hz());
    if frames >= u64::MAX as f64 {
        u64::MAX
    } else {
        frames as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: SampleRate = SampleRate(48_000);

    #[test]
    fn a_reversed_span_is_the_span_it_could_only_have_meant() {
        assert_eq!(Span::new(100, 40), Span::new(40, 100));
        assert_eq!(Span::new(40, 100).frames(), 60);
    }

    #[test]
    fn an_empty_span_contains_nothing_and_plays_nothing() {
        let empty = Span::new(500, 500);
        assert!(empty.is_empty());
        assert_eq!(empty.frames(), 0);
        assert!(!empty.contains(500));
        assert_eq!(empty.seconds(R), 0.0);
    }

    #[test]
    fn half_open_means_two_spans_tile() {
        let first = Span::new(0, 48_000);
        let second = Span::new(48_000, 96_000);
        assert!(first.contains(47_999));
        assert!(!first.contains(48_000));
        assert!(second.contains(48_000));
        assert_eq!(first.frames() + second.frames(), 96_000);
    }

    #[test]
    fn a_boundary_near_the_start_auditions_a_short_span_not_a_wrapped_one() {
        let early = Span::around(1_000, 48_000, 48_000);
        assert_eq!(early.start, 0);
        assert_eq!(early.end, 49_000);
    }

    #[test]
    fn clamping_past_the_end_collapses_rather_than_inverting() {
        let past = Span::new(200, 400).clamp_to(100);
        assert!(past.is_empty());
        assert_eq!(past.start, 100);

        let straddling = Span::new(50, 400).clamp_to(100);
        assert_eq!(straddling, Span::new(50, 100));
    }

    #[test]
    fn seconds_round_trip_through_frames() {
        assert_eq!(Span::from_seconds(R, 0.5, 1.5), Span::new(24_000, 72_000));
        assert!((Span::new(24_000, 72_000).seconds(R) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_time_before_the_needle_dropped_is_the_start() {
        assert_eq!(frames_at(R, -3.0), 0);
        assert_eq!(frames_at(R, f64::NAN), 0);
        assert_eq!(Span::from_seconds(R, -1.0, 1.0), Span::new(0, 48_000));
    }
}
