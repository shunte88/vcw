/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The non-destructive processing chain: the stages applied to audio on the way out (WP-30).
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

//! The non-destructive processing chain (WP-30, §4.1, §46 Phase 3).
//!
//! §4.1 is an absolute: captured PCM is immutable, and everything VCW does to it
//! is an instruction stored beside it. A [`Chain`] is those instructions resolved
//! into something that can run, applied in one place on the way out so that the
//! audition, the render and the export all hear the same thing.
//!
//! A stage comes in one of two shapes and the difference is the whole design.
//! A **parametric** stage *is* its parameters: §51's equalization is a curve id
//! and a sample rate, and re-deriving the filter from a row costs nothing. A
//! **repair** stage is a stored list of defects plus a cheap deterministic fix,
//! because detection is expensive and sometimes wrong, and a person has to be
//! able to veto one click without vetoing the rest.
//!
//! The order is fixed by the code and is not configurable: repairs first,
//! parametric second. A click is damage to the groove, so it is present in the
//! same domain the stored samples are in, and VCW's curve is something VCW adds
//! afterwards. Repairing after an IIR has smeared the impulse makes the damage
//! longer and lower in contrast than it is. The honest form of that claim is
//! that repairs run upstream of everything *VCW* applies; if the signal chain
//! equalized before the capture, which `captures.capture_eq` records, VCW cannot
//! get upstream of that and does not pretend to.
//!
//! A repair stage cannot work from the buffer alone. Interpolating across a gap
//! needs good samples either side of it, and a buffer has nothing before its first
//! frame and nothing after its last. [`Chain::context_frames`] is how much either
//! side the chain has to be able to see; a caller reads that much extra at both
//! ends and hands it over as a [`Context`]. That is WP-30 §4's first option - widen
//! the read span and discard the margin - and it covers a track's first frame and
//! every chunk boundary inside it with one mechanism, because to the stage those
//! are the same question.

pub mod repair;

pub use repair::{Damage, Repairs};

use vcw_eq::Filter;
use vcw_types::StorageFormat;

/// The name [`Chain::stages`] gives §51's stage.
///
/// Public because the refutation in `vcw_core` matches on it to give the curve
/// the name a person reads; matching a constant rather than parsing the string
/// is what keeps the two from drifting apart.
pub const EQ_STAGE: &str = "playback-eq";

/// The audio either side of the buffer being processed.
///
/// Borrowed from the caller's own read rather than buffered here: both call sites
/// already hold one allocation big enough for the margin and the chunk together,
/// so the three slices are one `split_at_mut` apart and nothing is copied.
///
/// [`Context::NONE`] is the honest answer when a caller has no margin - the first
/// frames of a capture, where no audio exists before them. A stage that cannot see
/// what it needs does the thing it can defend instead, which for a repair is to
/// mute rather than to invent.
#[derive(Debug, Clone, Copy, Default)]
pub struct Context<'a> {
    /// Frames immediately before the buffer, in the same stored format.
    pub before: &'a [u8],
    /// Frames immediately after it.
    pub after: &'a [u8],
}

impl Context<'_> {
    /// No margin either side.
    pub const NONE: Self = Self {
        before: &[],
        after: &[],
    };
}

/// What a span of audio passes through on its way out of the project.
///
/// Empty by default, which is the common case and must cost nothing: a project
/// nobody has stated a curve for and nobody has run a detector over gets a chain
/// that no buffer is handed to.
///
/// `PartialEq` so that an export plan holding one still compares, and `Clone`
/// because each exported file starts from a chain with its state zeroed rather
/// than carrying the end of one track into the start of the next.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chain {
    /// WP-30's stored defect list, or `None` when nothing audible was found.
    ///
    /// First, and the order is not configurable: a click is damage to the groove,
    /// so it is present in the same domain the stored samples are in, and VCW's
    /// curve is something VCW adds afterwards. Repairing after an IIR has smeared
    /// the impulse makes the damage longer and lower in contrast than it is.
    repairs: Option<Repairs>,
    /// §51's playback equalization, or `None` when the samples go out as stored.
    eq: Option<Filter>,
}

impl Chain {
    /// Whether this chain would change the audio handed to it.
    ///
    /// The predicate §9's bit-perfect refutation should ask, rather than asking
    /// after any one stage: a chain that gains a stage is covered by it the day
    /// the stage is added rather than the day somebody remembers.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.eq.is_none() && self.repairs.is_none()
    }

    /// How many frames either side of a buffer the chain has to be able to see.
    ///
    /// Zero unless a repair stage is in it, which is what keeps today's common
    /// case - a project nobody has run a detector over - reading exactly the
    /// frames it means to play. A caller that honors this reads `n` frames extra
    /// at each end of every buffer and passes them as [`Context`]; one that
    /// cannot, at the very start or end of a capture, passes what it has and the
    /// stage degrades rather than guesses.
    #[must_use]
    pub fn context_frames(&self) -> u64 {
        self.repairs.as_ref().map_or(0, Repairs::context)
    }

    /// The stages in force, as (name, detail), in the order they run (WP-30 §5).
    ///
    /// Empty exactly when [`Chain::is_empty`], so a caller that speaks up
    /// whenever this is not speaks up about exactly the audio that was changed,
    /// and a stage added later is covered the day it is added.
    ///
    /// A list rather than a sentence because the two callers want different
    /// words for the same fact: the export tag joins them, and §9's bit-perfect
    /// refutation gives §51's curve the name a person reads. Both are generated
    /// from the stages themselves, so neither can name a stage the chain does
    /// not have or stay quiet about one it does.
    ///
    /// Both repair numbers are stated, a zero included, because "412 repaired"
    /// on its own cannot be told apart from a side nobody reviewed.
    #[must_use]
    pub fn stages(&self) -> Vec<(&'static str, String)> {
        // The order of these two blocks is the order of `process_stored`, which
        // is what makes this a description of the chain rather than a list of
        // what is in it.
        let mut stages = Vec::new();
        if let Some(repairs) = self.repairs.as_ref() {
            stages.push((
                "repairs",
                format!("{} repaired, {} vetoed", repairs.count(), repairs.vetoed()),
            ));
        }
        if let Some(curve) = self.curve() {
            stages.push((EQ_STAGE, curve.to_owned()));
        }
        stages
    }

    /// The §51 curve in force, or `None`.
    ///
    /// Named here rather than left to the caller's own copy so that what the
    /// tags claim and what the audio went through cannot disagree.
    #[must_use]
    pub fn curve(&self) -> Option<&str> {
        self.eq.as_ref().map(Filter::curve)
    }

    /// Clears every stage's memory, for a seek or the start of a new file.
    ///
    /// The equalization sections are recursive, so the samples either side of a
    /// seek are not continuous and carrying state across one rings a transient
    /// into the first few milliseconds after it.
    pub fn reset(&mut self) {
        if let Some(repairs) = self.repairs.as_mut() {
            repairs.reset();
        }
        if let Some(eq) = self.eq.as_mut() {
            eq.reset();
        }
    }

    /// Runs the chain over one buffer of stored-format bytes, in place.
    ///
    /// `at_frame` is the frame number of the first frame in `bytes`, counted
    /// from the start of the capture rather than the start of the span. A
    /// parametric stage does not need it; a repair stage does, because the
    /// defects it applies are rows addressed by frame.
    ///
    /// `context` is the audio either side, [`Chain::context_frames`] of it where
    /// the caller could read that much. Only `bytes` is written.
    pub fn process_stored(
        &mut self,
        bytes: &mut [u8],
        context: Context<'_>,
        format: StorageFormat,
        at_frame: u64,
    ) {
        // The order of the chain is the order of these two statements, and it is
        // fixed here rather than offered as a setting. See the module note.
        if let Some(repairs) = self.repairs.as_mut() {
            repairs.apply(bytes, context, format, at_frame);
        }
        if let Some(eq) = self.eq.as_mut() {
            eq.process_stored(bytes, format);
        }
    }

    /// Adds the stored defect list to a chain that already knows its curve.
    ///
    /// Separate from [`Chain::from`] because the two are resolved by different
    /// questions - a curve can be refused and a defect list cannot - and a
    /// constructor taking both would have to answer the refusal twice.
    #[must_use]
    pub fn with_repairs(mut self, repairs: Option<Repairs>) -> Self {
        self.repairs = repairs;
        self
    }
}

impl From<Option<Filter>> for Chain {
    /// The chain a project with §51 resolved and nothing else detected.
    ///
    /// Takes the resolved filter rather than resolving one, because the caller
    /// has to name the curve anyway: §51 forbids applying one silently, and a
    /// refusal has to land where no audio has been written yet.
    fn from(eq: Option<Filter>) -> Self {
        Self { repairs: None, eq }
    }
}

#[cfg(test)]
mod tests {
    use super::{Chain, Context, Damage, Repairs};
    use vcw_eq::{Catalog, Filter, RIAA};
    use vcw_types::{CaptureEq, Repair, StorageFormat};

    /// A filter, and bytes with enough edge in them to ring one.
    fn fixture() -> (Filter, Vec<u8>) {
        let filter = Filter::playback(&Catalog::builtin(), RIAA, CaptureEq::Flat, 44_100, 2)
            .expect("riaa over a flat capture is the ordinary case");
        // A step, interleaved across two channels: a recursive filter that is
        // not reset carries the tail of one pass into the next, which is the
        // thing `reset` has to be seen to prevent.
        let mut bytes = Vec::new();
        for frame in 0..256u32 {
            let sample: i16 = if frame < 128 { 8_000 } else { -8_000 };
            bytes.extend_from_slice(&sample.to_le_bytes());
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        (filter, bytes)
    }

    /// A chain of one stage is that stage, and a chain of none is nothing.
    ///
    /// This is WP-30 §8 step 3's claim made runnable: `Chain` replaced
    /// `Option<Filter>` at two call sites and changed no audio. The frame
    /// position it now takes is read by no stage yet, so it cannot be asserted
    /// on here; what can be is that routing through the chain produces the same
    /// bytes the filter alone produced, that an empty chain is not merely cheap
    /// but inert, and that `reset` reaches the stage it wraps - without that
    /// last one a seek would ring and nothing would fail.
    #[test]
    fn a_chain_of_one_stage_is_that_stage() {
        let (mut filter, original) = fixture();
        let mut direct = original.clone();
        filter.process_stored(&mut direct, StorageFormat::Int16);
        assert_ne!(
            direct, original,
            "the fixture has to be audible to the curve"
        );

        let (filter, _) = fixture();
        let mut chain = Chain::from(Some(filter));
        assert!(!chain.is_empty());
        assert_eq!(chain.curve(), Some(RIAA));
        let mut through = original.clone();
        chain.process_stored(&mut through, Context::NONE, StorageFormat::Int16, 0);
        assert_eq!(through, direct, "the chain added nothing of its own");

        let mut empty = Chain::default();
        assert!(empty.is_empty());
        assert_eq!(empty.curve(), None);
        let mut untouched = original.clone();
        empty.process_stored(&mut untouched, Context::NONE, StorageFormat::Int16, 0);
        assert_eq!(
            untouched, original,
            "an empty chain is inert, not just cheap"
        );

        // Second pass on the same chain: without the reset the sections still
        // hold the end of the first one, so the first samples differ.
        let mut again = original.clone();
        chain.process_stored(&mut again, Context::NONE, StorageFormat::Int16, 256);
        assert_ne!(
            again, direct,
            "state carried, which is what a seek must clear"
        );
        chain.reset();
        let mut after_reset = original.clone();
        chain.process_stored(&mut after_reset, Context::NONE, StorageFormat::Int16, 0);
        assert_eq!(after_reset, direct, "reset reached the stage");
    }

    /// The chain describes itself, in the order it runs (WP-30 §5, step 7).
    ///
    /// Two callers read this - the `VCW_PROCESSING` tag and §9's bit-perfect
    /// refutation - and both are wrong in the same way if the order is wrong or
    /// a veto goes uncounted. The order is checked here rather than in either
    /// caller because this is the one place both stages are cheap to build.
    #[test]
    fn the_chain_says_what_it_does_in_the_order_it_does_it() {
        assert!(
            Chain::default().stages().is_empty(),
            "an empty chain claimed to do something"
        );

        let span = |start, end, repair| Damage {
            start,
            end,
            channels: 0b11,
            repair,
        };
        let repairs = Repairs::new(
            vec![
                span(1_000, 1_004, Repair::Interpolate),
                span(2_000, 2_004, Repair::None),
                span(3_000, 3_004, Repair::Mute),
            ],
            2,
            44_100,
        );
        let (filter, _) = fixture();
        let chain = Chain::from(Some(filter)).with_repairs(repairs);
        assert_eq!(
            chain.stages(),
            vec![
                ("repairs", "2 repaired, 1 vetoed".to_owned()),
                ("playback-eq", RIAA.to_owned()),
            ],
            "the tag has to read in the order `process_stored` runs"
        );

        // A side whose every defect is vetoed is a side nothing was done to, so
        // the chain says so and the tag does not appear at all.
        let vetoed = Repairs::new(vec![span(1_000, 1_004, Repair::None)], 2, 44_100);
        assert!(
            Chain::default().with_repairs(vetoed).stages().is_empty(),
            "a list of vetoes was reported as processing"
        );
    }
}
