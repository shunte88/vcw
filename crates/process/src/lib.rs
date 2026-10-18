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
//! Today the chain holds one stage, so it is not yet a chain. It exists at this
//! size on purpose: the position argument [`Chain::process_stored`] takes is the
//! plumbing a frame-addressed repair needs, and threading it through the two
//! call sites as a change with no behavior in it is cheaper to review than
//! threading it through at the same time as the thing that reads it.

use vcw_eq::Filter;
use vcw_types::StorageFormat;

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
        self.eq.is_none()
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
    pub fn process_stored(&mut self, bytes: &mut [u8], format: StorageFormat, at_frame: u64) {
        // The order of the chain is the order of these statements. Repairs go
        // above this line when they exist, which is why the position is read by
        // nothing yet.
        let _ = at_frame;
        if let Some(eq) = self.eq.as_mut() {
            eq.process_stored(bytes, format);
        }
    }
}

impl From<Option<Filter>> for Chain {
    /// The chain a project with §51 resolved and nothing else detected.
    ///
    /// Takes the resolved filter rather than resolving one, because the caller
    /// has to name the curve anyway: §51 forbids applying one silently, and a
    /// refusal has to land where no audio has been written yet.
    fn from(eq: Option<Filter>) -> Self {
        Self { eq }
    }
}

#[cfg(test)]
mod tests {
    use super::Chain;
    use vcw_eq::{Catalog, Filter, RIAA};
    use vcw_types::{CaptureEq, StorageFormat};

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
        chain.process_stored(&mut through, StorageFormat::Int16, 0);
        assert_eq!(through, direct, "the chain added nothing of its own");

        let mut empty = Chain::default();
        assert!(empty.is_empty());
        assert_eq!(empty.curve(), None);
        let mut untouched = original.clone();
        empty.process_stored(&mut untouched, StorageFormat::Int16, 0);
        assert_eq!(
            untouched, original,
            "an empty chain is inert, not just cheap"
        );

        // Second pass on the same chain: without the reset the sections still
        // hold the end of the first one, so the first samples differ.
        let mut again = original.clone();
        chain.process_stored(&mut again, StorageFormat::Int16, 256);
        assert_ne!(
            again, direct,
            "state carried, which is what a seek must clear"
        );
        chain.reset();
        let mut after_reset = original.clone();
        chain.process_stored(&mut after_reset, StorageFormat::Int16, 0);
        assert_eq!(after_reset, direct, "reset reached the stage");
    }
}
