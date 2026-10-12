/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Playback equalization curves - RIAA and the label characteristics that came before it.
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

//! Playback equalization curves (§51).
//!
//! A vinyl record is not cut flat. Bass is reduced and treble lifted before the groove
//! is made, and a player must undo it. Since 1954 that has meant RIAA, but the two
//! decades before it were a free-for-all: Columbia, Decca, EMI, NAB, AES and the rest
//! each cut to their own characteristic, and playing one back through RIAA is wrong by
//! as much as 8 dB in the bass.
//!
//! This crate holds the characteristics, as time constants with the citation they came
//! from, and designs each one into a filter that meets §51's +/-0.5 dB over 20 Hz to
//! 20 kHz at every sample rate §8 requires.
//!
//! ```
//! use vcw_eq::{Catalog, Filter};
//! use vcw_types::format::CaptureEq;
//!
//! let catalog = Catalog::builtin();
//! let columbia = catalog.require("columbia_lp")?;
//! assert!((columbia.response_db(50.0) - 13.568).abs() < 0.01);
//!
//! // A flat transfer takes the curve directly.
//! let mut filter = Filter::playback(&catalog, "columbia_lp", CaptureEq::Flat, 44_100, 2)?;
//! let mut audio = [0.0f32; 512];
//! filter.process(&mut audio);
//! # Ok::<(), vcw_eq::Error>(())
//! ```
//!
//! Nothing here touches the capture path. §9 is unchanged: this is a stored decision
//! applied on the way out, like every other edit instruction.
//!
//! Requirements: §51 (playback equalization), §8 (sample rates), §12 (imported
//! projects have no provenance). Design: `docs/design/WP-29-playback-equalization.md`.

pub mod catalog;
pub mod curve;
pub mod filter;

pub use catalog::{BUILTIN_TOML, Catalog, FLAT, RIAA};
pub use curve::{Curve, DEFAULT_REPLAY_SHELF_DB, REFERENCE_HZ};
pub use filter::{BAND_HZ, Filter, TOLERANCE_DB, worst_error_db};

/// What can go wrong reading a catalog or designing a filter.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The catalog text is not the TOML this crate expects.
    #[error("the curve catalog is malformed: {source}")]
    Malformed {
        /// The parse failure. Boxed because it is much larger than every other variant.
        source: Box<toml::de::Error>,
    },
    /// No curve goes by that id.
    #[error("no equalization curve is called '{id}' (known: {known})")]
    UnknownCurve {
        /// What was asked for.
        id: String,
        /// What is on offer, comma separated.
        known: String,
    },
    /// A curve arrived without a citation, which §51 requires of every one.
    #[error("curve '{id}' has no citation, and §51 requires one of every curve")]
    MissingCitation {
        /// The offending curve.
        id: String,
    },
    /// Two curves claim the same id.
    #[error("two curves are both called '{id}'")]
    DuplicateId {
        /// The repeated id.
        id: String,
    },
    /// A time constant is not a positive, finite number of microseconds.
    #[error("curve '{id}' has a {field} that is not a positive number of microseconds")]
    BadTimeConstant {
        /// The offending curve.
        id: String,
        /// Which of the three.
        field: &'static str,
    },
    /// The shelf does not sit below the turnover, so the curve would cut bass.
    #[error(
        "curve '{id}' has a shelf at or above its turnover, which would cut bass \
         rather than restore it - the shelf time constant is the larger number"
    )]
    ShelfBelowTurnover {
        /// The offending curve.
        id: String,
    },
    /// The capture's equalization was never stated, so no curve can be applied.
    #[error(
        "this capture's equalization was never recorded, so a curve cannot be applied \
         to it - state whether the signal chain was flat or applied RIAA"
    )]
    ProvenanceUnknown,
    /// A rate or channel count that cannot describe a stream.
    #[error("cannot filter a stream at {rate} Hz with {channels} channels")]
    BadStream {
        /// The rate asked for.
        rate: u32,
        /// The channel count asked for.
        channels: usize,
    },
    /// The design produced a pole outside the unit circle.
    ///
    /// Cannot happen for a catalog curve - the conformance test proves it at every
    /// rate - but a user-defined curve with absurd constants could reach it, and an
    /// unstable filter must never be handed to a playback thread.
    #[error("the designed filter is unstable (pole at {pole})")]
    Unstable {
        /// The offending pole.
        pole: f64,
    },
}
