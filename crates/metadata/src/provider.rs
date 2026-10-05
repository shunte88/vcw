/*
 *  provider.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What every metadata provider does, and nothing more (§28).
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

//! What every metadata provider does, and nothing more (§28).
//!
//! Two operations: search for candidates, fetch one in full. Search is deliberately
//! cheap and lossy - a list of [`Candidate`]s a person can scan - and fetch is the
//! expensive one that pulls a tracklist. That split is not an optimization, it is
//! how both providers are actually shaped, and it matches what the user does: look
//! at a shortlist, pick the right pressing, then commit.
//!
//! # Why `understands` exists
//!
//! §28 lists nine search criteria and no provider supports all nine. A UI that
//! offered a barcode field to MusicBrainz and silently dropped it would be lying;
//! [`Provider::understands`] lets it say "this provider ignores the barcode"
//! instead. [`crate::Query::ignored_by`] turns that into the list to complain about.
//!
//! # Errors are answers
//!
//! A provider returns [`crate::Error::Offline`] when networking is disabled and
//! that is a normal outcome, not a bug: §40's offline mode is the default, and the
//! only thing a caller does differently is show a different message.

use crate::error::Result;
use crate::query::{Criterion, Query};
use crate::release::{Candidate, ProviderId, Release};
use crate::{Cancel, Stats};

/// What a multi-provider search produced: the candidates, and who failed.
///
/// Named because the pair is the point - a search across providers returns both,
/// always, and a caller that dropped the failures would show a short list with no
/// explanation of why it is short.
pub type Sweep = (Vec<Candidate>, Vec<(ProviderId, crate::Error)>);

/// A source of release metadata.
///
/// `Send + Sync` because a provider is held once and used from whichever thread a
/// search lands on; it holds no mutable state of its own beyond counters.
pub trait Provider: std::fmt::Debug + Send + Sync {
    /// Which provider this is.
    fn id(&self) -> ProviderId;

    /// Candidates matching a query, best first as the provider ranks them.
    ///
    /// An empty list is a legitimate answer. [`crate::Error::NothingToSearch`] is
    /// the answer to a query with no criteria the provider understands, because
    /// asking a provider for everything it has is not a search.
    fn search(&self, query: &Query, cancel: &Cancel) -> Result<Vec<Candidate>>;

    /// One release in full, by its provider-specific identifier.
    ///
    /// The identifier is whatever [`Candidate::id`](crate::Candidate) carried, which
    /// is an MBID for MusicBrainz and a numeric release id for Discogs. Passing one
    /// provider's identifier to the other is a caller error and will read as a
    /// missing release.
    fn fetch(&self, id: &str, cancel: &Cancel) -> Result<Release>;

    /// The criteria this provider can actually act on.
    fn understands(&self) -> &'static [Criterion];

    /// Whether this provider can reach anything, for graying out a button.
    fn is_offline(&self) -> bool;

    /// What the provider's client has done, for diagnostics (§42).
    fn stats(&self) -> Stats;
}

/// Every criterion in §28, for a provider that really does understand them all.
pub const ALL_CRITERIA: &[Criterion] = &[
    Criterion::Artist,
    Criterion::Album,
    Criterion::Catalog,
    Criterion::Barcode,
    Criterion::Label,
    Criterion::Year,
    Criterion::Country,
    Criterion::ReleaseId,
    Criterion::Fingerprint,
];

/// Searches several providers and concatenates what they say, in order.
///
/// Deliberately not a merge. Two providers describing the same pressing do not
/// agree on much beyond the artist, and a machine that guessed they were the same
/// row would be making the one decision the user is best placed to make. Grouping
/// belongs in the UI, where the user can see both and choose.
///
/// A provider that fails does not fail the search: its error is returned alongside
/// the candidates so a caller can show "MusicBrainz: timed out" under a list that
/// still has the Discogs results in it. A search where *every* provider failed
/// returns the first error, because a list of nothing plus a footnote is not an
/// answer.
pub fn search_all(providers: &[&dyn Provider], query: &Query, cancel: &Cancel) -> Result<Sweep> {
    let mut candidates = Vec::new();
    let mut failures = Vec::new();
    for provider in providers {
        cancel.check()?;
        match provider.search(query, cancel) {
            Ok(found) => candidates.extend(found),
            Err(crate::Error::Canceled) => return Err(crate::Error::Canceled),
            Err(error) => failures.push((provider.id(), error)),
        }
    }
    if candidates.is_empty() && !failures.is_empty() {
        // Every provider failed, so there is no list to footnote. The first
        // failure is the one to report: it is the one the user waited on.
        return Err(failures.swap_remove(0).1);
    }
    Ok((candidates, failures))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    /// A provider whose answer is whatever the test says it is.
    ///
    /// The answer is a closure rather than a stored `Result` because
    /// [`Error`] is not `Clone`: it can carry an [`std::io::Error`], and a
    /// filesystem error is not something you copy.
    struct Stub {
        id: ProviderId,
        answer: Box<dyn Fn() -> Result<Vec<Candidate>> + Send + Sync>,
    }

    impl std::fmt::Debug for Stub {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Stub({})", self.id.as_str())
        }
    }

    fn candidate(id: &str) -> Candidate {
        Candidate {
            id: id.to_string(),
            artist: "Autechre".into(),
            album: "Amber".into(),
            ..Candidate::default()
        }
    }

    impl Provider for Stub {
        fn id(&self) -> ProviderId {
            self.id
        }

        fn search(&self, _query: &Query, _cancel: &Cancel) -> Result<Vec<Candidate>> {
            (self.answer)()
        }

        fn fetch(&self, _id: &str, _cancel: &Cancel) -> Result<Release> {
            unimplemented!("not exercised")
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

    fn ok(id: ProviderId, ids: &[&str]) -> Stub {
        let found: Vec<Candidate> = ids.iter().map(|i| candidate(i)).collect();
        Stub {
            id,
            answer: Box::new(move || Ok(found.clone())),
        }
    }

    fn failing(id: ProviderId, error: fn() -> Error) -> Stub {
        Stub {
            id,
            answer: Box::new(move || Err(error())),
        }
    }

    #[test]
    fn candidates_arrive_provider_by_provider_in_order() {
        let discogs = ok(ProviderId::Discogs, &["1", "2"]);
        let brainz = ok(ProviderId::MusicBrainz, &["mbid"]);
        let (found, failures) = search_all(
            &[&discogs, &brainz],
            &Query::new().artist("Autechre"),
            &Cancel::new(),
        )
        .expect("a search");
        assert_eq!(
            found.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["1", "2", "mbid"],
            "concatenated, not merged"
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn one_provider_failing_does_not_lose_the_others_results() {
        let discogs = failing(ProviderId::Discogs, || Error::MissingCredential {
            provider: ProviderId::Discogs,
            variable: "VCW_DISCOGS_TOKEN",
        });
        let brainz = ok(ProviderId::MusicBrainz, &["mbid"]);
        let (found, failures) = search_all(
            &[&discogs, &brainz],
            &Query::new().artist("Autechre"),
            &Cancel::new(),
        )
        .expect("a search");
        assert_eq!(found.len(), 1);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, ProviderId::Discogs);
    }

    #[test]
    fn every_provider_failing_is_a_failed_search() {
        let discogs = failing(ProviderId::Discogs, || Error::Offline {
            provider: ProviderId::Discogs,
        });
        let brainz = failing(ProviderId::MusicBrainz, || Error::Offline {
            provider: ProviderId::MusicBrainz,
        });
        let error = search_all(
            &[&discogs, &brainz],
            &Query::new().artist("Autechre"),
            &Cancel::new(),
        )
        .expect_err("no answer at all");
        assert!(matches!(error, Error::Offline { .. }), "{error:?}");
    }

    #[test]
    fn cancellation_stops_the_sweep_rather_than_being_collected() {
        let discogs = ok(ProviderId::Discogs, &["1"]);
        let cancel = Cancel::new();
        cancel.cancel();
        assert!(matches!(
            search_all(&[&discogs], &Query::new().artist("x"), &cancel),
            Err(Error::Canceled)
        ));
    }

    #[test]
    fn all_nine_criteria_are_listed_once() {
        assert_eq!(ALL_CRITERIA.len(), 9);
        let mut seen = ALL_CRITERIA.to_vec();
        seen.sort_by_key(|c| c.as_str());
        seen.dedup();
        assert_eq!(seen.len(), 9, "no duplicates");
    }
}
