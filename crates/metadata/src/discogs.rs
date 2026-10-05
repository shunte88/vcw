/*
 *  discogs.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The Discogs provider: the best vinyl pressing data there is (§28).
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

//! The Discogs provider: the best vinyl pressing data there is (§28).
//!
//! Discogs is the reason VCW has a metadata crate at all. It is the only database
//! that reliably distinguishes *pressings* - the 1994 Warp original from the 2016
//! repress, same tracklist, different catalog number - and pressing is exactly
//! what a person holding a record needs to identify.
//!
//! # A token is not optional
//!
//! Discogs requires authentication for search, so a build with no
//! `VCW_DISCOGS_TOKEN` gets [`Error::MissingCredential`] naming the variable and
//! the application carries on without this provider. §39 forbids storing the token
//! in a project file, so the environment is the only place it can come from.
//!
//! The token goes in an `Authorization` header. VRipr put it in the query string
//! as `&token=...`, which meant every cached URL, every log line and every URL
//! pasted into a bug report carried a live credential. The URL is the cache key
//! here, so that was not a small thing.
//!
//! # What the API gives and what it withholds
//!
//! Search returns a compact result with `catno`, `country`, `year` and a `format`
//! array, which is enough to choose between pressings and is why
//! [`Candidate`] has those fields. It does *not* return a tracklist, so choosing a
//! candidate costs a second request. That is the shape of the API, not a decision
//! made here.
//!
//! Genres arrive in two arrays: `genres` (broad) and `styles` (specific). Both are
//! used, joined and run through [`Genres`], because `Electronic` alone is not a
//! useful thing to write on a record and `Dub Techno` alone loses the browsing
//! category.

use std::sync::Arc;

use crate::client::{Cancel, Client, Stats};
use crate::credentials::{DISCOGS_TOKEN_VAR, Token};
use crate::error::{Error, Result};
use crate::genres::Genres;
use crate::net::{Header, Transport, encode};
use crate::positions::{self, Reading};
use crate::provider::Provider;
use crate::query::{Criterion, Query};
use crate::release::{ArtworkRef, Candidate, Medium, ProviderId, Release, TrackEntry};

/// The API root.
pub const API: &str = "https://api.discogs.com";

/// Where a person reads a release, as opposed to where a program does.
pub const WEB: &str = "https://www.discogs.com/release";

/// The criteria Discogs can act on.
///
/// No fingerprint: Discogs has no audio identification, which is what AcoustID is
/// for. Everything else maps onto a documented search parameter.
pub const UNDERSTANDS: &[Criterion] = &[
    Criterion::Artist,
    Criterion::Album,
    Criterion::Catalog,
    Criterion::Barcode,
    Criterion::Label,
    Criterion::Year,
    Criterion::Country,
    Criterion::ReleaseId,
];

/// The Discogs provider.
#[derive(Debug)]
pub struct Discogs {
    client: Client,
    token: Option<Token>,
    genres: Genres,
}

impl Discogs {
    /// A provider over a transport, with no token.
    ///
    /// Useful only for the offline path and for tests: without a token every
    /// search reports [`Error::MissingCredential`].
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            client: Client::new(ProviderId::Discogs, transport),
            token: None,
            genres: Genres::builtin(),
        }
    }

    /// Supplies the token.
    #[must_use]
    pub fn with_token(mut self, token: Option<Token>) -> Self {
        self.token = token;
        self
    }

    /// Replaces the genre mapping table (§32).
    #[must_use]
    pub fn with_genres(mut self, genres: Genres) -> Self {
        self.genres = genres;
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

    /// Whether a token is configured.
    #[must_use]
    pub const fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// The authorization header, or the error explaining its absence.
    ///
    /// Built fresh per request rather than stored, so the credential lives in one
    /// short-lived string and not in a field something might print.
    ///
    /// Offline is checked first, and the order matters. A user with networking
    /// switched off and no token configured is told about the networking, because
    /// that is the one of the two that would actually change the outcome - and
    /// because a build that cannot reach the network has no business reading a
    /// credential at all.
    fn authorization(&self) -> Result<Vec<Header>> {
        if self.client.is_offline() {
            return Err(Error::Offline {
                provider: ProviderId::Discogs,
            });
        }
        let token = self.token.as_ref().ok_or(Error::MissingCredential {
            provider: ProviderId::Discogs,
            variable: DISCOGS_TOKEN_VAR,
        })?;
        Ok(vec![Header::new(
            "Authorization",
            format!("Discogs token={}", token.expose()),
        )])
    }

    /// The search URL for a query.
    ///
    /// Structured parameters where Discogs has one - `artist`, `release_title`,
    /// `catno`, `barcode`, `label`, `year`, `country`, `format` - because a
    /// catalog number in a structured field finds the pressing and the same
    /// string in `q=` finds whatever else mentions it.
    #[must_use]
    pub fn search_url(&self, query: &Query) -> String {
        let mut url = format!("{API}/database/search?type=release");
        let mut add = |name: &str, value: &str| {
            url.push('&');
            url.push_str(name);
            url.push('=');
            url.push_str(&encode(value));
        };
        if let Some(artist) = query.artist.as_deref() {
            add("artist", artist);
        }
        if let Some(album) = query.album.as_deref() {
            add("release_title", album);
        }
        if let Some(catalog) = query.catalog.as_deref() {
            add("catno", catalog);
        }
        if let Some(barcode) = query.barcode.as_deref() {
            add("barcode", barcode);
        }
        if let Some(label) = query.label.as_deref() {
            add("label", label);
        }
        if let Some(year) = query.year {
            add("year", &year.to_string());
        }
        if let Some(country) = query.country.as_deref() {
            add("country", country);
        }
        // §28's vinyl-only default is the point of the application, so it is a
        // search parameter rather than something filtered out after the fact -
        // twenty-five CD pressings would otherwise fill a page of twenty-five.
        if query.vinyl_only {
            add("format", "Vinyl");
        }
        add("per_page", &query.limit.to_string());
        url
    }

    /// The release URL for an identifier.
    #[must_use]
    pub fn release_url(id: &str) -> String {
        format!("{API}/releases/{}", encode(id))
    }

    /// Turns one search result into a candidate.
    fn candidate(value: &serde_json::Value) -> Option<Candidate> {
        let id = match &value["id"] {
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::String(text) => text.clone(),
            _ => return None,
        };
        // Search results give one `title` of the form "Artist - Album". Splitting
        // on the first " - " is what the API leaves us: there is no separate
        // artist field on a search hit, and a release fetch is the only way to get
        // one reliably. A title with no separator is taken as the album.
        let title = value["title"].as_str().unwrap_or("").trim();
        let (artist, album) = match title.split_once(" - ") {
            Some((artist, album)) => (positions::clean_artist(artist), album.trim().to_string()),
            None => (String::new(), title.to_string()),
        };
        Some(Candidate {
            id: id.clone(),
            artist,
            album,
            year: year_of(&value["year"]),
            label: first_string(&value["label"]),
            catalog: value["catno"].as_str().unwrap_or("").trim().to_string(),
            country: value["country"].as_str().unwrap_or("").trim().to_string(),
            format: strings(&value["format"]).join(", "),
            tracks: None,
            url: Some(format!("{WEB}/{id}")),
        })
    }

    /// Turns a release body into a release.
    fn release(&self, id: &str, value: &serde_json::Value) -> Release {
        let label = value["labels"].as_array().and_then(|l| l.first());
        Release {
            id: id.to_string(),
            album: value["title"].as_str().unwrap_or("").trim().to_string(),
            album_artist: value["artists"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|a| a["name"].as_str())
                .map(positions::clean_artist)
                .unwrap_or_default(),
            year: year_of(&value["year"]),
            genres: self.genres.normalize_all(
                strings(&value["genres"])
                    .into_iter()
                    .chain(strings(&value["styles"])),
            ),
            label: label
                .and_then(|l| l["name"].as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            catalog: label
                .and_then(|l| l["catno"].as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            country: value["country"].as_str().unwrap_or("").trim().to_string(),
            barcode: barcode_of(&value["identifiers"]),
            musicbrainz_id: None,
            discogs_id: Some(id.to_string()),
            artwork: artwork_of(&value["images"]),
            media: media_of(&value["formats"], &value["tracklist"]),
        }
    }
}

impl Provider for Discogs {
    fn id(&self) -> ProviderId {
        ProviderId::Discogs
    }

    fn search(&self, query: &Query, cancel: &Cancel) -> Result<Vec<Candidate>> {
        if query.is_empty() {
            return Err(Error::NothingToSearch {
                provider: ProviderId::Discogs,
            });
        }
        if query.ignored_by(UNDERSTANDS).len() == query.criteria().len() {
            // Every criterion given is one Discogs cannot act on, so a search
            // would return the whole database. That is not a search.
            return Err(Error::NothingToSearch {
                provider: ProviderId::Discogs,
            });
        }
        let headers = self.authorization()?;
        let body = self
            .client
            .body(&self.search_url(query), &headers, cancel)?;
        let value = parse(&body)?;
        let results = value["results"].as_array().ok_or(Error::Malformed {
            provider: ProviderId::Discogs,
            detail: "no results array".into(),
        })?;
        Ok(results
            .iter()
            .filter_map(Self::candidate)
            .take(query.limit)
            .collect())
    }

    fn fetch(&self, id: &str, cancel: &Cancel) -> Result<Release> {
        if id.trim().is_empty() {
            return Err(Error::NothingToSearch {
                provider: ProviderId::Discogs,
            });
        }
        let headers = self.authorization()?;
        let body = self.client.body(&Self::release_url(id), &headers, cancel)?;
        Ok(self.release(id, &parse(&body)?))
    }

    fn understands(&self) -> &'static [Criterion] {
        UNDERSTANDS
    }

    fn is_offline(&self) -> bool {
        self.client.is_offline()
    }

    fn stats(&self) -> Stats {
        self.client.stats()
    }
}

/// Parses a body, reporting a provider error rather than a serde one.
fn parse(body: &[u8]) -> Result<serde_json::Value> {
    serde_json::from_slice(body).map_err(|error| Error::Malformed {
        provider: ProviderId::Discogs,
        detail: error.to_string(),
    })
}

/// The strings in a JSON array, or nothing.
fn strings(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|item| item.as_str())
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The first string in an array, or an empty string.
fn first_string(value: &serde_json::Value) -> String {
    strings(value).into_iter().next().unwrap_or_default()
}

/// A year from either a number or a string, rejecting the impossible.
///
/// Discogs writes the year as a number on a release and as a string on a search
/// hit, and sometimes as `0` for a release nobody dated. A zero year is not a year.
fn year_of(value: &serde_json::Value) -> Option<u32> {
    let year = match value {
        serde_json::Value::Number(number) => u32::try_from(number.as_u64()?).ok()?,
        serde_json::Value::String(text) => text.trim().get(..4)?.parse().ok()?,
        _ => return None,
    };
    (year > 0).then_some(year)
}

/// The barcode from the `identifiers` array.
fn barcode_of(value: &serde_json::Value) -> Option<String> {
    value.as_array()?.iter().find_map(|item| {
        (item["type"].as_str()?.eq_ignore_ascii_case("barcode"))
            .then(|| item["value"].as_str().map(|v| v.trim().to_string()))
            .flatten()
            .filter(|barcode| !barcode.is_empty())
    })
}

/// Artwork references, the primary image first.
fn artwork_of(value: &serde_json::Value) -> Vec<ArtworkRef> {
    let Some(images) = value.as_array() else {
        return Vec::new();
    };
    let mut artwork: Vec<ArtworkRef> = images
        .iter()
        .filter_map(|image| {
            let url = image["uri"]
                .as_str()
                .or_else(|| image["resource_url"].as_str())?;
            if url.trim().is_empty() {
                return None;
            }
            Some(ArtworkRef {
                url: url.trim().to_string(),
                primary: image["type"].as_str() == Some("primary"),
                width: image["width"].as_u64().and_then(|w| u32::try_from(w).ok()),
                height: image["height"].as_u64().and_then(|h| u32::try_from(h).ok()),
            })
        })
        .collect();
    // Stable, so the order of equally primary images is the provider's own.
    artwork.sort_by_key(|reference| !reference.primary);
    artwork
}

/// Splits a Discogs tracklist across the media the `formats` array describes.
///
/// Discogs gives one flat tracklist and a separate description of the carriers, so
/// the medium a track belongs to has to be read off its own position letter. That
/// is the only signal there is: `C1` is on disc two of a 2xLP because C is the
/// third side, not because anything in the JSON said so.
fn media_of(formats: &serde_json::Value, tracklist: &serde_json::Value) -> Vec<Medium> {
    let format = formats
        .as_array()
        .and_then(|f| f.first())
        .map(|first| {
            let name = first["name"].as_str().unwrap_or("").trim();
            let descriptions = strings(&first["descriptions"]);
            if descriptions.is_empty() {
                name.to_string()
            } else {
                format!("{name}, {}", descriptions.join(", "))
            }
        })
        .unwrap_or_default();
    let count: u32 = formats
        .as_array()
        .and_then(|f| f.first())
        .and_then(|first| first["qty"].as_str())
        .and_then(|qty| qty.trim().parse().ok())
        .unwrap_or(1);

    let entries = tracks_of(tracklist);
    let mut media: Vec<Medium> = (1..=count.max(1))
        .map(|position| Medium {
            position,
            format: format.clone(),
            tracks: Vec::new(),
        })
        .collect();

    for entry in entries {
        // A resolved side names its own disc; an unresolved one goes on the first,
        // which is where a tracklist nobody lettered belongs.
        let disc = entry.resolved.map_or(1, |position| position.side.disc());
        let index = usize::try_from(disc.saturating_sub(1)).unwrap_or(0);
        match media.get_mut(index) {
            Some(medium) => medium.tracks.push(entry),
            None => {
                // The tracklist names more sides than `qty` claimed discs. Believe
                // the tracklist: it came off the label.
                while media.len() <= index {
                    let position = u32::try_from(media.len() + 1).unwrap_or(1);
                    media.push(Medium {
                        position,
                        format: format.clone(),
                        tracks: Vec::new(),
                    });
                }
                media[index].tracks.push(entry);
            }
        }
    }
    media.retain(|medium| !medium.tracks.is_empty());
    media
}

/// Reads a tracklist, resolving positions and inferring a numeric split.
///
/// Headings are skipped: Discogs uses them for a side title or a suite name, and a
/// heading has no position and no duration. Tracks are returned in the provider's
/// order, which is the order they play.
fn tracks_of(tracklist: &serde_json::Value) -> Vec<TrackEntry> {
    let Some(items) = tracklist.as_array() else {
        return Vec::new();
    };
    let mut entries: Vec<TrackEntry> = Vec::new();
    let mut numeric: Vec<usize> = Vec::new();

    for item in items {
        if item["type_"].as_str().unwrap_or("track") == "heading" {
            continue;
        }
        let title = item["title"].as_str().unwrap_or("").trim();
        if title.is_empty() {
            continue;
        }
        let position = item["position"].as_str().unwrap_or("").trim();
        let reading = positions::read(position, 1);
        if matches!(reading, Reading::Numeric(_)) {
            numeric.push(entries.len());
        }
        entries.push(TrackEntry {
            position: position.to_string(),
            resolved: match reading {
                Reading::Exact(resolved) => Some(resolved),
                _ => None,
            },
            title: title.to_string(),
            artist: item["artists"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|a| a["name"].as_str())
                .map(positions::clean_artist)
                .filter(|name| !name.is_empty()),
            duration: positions::read_duration(item["duration"].as_str().unwrap_or("")),
        });
    }

    // A wholly numeric tracklist gets the halfway split. A partly numeric one does
    // not: if some rows carry a side letter, the ones that do not are a data
    // problem on the provider's end and guessing would contradict the rows that
    // are right.
    if !numeric.is_empty() && numeric.len() == entries.len() {
        let inferred = positions::split_numeric(entries.len(), 1);
        for (entry, resolved) in entries.iter_mut().zip(inferred) {
            entry.resolved = Some(resolved);
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::Recorded;
    use crate::net::Offline;
    use crate::policy::{Limiter, TestClock};

    fn discogs(transport: Arc<dyn Transport>) -> Discogs {
        let client = Client::new(ProviderId::Discogs, transport)
            .with_clock(Arc::new(TestClock::new()))
            .with_limiter(Limiter::unlimited());
        Discogs::new(Arc::new(Offline))
            .with_client(client)
            .with_token(Token::new("sekrit"))
    }

    #[test]
    fn a_search_url_uses_the_structured_fields() {
        let provider = Discogs::new(Arc::new(Offline));
        let url = provider.search_url(
            &Query::new()
                .artist("Autechre")
                .album("Amber")
                .catalog("WARPLP25")
                .limit(10),
        );
        assert!(url.starts_with(&format!("{API}/database/search?type=release")));
        assert!(url.contains("&artist=Autechre"), "{url}");
        assert!(url.contains("&release_title=Amber"), "{url}");
        assert!(url.contains("&catno=WARPLP25"), "{url}");
        assert!(
            url.contains("&format=Vinyl"),
            "vinyl-only is the default: {url}"
        );
        assert!(url.contains("&per_page=10"), "{url}");
        assert!(
            !url.contains("token"),
            "the credential is a header, never the cache key: {url}"
        );
    }

    #[test]
    fn a_query_that_is_not_vinyl_only_does_not_filter_on_format() {
        let provider = Discogs::new(Arc::new(Offline));
        let url = provider.search_url(&Query::new().artist("x").vinyl_only(false));
        assert!(!url.contains("format="), "{url}");
    }

    #[test]
    fn everything_is_percent_encoded() {
        let provider = Discogs::new(Arc::new(Offline));
        let url = provider.search_url(&Query::new().artist("Simon & Garfunkel"));
        assert!(url.contains("artist=Simon%20%26%20Garfunkel"), "{url}");
    }

    #[test]
    fn without_a_token_a_search_says_which_variable_to_set() {
        let provider = Discogs::new(Arc::new(Offline))
            .with_client(Client::new(ProviderId::Discogs, Arc::new(Recorded::new())));
        assert!(!provider.has_token());
        let error = provider
            .search(&Query::new().artist("Autechre"), &Cancel::new())
            .expect_err("no search");
        assert_eq!(
            error.to_string(),
            "Discogs needs a credential: set VCW_DISCOGS_TOKEN"
        );
    }

    #[test]
    fn the_token_goes_out_in_a_header() {
        let transport =
            Arc::new(Recorded::new().json_matching("/database/search", r#"{"results":[]}"#));
        let provider = discogs(transport.clone());
        provider
            .search(&Query::new().artist("Autechre"), &Cancel::new())
            .expect("a search");
        let sent = transport.requests().remove(0);
        assert_eq!(
            sent.header("authorization"),
            Some("Discogs token=sekrit"),
            "in a header"
        );
        assert!(
            !sent.url.contains("sekrit"),
            "and not in the URL: {}",
            sent.url
        );
    }

    #[test]
    fn an_empty_query_is_not_a_search() {
        let provider = discogs(Arc::new(Recorded::new()));
        assert!(matches!(
            provider.search(&Query::new(), &Cancel::new()),
            Err(Error::NothingToSearch { .. })
        ));
    }

    #[test]
    fn a_query_discogs_cannot_act_on_is_not_a_search() {
        let provider = discogs(Arc::new(Recorded::new()));
        let query = Query::new().fingerprint(crate::query::Fingerprint::new("ABC", 30));
        assert!(
            matches!(
                provider.search(&query, &Cancel::new()),
                Err(Error::NothingToSearch { .. })
            ),
            "Discogs has no audio identification"
        );
        assert_eq!(provider.understands().len(), 8);
    }

    #[test]
    fn a_search_hit_becomes_a_candidate_with_its_pressing_details() {
        let body = r#"{"results":[{
            "id": 1234,
            "title": "Autechre (3) - Amber",
            "year": "1994",
            "label": ["Warp Records", "Warp"],
            "catno": "WARPLP25",
            "country": "UK",
            "format": ["Vinyl", "2xLP", "Album"]
        }]}"#;
        let provider = discogs(Arc::new(
            Recorded::new().json_matching("/database/search", body),
        ));
        let found = provider
            .search(&Query::new().album("Amber"), &Cancel::new())
            .expect("a search");
        assert_eq!(found.len(), 1);
        let hit = &found[0];
        assert_eq!(hit.id, "1234");
        assert_eq!(hit.artist, "Autechre", "the (3) is a disambiguator");
        assert_eq!(hit.album, "Amber");
        assert_eq!(hit.year, Some(1994));
        assert_eq!(hit.label, "Warp Records", "the first of several");
        assert_eq!(hit.catalog, "WARPLP25");
        assert_eq!(hit.format, "Vinyl, 2xLP, Album");
        assert_eq!(
            hit.url.as_deref(),
            Some("https://www.discogs.com/release/1234")
        );
        assert!(hit.summary().contains("WARPLP25"));
    }

    #[test]
    fn a_title_with_no_separator_is_taken_as_the_album() {
        let body = r#"{"results":[{"id":1,"title":"Amber"}]}"#;
        let provider = discogs(Arc::new(
            Recorded::new().json_matching("/database/search", body),
        ));
        let found = provider
            .search(&Query::new().album("Amber"), &Cancel::new())
            .expect("a search");
        assert_eq!(found[0].album, "Amber");
        assert!(found[0].artist.is_empty());
    }

    #[test]
    fn a_year_of_zero_is_not_a_year() {
        let body = r#"{"results":[{"id":1,"title":"a - b","year":0}]}"#;
        let provider = discogs(Arc::new(
            Recorded::new().json_matching("/database/search", body),
        ));
        let found = provider
            .search(&Query::new().album("b"), &Cancel::new())
            .expect("a search");
        assert_eq!(found[0].year, None);
    }

    #[test]
    fn a_two_record_release_becomes_two_media_with_four_sides() {
        let body = r#"{
            "title": "Amber",
            "artists": [{"name": "Autechre (3)"}],
            "year": 1994,
            "country": "UK",
            "genres": ["Electronic"],
            "styles": ["Ambient", "IDM"],
            "labels": [{"name": "Warp Records", "catno": "WARPLP25"}],
            "identifiers": [{"type": "Barcode", "value": " 5021603025011 "}],
            "formats": [{"name": "Vinyl", "qty": "2", "descriptions": ["LP", "Album"]}],
            "images": [
                {"type": "secondary", "uri": "https://img/back.jpg", "width": 600, "height": 600},
                {"type": "primary", "uri": "https://img/front.jpg", "width": 600, "height": 600}
            ],
            "tracklist": [
                {"type_": "heading", "position": "", "title": "Side A"},
                {"type_": "track", "position": "A1", "title": "Foil", "duration": "5:00"},
                {"type_": "track", "position": "A2", "title": "Montreal", "duration": "4:30"},
                {"type_": "track", "position": "B1", "title": "Silverside", "duration": "6:00"},
                {"type_": "track", "position": "C1", "title": "Slip", "duration": "4:00"},
                {"type_": "track", "position": "D1", "title": "Nil", "duration": "7:00"}
            ]
        }"#;
        let provider = discogs(Arc::new(Recorded::new().json_matching("/releases/", body)));
        let release = provider.fetch("1234", &Cancel::new()).expect("a release");

        assert_eq!(release.album_artist, "Autechre");
        assert_eq!(release.catalog, "WARPLP25");
        assert_eq!(release.barcode.as_deref(), Some("5021603025011"));
        assert_eq!(release.discogs_id.as_deref(), Some("1234"));
        assert_eq!(
            release.genres,
            ["Electronic", "Ambient", "IDM"],
            "genres and styles, both, through the mapping table"
        );
        assert_eq!(release.artwork.len(), 2);
        assert!(release.artwork[0].primary, "the front cover comes first");

        assert_eq!(release.media.len(), 2, "qty 2");
        assert_eq!(release.media[0].format, "Vinyl, LP, Album");
        assert!(release.media[0].is_vinyl());
        assert_eq!(
            release.media[0].tracks.len(),
            3,
            "the heading is not a track"
        );
        assert_eq!(release.media[1].tracks.len(), 2, "sides C and D");

        let sides: Vec<char> = release.sides().iter().map(|s| s.letter()).collect();
        assert_eq!(sides, ['A', 'B', 'C', 'D']);
        assert_eq!(release.side_seconds(vcw_types::Side::A), Some(570.0));
        assert_eq!(release.tracks().len(), 5);
    }

    #[test]
    fn a_numeric_tracklist_is_split_in_half() {
        let body = r#"{
            "title": "Untitled",
            "formats": [{"name": "Vinyl", "qty": "1", "descriptions": ["LP"]}],
            "tracklist": [
                {"position": "1", "title": "One", "duration": "3:00"},
                {"position": "2", "title": "Two", "duration": "3:00"},
                {"position": "3", "title": "Three", "duration": "3:00"},
                {"position": "4", "title": "Four", "duration": "3:00"},
                {"position": "5", "title": "Five", "duration": "3:00"}
            ]
        }"#;
        let provider = discogs(Arc::new(Recorded::new().json_matching("/releases/", body)));
        let release = provider.fetch("7", &Cancel::new()).expect("a release");
        let written: Vec<String> = release
            .tracks()
            .iter()
            .map(|t| t.resolved.map(|p| p.alpha()).unwrap_or_default())
            .collect();
        assert_eq!(written, ["A1", "A2", "A3", "B1", "B2"]);
        assert_eq!(
            release.tracks()[0].position,
            "1",
            "the provider's own string is kept, so a UI can show what the label said"
        );
    }

    #[test]
    fn a_letter_run_tracklist_reads_as_track_numbers() {
        let body = r#"{
            "title": "Twelve",
            "formats": [{"name": "Vinyl", "qty": "1", "descriptions": ["12\""]}],
            "tracklist": [
                {"position": "A", "title": "One"},
                {"position": "AA", "title": "Two"},
                {"position": "B", "title": "Three"}
            ]
        }"#;
        let provider = discogs(Arc::new(Recorded::new().json_matching("/releases/", body)));
        let release = provider.fetch("7", &Cancel::new()).expect("a release");
        let written: Vec<String> = release
            .tracks()
            .iter()
            .map(|t| t.resolved.map(|p| p.alpha()).unwrap_or_default())
            .collect();
        assert_eq!(written, ["A1", "A2", "B1"]);
    }

    #[test]
    fn a_partly_numeric_tracklist_is_not_guessed_at() {
        let body = r#"{
            "title": "Mixed",
            "formats": [{"name": "Vinyl", "qty": "1"}],
            "tracklist": [
                {"position": "A1", "title": "One"},
                {"position": "2", "title": "Two"}
            ]
        }"#;
        let provider = discogs(Arc::new(Recorded::new().json_matching("/releases/", body)));
        let release = provider.fetch("7", &Cancel::new()).expect("a release");
        let tracks = release.tracks();
        assert_eq!(tracks[0].resolved.map(|p| p.alpha()).as_deref(), Some("A1"));
        assert_eq!(
            tracks[1].resolved, None,
            "the rows that are right are not overridden by a guess"
        );
        assert_eq!(tracks[1].title, "Two", "and the track is still listed");
    }

    #[test]
    fn more_sides_than_the_stated_disc_count_believes_the_tracklist() {
        let body = r#"{
            "title": "Mislabeled",
            "formats": [{"name": "Vinyl", "qty": "1"}],
            "tracklist": [
                {"position": "A1", "title": "One"},
                {"position": "C1", "title": "Three"}
            ]
        }"#;
        let provider = discogs(Arc::new(Recorded::new().json_matching("/releases/", body)));
        let release = provider.fetch("7", &Cancel::new()).expect("a release");
        assert_eq!(release.media.len(), 2, "C is on the second record");
        assert_eq!(release.media[1].position, 2);
    }

    #[test]
    fn a_body_that_is_not_json_is_reported_as_malformed() {
        let provider = discogs(Arc::new(Recorded::new().answering(
            Discogs::release_url("1"),
            crate::net::Response::ok("<html>maintenance</html>"),
        )));
        let error = provider.fetch("1", &Cancel::new()).expect_err("no release");
        assert!(matches!(error, Error::Malformed { .. }), "{error:?}");
    }

    #[test]
    fn a_search_with_no_results_array_is_malformed_not_empty() {
        let provider = discogs(Arc::new(
            Recorded::new().json_matching("/database/search", r#"{"pagination":{}}"#),
        ));
        let error = provider
            .search(&Query::new().artist("x"), &Cancel::new())
            .expect_err("no search");
        assert!(matches!(error, Error::Malformed { .. }), "{error:?}");
    }

    #[test]
    fn offline_is_an_answer_and_the_token_is_never_read() {
        let provider = Discogs::new(Arc::new(Offline)).with_token(Token::new("sekrit"));
        assert!(provider.is_offline());
        let error = provider
            .search(&Query::new().artist("Autechre"), &Cancel::new())
            .expect_err("no search");
        assert!(matches!(error, Error::Offline { .. }), "{error:?}");
    }

    #[test]
    fn offline_and_tokenless_reports_the_one_that_matters() {
        let provider = Discogs::new(Arc::new(Offline));
        assert!(!provider.has_token());
        let error = provider
            .search(&Query::new().artist("Autechre"), &Cancel::new())
            .expect_err("no search");
        assert!(
            matches!(error, Error::Offline { .. }),
            "setting a token would not help: {error:?}"
        );
    }

    #[test]
    fn fetching_nothing_is_not_a_request() {
        let transport = Arc::new(Recorded::new());
        let provider = discogs(transport.clone());
        assert!(matches!(
            provider.fetch("   ", &Cancel::new()),
            Err(Error::NothingToSearch { .. })
        ));
        assert_eq!(transport.calls(), 0);
    }
}
