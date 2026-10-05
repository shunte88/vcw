/*
 *  live_providers.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Ignored by default: the tests that hold the fixtures to the real services.
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

// Every test here does real HTTP, so the whole file is the agent's: without
// the `net` feature there is no transport that could reach a service, and a
// build that cannot is exactly what §40 asks for.
#![cfg(feature = "net")]

//! Ignored by default: the tests that hold the fixtures to the real services.
//!
//! Run with `cargo test -p vcw-metadata -- --ignored`. Nothing in CI runs these
//! and nothing should: a test that needs a third-party service to be up is not a
//! test of VCW. What they are for is the thing fixtures cannot do, which is notice
//! that a provider changed its mind.
//!
//! The `format:` finding is the case in point. `format:vinyl` used to look
//! obviously correct and returns nothing, and only a live query said so. A fixture
//! captured from a wrong query is a wrong answer that passes forever.
//!
//! MusicBrainz needs no credential, so these run for anyone. The Discogs ones read
//! `VCW_DISCOGS_TOKEN` from the environment and skip themselves without it, which
//! is §39's rule: there is nowhere else a token may come from.
//!
//! They are polite. One request a second through the real limiter, a real user
//! agent naming VCW and its homepage, and no more requests than the assertion
//! needs.

use std::sync::Arc;
use std::time::Duration;

use vcw_metadata::credentials::{Credentials, Token};
use vcw_metadata::net::user_agent;
use vcw_metadata::policy::Limiter;
use vcw_metadata::query::Fingerprint;
use vcw_metadata::{
    AcoustId, Agent, Cancel, Client, Discogs, MusicBrainz, Provider, ProviderId, Query, Side,
};

const AMBER_1994: &str = "bd5b1270-7468-47f0-9c9a-928199f9e4ad";

/// A client that will really talk to a provider, at the published rate.
fn live(provider: ProviderId) -> Client {
    let credentials = Credentials::from_env();
    Client::new(provider, Arc::new(Agent::new()))
        .with_limiter(Limiter::every(Duration::from_secs(1)))
        .with_user_agent(user_agent(credentials.contact()))
}

/// The token, or `None`, in which case the caller skips itself.
fn discogs_token() -> Option<Token> {
    Credentials::from_env().discogs().cloned()
}

#[test]
#[ignore = "talks to musicbrainz.org"]
fn musicbrainz_still_answers_the_captured_search() {
    let provider =
        MusicBrainz::new(Arc::new(Agent::new())).with_client(live(ProviderId::MusicBrainz));
    let found = provider
        .search(
            &Query::new().artist("Autechre").album("Amber").limit(25),
            &Cancel::new(),
        )
        .expect("a search");
    assert!(
        found.iter().any(|c| c.id == AMBER_1994),
        "the 1994 pressing is what the fixture was captured from; got {:?}",
        found.iter().map(|c| c.summary()).collect::<Vec<_>>()
    );
    assert!(
        found.iter().any(|c| c.catalog == "WARPLP25"),
        "and the catalog number that identifies it"
    );
}

#[test]
#[ignore = "talks to musicbrainz.org"]
fn the_vinyl_format_filter_still_has_to_name_exact_format_names() {
    // The finding this file exists for. `format:vinyl` is valid Lucene, returns a
    // 200, and matches nothing, because MusicBrainz stores the medium format as a
    // name and matches it exactly. If this ever starts returning results, VCW's
    // four-way disjunction is no longer necessary - and until then, dropping it
    // would silently empty every search.
    let client = live(ProviderId::MusicBrainz);
    let cancel = Cancel::new();
    let count = |query: &str| -> u64 {
        let url = format!(
            "https://musicbrainz.org/ws/2/release?query={}&limit=1&fmt=json",
            vcw_metadata::net::encode(query)
        );
        let body = client.body(&url, &[], &cancel).expect("a search");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        value["count"].as_u64().expect("a count")
    };

    assert_eq!(
        count(r#"artist:"Autechre" AND release:"Amber" AND format:"vinyl""#),
        0,
        "lowercase vinyl matches no medium format name"
    );
    assert!(
        count(r#"artist:"Autechre" AND release:"Amber" AND format:"12\" Vinyl""#) > 0,
        "the exact name does"
    );
}

#[test]
#[ignore = "talks to musicbrainz.org"]
fn the_release_fixture_still_describes_the_record_it_describes() {
    let provider =
        MusicBrainz::new(Arc::new(Agent::new())).with_client(live(ProviderId::MusicBrainz));
    let release = provider
        .fetch(AMBER_1994, &Cancel::new())
        .expect("a release");
    assert_eq!(release.album, "Amber");
    assert_eq!(release.album_artist, "Autechre");
    assert_eq!(release.catalog, "WARPLP25");
    assert_eq!(release.media.len(), 2, "a 2xLP");
    assert_eq!(
        release
            .sides()
            .iter()
            .map(|s| s.letter())
            .collect::<Vec<_>>(),
        ['A', 'B', 'C', 'D']
    );
    assert!(!release.side_tracks(Side::A).is_empty());
    assert!(
        !release.genres.is_empty(),
        "either the release or its group has genres; if both are empty the \
         release-group include has stopped working"
    );
}

#[test]
#[ignore = "talks to musicbrainz.org"]
fn a_release_that_does_not_exist_reads_as_missing_and_not_as_a_hang() {
    let provider =
        MusicBrainz::new(Arc::new(Agent::new())).with_client(live(ProviderId::MusicBrainz));
    let error = provider
        .fetch("00000000-0000-0000-0000-000000000000", &Cancel::new())
        .expect_err("no such release");
    assert!(
        error.to_string().contains("404") || error.to_string().contains("400"),
        "{error}"
    );
}

#[test]
#[ignore = "talks to coverartarchive.org"]
fn the_cover_art_archive_url_convention_still_holds() {
    // MusicBrainz returns no image URLs, so VCW synthesises one. This is the only
    // thing that can tell us the convention changed.
    let provider =
        MusicBrainz::new(Arc::new(Agent::new())).with_client(live(ProviderId::MusicBrainz));
    let cancel = Cancel::new();
    let release = provider.fetch(AMBER_1994, &cancel).expect("a release");
    let front = release
        .artwork
        .iter()
        .find(|reference| reference.primary)
        .expect("the fixture says a front cover exists");
    let artwork = vcw_metadata::artwork::fetch(&live(ProviderId::MusicBrainz), front, &cancel)
        .expect("a cover");
    assert!(
        artwork.len() > 1_024,
        "{} bytes is not a cover",
        artwork.len()
    );
}

#[test]
#[ignore = "talks to api.discogs.com and needs VCW_DISCOGS_TOKEN"]
fn discogs_still_answers_a_catalog_number_search() {
    let Some(token) = discogs_token() else {
        eprintln!("skipped: VCW_DISCOGS_TOKEN is not set");
        return;
    };
    let provider = Discogs::new(Arc::new(Agent::new()))
        .with_client(live(ProviderId::Discogs))
        .with_token(Some(token));
    let found = provider
        .search(&Query::new().catalog("WARPLP25").limit(25), &Cancel::new())
        .expect("a search");
    assert!(!found.is_empty(), "a catalog number should find a pressing");
    assert!(
        found
            .iter()
            .all(|c| c.format.to_lowercase().contains("vinyl")),
        "the vinyl-only default is a search parameter, not a filter applied later: {:?}",
        found.iter().map(|c| c.format.clone()).collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "talks to api.discogs.com and needs VCW_DISCOGS_TOKEN"]
fn a_discogs_release_still_parses_into_sides() {
    let Some(token) = discogs_token() else {
        eprintln!("skipped: VCW_DISCOGS_TOKEN is not set");
        return;
    };
    let provider = Discogs::new(Arc::new(Agent::new()))
        .with_client(live(ProviderId::Discogs))
        .with_token(Some(token));
    let cancel = Cancel::new();
    let found = provider
        .search(&Query::new().catalog("WARPLP25").limit(5), &cancel)
        .expect("a search");
    let release = provider.fetch(&found[0].id, &cancel).expect("a release");
    assert!(!release.media.is_empty());
    assert!(release.has_vinyl());
    assert!(
        !release.tracks().is_empty(),
        "a Discogs release fetch is the only way to get a tracklist"
    );
    assert!(
        release.tracks().iter().any(|t| t.resolved.is_some()),
        "and at least some positions should read as sides: {:?}",
        release
            .tracks()
            .iter()
            .map(|t| t.position.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "talks to api.discogs.com"]
fn discogs_without_a_token_is_refused_rather_than_answered() {
    // Worth knowing, because if Discogs ever allows anonymous search the token
    // stops being mandatory and the provider should say so.
    let provider = Discogs::new(Arc::new(Agent::new()))
        .with_client(live(ProviderId::Discogs))
        .with_token(Token::new("obviously-not-a-token"));
    let error = provider
        .search(&Query::new().artist("Autechre"), &Cancel::new())
        .expect_err("refused");
    assert!(
        error.to_string().contains("rejected") || error.to_string().contains("401"),
        "{error}"
    );
}

/// The AcoustID key, or `None`, in which case the caller skips itself.
fn acoustid_key() -> Option<Token> {
    Credentials::from_env().acoustid().cloned()
}

/// A real fingerprint of 400 s of a record, 9,856 characters of base64.
const LONG_FINGERPRINT: &str = include_str!("fixtures/acoustid_fingerprint_400s.txt");

/// The AcoustID track the recording fixture was captured from.
const SILVERSIDE_TRACK: &str = "71f9f5bd-3798-4f8c-8604-0c809478c82f";

#[test]
#[ignore = "talks to api.acoustid.org and needs VCW_ACOUSTID_KEY"]
fn acoustid_still_reads_only_a_space_as_the_meta_separator() {
    // The `format:vinyl` of this provider, and worse, because it fails *upward*:
    // the request is accepted, the status is `ok`, the match is there and every
    // field of metadata is missing. AcoustID's documentation writes the meta list
    // with `+`, which is a space in a query string and a literal plus in a form
    // body - and this is a form body, because a fingerprint does not fit in a URL.
    //
    // Measured 2026-10-04: `%2B` 0 recordings, `%2C` 0 recordings, `%20` 1
    // recording with 13 releases. If a separator that used to return nothing ever
    // starts working, VCW's spelling is merely unnecessary; if the space ever
    // stops, every lookup goes quietly blank.
    let Some(key) = acoustid_key() else {
        eprintln!("skipped: VCW_ACOUSTID_KEY is not set");
        return;
    };
    let client = live(ProviderId::AcoustId);
    let cancel = Cancel::new();
    let recordings = |separator: &str| -> usize {
        let form = format!(
            "client={}&meta=recordings{separator}releases{separator}tracks&trackid={SILVERSIDE_TRACK}",
            vcw_metadata::net::encode(key.expose())
        );
        let body = client
            .post_form(vcw_metadata::acoustid::API, &form, &[], &cancel)
            .expect("a lookup");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["status"], "ok", "and it answers `ok` either way");
        value["results"][0]["recordings"]
            .as_array()
            .map_or(0, Vec::len)
    };

    assert_eq!(recordings("%20"), 1, "a space is the separator that works");
    assert_eq!(
        recordings("%2B"),
        0,
        "a literal plus still reads as one unknown name, silently"
    );
    assert_eq!(recordings("%2C"), 0, "and so does a comma");
}

#[test]
#[ignore = "talks to api.acoustid.org and needs VCW_ACOUSTID_KEY"]
fn acoustid_accepts_a_fingerprint_that_would_not_fit_in_a_url() {
    // The measurement that made `Transport` grow a body, asserted against the
    // service rather than against a server's documented limit: 9,856 characters of
    // fingerprint plus the other parameters, which no request line would carry.
    let Some(key) = acoustid_key() else {
        eprintln!("skipped: VCW_ACOUSTID_KEY is not set");
        return;
    };
    let provider = AcoustId::new(Arc::new(Agent::new()))
        .with_client(live(ProviderId::AcoustId))
        .with_key(Some(key));
    let found = provider
        .lookup(
            &Fingerprint::new(LONG_FINGERPRINT.trim(), 400),
            &Cancel::new(),
        )
        .expect("the service accepts it, whether or not it knows the record");
    // No assertion on the contents: this is a vinyl transfer, and AcoustID's index
    // is submitted from digital releases. An empty answer is the expected one and
    // is not what this test is about.
    eprintln!("{} match(es) for a 400 s side", found.len());
}

#[test]
#[ignore = "talks to api.acoustid.org"]
fn acoustid_without_a_key_is_refused_rather_than_answered() {
    let provider = AcoustId::new(Arc::new(Agent::new()))
        .with_client(live(ProviderId::AcoustId))
        .with_key(Token::new("obviously-not-a-key"));
    let error = provider
        .lookup(
            &Fingerprint::new(LONG_FINGERPRINT.trim(), 400),
            &Cancel::new(),
        )
        .expect_err("refused");
    assert!(
        matches!(error, vcw_metadata::Error::Rejected { .. }),
        "a bad key is a rejection and not a fingerprint problem: {error}"
    );
}

#[test]
#[ignore = "talks to musicbrainz.org"]
fn the_recording_fixture_still_describes_the_recording_it_describes() {
    let provider =
        MusicBrainz::new(Arc::new(Agent::new())).with_client(live(ProviderId::MusicBrainz));
    let found = provider
        .recording("91fa0fd1-60d9-4536-8316-2e4489812fa4", &Cancel::new())
        .expect("a recording");
    assert_eq!(found.title, "Silverside");
    assert_eq!(found.artist, "Autechre");
    assert_eq!(
        found.seconds().map(|s| s.round()),
        Some(331.0),
        "the length the fixture was captured with"
    );
    assert!(
        found.releases.iter().filter(|r| r.is_vinyl()).count() >= 2,
        "two of its pressings are records, and new ones only get added: {:?}",
        found
            .releases
            .iter()
            .map(|r| format!("{} {}", r.title, r.format))
            .collect::<Vec<_>>()
    );
}
