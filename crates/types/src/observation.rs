/*
 *  observation.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The evidence model: what analysis publishes instead of editing tracks (§23, §24).
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

//! The evidence model: what analysis publishes instead of editing tracks.
//!
//! §23 is a rule about direction, not a data structure: "analysis subsystems shall
//! publish observations rather than directly modifying tracks". Everything here is
//! therefore *reportable* and nothing here is authoritative. A detector says what it
//! saw and how sure it is; deciding what the project does about it belongs to the
//! resolver, and past that, to the operator.
//!
//! §24 fixes the shape of a boundary observation: position, confidence, provenance
//! and supporting evidence. All four are mandatory here, because a boundary whose
//! origin cannot be established is one a UI cannot safely offer to move.
//!
//! Requirements: §22 (detection), §23 (evidence model), §24 (boundary evidence).
//!
//! # Positions are frames
//!
//! VRipr worked in seconds and VCW does not. A frame is exact, it is what the project
//! stores, and it is what [`crate::Span`] already speaks; seconds are a presentation
//! concern and reintroducing them here would mean two roundings between the detector
//! and the edit. `Observed::seconds` exists for printing and nothing else.

use serde::{Deserialize, Serialize};

use crate::rate::SampleRate;

/// What put a boundary where it is.
///
/// §24 lists the potential sources and this is that list, with one addition the
/// requirement implies rather than states: [`Provenance::User`] outranks everything,
/// because §24 also says a user-confirmed boundary shall not be moved by automatic
/// analysis. Ordering is deliberate and is the resolver's tie-break: later variants
/// win, and `User` is last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Provenance {
    /// A gap in the audio: the level fell far enough, for long enough.
    Silence,
    /// The spectrum changed character, whether or not the level did. Loud groove
    /// noise between tracks is the case this exists for.
    SpectralChange,
    /// The hidden Markov model's most likely state path changed here.
    Hmm,
    /// Two fingerprints either side disagree about what is playing (§25).
    Fingerprint,
    /// A release's published track durations put a boundary here (§28).
    MetadataDuration,
    /// The side topology says there must be one: n tracks, this long a side (§29).
    ReleaseTopology,
    /// A human said so. Never moved by analysis.
    User,
}

impl Provenance {
    /// The kebab-case name, which is also what the event bus and the UI use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Silence => "silence",
            Self::SpectralChange => "spectral-change",
            Self::Hmm => "hmm",
            Self::Fingerprint => "fingerprint",
            Self::MetadataDuration => "metadata-duration",
            Self::ReleaseTopology => "release-topology",
            Self::User => "user",
        }
    }

    /// Whether analysis is allowed to move a boundary from this source.
    ///
    /// §24, and the one rule in this module that is not advisory.
    #[must_use]
    pub const fn is_locked(self) -> bool {
        matches!(self, Self::User)
    }
}

/// Which way the audio crosses a boundary.
///
/// A detector finds the edges of sound, and the two edges are not interchangeable:
/// a start is an onset and an end is a decay, so a detector's certainty about one
/// says nothing about the other and the two are resolved separately.
///
/// An earlier version of this comment said the kind decides how much padding a
/// splitter applies, citing §33. It does not: §33 has no padding requirement, and
/// WP-14's splitter cuts exactly the span between two boundaries. The distinction
/// is real, the justification given for it was invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Edge {
    /// Silence into sound: a track begins.
    Start,
    /// Sound into silence: a track ends.
    End,
}

impl Edge {
    /// The kebab-case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

/// One measurement that supported a conclusion.
///
/// Deliberately a name and a number rather than a typed union. §24 requires the
/// evidence to be *carried*, and every detector's evidence is a handful of scalars it
/// already computed - level in dB, flatness, gap length in frames. A typed variant
/// per detector would have to be extended before a detector could report anything
/// new, which is exactly the wrong direction for a field that exists to explain a
/// decision after the fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// What was measured, in kebab-case: `level-db`, `flatness`, `gap-frames`.
    pub name: String,
    /// The value, in whatever unit the name implies.
    pub value: f64,
}

impl Evidence {
    /// A named measurement.
    #[must_use]
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

/// A boundary, and the case for it (§24).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryObservation {
    /// The frame the boundary sits at, in the capture's own timeline.
    pub at: u64,
    /// Which way the audio crosses it.
    pub edge: Edge,
    /// How sure the detector is, in `0.0..=1.0`.
    ///
    /// **Not a probability unless the provenance says so.** [`Provenance::Hmm`]
    /// reports a posterior under its own model, which is a probability about the
    /// model rather than about the record. Every other detector reports a
    /// margin-derived score, and the distinction is documented where each one
    /// computes it.
    pub confidence: f32,
    /// What put it here.
    pub provenance: Provenance,
    /// The measurements behind it.
    pub evidence: Vec<Evidence>,
}

impl BoundaryObservation {
    /// A boundary with no evidence attached yet.
    #[must_use]
    pub fn new(at: u64, edge: Edge, confidence: f32, provenance: Provenance) -> Self {
        Self {
            at,
            edge,
            confidence: confidence.clamp(0.0, 1.0),
            provenance,
            evidence: Vec::new(),
        }
    }

    /// Adds a measurement to the case.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: f64) -> Self {
        self.evidence.push(Evidence::new(name, value));
        self
    }

    /// Adds a measurement to a case already built.
    ///
    /// The sibling of [`BoundaryObservation::with`] for a detector that scores a
    /// boundary in two stages, which the spectral one does: the level evidence is the
    /// same as every other detector's and is gathered once, and the flatness evidence
    /// only that detector has.
    pub fn note(&mut self, name: impl Into<String>, value: f64) {
        self.evidence.push(Evidence::new(name, value));
    }

    /// The position in seconds, for printing.
    #[must_use]
    pub fn seconds(&self, rate: SampleRate) -> f64 {
        let hz = f64::from(rate.hz());
        if hz <= 0.0 {
            return 0.0;
        }
        self.at as f64 / hz
    }

    /// Looks up one piece of evidence by name.
    #[must_use]
    pub fn measurement(&self, name: &str) -> Option<f64> {
        self.evidence
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.value)
    }
}

/// A level reading over a window, for the record rather than for a meter (§17).
///
/// Distinct from [`crate::Summary`], which is the waveform pyramid's storage
/// triplet: this is an *observation*, timestamped in the capture's timeline and
/// published, and it is what a clip or a level warning is derived from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LevelObservation {
    /// First frame of the window.
    pub at: u64,
    /// Frames the reading covers.
    pub frames: u64,
    /// Peak in the window, as a fraction of full scale.
    pub peak: f32,
    /// RMS in the window, as a fraction of full scale.
    pub rms: f32,
}

/// A stretch of audio the detector considers silent (§22).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SilenceObservation {
    /// First frame of the gap.
    pub at: u64,
    /// How long it lasts.
    pub frames: u64,
    /// The quietest level in it, in dBFS.
    pub floor_db: f64,
}

/// Samples at or beyond full scale (§18).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClipObservation {
    /// Where it started.
    pub at: u64,
    /// Consecutive samples at the rail.
    pub samples: u64,
    /// Which channel, zero-based.
    pub channel: usize,
}

/// A fingerprint taken over a region (§25).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FingerprintObservation {
    /// First frame the fingerprint covers.
    pub at: u64,
    /// How much audio went into it.
    pub frames: u64,
    /// The fingerprint itself, base64 as the service expects it.
    pub fingerprint: String,
}

/// A candidate identity for a region, from whatever provider offered it (§26).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentificationObservation {
    /// First frame the identification covers.
    pub at: u64,
    /// How much audio it was based on.
    pub frames: u64,
    /// Who said so.
    pub source: String,
    /// How sure they were, in `0.0..=1.0`.
    pub confidence: f32,
    /// What they said, as a provider-specific identifier.
    pub identity: String,
}

/// §23's observation union: everything analysis is allowed to say.
///
/// The variants are §23's list verbatim. `#[non_exhaustive]` because §23 is explicit
/// that this is a publishing mechanism rather than a closed protocol - a new analysis
/// subsystem adds a variant, and no consumer should break for not knowing it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum AudioObservation {
    /// A level reading.
    Level(LevelObservation),
    /// A gap.
    Silence(SilenceObservation),
    /// A boundary, with its case.
    Boundary(BoundaryObservation),
    /// A fingerprint.
    Fingerprint(FingerprintObservation),
    /// A candidate identity.
    Identification(IdentificationObservation),
    /// Samples at the rail.
    Clip(ClipObservation),
}

impl AudioObservation {
    /// The kebab-case variant name, which is what the event bus publishes.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Level(_) => "level",
            Self::Silence(_) => "silence",
            Self::Boundary(_) => "boundary",
            Self::Fingerprint(_) => "fingerprint",
            Self::Identification(_) => "identification",
            Self::Clip(_) => "clip",
        }
    }

    /// The frame the observation starts at, whatever kind it is.
    #[must_use]
    pub const fn at(&self) -> u64 {
        match self {
            Self::Level(o) => o.at,
            Self::Silence(o) => o.at,
            Self::Boundary(o) => o.at,
            Self::Fingerprint(o) => o.at,
            Self::Identification(o) => o.at,
            Self::Clip(o) => o.at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_boundary_is_the_only_locked_one() {
        // §24: user-confirmed boundaries shall not be moved by automatic
        // analysis. Everything else is a suggestion, including the HMM's.
        for provenance in [
            Provenance::Silence,
            Provenance::SpectralChange,
            Provenance::Hmm,
            Provenance::Fingerprint,
            Provenance::MetadataDuration,
            Provenance::ReleaseTopology,
        ] {
            assert!(!provenance.is_locked(), "{provenance:?} claimed a lock");
        }
        assert!(Provenance::User.is_locked());
    }

    #[test]
    fn user_outranks_every_automatic_source() {
        // The ordering is load-bearing: the resolver uses it to break ties, so
        // it is asserted rather than left to the order of the variants.
        let mut sources = [
            Provenance::Hmm,
            Provenance::User,
            Provenance::Silence,
            Provenance::MetadataDuration,
        ];
        sources.sort_unstable();
        assert_eq!(*sources.last().expect("not empty"), Provenance::User);
    }

    #[test]
    fn confidence_cannot_be_reported_outside_its_range() {
        // A detector that divides by a variance of nearly zero produces
        // infinities, and a UI that draws confidence as a bar cannot.
        let over = BoundaryObservation::new(0, Edge::Start, 4.2, Provenance::Hmm);
        let under = BoundaryObservation::new(0, Edge::Start, -1.0, Provenance::Hmm);
        assert!((over.confidence - 1.0).abs() < f32::EPSILON);
        assert!(under.confidence.abs() < f32::EPSILON);
    }

    #[test]
    fn evidence_is_carried_and_can_be_read_back() {
        let observation = BoundaryObservation::new(96_000, Edge::End, 0.8, Provenance::Silence)
            .with("level-db", -62.5)
            .with("gap-frames", 48_000.0);
        assert_eq!(observation.measurement("level-db"), Some(-62.5));
        assert_eq!(observation.measurement("gap-frames"), Some(48_000.0));
        assert_eq!(observation.measurement("flatness"), None);
    }

    #[test]
    fn a_position_is_frames_and_seconds_are_derived() {
        let observation = BoundaryObservation::new(96_000, Edge::Start, 0.5, Provenance::Hmm);
        assert_eq!(observation.at, 96_000);
        assert!((observation.seconds(SampleRate(48_000)) - 2.0).abs() < 1e-12);
        // And a nonsense rate does not produce a nonsense answer.
        assert!((observation.seconds(SampleRate(0)) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn every_observation_reports_where_it_was_taken() {
        let observations = [
            AudioObservation::Level(LevelObservation {
                at: 1,
                frames: 2,
                peak: 0.5,
                rms: 0.25,
            }),
            AudioObservation::Silence(SilenceObservation {
                at: 2,
                frames: 3,
                floor_db: -70.0,
            }),
            AudioObservation::Boundary(BoundaryObservation::new(
                3,
                Edge::Start,
                1.0,
                Provenance::Hmm,
            )),
            AudioObservation::Clip(ClipObservation {
                at: 4,
                samples: 5,
                channel: 1,
            }),
        ];
        let positions: Vec<u64> = observations.iter().map(AudioObservation::at).collect();
        assert_eq!(positions, vec![1, 2, 3, 4]);
        let names: Vec<&str> = observations.iter().map(AudioObservation::name).collect();
        assert_eq!(names, vec!["level", "silence", "boundary", "clip"]);
    }

    #[test]
    fn an_observation_survives_a_round_trip_as_json() {
        // It crosses the IPC boundary at WP-15 and is persisted at WP-13, so the
        // tagged representation is part of the contract rather than a detail.
        let observation = AudioObservation::Boundary(
            BoundaryObservation::new(48_000, Edge::End, 0.75, Provenance::SpectralChange)
                .with("flatness", 0.91),
        );
        let json = serde_json::to_string(&observation).expect("serialize");
        assert!(json.contains("\"kind\":\"boundary\""), "{json}");
        assert!(
            json.contains("\"provenance\":\"spectral-change\""),
            "{json}"
        );
        let back: AudioObservation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, observation);
    }
}
