/*
 *  acoustid.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The AcoustID provider: a fingerprint goes out, recordings come back (§26, §27).
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

//! The AcoustID provider: a fingerprint goes out, recordings come back (§26, §27).
//!
//! This is the one provider that is not asked a question in words. WP-21 computes a
//! chromaprint over a region of captured audio; this module hands that to AcoustID
//! and gets back the recordings it resembles, each with a score. What *release* the
//! user is holding is a different question, and deliberately not answered here -
//! see [`crate::Recording`] for why the two are separate types.
//!
//! # It has to be a POST, and that is measured
//!
//! A real vinyl side fingerprints to about 28 base64 characters per second of
//! audio. Measured on a ten-minute side out of `/data2/source_rips`:
//!
//! | audio  | fingerprint |
//! |--------|-------------|
//! | 120 s  | 3,303 chars |
//! | 300 s  | 8,476 chars |
//! | 600 s  | 16,884 chars|
//!
//! The common default limit on an HTTP request line is 8 KB, so a five-minute track
//! is already at it and a ten-minute one is twice over. A GET would work on the
//! short tracks and fail on the long ones, which is the worst shape a bug can have.
//! So every lookup is a form POST - see [`crate::net::Request::post_form`] - and the
//! fingerprint travels in the body.
//!
//! # The credential travels in the body too
//!
//! AcoustID's `client` parameter is the API key, and in a POST it is a form field.
//! That is better than a query string, not worse: §39 keeps credentials out of
//! files, and a body is not logged, not cached under a key and not printed by
//! [`Request`](crate::net::Request)'s `Debug`. It is also why a lookup is
//! **uncached** - see [`Client::post_form`].
//!
//! # One request per track, and that is the cost
//!
//! AcoustID asks for one request a second, which [`crate::client::default_limiter`]
//! already applies. The thing that matters for a twelve-track side is that the
//! lookup asks for everything in one go: `meta=recordings+releases+tracks` returns
//! each recording's title, artist, length *and* the releases it appears on with the
//! position it occupies on each. VRipr asked for `meta=recordings` and then made a
//! MusicBrainz request per recording - up to fifteen per track, at a request a
//! second - which is why identifying a side took minutes. The same information
//! arrives here in one response.
//!
//! [`crate::MusicBrainz::recording`] still exists for the case this cannot cover: a
//! match whose metadata AcoustID does not hold, where all that comes back is an
//! MBID.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::client::{Cancel, Client, Stats};
use crate::credentials::{ACOUSTID_KEY_VAR, Token};
use crate::error::{Error, Result};
use crate::net::{Transport, encode};
use crate::query::Fingerprint;
use crate::release::{ProviderId, Recording, RecordingRelease};

/// The lookup endpoint.
pub const API: &str = "https://api.acoustid.org/v2/lookup";

/// What a lookup asks to be included with each match.
///
/// All three are load-bearing: `recordings` is the title, artist and length,
/// `releases` is which pressings the recording appears on - §26's constraint - and
/// `tracks` is the position it occupies on each of them, which is what makes a
/// second match evidence about the first.
///
/// **Space separated, and that is not cosmetic.** AcoustID's documentation writes
/// this list with `+`, which is correct in a query string where `+` *means* a
/// space. In a form body a literal plus has to be sent as `%2B`, and AcoustID then
/// reads `recordings%2Breleases%2Btracks` as one unknown name and returns
/// `{"status": "ok"}` with the matches present and **every piece of metadata
/// missing** - no title, no artist, no releases. Measured against the live service
/// on 2026-10-04: `%2B` gave 0 recordings, a comma gave 0 recordings, and both
/// `+` (a space) and `%20` gave 1 recording with 13 releases. So the value is
/// spelled with spaces and goes through [`encode`], which produces `%20`.
///
/// This is the worst shape a bug can have - a successful response that is quietly
/// empty - so the form builder has a test that fails if the separator regresses.
pub const META: &str = "recordings releases tracks";

/// The error code AcoustID uses for a key it will not accept.
///
/// From the service's own list. It is called out because it is the one code that
/// means the user has something to fix rather than VCW having asked badly.
const INVALID_KEY: u64 = 4;

/// One recording AcoustID thinks the audio is, and how sure it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    /// The service's own confidence, 0.0 to 1.0.
    ///
    /// Evidence, not a verdict: §26 is explicit that identification weighs several
    /// matches together, and a 0.9 on one track of a side that agrees with nothing
    /// else is worth less than three 0.6s that agree.
    pub score: f32,
    /// What it matched.
    pub recording: Recording,
}

/// The AcoustID provider.
///
/// Not a [`Provider`](crate::Provider): that trait searches with words and returns
/// releases, and this service does neither. The shape it shares with the other two
/// is the [`Client`] - the same limiter, retry policy, counters and offline rule.
#[derive(Debug)]
pub struct AcoustId {
    client: Client,
    key: Option<Token>,
}

impl AcoustId {
    /// A provider over a transport, with no key.
    ///
    /// Useful only for the offline path and for tests: without a key every lookup
    /// reports [`Error::MissingCredential`].
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            client: Client::new(ProviderId::AcoustId, transport),
            key: None,
        }
    }

    /// Supplies the API key.
    #[must_use]
    pub fn with_key(mut self, key: Option<Token>) -> Self {
        self.key = key;
        self
    }

    /// Replaces the request client, for a caller that wants its own cache or clock.
    #[must_use]
    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    /// The request client, for a caller assembling its own.
    #[must_use]
    pub const fn client(&self) -> &Client {
        &self.client
    }

    /// Whether a key is configured.
    #[must_use]
    pub const fn has_key(&self) -> bool {
        self.key.is_some()
    }

    /// Whether this provider can reach anything, for graying out a button.
    #[must_use]
    pub fn is_offline(&self) -> bool {
        self.client.is_offline()
    }

    /// What the provider's client has done, for diagnostics (§42).
    #[must_use]
    pub fn stats(&self) -> Stats {
        self.client.stats()
    }

    /// The recordings a fingerprint resembles, best score first.
    ///
    /// An empty list is a legitimate answer and the common one for a record nobody
    /// has submitted: a private pressing, a flexi, a test cut. That is not an
    /// error, and §26 says so - identification is evidence, and "no evidence" is a
    /// state the resolver understands.
    pub fn lookup(&self, fingerprint: &Fingerprint, cancel: &Cancel) -> Result<Vec<Match>> {
        if fingerprint.code.is_empty() {
            return Err(Error::NothingToSearch {
                provider: ProviderId::AcoustId,
            });
        }
        let form = self.form(fingerprint)?;
        let body = self
            .client
            .post_form(API, &form, &[], cancel)
            .map_err(refined)?;
        let value = parse(&body)?;
        check(&value)?;
        Ok(matches(&value))
    }

    /// The form body for a lookup, or the error explaining why there is none.
    ///
    /// Built fresh per request rather than stored, so the credential lives in one
    /// short-lived string and not in a field something might print.
    ///
    /// Offline is checked before the key, the same order and for the same reason as
    /// [`crate::Discogs`]: a user with networking switched off is told about the
    /// networking, because that is the one of the two that would change the
    /// outcome, and a build that cannot reach the network has no business reading a
    /// credential at all.
    fn form(&self, fingerprint: &Fingerprint) -> Result<String> {
        if self.client.is_offline() {
            return Err(Error::Offline {
                provider: ProviderId::AcoustId,
            });
        }
        let key = self.key.as_ref().ok_or(Error::MissingCredential {
            provider: ProviderId::AcoustId,
            variable: ACOUSTID_KEY_VAR,
        })?;
        Ok(format!(
            "client={}&meta={}&duration={}&fingerprint={}",
            encode(key.expose()),
            encode(META),
            fingerprint.seconds,
            encode(&fingerprint.code)
        ))
    }
}

/// AcoustID's own explanation of an error status, where it sent one.
///
/// It does, and the useful half is in the body: a key it will not accept comes back
/// as `400 {"error": {"code": 4, "message": "invalid API key"}}`, not as a 401 and
/// not as the 200 refusal [`check`] handles. Without this a user is told
/// `AcoustID returned HTTP 400` and left to guess which of the three things they
/// typed was wrong.
///
/// Only [`Error::Http`] is reconsidered, and only when its message really is the
/// service's JSON - [`crate::net::Response::preview`] caps it at 200 characters, so
/// a longer body fails to parse and the HTTP error stands, which is the right
/// outcome for anything that is not one of these.
fn refined(error: Error) -> Error {
    let Error::Http { message, .. } = &error else {
        return error;
    };
    match serde_json::from_str::<Value>(message) {
        Ok(value) => check(&value).err().unwrap_or(error),
        Err(_) => error,
    }
}

/// Reads the response as JSON, or says it could not.
fn parse(body: &[u8]) -> Result<Value> {
    serde_json::from_slice(body).map_err(|error| Error::Malformed {
        provider: ProviderId::AcoustId,
        detail: error.to_string(),
    })
}

/// Whether the service actually answered the question.
///
/// AcoustID refuses inside a 200: an unreadable fingerprint, a missing parameter or
/// an exhausted quota all arrive as `{"status": "error"}` with a message, which the
/// [`Client`] cannot see because the status code was fine. A caller that skipped
/// this would read every one of those as "no match", and tell the user their record
/// is unknown when the truth is that VCW asked badly.
fn check(value: &Value) -> Result<()> {
    let status = value["status"].as_str().ok_or_else(|| Error::Malformed {
        provider: ProviderId::AcoustId,
        detail: "the response carried no status".into(),
    })?;
    if status == "ok" {
        return Ok(());
    }
    let message = value["error"]["message"]
        .as_str()
        .unwrap_or(status)
        .to_string();
    if value["error"]["code"].as_u64() == Some(INVALID_KEY) {
        return Err(Error::Rejected {
            provider: ProviderId::AcoustId,
        });
    }
    Err(Error::Refused {
        provider: ProviderId::AcoustId,
        message,
    })
}

/// Every recording in the response, best score first and each one once.
///
/// A recording can appear under more than one result - two fingerprint clusters of
/// the same performance - and the highest score wins. VRipr sorted and then called
/// `dedup_by` on the recording id, which only removes *neighbors*: after a sort by
/// score the duplicates are nowhere near each other, so they survived and the same
/// recording was offered twice.
fn matches(value: &Value) -> Vec<Match> {
    let mut out: Vec<Match> = Vec::new();
    for result in value["results"].as_array().into_iter().flatten() {
        let score = result["score"].as_f64().unwrap_or_default() as f32;
        for found in result["recordings"].as_array().into_iter().flatten() {
            let recording = recording(found);
            if recording.id.is_empty() {
                continue;
            }
            match out.iter_mut().find(|m| m.recording.id == recording.id) {
                Some(seen) => seen.score = seen.score.max(score),
                None => out.push(Match { score, recording }),
            }
        }
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// One recording out of a lookup response.
fn recording(value: &Value) -> Recording {
    Recording {
        id: text(&value["id"]),
        title: text(&value["title"]),
        artist: artists(&value["artists"]),
        // Seconds, and fractional: AcoustID answers `331.066`, so `as_u64` reads
        // every duration it has ever sent as absent.
        duration: value["duration"]
            .as_f64()
            .filter(|seconds| *seconds > 0.0)
            .map(Duration::from_secs_f64),
        releases: releases(&value["releases"]),
    }
}

/// The artist credit, joined the way the service presents it.
///
/// `joinphrase` is honored where the service gives one, so a collaboration reads
/// `Autechre & Hafler Trio` rather than `Autechre, Hafler Trio`. A missing phrase on
/// anything but the last credit falls back to `, `, which is what AcoustID's own
/// display does.
fn artists(value: &Value) -> String {
    let credits: Vec<&Value> = value.as_array().into_iter().flatten().collect();
    let mut out = String::new();
    for (index, credit) in credits.iter().enumerate() {
        out.push_str(&text(&credit["name"]));
        if index + 1 < credits.len() {
            let phrase = credit["joinphrase"].as_str().unwrap_or("");
            out.push_str(if phrase.is_empty() { ", " } else { phrase });
        }
    }
    out
}

/// The releases a recording appears on, with where it sits on each.
///
/// The first medium only. A recording that appears twice on one release - the single
/// edit and the album version, or a track repeated across two discs of a
/// compilation - is a real thing and rare enough that the first position is the
/// honest simplification: the alternative is a position list nothing downstream
/// reads. The counts are `None` where the service said nothing, because a missing
/// position and the first position are different claims.
fn releases(value: &Value) -> Vec<RecordingRelease> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|release| {
            let medium = release["mediums"].as_array().and_then(|m| m.first());
            RecordingRelease {
                id: text(&release["id"]),
                title: text(&release["title"]),
                medium: medium.and_then(|m| number(&m["position"])),
                track_count: medium
                    .and_then(|m| number(&m["track_count"]))
                    .or_else(|| number(&release["track_count"])),
                position: medium
                    .and_then(|m| m["tracks"].as_array())
                    .and_then(|t| t.first())
                    .and_then(|t| number(&t["position"])),
                format: medium.map(|m| text(&m["format"])).unwrap_or_default(),
            }
        })
        .collect()
}

/// A JSON string, or an empty one.
fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

/// A JSON number as a `u32`, or `None` if it was absent or absurd.
fn number(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::Recorded;
    use crate::net::{Offline, Response};
    use crate::policy::{Limiter, TestClock};

    const SILVERSIDE: &str = include_str!("../tests/fixtures/acoustid_lookup_silverside.json");
    const NO_MATCH: &str = include_str!("../tests/fixtures/acoustid_lookup_no_match.json");

    /// A real fingerprint of 400 s of a record, 9,856 characters of base64.
    ///
    /// Here so the premise of this whole module can be asserted rather than
    /// described: see [`a_real_fingerprint_does_not_fit_in_a_request_line`].
    const LONG: &str = include_str!("../tests/fixtures/acoustid_fingerprint_400s.txt");

    /// A provider whose client has no clock to wait on and no limiter to wait for.
    fn provider(transport: Arc<dyn Transport>) -> AcoustId {
        let client = Client::new(ProviderId::AcoustId, transport)
            .with_clock(Arc::new(TestClock::new()))
            .with_limiter(Limiter::unlimited());
        AcoustId::new(Arc::new(Offline))
            .with_client(client)
            .with_key(Token::new("testkey123"))
    }

    fn fingerprint() -> Fingerprint {
        Fingerprint::new("AQABz0q3", 198)
    }

    /// The silent failure this module exists to avoid.
    ///
    /// A `+` percent-encoded into the form body as `%2B` is what AcoustID's own
    /// documentation reads as if it were a query string, and against the live
    /// service it returns `{"status": "ok"}` with the match present and every field
    /// of metadata missing. Nothing in a response says that happened, so the only
    /// place it can be caught is here. Measured 2026-10-04: `%2B` 0 recordings,
    /// `,` 0 recordings, `%20` 1 recording with 13 releases.
    #[test]
    fn the_meta_separator_survives_form_encoding() {
        // Not the offline transport: `form` reports offline before it builds
        // anything, which is the behavior the next test down asserts.
        let form = provider(Arc::new(Recorded::new()))
            .form(&fingerprint())
            .expect("a key and a transport that can reach something");
        assert!(
            form.contains("meta=recordings%20releases%20tracks"),
            "the separator has to decode to a space: {form}"
        );
        assert!(
            !form.contains("%2B") && !form.contains("%2C"),
            "a literal plus or comma reads as one unknown name: {form}"
        );
    }

    #[test]
    fn the_form_carries_the_key_the_duration_and_the_fingerprint() {
        let form = provider(Arc::new(Recorded::new()))
            .form(&fingerprint())
            .expect("a form");
        assert!(form.contains("client=testkey123"), "{form}");
        assert!(form.contains("duration=198"), "{form}");
        assert!(form.contains("fingerprint=AQABz0q3"), "{form}");
    }

    #[test]
    fn a_lookup_posts_the_fingerprint_rather_than_putting_it_in_the_url() {
        // The whole reason `Transport` grew a body: a five-minute track is 8.5 KB
        // of base64 and a ten-minute one 16.9 KB, both past the request-line limit
        // a default HTTP server enforces.
        let recorded = Arc::new(Recorded::new().json(API, SILVERSIDE));
        let found = provider(recorded.clone())
            .lookup(&fingerprint(), &Cancel::new())
            .expect("the recorded answer");

        let request = recorded.requests().pop().expect("one request");
        assert_eq!(request.url, API, "no query string at all");
        let body = String::from_utf8(request.body.expect("a POST body")).expect("utf-8");
        assert!(body.contains("fingerprint=AQABz0q3"), "{body}");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn a_match_carries_the_recording_its_length_and_where_it_was_pressed() {
        let found = provider(Arc::new(Recorded::new().json(API, SILVERSIDE)))
            .lookup(&fingerprint(), &Cancel::new())
            .expect("the recorded answer");

        let best = &found[0];
        assert_eq!(best.score, 1.0);
        assert_eq!(best.recording.title, "Silverside");
        assert_eq!(best.recording.artist, "Autechre");
        // 331.066 in the body, and a float: `as_u64` reads it as absent.
        assert_eq!(best.recording.seconds(), Some(331.066));
        assert_eq!(best.recording.releases.len(), 13);

        let records: Vec<&RecordingRelease> = best
            .recording
            .releases
            .iter()
            .filter(|r| r.is_vinyl())
            .collect();
        assert_eq!(records.len(), 2, "two of the thirteen are records");
        assert_eq!(records[0].title, "Amber");
        assert_eq!(records[0].position, Some(3), "A3 is the third track");
        assert_eq!(records[0].medium, Some(1), "one disc");
    }

    #[test]
    fn no_match_is_an_answer_and_not_an_error() {
        // What every fingerprint taken off a record actually gets: AcoustID's index
        // is submitted from digital releases, and a vinyl transfer is a different
        // master at a slightly different speed. Recorded from the live service with
        // a real 198 s fingerprint off `boc.vcw`.
        let found = provider(Arc::new(Recorded::new().json(API, NO_MATCH)))
            .lookup(&fingerprint(), &Cancel::new())
            .expect("an empty list, not a failure");
        assert!(found.is_empty());
    }

    #[test]
    fn a_refusal_inside_a_200_is_not_read_as_no_match() {
        // The one wrong answer: telling the user their record is unknown when the
        // truth is that VCW asked badly.
        let body = br#"{"status":"error","error":{"code":3,"message":"invalid fingerprint"}}"#;
        let transport = Recorded::new().answering(API, Response::ok(body.to_vec()));
        let error = provider(Arc::new(transport))
            .lookup(&fingerprint(), &Cancel::new())
            .expect_err("a refusal");
        assert!(
            matches!(&error, Error::Refused { message, .. } if message == "invalid fingerprint"),
            "{error:?}"
        );
    }

    #[test]
    fn a_rejected_key_in_a_400_says_so_rather_than_quoting_the_status() {
        // What the live service actually does, measured 2026-10-04. The status is
        // 400, the body is where the reason is, and `AcoustID returned HTTP 400`
        // tells a user nothing about the key they need to fix.
        let body = br#"{"error": {"code": 4, "message": "invalid API key"}, "status": "error"}"#;
        let transport = Recorded::new().answering(API, Response::status(400, body.to_vec()));
        let error = provider(Arc::new(transport))
            .lookup(&fingerprint(), &Cancel::new())
            .expect_err("a rejection");
        assert!(matches!(error, Error::Rejected { .. }), "{error:?}");
        assert!(!error.is_transient(), "asking again will not help");
    }

    #[test]
    fn an_error_status_that_is_not_acoustids_own_stays_an_http_error() {
        // A proxy's HTML page, a gateway's plain text, a truncated body: none of
        // them are the service talking, and inventing a reason from them would be
        // worse than quoting the status.
        let transport =
            Recorded::new().answering(API, Response::status(502, b"<html>bad gateway".to_vec()));
        let error = provider(Arc::new(transport))
            .lookup(&fingerprint(), &Cancel::new())
            .expect_err("a gateway failure");
        assert!(
            matches!(error, Error::Http { status: 502, .. }),
            "{error:?}"
        );
        assert!(error.is_transient(), "a 502 is worth asking again about");
    }

    #[test]
    fn a_rejected_key_says_so_rather_than_blaming_the_fingerprint() {
        let body = br#"{"status":"error","error":{"code":4,"message":"invalid API key"}}"#;
        let transport = Recorded::new().answering(API, Response::ok(body.to_vec()));
        let error = provider(Arc::new(transport))
            .lookup(&fingerprint(), &Cancel::new())
            .expect_err("a rejection");
        assert!(matches!(error, Error::Rejected { .. }), "{error:?}");
        assert!(!error.is_transient(), "asking again will not help");
    }

    #[test]
    fn the_same_recording_under_two_results_is_offered_once_at_its_best_score() {
        // VRipr sorted by score and then called `dedup_by` on the recording id,
        // which only removes neighbors: after the sort the duplicates are nowhere
        // near each other, so the same recording was offered twice.
        let body = br#"{"status":"ok","results":[
            {"score":0.4,"recordings":[{"id":"aaa","title":"One"}]},
            {"score":0.9,"recordings":[{"id":"aaa","title":"One"}]},
            {"score":0.6,"recordings":[{"id":"bbb","title":"Two"}]}]}"#;
        let transport = Recorded::new().answering(API, Response::ok(body.to_vec()));
        let found = provider(Arc::new(transport))
            .lookup(&fingerprint(), &Cancel::new())
            .expect("three results, two recordings");

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].recording.id, "aaa");
        assert_eq!(found[0].score, 0.9, "the better of the two scores");
        assert_eq!(found[1].recording.id, "bbb");
    }

    #[test]
    fn offline_is_reported_before_a_missing_key() {
        // A user with networking switched off is told about the networking: it is
        // the one of the two that would change the outcome.
        let provider = AcoustId::new(Arc::new(Offline));
        assert!(provider.is_offline());
        assert!(!provider.has_key());
        assert!(matches!(
            provider.lookup(&fingerprint(), &Cancel::new()),
            Err(Error::Offline { .. })
        ));
    }

    #[test]
    fn a_lookup_with_no_key_names_the_variable_that_would_supply_one() {
        let client = Client::new(ProviderId::AcoustId, Arc::new(Recorded::new()))
            .with_clock(Arc::new(TestClock::new()))
            .with_limiter(Limiter::unlimited());
        let provider = AcoustId::new(Arc::new(Offline)).with_client(client);
        let error = provider
            .lookup(&fingerprint(), &Cancel::new())
            .expect_err("no key");
        assert!(
            matches!(error, Error::MissingCredential { variable, .. } if variable == ACOUSTID_KEY_VAR),
            "{error:?}"
        );
    }

    #[test]
    fn an_empty_fingerprint_is_refused_before_a_request_is_made() {
        let recorded = Arc::new(Recorded::new());
        let error = provider(recorded.clone())
            .lookup(&Fingerprint::new("", 198), &Cancel::new())
            .expect_err("nothing to look up");
        assert!(matches!(error, Error::NothingToSearch { .. }), "{error:?}");
        assert_eq!(recorded.calls(), 0, "and no request was made");
    }

    #[test]
    fn a_response_that_is_not_a_lookup_is_malformed_rather_than_empty() {
        let transport = Recorded::new().answering(API, Response::ok(b"<html>502</html>".to_vec()));
        let error = provider(Arc::new(transport))
            .lookup(&fingerprint(), &Cancel::new())
            .expect_err("not JSON");
        assert!(matches!(error, Error::Malformed { .. }), "{error:?}");
    }

    /// Why [`Request::post_form`](crate::net::Request::post_form) had to exist.
    ///
    /// 8 KB is the default limit on a request line in nginx, Apache and most
    /// everything else, and it counts the whole line - method, URL and version. A
    /// 400-second side is 9,856 characters of fingerprint before any other
    /// parameter, so the GET this module would otherwise have sent is not long,
    /// it is rejected.
    #[test]
    fn a_real_fingerprint_does_not_fit_in_a_request_line() {
        let code = LONG.trim();
        assert_eq!(
            code.len(),
            9_856,
            "the fixture is the one that was measured"
        );
        let form = provider(Arc::new(Recorded::new()))
            .form(&Fingerprint::new(code, 400))
            .expect("a form");
        assert!(
            form.len() > 8_192,
            "the form is {} bytes, which would have fitted",
            form.len()
        );
    }

    #[test]
    fn a_fingerprint_does_not_print_itself() {
        // Not a credential, but derived from a recording, and a `Debug` is one
        // `tracing` call from a log file.
        let shown = format!("{:?}", fingerprint());
        assert!(!shown.contains("AQABz0q3"), "{shown}");
    }
}
