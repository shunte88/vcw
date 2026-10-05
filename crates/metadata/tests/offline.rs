/*
 *  offline.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-12's exit criterion: the application is fully usable with networking disabled (§40).
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

//! WP-12's exit criterion: the application is fully usable with networking
//! disabled (§40).
//!
//! Two halves, and both are asserted here rather than asserted in prose:
//!
//! 1. **Fixture-backed offline tests.** Every provider path in this crate runs
//!    through [`Recorded`], so nothing in the test suite depends on a service
//!    being up, on a credential existing, or on a rate limit.
//! 2. **Fully usable with networking disabled.** With [`Offline`] as the
//!    transport, every provider call returns a message rather than hanging,
//!    failing an assertion or panicking - and the whole of the non-provider
//!    surface (queries, genres, positions, releases, artwork validation) keeps
//!    working, because none of it ever needed a network.
//!
//! The third leg of the promise is not a test but a build:
//! `cargo check -p vcw-metadata --no-default-features` compiles without the HTTP
//! agent existing at all. It is a gate leg.

use std::sync::Arc;
use std::time::Duration;

use vcw_metadata::AcoustId;
use vcw_metadata::cache::Memory;
use vcw_metadata::fixtures::Recorded;
use vcw_metadata::net::{Offline, Response};
use vcw_metadata::policy::{Limiter, TestClock};
use vcw_metadata::query::Fingerprint;
use vcw_metadata::{
    Artwork, Cancel, Client, Criterion, Discogs, Error, Genres, MusicBrainz, Provider, ProviderId,
    Query, Token, Transport, search_all,
};

const SEARCH: &str = include_str!("fixtures/musicbrainz_search_amber_vinyl.json");
const RELEASE: &str = include_str!("fixtures/musicbrainz_release_amber_1994.json");
const AMBER_1994: &str = "bd5b1270-7468-47f0-9c9a-928199f9e4ad";

/// A client with no network, no real clock and no rate limit to wait for.
fn client(provider: ProviderId, transport: Arc<dyn Transport>) -> Client {
    Client::new(provider, transport)
        .with_clock(Arc::new(TestClock::new()))
        .with_limiter(Limiter::unlimited())
}

#[test]
fn with_networking_disabled_every_provider_answers_with_a_message() {
    let cancel = Cancel::new();
    let query = Query::new().artist("Autechre").album("Amber");

    let discogs = Discogs::new(Arc::new(Offline)).with_token(Token::new("sekrit"));
    let brainz = MusicBrainz::new(Arc::new(Offline));

    for provider in [&discogs as &dyn Provider, &brainz] {
        assert!(provider.is_offline());
        let error = provider
            .search(&query, &cancel)
            .expect_err("offline is an answer");
        assert!(
            matches!(error, Error::Offline { .. }),
            "{:?} gave {error:?}",
            provider.id()
        );
        // A message a person can read, naming the provider and no internals.
        let message = error.to_string();
        assert!(message.contains("networking is disabled"), "{message}");
        assert!(message.contains(provider.id().display_name()), "{message}");
        assert!(
            !error.is_transient(),
            "not worth retrying: the transport will refuse again until a person              switches networking on"
        );

        let error = provider
            .fetch(AMBER_1994, &cancel)
            .expect_err("also offline");
        assert!(matches!(error, Error::Offline { .. }), "{error:?}");
    }
}

#[test]
fn with_networking_disabled_a_fingerprint_lookup_answers_with_a_message_too() {
    // Not covered by the loop above, because AcoustID is not a `Provider`: it
    // takes audio evidence rather than words. The promise is the same one, and a
    // lookup that hung or panicked here would break §40 just as thoroughly.
    let provider = AcoustId::new(Arc::new(Offline)).with_key(Token::new("sekrit"));
    assert!(provider.is_offline());
    let error = provider
        .lookup(&Fingerprint::new("AQABz0q3", 198), &Cancel::new())
        .expect_err("offline is an answer");
    assert!(matches!(error, Error::Offline { .. }), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("networking is disabled"), "{message}");
    assert!(message.contains("AcoustID"), "{message}");
    assert!(!error.is_transient());
}

#[test]
fn an_offline_sweep_across_both_providers_reports_both_failures_and_does_not_hang() {
    let discogs = Discogs::new(Arc::new(Offline));
    let brainz = MusicBrainz::new(Arc::new(Offline));
    let error = search_all(
        &[&discogs, &brainz],
        &Query::new().artist("Autechre"),
        &Cancel::new(),
    )
    .expect_err("nothing to show");
    assert!(matches!(error, Error::Offline { .. }), "{error:?}");
}

#[test]
fn one_provider_offline_does_not_stop_the_other() {
    // The realistic case: MusicBrainz needs no token so it works, Discogs has
    // none configured so it does not, and the user still gets results.
    let brainz = MusicBrainz::new(Arc::new(Offline)).with_client(client(
        ProviderId::MusicBrainz,
        Arc::new(Recorded::new().json_matching("/release?query=", SEARCH)),
    ));
    let discogs = Discogs::new(Arc::new(Offline));

    let (candidates, failures) = search_all(
        &[&discogs, &brainz],
        &Query::new().artist("Autechre").album("Amber"),
        &Cancel::new(),
    )
    .expect("a search");
    assert_eq!(candidates.len(), 2, "both pressings");
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, ProviderId::Discogs);
    assert!(
        failures[0].1.to_string().contains("networking is disabled"),
        "and the UI can say why the list is short: {}",
        failures[0].1
    );
}

#[test]
fn the_whole_search_to_release_path_runs_from_fixtures() {
    let transport = Arc::new(
        Recorded::new()
            .json_matching("/release?query=", SEARCH)
            .json_matching(format!("/release/{AMBER_1994}"), RELEASE),
    );
    let provider = MusicBrainz::new(Arc::new(Offline))
        .with_client(client(ProviderId::MusicBrainz, transport.clone()));
    let cancel = Cancel::new();

    // Search, choose the original pressing by its catalog number, fetch it.
    let candidates = provider
        .search(&Query::new().artist("Autechre").album("Amber"), &cancel)
        .expect("a search");
    let chosen = candidates
        .iter()
        .find(|candidate| candidate.catalog == "WARPLP25")
        .expect("the 1994 pressing");
    let release = provider.fetch(&chosen.id, &cancel).expect("a release");

    assert_eq!(release.album, "Amber");
    assert_eq!(release.sides().len(), 4);
    assert_eq!(release.tracks().len(), 11);
    assert!(release.has_vinyl());
    assert_eq!(transport.calls(), 2, "one search, one fetch, nothing else");
    assert_eq!(
        provider.stats().requests,
        2,
        "and the client counted the same two"
    );
}

#[test]
fn a_second_look_at_the_same_release_costs_no_request() {
    let transport =
        Arc::new(Recorded::new().json_matching(format!("/release/{AMBER_1994}"), RELEASE));
    let clock = Arc::new(TestClock::new());
    let provider = MusicBrainz::new(Arc::new(Offline)).with_client(
        Client::new(ProviderId::MusicBrainz, transport.clone())
            .with_clock(clock.clone())
            .with_cache(Arc::new(Memory::new(clock)))
            .with_limiter(Limiter::unlimited()),
    );
    let cancel = Cancel::new();
    let first = provider.fetch(AMBER_1994, &cancel).expect("a release");
    let second = provider
        .fetch(AMBER_1994, &cancel)
        .expect("the same release");
    assert_eq!(first, second);
    assert_eq!(transport.calls(), 1);
    assert_eq!(provider.stats().cache_hits, 1);
}

#[test]
fn a_disk_cache_survives_the_process_that_wrote_it() {
    // §40's cache is not a memoisation of one session: a user who searched for a
    // record yesterday should not re-ask the provider today.
    let dir = tempfile::tempdir().expect("a tempdir");
    let transport =
        Arc::new(Recorded::new().json_matching(format!("/release/{AMBER_1994}"), RELEASE));
    let cancel = Cancel::new();

    let fetch_once = || {
        let cache = Arc::new(vcw_metadata::Disk::with_ttl(
            dir.path(),
            Duration::from_secs(3_600),
        ));
        let provider = MusicBrainz::new(Arc::new(Offline)).with_client(
            Client::new(ProviderId::MusicBrainz, transport.clone())
                .with_clock(Arc::new(TestClock::new()))
                .with_cache(cache)
                .with_limiter(Limiter::unlimited()),
        );
        let release = provider.fetch(AMBER_1994, &cancel).expect("a release");
        (release, provider.stats())
    };

    let (first, cold) = fetch_once();
    let (second, warm) = fetch_once();
    assert_eq!(first, second);
    assert_eq!((cold.requests, cold.cache_hits), (1, 0));
    assert_eq!(
        (warm.requests, warm.cache_hits),
        (0, 1),
        "a different client, the same answer, no request"
    );
    assert_eq!(transport.calls(), 1);
}

#[test]
fn a_user_who_closes_the_dialog_stops_the_search() {
    let transport = Arc::new(Recorded::new().json_matching("/release?query=", SEARCH));
    let provider = MusicBrainz::new(Arc::new(Offline))
        .with_client(client(ProviderId::MusicBrainz, transport.clone()));
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(matches!(
        provider.search(&Query::new().artist("Autechre"), &cancel),
        Err(Error::Canceled)
    ));
    assert_eq!(transport.calls(), 0);
}

#[test]
fn everything_that_is_not_a_provider_works_with_no_transport_at_all() {
    // The point of §40: the parts of metadata handling that do not involve a
    // service keep working when there is no service. A user editing genres or
    // reading a tracklist off a sleeve is not blocked by being offline.
    let genres = Genres::builtin();
    assert_eq!(
        genres.normalize("HH; Mn"),
        ["Hip-Hop", "Hip Hop", "Minimal"]
    );

    let query = Query::new().artist("Autechre").catalog("WARPLP25");
    assert_eq!(query.criteria(), [Criterion::Artist, Criterion::Catalog]);
    assert!(query.vinyl_only, "and vinyl is still the default");
    assert_eq!(query.free_text(), "Autechre WARPLP25");

    let positions = vcw_metadata::positions::split_numeric(4, 2);
    assert_eq!(
        positions.iter().map(|p| p.alpha()).collect::<Vec<_>>(),
        ["C1", "C2", "D1", "D2"]
    );

    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
    jpeg.resize(1_024, 0);
    let artwork = Artwork::accept("file:///cover.jpg", true, jpeg).expect("an image");
    assert_eq!(artwork.file_name("front"), "front.jpg");
}

#[test]
fn a_provider_says_what_it_ignored_rather_than_dropping_it() {
    // A user who typed a barcode and got results deserves to know MusicBrainz
    // never looked at it. Silently answering a different question is the failure
    // mode this exists to prevent.
    let brainz = MusicBrainz::new(Arc::new(Offline));
    let discogs = Discogs::new(Arc::new(Offline));
    let query = Query::new().artist("Autechre").barcode("5021603025011");

    assert_eq!(query.ignored_by(brainz.understands()), [Criterion::Barcode]);
    assert!(
        query.ignored_by(discogs.understands()).is_empty(),
        "Discogs does index barcodes, which is a reason to prefer it here"
    );
}

#[test]
fn no_url_anywhere_in_a_discogs_exchange_carries_the_token() {
    // §39, as a property of the traffic rather than a claim in a comment. The URL
    // is the cache key, the log line and the thing pasted into a bug report.
    let transport = Arc::new(
        Recorded::new()
            .json_matching(
                "/database/search",
                r#"{"results":[{"id":1,"title":"a - b"}]}"#,
            )
            .json_matching("/releases/", r#"{"title":"b","formats":[],"tracklist":[]}"#),
    );
    let provider = Discogs::new(Arc::new(Offline))
        .with_client(client(ProviderId::Discogs, transport.clone()))
        .with_token(Token::new("a-real-looking-token"));
    let cancel = Cancel::new();
    provider
        .search(&Query::new().artist("Autechre"), &cancel)
        .expect("a search");
    provider.fetch("1", &cancel).expect("a release");

    for request in transport.requests() {
        assert!(
            !request.url.contains("a-real-looking-token"),
            "credential in a url: {}",
            request.url
        );
        assert!(
            !format!("{request:?}").contains("a-real-looking-token"),
            "credential in a debug print: {request:?}"
        );
        assert_eq!(
            request.header("authorization"),
            Some("Discogs token=a-real-looking-token"),
            "it did go out, in the header"
        );
    }
}

#[test]
fn a_provider_outage_reads_as_an_outage_and_not_as_an_empty_record() {
    let transport = Arc::new(Recorded::new().answering(
        MusicBrainz::release_url(AMBER_1994),
        Response::status(503, "Service Unavailable"),
    ));
    let provider = MusicBrainz::new(Arc::new(Offline))
        .with_client(client(ProviderId::MusicBrainz, transport.clone()));
    let error = provider
        .fetch(AMBER_1994, &Cancel::new())
        .expect_err("no release");
    match error {
        Error::Http { status, .. } => assert_eq!(status, 503),
        other => panic!("wrong error: {other:?}"),
    }
    assert_eq!(transport.calls(), 3, "retried, because a 503 passes");
}
