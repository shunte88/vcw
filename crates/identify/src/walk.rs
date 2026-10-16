/*
 *  walk.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The escalation walk: makes the lookups §26 escalates through, and stops when the evidence is enough.
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

//! The escalation walk: makes the lookups, and stops when the evidence is enough.
//!
//! [`crate::resolver`] says what to ask next and what the answers come to; this is
//! the part that asks. §26's escalation only saves anything if the walk stops as
//! soon as it can, so after every lookup the whole candidate list is re-resolved and
//! an [`Outcome::Resolved`] ends the walk. A record whose catalog number is on the
//! sleeve costs one request; the audio lookup at the end of the order is reached
//! only by a record that was not found by name at all.
//!
//! # Providers are found, not configured
//!
//! Each lookup names the service it belongs to and goes looking for it in the slice
//! it was handed. A provider that is not there - no Discogs token, a build with
//! `net` off - is not an error and not a failed step: its lookups are simply not
//! made, and [`Walked::steps`] records what was. That is also how a caller asks for
//! a subset, by handing over fewer providers rather than by setting flags.
//!
//! # The audio lookup is paid for twice
//!
//! Once to compute the fingerprint and once to turn AcoustID's answer into
//! candidates, because AcoustID returns recordings and the resolver ranks releases.
//! Both costs land only if the walk gets that far, which is why [`Audio::fingerprint`]
//! is a closure rather than a value: a caller passes the means to fingerprint, not
//! the fingerprint.

use vcw_metadata::acoustid::Match;
use vcw_metadata::query::Fingerprint;
use vcw_metadata::release::ProviderId;
use vcw_metadata::{AcoustId, Cancel, Error, Provider, Query};

use crate::candidate::{Claim, assess};
use crate::evidence::{Fact, Kind, Observed, Source};
use crate::resolver::{Lookup, Outcome, next_lookup, resolve};

/// How many releases one AcoustID answer may cost in fetches.
///
/// Thirteen releases came back for one Autechre recording and several matches
/// multiply that. The budget is what keeps the last lookup in the order from also
/// being an unbounded one; the releases it spends are chosen vinyl first.
pub const FETCH_BUDGET: usize = 8;

/// What [`Lookup::IdentifyAudio`] needs, for a caller willing to pay for it.
pub struct Audio<'a> {
    /// The service.
    pub acoustid: &'a AcoustId,
    /// How to fingerprint the audio, called at most once and only if the walk gets
    /// as far as needing it.
    ///
    /// `None` from the closure means the audio could not be fingerprinted, which
    /// ends the lookup without making it a failure: there was nothing to ask with.
    ///
    /// The fingerprint's `seconds` must be the believed length of the *recording*,
    /// not of the window the code was taken over - AcoustID filters on it exactly.
    /// See [`Fingerprint`].
    pub fingerprint: &'a dyn Fn() -> Option<Fingerprint>,
}

impl std::fmt::Debug for Audio<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("acoustid", &self.acoustid)
            .finish_non_exhaustive()
    }
}

/// One lookup that was actually made.
#[derive(Debug)]
pub struct Step {
    /// Which lookup.
    pub lookup: Lookup,
    /// How many new candidates it contributed. A pressing another lookup already
    /// returned is not counted twice.
    pub found: usize,
    /// Why it contributed less than it might have, when something went wrong.
    ///
    /// A failure here does not end the walk unless it is [`Error::Canceled`]: §40's
    /// point about a search is that one provider being down is a footnote under a
    /// list, not the absence of one.
    pub failed: Option<Error>,
}

/// What a walk came to.
#[derive(Debug)]
pub struct Walked {
    /// What the evidence supports.
    pub outcome: Outcome,
    /// Every lookup made, in the order made.
    ///
    /// A lookup VCW had no provider for is absent rather than present and empty,
    /// because it was not made. The length of this is what the walk cost.
    pub steps: Vec<Step>,
    /// Everything known by the end, which is what went in plus whatever AcoustID
    /// recognized in the audio.
    pub observed: Observed,
}

/// Walks the lookup order until the evidence resolves or the order runs out.
///
/// Never an error. Every way a lookup can fail is a [`Step`] that says so, because
/// a walk that gave up on the first timeout would be a worse answer than the one
/// the other three lookups can still produce.
///
/// ```no_run
/// use vcw_identify::evidence::{Fact, Observed, Source};
/// use vcw_identify::walk::walk;
/// use vcw_metadata::Cancel;
///
/// let mut observed = Observed::new();
/// observed.add(Source::Stated, Fact::Catalog("CHRH 1296".into()));
///
/// let walked = walk(&[], None, &observed, &Cancel::new());
/// assert!(walked.steps.is_empty(), "no providers, so no lookups were made");
/// ```
#[must_use]
pub fn walk(
    providers: &[&dyn Provider],
    audio: Option<&Audio<'_>>,
    observed: &Observed,
    cancel: &Cancel,
) -> Walked {
    let mut observed = observed.clone();
    let mut tried: Vec<Lookup> = Vec::new();
    let mut steps: Vec<Step> = Vec::new();
    let mut claims: Vec<Claim> = Vec::new();
    let mut outcome = Outcome::Nothing;

    while let Some(lookup) = next_lookup(&observed, &tried) {
        tried.push(lookup);
        let Some(step) = make(lookup, providers, audio, &mut observed, &mut claims, cancel) else {
            continue;
        };
        let canceled = matches!(step.failed, Some(Error::Canceled));
        steps.push(step);
        if canceled {
            break;
        }
        // Re-resolved from the whole list rather than from the new candidates,
        // and re-assessed rather than reused: a lookup can add a fact as well as a
        // candidate, and a recording AcoustID heard changes what the candidates an
        // earlier lookup found are worth.
        outcome = resolve(
            claims
                .iter()
                .map(|claim| assess(claim.clone(), &observed))
                .collect(),
        );
        if matches!(outcome, Outcome::Resolved { .. }) {
            break;
        }
    }

    Walked {
        outcome,
        steps,
        observed,
    }
}

/// Makes one lookup, or says it could not be made.
///
/// `None` is "there was nothing to make it with" - no provider for this service, no
/// fingerprint - which is different from a request that was sent and came back
/// empty, and the two must not read alike in [`Walked::steps`].
fn make(
    lookup: Lookup,
    providers: &[&dyn Provider],
    audio: Option<&Audio<'_>>,
    observed: &mut Observed,
    claims: &mut Vec<Claim>,
    cancel: &Cancel,
) -> Option<Step> {
    match lookup {
        Lookup::CatalogAtDiscogs => {
            let provider = find(providers, ProviderId::Discogs)?;
            let catalog = stated(observed, Kind::Catalog)?;
            Some(search(
                lookup,
                provider,
                Query::new().catalog(catalog),
                claims,
                cancel,
            ))
        }
        Lookup::SearchDiscogs => {
            let provider = find(providers, ProviderId::Discogs)?;
            Some(search(lookup, provider, by_name(observed), claims, cancel))
        }
        Lookup::SearchMusicBrainz => {
            let provider = find(providers, ProviderId::MusicBrainz)?;
            Some(search(lookup, provider, by_name(observed), claims, cancel))
        }
        Lookup::IdentifyAudio => identify(audio?, providers, observed, claims, cancel),
    }
}

/// The provider for one service, if the caller handed it over.
fn find<'a>(providers: &'a [&'a dyn Provider], id: ProviderId) -> Option<&'a dyn Provider> {
    providers
        .iter()
        .copied()
        .find(|provider| provider.id() == id)
}

/// What a person stated of one kind, as the string to send.
fn stated(observed: &Observed, kind: Kind) -> Option<String> {
    match observed.stated(kind)? {
        Fact::Artist(value) | Fact::Album(value) | Fact::Catalog(value) | Fact::Label(value) => {
            Some(value.clone())
        }
        _ => None,
    }
}

/// The query both name searches send.
fn by_name(observed: &Observed) -> Query {
    let mut query = Query::new();
    if let Some(artist) = stated(observed, Kind::Artist) {
        query = query.artist(artist);
    }
    if let Some(album) = stated(observed, Kind::Album) {
        query = query.album(album);
    }
    query
}

/// One text search, folded into the candidate list.
fn search(
    lookup: Lookup,
    provider: &dyn Provider,
    query: Query,
    claims: &mut Vec<Claim>,
    cancel: &Cancel,
) -> Step {
    match provider.search(&query, cancel) {
        Ok(found) => {
            let before = claims.len();
            for candidate in &found {
                keep(claims, Claim::of(candidate));
            }
            Step {
                lookup,
                found: claims.len() - before,
                failed: None,
            }
        }
        Err(failed) => Step {
            lookup,
            found: 0,
            failed: Some(failed),
        },
    }
}

/// Keeps a claim unless something already held answers to the same identifier.
///
/// The two Discogs lookups routinely return the same pressing. Held twice it would
/// be its own runner-up, and two candidates a hair apart is exactly the shape
/// [`crate::Doubt::TooClose`] refuses to choose between - so the duplicate would
/// turn an answer into a question.
fn keep(claims: &mut Vec<Claim>, claim: Claim) {
    let known = claims
        .iter()
        .any(|held| held.ids.iter().any(|id| claim.ids.contains(id)));
    if !known {
        claims.push(claim);
    }
}

/// The audio lookup: recognize the recordings, then fetch the releases they name.
fn identify(
    audio: &Audio<'_>,
    providers: &[&dyn Provider],
    observed: &mut Observed,
    claims: &mut Vec<Claim>,
    cancel: &Cancel,
) -> Option<Step> {
    let lookup = Lookup::IdentifyAudio;
    let fingerprint = (audio.fingerprint)()?;
    let matched = match audio.acoustid.lookup(&fingerprint, cancel) {
        Ok(matched) => matched,
        Err(failed) => {
            return Some(Step {
                lookup,
                found: 0,
                failed: Some(failed),
            });
        }
    };

    for found in &matched {
        observed.add(
            Source::AcoustId,
            Fact::Recording {
                id: found.recording.id.clone(),
                score: found.score,
                releases: found
                    .recording
                    .releases
                    .iter()
                    .map(|release| release.id.clone())
                    .collect(),
            },
        );
    }

    // No MusicBrainz, no candidates: AcoustID names MusicBrainz releases and
    // nothing else here can turn one into a claim. The recordings are still
    // evidence for whatever the text searches found.
    let Some(provider) = find(providers, ProviderId::MusicBrainz) else {
        return Some(Step {
            lookup,
            found: 0,
            failed: None,
        });
    };

    let before = claims.len();
    let mut failed = None;
    for id in to_fetch(&matched) {
        match provider.fetch(&id, cancel) {
            Ok(release) => keep(claims, Claim::of_release(&release)),
            Err(Error::Canceled) => {
                failed = Some(Error::Canceled);
                break;
            }
            Err(error) => failed = Some(error),
        }
    }
    Some(Step {
        lookup,
        found: claims.len() - before,
        failed,
    })
}

/// The releases to fetch: every vinyl appearance first, then the rest.
///
/// Vinyl first because the user is holding a record. The rest at all because
/// AcoustID's release lists come from digital submissions and routinely omit the
/// pressing entirely - a CD release of the same album still names the artist and the
/// title, which is more than this lookup otherwise has to offer the record it exists
/// for.
fn to_fetch(matched: &[Match]) -> Vec<String> {
    let mut wanted: Vec<String> = Vec::new();
    for vinyl in [true, false] {
        for found in matched {
            for release in &found.recording.releases {
                if release.is_vinyl() == vinyl && !wanted.contains(&release.id) {
                    wanted.push(release.id.clone());
                }
            }
        }
    }
    wanted.truncate(FETCH_BUDGET);
    wanted
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use vcw_metadata::client::Stats;
    use vcw_metadata::fixtures::Recorded;
    use vcw_metadata::policy::{Limiter, TestClock};
    use vcw_metadata::provider::ALL_CRITERIA;
    use vcw_metadata::release::{Candidate as Found, Medium, Release, TrackEntry};
    use vcw_metadata::{Client, Criterion, Token};

    use super::*;
    use crate::resolver::Doubt;

    /// A provider whose answer is whatever the test fixed.
    #[derive(Debug)]
    struct Stub {
        id: ProviderId,
        found: Vec<Found>,
        release: Option<Release>,
        fails: bool,
        searches: AtomicUsize,
        fetches: AtomicUsize,
    }

    impl Stub {
        fn new(id: ProviderId) -> Self {
            Self {
                id,
                found: Vec::new(),
                release: None,
                fails: false,
                searches: AtomicUsize::new(0),
                fetches: AtomicUsize::new(0),
            }
        }

        fn finding(mut self, found: Vec<Found>) -> Self {
            self.found = found;
            self
        }

        fn holding(mut self, release: Release) -> Self {
            self.release = Some(release);
            self
        }

        const fn failing(mut self) -> Self {
            self.fails = true;
            self
        }
    }

    impl Provider for Stub {
        fn id(&self) -> ProviderId {
            self.id
        }

        fn search(&self, _query: &Query, _cancel: &Cancel) -> vcw_metadata::Result<Vec<Found>> {
            self.searches.fetch_add(1, Ordering::SeqCst);
            if self.fails {
                return Err(Error::Offline { provider: self.id });
            }
            Ok(self.found.clone())
        }

        fn fetch(&self, id: &str, _cancel: &Cancel) -> vcw_metadata::Result<Release> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            self.release
                .clone()
                .filter(|release| release.id == id)
                .ok_or_else(|| Error::Malformed {
                    provider: self.id,
                    detail: format!("no release {id} here"),
                })
        }

        fn understands(&self) -> &'static [Criterion] {
            ALL_CRITERIA
        }

        fn is_offline(&self) -> bool {
            false
        }

        fn stats(&self) -> Stats {
            Stats::default()
        }
    }

    fn pressing(id: &str) -> Found {
        Found {
            id: id.to_owned(),
            artist: "Autechre".into(),
            album: "Amber".into(),
            catalog: "WARP LP 25".into(),
            ..Found::default()
        }
    }

    /// Catalog, artist and title, all off the sleeve: enough to resolve outright.
    fn sleeve() -> Observed {
        let mut observed = Observed::new();
        observed
            .add(Source::Stated, Fact::Catalog("WARPLP25".into()))
            .add(Source::Stated, Fact::Artist("Autechre".into()))
            .add(Source::Stated, Fact::Album("Amber".into()));
        observed
    }

    fn chosen(outcome: &Outcome) -> &str {
        match outcome {
            Outcome::Resolved { chosen } => &chosen.assessed.claim.ids[0],
            other => panic!("expected a resolved release, got {other:?}"),
        }
    }

    #[test]
    fn a_catalog_number_costs_one_request() {
        let discogs = Stub::new(ProviderId::Discogs).finding(vec![pressing("d1")]);
        let walked = walk(&[&discogs], None, &sleeve(), &Cancel::new());

        assert_eq!(chosen(&walked.outcome), "d1");
        assert_eq!(walked.steps.len(), 1, "{:?}", walked.steps);
        assert_eq!(walked.steps[0].lookup, Lookup::CatalogAtDiscogs);
        assert_eq!(
            discogs.searches.load(Ordering::SeqCst),
            1,
            "the walk stops at the first answer good enough to state"
        );
    }

    #[test]
    fn a_lookup_with_no_provider_is_absent_rather_than_failed() {
        let walked = walk(&[], None, &sleeve(), &Cancel::new());

        assert!(
            walked.steps.is_empty(),
            "nothing was asked, so nothing is reported: {:?}",
            walked.steps
        );
        assert!(matches!(walked.outcome, Outcome::Nothing));
    }

    #[test]
    fn one_provider_failing_does_not_end_the_walk() {
        let discogs = Stub::new(ProviderId::Discogs).failing();
        let brainz = Stub::new(ProviderId::MusicBrainz).finding(vec![pressing("mb1")]);
        let walked = walk(&[&discogs, &brainz], None, &sleeve(), &Cancel::new());

        assert_eq!(chosen(&walked.outcome), "mb1");
        let made: Vec<Lookup> = walked.steps.iter().map(|step| step.lookup).collect();
        assert_eq!(
            made,
            vec![
                Lookup::CatalogAtDiscogs,
                Lookup::SearchDiscogs,
                Lookup::SearchMusicBrainz
            ],
            "both Discogs lookups were still tried, and both are reported as failed"
        );
        assert!(walked.steps[0].failed.is_some());
        assert!(walked.steps[2].failed.is_none());
    }

    #[test]
    fn the_same_pressing_returned_twice_is_held_once() {
        // Without the dedup this is the bug: the second Discogs lookup returns the
        // same release, it becomes its own runner-up at an identical score, and
        // `TooClose` turns an answer into a question.
        let discogs = Stub::new(ProviderId::Discogs).finding(vec![pressing("d1"), pressing("d2")]);
        let walked = walk(&[&discogs], None, &sleeve(), &Cancel::new());

        assert!(
            matches!(
                walked.outcome,
                Outcome::Ask {
                    because: Doubt::TooClose,
                    ..
                }
            ),
            "two real pressings tie, which is the shape the dedup must not hide: {:?}",
            walked.outcome
        );
        assert_eq!(walked.steps[1].lookup, Lookup::SearchDiscogs);
        assert_eq!(
            walked.steps[1].found, 0,
            "the second lookup returned the same two releases and added neither"
        );
    }

    #[test]
    fn the_audio_is_not_fingerprinted_when_the_name_was_enough() {
        let asked = AtomicBool::new(false);
        let fingerprint = || {
            asked.store(true, Ordering::SeqCst);
            Some(Fingerprint::new("AQABz0q3", 198))
        };
        let acoustid = AcoustId::new(Arc::new(Recorded::new()));
        let audio = Audio {
            acoustid: &acoustid,
            fingerprint: &fingerprint,
        };
        let discogs = Stub::new(ProviderId::Discogs).finding(vec![pressing("d1")]);

        let walked = walk(&[&discogs], Some(&audio), &sleeve(), &Cancel::new());

        assert_eq!(chosen(&walked.outcome), "d1");
        assert!(
            !asked.load(Ordering::SeqCst),
            "the expensive lookup is last in the order so that it is never reached"
        );
    }

    /// One AcoustID answer: one recording, on one record and one CD.
    const HEARD: &str = r#"{"status":"ok","results":[{"score":0.93,"recordings":[
        {"id":"rec-1","title":"Foil","duration":331.0,
         "artists":[{"name":"Autechre"}],
         "releases":[
           {"id":"cd-1","title":"Amber","mediums":[{"format":"CD","position":1}]},
           {"id":"lp-1","title":"Amber","mediums":[{"format":"12\" Vinyl","position":1}]}
         ]}]}]}"#;

    fn pressed() -> Release {
        Release {
            id: "lp-1".into(),
            album: "Amber".into(),
            album_artist: "Autechre".into(),
            catalog: "WARP LP 25".into(),
            media: vec![Medium {
                tracks: vec![TrackEntry::default()],
                ..Medium::default()
            }],
            ..Release::default()
        }
    }

    fn heard() -> AcoustId {
        let client = Client::new(
            ProviderId::AcoustId,
            Arc::new(Recorded::new().json(vcw_metadata::acoustid::API, HEARD)),
        )
        .with_clock(Arc::new(TestClock::new()))
        .with_limiter(Limiter::unlimited());
        AcoustId::new(Arc::new(Recorded::new()))
            .with_client(client)
            .with_key(Token::new("testkey123"))
    }

    #[test]
    fn a_recognized_recording_becomes_a_candidate_through_musicbrainz() {
        // The record nobody can name: no catalog number, nothing on the sleeve that
        // a text search found. This is the walk reaching its last resort and the
        // last resort working.
        let mut observed = Observed::new();
        observed.add(Source::Stated, Fact::Artist("Autechre".into()));

        let fingerprint = || Some(Fingerprint::new("AQABz0q3", 331));
        let acoustid = heard();
        let audio = Audio {
            acoustid: &acoustid,
            fingerprint: &fingerprint,
        };
        let brainz = Stub::new(ProviderId::MusicBrainz).holding(pressed());

        let walked = walk(&[&brainz], Some(&audio), &observed, &Cancel::new());

        let audio_step = walked
            .steps
            .iter()
            .find(|step| step.lookup == Lookup::IdentifyAudio)
            .expect("the audio lookup was reached");
        assert_eq!(audio_step.found, 1, "the vinyl release became a candidate");
        assert!(
            walked
                .observed
                .items()
                .iter()
                .any(|item| item.source == Source::AcoustId && item.fact.kind() == Kind::Recording),
            "what AcoustID heard is kept as evidence, not just as a candidate"
        );
        assert_eq!(
            brainz.fetches.load(Ordering::SeqCst),
            2,
            "the record first, then the CD, both inside the budget"
        );
    }

    #[test]
    fn the_record_is_fetched_before_the_cd() {
        let cd = |id: &str| vcw_metadata::release::RecordingRelease {
            id: id.to_owned(),
            format: "CD".into(),
            ..Default::default()
        };
        let lp = |id: &str| vcw_metadata::release::RecordingRelease {
            id: id.to_owned(),
            format: "12\" Vinyl".into(),
            ..Default::default()
        };
        let matched = vec![Match {
            score: 0.9,
            recording: vcw_metadata::release::Recording {
                releases: vec![cd("a"), lp("b"), cd("c"), lp("d")],
                ..Default::default()
            },
        }];

        assert_eq!(to_fetch(&matched), vec!["b", "d", "a", "c"]);
    }

    #[test]
    fn the_fetch_budget_caps_what_one_answer_can_cost() {
        let many: Vec<vcw_metadata::release::RecordingRelease> = (0..30)
            .map(|n| vcw_metadata::release::RecordingRelease {
                id: format!("r{n}"),
                format: "12\" Vinyl".into(),
                ..Default::default()
            })
            .collect();
        let matched = vec![Match {
            score: 0.9,
            recording: vcw_metadata::release::Recording {
                releases: many,
                ..Default::default()
            },
        }];

        assert_eq!(to_fetch(&matched).len(), FETCH_BUDGET);
    }
}
