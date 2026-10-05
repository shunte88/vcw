/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Turning evidence into an identified release.
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

//! Turning evidence into an identified release.
//!
//! Requirements: §23 (evidence), §24 (boundary evidence), §26 (progressive
//! identification), §27 (identification engine), §28 (providers). Phase 2, WP-23.
//! Design: `docs/design/WP-23-identification-resolver.md`.
//!
//! §27 draws the line this crate sits on:
//!
//! > Fingerprinting answers: what recording does this audio resemble?
//! > Identification answers: given all current evidence, what release, side and track
//! > are being recorded?
//!
//! # It starts with what a person typed
//!
//! A VCW project starts from a record in somebody's hand, and project setup asks for
//! what is printed on it: artist, title, catalogue number, mono or stereo, and whether
//! RIAA equalisation is to be applied. Four of those are release identity, stated by
//! someone looking at the object, and they arrive before the first sample does.
//!
//! So this crate is not a machine for guessing what a record is from its audio. It
//! turns a stated identity into a specific *pressing*, checks the audio is consistent
//! with it, and lays the side out. The catalogue number is the lever: artist and title
//! identify a work, a catalogue number identifies a pressing, and §28 asks VCW to tell
//! pressings apart.
//!
//! [`resolver::Lookup::ORDER`] is that policy as code. Discogs by catalogue number,
//! then Discogs by name, then MusicBrainz by name, and only then the audio: measured
//! on `/data2/source_rips`, about a third of real rips do not resolve at MusicBrainz
//! from artist and album at all, and identifying a record from audio alone cost 63 to
//! 138 requests in the prototypes against one for a text lookup.
//!
//! # The shape of it
//!
//! ```text
//!   evidence    facts, each with a source. Nothing is judged.
//!      |
//!      v
//!   candidate   one release's account of them: agrees, disagrees, silent.
//!      |
//!      v
//!   confidence  the weights and the thresholds. Every number in one table.
//!      |
//!      v
//!   resolver    one release, a question, or nothing.
//! ```
//!
//! The split is so that a wrong identification traces back to the evidence that caused
//! it instead of disappearing into one opaque number, which is what §26's
//! "evidence-based rather than a single-match decision" has to mean in practice.
//!
//! # Deciding, from a sleeve and a search
//!
//! ```
//! use vcw_identify::candidate::{Claim, assess};
//! use vcw_identify::evidence::{Fact, Observed, Source};
//! use vcw_identify::resolver::{Lookup, Outcome, next_lookup, resolve};
//!
//! // What a person typed at setup, plus what the detectors counted.
//! let mut observed = Observed::new();
//! observed
//!     .add(Source::Stated, Fact::Artist("Ultravox".into()))
//!     .add(Source::Stated, Fact::Album("Vienna".into()))
//!     .add(Source::Stated, Fact::Catalogue("CHRH 1296".into()))
//!     .add(Source::Signal, Fact::Count(9));
//!
//! // A catalogue number was typed, so that is the first and cheapest question.
//! assert_eq!(next_lookup(&observed, &[]), Some(Lookup::CatalogueAtDiscogs));
//!
//! // What it answered with, scored against everything known.
//! let found = Claim {
//!     ids: vec!["1".into()],
//!     artist: "Ultravox".into(),
//!     album: "Vienna".into(),
//!     catalog: "CHRH1296".into(),   // spelled without the space, and still a match
//!     tracks: Some(9),
//!     ..Claim::default()
//! };
//! match resolve(vec![assess(found, &observed)]) {
//!     Outcome::Resolved { chosen } => assert!(chosen.confidence.value() >= 0.8),
//!     other => panic!("the whole sleeve agrees: {other:?}"),
//! }
//! ```
//!
//! # What it writes
//!
//! Nothing. It emits observations (§23) that existing plumbing consumes: an
//! identification per confirmed track, and a boundary per confirmed start with its
//! provenance named (§24). §26's rule that automatic identification never silently
//! replaces what a person confirmed is why [`resolver::Doubt::ContradictsAPerson`]
//! exists and why a contradiction can never resolve.

pub mod candidate;
pub mod confidence;
pub mod evidence;
pub mod resolver;

pub use candidate::{Agreement, Assessed, Claim, Verdict, assess};
pub use confidence::{Confidence, Stance, weigh};
pub use evidence::{Fact, Item, Kind, Observed, Source};
pub use resolver::{Doubt, Lookup, Outcome, Ranked, next_lookup, resolve};
