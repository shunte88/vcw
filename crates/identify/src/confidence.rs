/*
 *  confidence.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The weights, and the thresholds below which the application asks rather than asserts.
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

//! The weights, and the thresholds below which the application asks rather than
//! asserts.
//!
//! Requirements: §26 (identification is evidence-based, and never silently replaces
//! what a person confirmed), §28 (pressings).
//!
//! Every number in this file is in this file. That is the point of it: a wrong
//! identification is diagnosed by reading one table and the [`crate::candidate`]
//! agreements that fed it, not by tracing arithmetic through four modules. The values
//! are judgements rather than measurements, and they are each justified where they sit
//! so that changing one is an argument with a comment and not a guess.

use crate::candidate::{Assessed, Verdict};
use crate::evidence::{Fact, Kind};

/// At or above this, the application may state the release without asking.
///
/// Reaching it needs more than one kind of agreement: an exact catalogue number is
/// 0.55, which is deliberately not enough on its own, because a catalogue number
/// mistyped into a search box can match the wrong record and the artist and title
/// are free to check.
pub const CERTAIN: f32 = 0.80;

/// At or above this, the candidate is worth showing. Below it, it is noise.
///
/// Set at exactly what a release agreeing on artist and title is worth, because that
/// is the normal result of a text search and the normal state of "right album, unknown
/// pressing" - which is a question to put to a person, not a candidate to discard. One
/// field agreeing is below it, and should be: an artist search returns every record
/// that artist made.
pub const LIKELY: f32 = 0.40;

/// How far ahead the best candidate must be before it is chosen without asking.
///
/// Two pressings of the same record agree with artist, title and track count equally
/// well, and the catalogue number is often the only thing between them. Where it is
/// absent they tie, and a tie must be a question rather than a coin toss: picking the
/// 2020 reissue for a 1980 original gets the mastering and the year wrong in the tags
/// of every track.
pub const MARGIN: f32 = 0.15;

/// The most that identified recordings can contribute between them.
///
/// §26 says multiple identified tracks constrain the likely release, so they
/// accumulate. The cap is because they accumulate on the *work*, not the pressing: ten
/// confirmed tracks prove the audio is this album and say nothing about which pressing
/// of it, so without a cap a well-fingerprinted record would be stated with certainty
/// and the wrong catalogue number.
pub const RECORDING_CAP: f32 = 0.60;

/// What one agreement is worth, and what one disagreement costs.
///
/// Both are positive magnitudes; the sign is applied by [`weigh`].
#[must_use]
pub const fn credit(kind: Kind) -> (f32, f32) {
    match kind {
        // The one identifier a vinyl pressing reliably carries, and the only field
        // that distinguishes two pressings of one record. Worth more than anything
        // else and still not sufficient alone.
        Kind::Catalogue => (0.55, 0.40),
        // Agreement is cheap because almost every candidate in a search for an
        // artist agrees about the artist. Disagreement is expensive because it means
        // the search returned something else entirely. The two together are set to
        // reach `LIKELY` exactly and `CERTAIN` not at all: knowing the album without
        // the catalogue number is precisely the state in which VCW must ask.
        Kind::Artist => (0.20, 0.50),
        Kind::Album => (0.20, 0.50),
        // A real constraint: a single LP offered for a double's capture disagrees
        // here, and nothing else in a text search would catch it.
        Kind::Count => (0.15, 0.30),
        // Measured from the audio, so a disagreement is a fact about the object and
        // not about a database. Worth less than the count only because catalogued
        // durations are approximate.
        Kind::Side => (0.10, 0.30),
        // Labels get renamed, licensed and spelled differently by each provider, so
        // a mismatch is weak evidence of anything.
        Kind::Label => (0.05, 0.10),
        // A reissue legitimately carries a different year from the pressing a person
        // typed, and providers disagree about which year they mean. Nearly inert on
        // purpose, kept because agreement is still a small positive.
        Kind::Year => (0.04, 0.04),
        // Handled separately in `weigh`: scaled by the score, summed, and capped.
        // Never a disagreement, for the reason `candidate::verdict` gives.
        Kind::Recording => (0.20, 0.00),
    }
}

/// How well a candidate accounts for the evidence, from 0.0 to 1.0.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Confidence(f32);

impl Confidence {
    /// The score itself.
    #[must_use]
    pub const fn value(self) -> f32 {
        self.0
    }

    /// What the application should do about it.
    #[must_use]
    pub fn stance(self) -> Stance {
        if self.0 >= CERTAIN {
            Stance::Certain
        } else if self.0 >= LIKELY {
            Stance::Likely
        } else {
            Stance::Ask
        }
    }
}

/// What a score means for the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stance {
    /// State it.
    Certain,
    /// Offer it, with the evidence.
    Likely,
    /// Ask.
    Ask,
}

/// Scores one assessed candidate.
///
/// Agreements add, disagreements subtract, and both are scaled by how much the source
/// of the fact is worth. Recordings are summed separately so the cap can apply to them
/// as a group rather than to each one.
#[must_use]
pub fn weigh(assessed: &Assessed) -> Confidence {
    let mut score = 0.0_f32;
    let mut recordings = 0.0_f32;
    for agreement in &assessed.agreements {
        let kind = agreement.fact.kind();
        let (agrees, disagrees) = credit(kind);
        let weight = agreement.source.weight();
        match (kind, agreement.verdict) {
            (Kind::Recording, Verdict::Agrees) => {
                // Scaled by the score, because a 0.64 match on a worn vinyl track
                // is weaker evidence than a 0.99 one and the difference is exactly
                // what the score is for.
                let Fact::Recording { score: matched, .. } = &agreement.fact else {
                    continue;
                };
                recordings += agrees * weight * matched.clamp(0.0, 1.0);
            }
            (_, Verdict::Agrees) => score += agrees * weight,
            (_, Verdict::Disagrees) => score -= disagrees * weight,
            (_, Verdict::Silent) => {}
        }
    }
    Confidence((score + recordings.min(RECORDING_CAP)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::{Claim, assess};
    use crate::evidence::{Fact, Observed, Source};
    use std::time::Duration;

    fn claim() -> Claim {
        Claim {
            ids: vec!["1".into()],
            artist: "Ultravox".into(),
            album: "Vienna".into(),
            catalog: "CHRH1296".into(),
            label: "Chrysalis".into(),
            year: Some(1980),
            tracks: Some(9),
            sides: vec![Duration::from_secs(1300)],
        }
    }

    fn weigh_with(build: impl FnOnce(&mut Observed)) -> Confidence {
        let mut observed = Observed::new();
        build(&mut observed);
        weigh(&assess(claim(), &observed))
    }

    #[test]
    fn a_catalogue_number_alone_is_offered_and_not_asserted() {
        let score = weigh_with(|o| {
            o.add(Source::Stated, Fact::Catalogue("CHRH 1296".into()));
        });
        assert_eq!(score.stance(), Stance::Likely, "{score:?}");
        assert!(score.value() < CERTAIN, "a number can be mistyped");
    }

    #[test]
    fn the_whole_sleeve_agreeing_is_stated_without_asking() {
        let score = weigh_with(|o| {
            o.add(Source::Stated, Fact::Catalogue("CHRH 1296".into()))
                .add(Source::Stated, Fact::Artist("Ultravox".into()))
                .add(Source::Stated, Fact::Album("Vienna".into()))
                .add(Source::Signal, Fact::Count(9));
        });
        assert_eq!(score.stance(), Stance::Certain, "{score:?}");
    }

    #[test]
    fn a_right_catalogue_number_does_not_rescue_the_wrong_artist() {
        let score = weigh_with(|o| {
            o.add(Source::Stated, Fact::Catalogue("CHRH 1296".into()))
                .add(Source::Stated, Fact::Artist("Depeche Mode".into()));
        });
        assert_eq!(score.stance(), Stance::Ask, "{score:?}");
    }

    #[test]
    fn a_contradicted_track_count_loses_to_a_candidate_that_is_merely_silent() {
        let mut observed = Observed::new();
        observed
            .add(Source::Stated, Fact::Artist("Ultravox".into()))
            .add(Source::Signal, Fact::Count(12));

        let contradicts = weigh(&assess(claim(), &observed));
        let mut quiet = claim();
        quiet.tracks = None;
        let silent = weigh(&assess(quiet, &observed));
        assert!(
            silent > contradicts,
            "silent {silent:?} must beat contradicted {contradicts:?}"
        );
    }

    #[test]
    fn a_providers_own_agreement_counts_half_of_a_persons() {
        let stated = weigh_with(|o| {
            o.add(Source::Stated, Fact::Catalogue("CHRH 1296".into()));
        });
        let theirs = weigh_with(|o| {
            o.add(Source::Discogs, Fact::Catalogue("CHRH 1296".into()));
        });
        assert!(
            (stated.value() - theirs.value() * 2.0).abs() < 1e-6,
            "{stated:?} {theirs:?}"
        );
    }

    #[test]
    fn identified_recordings_accumulate_but_cannot_choose_a_pressing_on_their_own() {
        let score = weigh_with(|o| {
            for n in 0..12 {
                o.add(
                    Source::AcoustId,
                    Fact::Recording {
                        id: format!("rec-{n}"),
                        score: 1.0,
                        releases: vec!["1".into()],
                    },
                );
            }
        });
        assert!(score.value() >= LIKELY, "twelve tracks is real evidence");
        assert!(
            score.value() < CERTAIN,
            "but it identifies the album, not the pressing: {score:?}"
        );
    }

    #[test]
    fn a_weak_match_is_worth_less_than_a_strong_one() {
        let strong = weigh_with(|o| {
            o.add(
                Source::AcoustId,
                Fact::Recording {
                    id: "r".into(),
                    score: 0.99,
                    releases: vec!["1".into()],
                },
            );
        });
        let weak = weigh_with(|o| {
            o.add(
                Source::AcoustId,
                Fact::Recording {
                    id: "r".into(),
                    score: 0.64,
                    releases: vec!["1".into()],
                },
            );
        });
        assert!(strong > weak, "{strong:?} {weak:?}");
    }

    #[test]
    fn nothing_known_scores_nothing_and_a_pile_of_disagreement_does_not_go_negative() {
        assert_eq!(weigh_with(|_| {}).value(), 0.0);
        let sunk = weigh_with(|o| {
            o.add(Source::Stated, Fact::Artist("Wrong".into()))
                .add(Source::Stated, Fact::Album("Also Wrong".into()))
                .add(Source::Stated, Fact::Catalogue("XX 1".into()));
        });
        assert_eq!(sunk.value(), 0.0, "clamped, not negative");
        assert_eq!(sunk.stance(), Stance::Ask);
    }
}
