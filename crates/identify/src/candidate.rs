/*
 *  candidate.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A candidate release, and what it does and does not agree with.
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

//! A candidate release, and what it does and does not agree with.
//!
//! Requirements: §26 (multiple identified tracks constrain the likely release), §28
//! (telling two pressings apart).
//!
//! # Why the verdict is three-way
//!
//! Silent is not a small disagreement, it is a different thing entirely. MusicBrainz
//! very often has no catalogue number; a release that *has none* is merely unsupported
//! by the number on somebody's sleeve, while a release that has a *different* one is
//! contradicted and should lose to a candidate with less evidence in its favour. Fold
//! those two into "did not match" and the better-documented database loses every time
//! it is honest about a mismatch.

use std::time::Duration;

use vcw_metadata::release::{Candidate as Found, Release};

use crate::evidence::{Fact, Item, Kind, Observed, Source, fold_catalogue, fold_text};

/// How much two durations may differ and still be the same side.
///
/// A side's audio is not the sum of its catalogued track lengths: there is a lead-in,
/// run-out and a gap between every track, and the catalogued lengths themselves are
/// the master's rather than the groove's. Measured across a real double LP, confirmed
/// track starts sat 2 to 10 s past their catalogued cumulative position, so a minute
/// and a half of slack over a whole side is loose enough not to reject a correct
/// release and tight enough to reject a single LP offered for a double's capture.
pub const SIDE_SLACK: Duration = Duration::from_secs(90);

/// What a candidate release claims about itself.
///
/// Built from either a search result or a fetched release, so the comparison below is
/// written once. A search result knows less, and that shows up as [`Verdict::Silent`]
/// rather than as a worse score.
#[derive(Debug, Clone, Default)]
pub struct Claim {
    /// Every identifier this release answers to, for matching a recording's release
    /// list: the provider's own id plus any cross-referenced ids.
    pub ids: Vec<String>,
    /// The credited artist.
    pub artist: String,
    /// The release title.
    pub album: String,
    /// The catalogue number, empty when the provider has none.
    pub catalog: String,
    /// The label, empty when the provider has none.
    pub label: String,
    /// The year, when stated.
    pub year: Option<u32>,
    /// How many tracks, when known.
    pub tracks: Option<usize>,
    /// The total length of each side whose tracks all have durations.
    pub sides: Vec<Duration>,
}

impl Claim {
    /// What a search result claims. Durations and sides are unknown at this stage.
    #[must_use]
    pub fn of(found: &Found) -> Self {
        Self {
            ids: vec![found.id.clone()],
            artist: found.artist.clone(),
            album: found.album.clone(),
            catalog: found.catalog.clone(),
            label: found.label.clone(),
            year: found.year,
            tracks: found.tracks,
            sides: Vec::new(),
        }
    }

    /// What a fetched release claims, including its side lengths.
    #[must_use]
    pub fn of_release(release: &Release) -> Self {
        let mut ids = vec![release.id.clone()];
        for id in [&release.musicbrainz_id, &release.discogs_id]
            .into_iter()
            .flatten()
        {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        Self {
            ids,
            artist: release.album_artist.clone(),
            album: release.album.clone(),
            catalog: release.catalog.clone(),
            label: release.label.clone(),
            year: release.year,
            tracks: Some(release.tracks().len()),
            sides: side_lengths(release),
        }
    }
}

/// The total length of each side whose every track has a stated duration.
///
/// A side with one missing duration is left out rather than under-reported: a total
/// that is short by one track would look like a contradiction, which is worse than
/// saying nothing.
fn side_lengths(release: &Release) -> Vec<Duration> {
    let mut sides: Vec<(String, Option<Duration>)> = Vec::new();
    for track in release.tracks() {
        let key = track
            .resolved
            .map_or_else(|| track.position.clone(), |p| p.side.to_string());
        let slot = match sides.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot,
            None => {
                sides.push((key, Some(Duration::ZERO)));
                sides.last_mut().expect("just pushed")
            }
        };
        slot.1 = match (slot.1, track.duration) {
            (Some(running), Some(length)) => Some(running + length),
            _ => None,
        };
    }
    sides.into_iter().filter_map(|(_, total)| total).collect()
}

/// Whether a candidate agrees with one fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The candidate says the same thing.
    Agrees,
    /// The candidate says something else.
    Disagrees,
    /// The candidate has nothing to say about it.
    Silent,
}

/// One fact, who said it, and what the candidate made of it.
#[derive(Debug, Clone)]
pub struct Agreement {
    /// The fact being tested.
    pub fact: Fact,
    /// Who stated the fact.
    pub source: Source,
    /// What the candidate had to say.
    pub verdict: Verdict,
}

/// A candidate and its account of the evidence.
#[derive(Debug, Clone)]
pub struct Assessed {
    /// What the candidate claims.
    pub claim: Claim,
    /// One entry per fact, in the order the facts arrived.
    pub agreements: Vec<Agreement>,
}

impl Assessed {
    /// Every fact of one kind that the candidate contradicted.
    pub fn disagreements(&self) -> impl Iterator<Item = &Agreement> {
        self.agreements
            .iter()
            .filter(|a| a.verdict == Verdict::Disagrees)
    }

    /// Whether the candidate contradicted anything a person stated.
    ///
    /// Separate from the score because §26 makes it a different kind of problem: a
    /// release that disputes what somebody read off the sleeve is not a low-scoring
    /// answer, it is the wrong record.
    #[must_use]
    pub fn contradicts_a_person(&self) -> bool {
        self.disagreements().any(|a| a.source == Source::Stated)
    }
}

/// Tests one candidate against everything known.
#[must_use]
pub fn assess(claim: Claim, observed: &Observed) -> Assessed {
    let agreements = observed
        .items()
        .iter()
        .map(|item| Agreement {
            fact: item.fact.clone(),
            source: item.source,
            verdict: verdict(&claim, item),
        })
        .collect();
    Assessed { claim, agreements }
}

/// A candidate's verdict on one fact.
fn verdict(claim: &Claim, item: &Item) -> Verdict {
    match &item.fact {
        Fact::Artist(name) => text(&claim.artist, name),
        Fact::Album(title) => text(&claim.album, title),
        Fact::Catalogue(number) => {
            if claim.catalog.is_empty() {
                Verdict::Silent
            } else if fold_catalogue(&claim.catalog) == fold_catalogue(number) {
                Verdict::Agrees
            } else {
                Verdict::Disagrees
            }
        }
        Fact::Label(name) => text(&claim.label, name),
        Fact::Year(year) => match claim.year {
            None => Verdict::Silent,
            Some(theirs) if theirs == *year => Verdict::Agrees,
            Some(_) => Verdict::Disagrees,
        },
        Fact::Count(count) => match claim.tracks {
            None => Verdict::Silent,
            Some(theirs) if theirs == *count => Verdict::Agrees,
            Some(_) => Verdict::Disagrees,
        },
        Fact::Side(length) => {
            if claim.sides.is_empty() {
                Verdict::Silent
            } else if claim
                .sides
                .iter()
                .any(|side| side.abs_diff(*length) <= SIDE_SLACK)
            {
                Verdict::Agrees
            } else {
                Verdict::Disagrees
            }
        }
        // Never Disagrees: AcoustID's release lists come from digital submissions
        // and routinely omit a vinyl pressing entirely, so a candidate missing from
        // the list is unmentioned rather than excluded.
        Fact::Recording { releases, .. } => {
            if claim.ids.iter().any(|id| releases.contains(id)) {
                Verdict::Agrees
            } else {
                Verdict::Silent
            }
        }
    }
}

/// Compares two pieces of free text, treating an empty claim as no claim.
fn text(claim: &str, stated: &str) -> Verdict {
    if claim.is_empty() {
        Verdict::Silent
    } else if fold_text(claim) == fold_text(stated) {
        Verdict::Agrees
    } else {
        Verdict::Disagrees
    }
}

/// The kinds of fact a candidate contradicted, for a report.
#[must_use]
pub fn contradicted(assessed: &Assessed) -> Vec<Kind> {
    let mut kinds: Vec<Kind> = Vec::new();
    for agreement in assessed.disagreements() {
        let kind = agreement.fact.kind();
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vienna() -> Claim {
        Claim {
            ids: vec!["1".into()],
            artist: "Ultravox".into(),
            album: "Vienna".into(),
            catalog: "CHRH1296".into(),
            label: "Chrysalis".into(),
            year: Some(1980),
            tracks: Some(9),
            sides: vec![Duration::from_secs(1300), Duration::from_secs(1260)],
        }
    }

    #[test]
    fn a_catalogue_number_agrees_through_a_difference_of_spelling() {
        let mut observed = Observed::new();
        observed.add(Source::Stated, Fact::Catalogue("CHRH 1296".into()));
        let assessed = assess(vienna(), &observed);
        assert_eq!(assessed.agreements[0].verdict, Verdict::Agrees);
    }

    #[test]
    fn a_release_with_no_catalogue_number_is_silent_and_not_wrong() {
        let mut observed = Observed::new();
        observed.add(Source::Stated, Fact::Catalogue("CHRH 1296".into()));
        let mut claim = vienna();
        claim.catalog = String::new();
        let assessed = assess(claim, &observed);
        assert_eq!(assessed.agreements[0].verdict, Verdict::Silent);
        assert!(!assessed.contradicts_a_person(), "silence is not a dispute");
    }

    #[test]
    fn a_different_catalogue_number_contradicts_the_person_who_read_it() {
        let mut observed = Observed::new();
        observed.add(Source::Stated, Fact::Catalogue("CHRH 1297".into()));
        let assessed = assess(vienna(), &observed);
        assert_eq!(assessed.agreements[0].verdict, Verdict::Disagrees);
        assert!(assessed.contradicts_a_person());
        assert_eq!(contradicted(&assessed), vec![Kind::Catalogue]);
    }

    #[test]
    fn a_side_agrees_within_the_slack_and_not_outside_it() {
        let mut observed = Observed::new();
        // 1300 s claimed, 1340 s captured: a lead-in and eight gaps.
        observed.add(Source::Signal, Fact::Side(Duration::from_secs(1340)));
        assert_eq!(
            assess(vienna(), &observed).agreements[0].verdict,
            Verdict::Agrees
        );

        let mut observed = Observed::new();
        // 35 minutes of audio against a longest side of 21: a double offered for
        // a single, which is exactly the mistake this is here to catch.
        observed.add(Source::Signal, Fact::Side(Duration::from_secs(2100)));
        assert_eq!(
            assess(vienna(), &observed).agreements[0].verdict,
            Verdict::Disagrees
        );
    }

    #[test]
    fn an_identified_recording_that_lists_another_pressing_says_nothing_against_this_one() {
        let mut observed = Observed::new();
        observed.add(
            Source::AcoustId,
            Fact::Recording {
                id: "rec-1".into(),
                score: 0.86,
                releases: vec!["some-cd-release".into()],
            },
        );
        let assessed = assess(vienna(), &observed);
        assert_eq!(
            assessed.agreements[0].verdict,
            Verdict::Silent,
            "an incomplete release list must not exclude a pressing"
        );

        let mut observed = Observed::new();
        observed.add(
            Source::AcoustId,
            Fact::Recording {
                id: "rec-1".into(),
                score: 0.86,
                releases: vec!["1".into()],
            },
        );
        assert_eq!(
            assess(vienna(), &observed).agreements[0].verdict,
            Verdict::Agrees
        );
    }

    #[test]
    fn a_side_with_a_missing_duration_is_left_out_rather_than_under_reported() {
        use vcw_metadata::release::{Medium, TrackEntry};
        let track = |position: &str, secs: Option<u64>| TrackEntry {
            position: position.into(),
            resolved: match vcw_metadata::positions::read(position, 1) {
                vcw_metadata::positions::Reading::Exact(at) => Some(at),
                _ => None,
            },
            title: position.into(),
            artist: None,
            duration: secs.map(Duration::from_secs),
        };
        let release = Release {
            id: "r".into(),
            album: "x".into(),
            album_artist: "y".into(),
            year: None,
            genres: Vec::new(),
            label: String::new(),
            catalog: String::new(),
            country: String::new(),
            barcode: None,
            musicbrainz_id: None,
            discogs_id: None,
            artwork: Vec::new(),
            media: vec![Medium {
                position: 1,
                format: "Vinyl".into(),
                tracks: vec![
                    track("A1", Some(100)),
                    track("A2", None),
                    track("B1", Some(200)),
                    track("B2", Some(300)),
                ],
            }],
        };
        let claim = Claim::of_release(&release);
        assert_eq!(
            claim.sides,
            vec![Duration::from_secs(500)],
            "side A has an unknown total and must not be reported as 100 s"
        );
        assert_eq!(claim.tracks, Some(4));
    }
}
