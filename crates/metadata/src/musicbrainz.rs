/*
 *  musicbrainz.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The MusicBrainz provider: no credential, exact media, and Lucene (§28).
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

//! The MusicBrainz provider: no credential, exact media, and Lucene (§28).
//!
//! MusicBrainz is the provider that works out of the box. It needs no token, which
//! makes it the one VCW can rely on being available, and its data model has
//! *media* as first-class objects - a 2xLP really is two media with their own
//! formats and track lists - so the disc topology comes out of the API rather than
//! being inferred from position letters. What it wants in return is one request a
//! second and a user agent that says who is calling; both are in [`Client`].
//!
//! # Searching is Lucene, which means escaping
//!
//! The search endpoint takes a Lucene query in `query=`, so an artist called
//! `AC/DC` or a title with a colon in it will produce a syntax error or, worse,
//! silently different results, unless the special characters are escaped. See
//! [`escape`].
//!
//! # The format field matches exact medium names
//!
//! Measured, not assumed: `format:vinyl` returns **nothing**. MusicBrainz stores
//! four distinct format names - `Vinyl`, `12" Vinyl`, `7" Vinyl` and `10" Vinyl` -
//! and `format:` matches them exactly. A vinyl filter is therefore the disjunction
//! in [`VINYL_FORMATS`], not the word "vinyl". Getting this wrong is silent: the
//! query is valid and every result is missing.
//!
//! # Genres may be on the release group instead
//!
//! A release's `genres` array is frequently empty while its release group's is
//! populated - the 1994 Warp pressing of *Amber* has none of its own and
//! `ambient`, `ambient techno`, `idm` on the group. It is also frequently *thin*:
//! the 2013 pressing of *Tomorrow's Harvest* has `electronic` with a single vote
//! of its own while the group has five genres with fourteen votes between them.
//!
//! So both arrays are read and concatenated, release first, rather than the
//! group being a fallback for an empty release. That is why
//! `inc=release-groups+genres` is not optional, and why a release having one
//! genre is not a reason to stop looking.
//!
//! MusicBrainz tags are also lowercase by convention, every one of them, so they
//! are title-cased before the §32 lookup. The lookup is case-insensitive either
//! way, so this changes nothing for a tag the table knows; what it fixes is the
//! tag the table does *not* know, which under the pass-through rule would
//! otherwise be written onto the record as `ambient techno` next to a mapped
//! `IDM`. See [`title_case`] for the two things it deliberately does not do.
//!
//! # There are no artwork URLs
//!
//! MusicBrainz holds no images. What it holds is `cover-art-archive.front`, a
//! boolean saying the Cover Art Archive has a front cover, whose URL is then a
//! known shape. So the artwork reference is synthesised rather than read, and only
//! when that flag is set.

use std::sync::Arc;
use std::time::Duration;

use crate::client::{Cancel, Client, Stats};
use crate::error::{Error, Result};
use crate::genres::Genres;
use crate::net::{Transport, encode};
use crate::positions::{self, Reading};
use crate::provider::Provider;
use crate::query::{Criterion, Query};
use crate::release::{
    ArtworkRef, Candidate, Medium, ProviderId, Recording, RecordingRelease, Release, TrackEntry,
};

/// The web service root.
pub const API: &str = "https://musicbrainz.org/ws/2";

/// Where a person reads a release.
pub const WEB: &str = "https://musicbrainz.org/release";

/// The Cover Art Archive, which is where MusicBrainz's images actually live.
pub const COVER_ART: &str = "https://coverartarchive.org/release";

/// What a release fetch has to ask for.
///
/// Every one of these is load-bearing: `recordings` is the tracklist, `labels` is
/// the catalog number that identifies the pressing, `artist-credits` is the
/// per-track artist on a compilation, `release-groups` is where the genres often
/// are, and `genres` is §32's input.
pub const RELEASE_INCLUDES: &str = "recordings+artist-credits+labels+release-groups+genres";

/// What a recording fetch has to ask for.
///
/// `artists` is the credit, `releases` is which pressings carry the recording - the
/// answer §26 wants - and `media` is where on each of them it sits. Without `media`
/// the releases come back as bare titles, which cannot distinguish the LP from the
/// CD reissue it shares a title with.
pub const RECORDING_INCLUDES: &str = "artists+releases+media";

/// Every format name MusicBrainz uses for a record.
///
/// Measured against the live service on 2026-09-26: `Vinyl` 37,643 releases,
/// `12" Vinyl` 408,724, `7" Vinyl` 159,636, `10" Vinyl` 10,888. A `format:vinyl`
/// filter matches none of them.
pub const VINYL_FORMATS: &[&str] = &["Vinyl", "12\" Vinyl", "7\" Vinyl", "10\" Vinyl"];

/// The criteria MusicBrainz can act on.
///
/// No barcode in *search*: MusicBrainz indexes a barcode field but populates it
/// thinly enough that a barcode search mostly returns nothing, and a criterion
/// that silently finds nothing is worse than one declared unsupported. No
/// fingerprint either - that is AcoustID, which returns MBIDs and is WP-14's job.
pub const UNDERSTANDS: &[Criterion] = &[
    Criterion::Artist,
    Criterion::Album,
    Criterion::Catalog,
    Criterion::Label,
    Criterion::Year,
    Criterion::Country,
    Criterion::ReleaseId,
];

/// The MusicBrainz provider.
#[derive(Debug)]
pub struct MusicBrainz {
    client: Client,
    genres: Genres,
}

impl MusicBrainz {
    /// A provider over a transport.
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            client: Client::new(ProviderId::MusicBrainz, transport),
            genres: Genres::builtin(),
        }
    }

    /// Replaces the genre mapping table (§32).
    #[must_use]
    pub fn with_genres(mut self, genres: Genres) -> Self {
        self.genres = genres;
        self
    }

    /// Replaces the request client.
    #[must_use]
    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    /// The request client.
    #[must_use]
    pub const fn client(&self) -> &Client {
        &self.client
    }

    /// The Lucene query for a search.
    ///
    /// Fielded terms joined with `AND`, each value quoted and escaped, and the
    /// vinyl filter as a parenthesised `OR` of the four real format names.
    #[must_use]
    pub fn lucene(query: &Query) -> String {
        let mut terms: Vec<String> = Vec::new();
        if let Some(artist) = query.artist.as_deref() {
            terms.push(format!("artist:\"{}\"", escape(artist)));
        }
        if let Some(album) = query.album.as_deref() {
            terms.push(format!("release:\"{}\"", escape(album)));
        }
        if let Some(catalog) = query.catalog.as_deref() {
            terms.push(format!("catno:\"{}\"", escape(catalog)));
        }
        if let Some(label) = query.label.as_deref() {
            terms.push(format!("label:\"{}\"", escape(label)));
        }
        if let Some(year) = query.year {
            terms.push(format!("date:{year}"));
        }
        if let Some(country) = query.country.as_deref() {
            terms.push(format!("country:\"{}\"", escape(country)));
        }
        if query.vinyl_only {
            let formats: Vec<String> = VINYL_FORMATS
                .iter()
                .map(|format| format!("format:\"{}\"", escape(format)))
                .collect();
            terms.push(format!("({})", formats.join(" OR ")));
        }
        terms.join(" AND ")
    }

    /// The search URL for a query.
    #[must_use]
    pub fn search_url(query: &Query) -> String {
        format!(
            "{API}/release?query={}&limit={}&fmt=json",
            encode(&Self::lucene(query)),
            query.limit
        )
    }

    /// The release URL for an MBID.
    #[must_use]
    pub fn release_url(id: &str) -> String {
        format!(
            "{API}/release/{}?inc={RELEASE_INCLUDES}&fmt=json",
            encode(id)
        )
    }

    /// The recording URL for an MBID.
    #[must_use]
    pub fn recording_url(id: &str) -> String {
        format!(
            "{API}/recording/{}?inc={RECORDING_INCLUDES}&fmt=json",
            encode(id)
        )
    }

    /// One recording in full, by its MusicBrainz MBID (§26).
    ///
    /// The second half of a fingerprint lookup. [`crate::AcoustId`] returns
    /// recordings with the metadata AcoustID holds, which is usually everything
    /// needed; this is for the match where it holds nothing but an MBID, and for
    /// the canonical spelling of a title or a credit when the two disagree.
    ///
    /// What it deliberately does not do is resolve a *side*. A vinyl release is one
    /// medium per disc in this data model, with the side living in the track
    /// number (`A3`), and a recording fetch gives the numeric position within the
    /// medium instead. The side-aware path is [`Provider::fetch`] on the release, which
    /// returns [`TrackEntry::resolved`]; a resolver holding several matches on one
    /// release should fetch it once rather than ask here per track.
    pub fn recording(&self, id: &str, cancel: &Cancel) -> Result<Recording> {
        if id.trim().is_empty() {
            return Err(Error::NothingToSearch {
                provider: ProviderId::MusicBrainz,
            });
        }
        let body = self.client.body(&Self::recording_url(id), &[], cancel)?;
        Ok(recording_of(id, &parse(&body)?))
    }

    /// Turns one search result into a candidate.
    fn candidate(value: &serde_json::Value) -> Option<Candidate> {
        let id = value["id"].as_str()?.to_string();
        let label = value["label-info"].as_array().and_then(|l| l.first());
        Some(Candidate {
            id: id.clone(),
            artist: credit(&value["artist-credit"]),
            album: value["title"].as_str().unwrap_or("").trim().to_string(),
            year: year_of(value["date"].as_str()),
            label: label
                .and_then(|l| l["label"]["name"].as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            catalog: label
                .and_then(|l| l["catalog-number"].as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            country: value["country"].as_str().unwrap_or("").trim().to_string(),
            format: media_summary(&value["media"]),
            tracks: value["track-count"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok()),
            url: Some(format!("{WEB}/{id}")),
        })
    }

    /// Turns a release body into a release.
    fn release(&self, id: &str, value: &serde_json::Value) -> Release {
        let label = value["label-info"].as_array().and_then(|l| l.first());
        // Both arrays, release first. Not a fallback: a release's own genres are
        // the more specific statement and so they lead, but a release with one
        // low-voted genre of its own is thin rather than authoritative, and
        // discarding the group's because the release had *something* loses most
        // of what MusicBrainz knows. Tomorrow's Harvest is the case that found
        // this: the release carries `electronic` with one vote, the group
        // carries `electronic` 4, `ambient` 3, `idm` 3, `downtempo` 1 and
        // `electronica` 1, and the export went out tagged with one genre.
        //
        // Concatenated rather than merged by vote count, because the counts are
        // not comparable: a release's votes are cast by the handful of people
        // who edited that pressing, a group's by everyone who ever tagged the
        // record. `normalize_all` dedupes through §32's table, so a name in both
        // arrays is written once, and whether two near-neighbors collapse is
        // the table's call rather than this function's: genre.dat keeps
        // `Electronica` distinct from `Electronic`, so both go on the sleeve.
        let mut names = genre_names(&value["genres"]);
        names.extend(genre_names(&value["release-group"]["genres"]));
        let names: Vec<String> = names.iter().map(|name| title_case(name)).collect();
        Release {
            id: id.to_string(),
            album: value["title"].as_str().unwrap_or("").trim().to_string(),
            album_artist: credit(&value["artist-credit"]),
            year: year_of(value["date"].as_str()),
            genres: self.genres.normalize_all(names),
            label: label
                .and_then(|l| l["label"]["name"].as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            catalog: label
                .and_then(|l| l["catalog-number"].as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            country: value["country"].as_str().unwrap_or("").trim().to_string(),
            barcode: value["barcode"]
                .as_str()
                .map(|b| b.trim().to_string())
                .filter(|b| !b.is_empty()),
            musicbrainz_id: Some(id.to_string()),
            discogs_id: None,
            artwork: artwork_of(id, &value["cover-art-archive"]),
            media: media_of(&value["media"]),
        }
    }
}

impl Provider for MusicBrainz {
    fn id(&self) -> ProviderId {
        ProviderId::MusicBrainz
    }

    fn search(&self, query: &Query, cancel: &Cancel) -> Result<Vec<Candidate>> {
        if query.is_empty() {
            return Err(Error::NothingToSearch {
                provider: ProviderId::MusicBrainz,
            });
        }
        // A query whose only criteria are ignored would become a Lucene string of
        // nothing but the format filter, which matches every record ever pressed.
        if query.ignored_by(UNDERSTANDS).len() == query.criteria().len() {
            return Err(Error::NothingToSearch {
                provider: ProviderId::MusicBrainz,
            });
        }
        let body = self.client.body(&Self::search_url(query), &[], cancel)?;
        let value = parse(&body)?;
        let releases = value["releases"].as_array().ok_or(Error::Malformed {
            provider: ProviderId::MusicBrainz,
            detail: "no releases array".into(),
        })?;
        Ok(releases
            .iter()
            .filter_map(Self::candidate)
            .take(query.limit)
            .collect())
    }

    fn fetch(&self, id: &str, cancel: &Cancel) -> Result<Release> {
        if id.trim().is_empty() {
            return Err(Error::NothingToSearch {
                provider: ProviderId::MusicBrainz,
            });
        }
        let body = self.client.body(&Self::release_url(id), &[], cancel)?;
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

/// Escapes the characters Lucene treats as syntax.
///
/// Values are quoted, so the dangerous ones are the quote and the backslash, but
/// the rest are escaped too because a `field:"..."` term is not the only place a
/// value can end up and an over-escaped value still matches.
///
/// ```
/// # use vcw_metadata::musicbrainz::escape;
/// assert_eq!(escape("AC/DC"), r"AC\/DC");
/// assert_eq!(escape(r#"Say "Hello""#), r#"Say \"Hello\""#);
/// assert_eq!(escape("Autechre"), "Autechre");
/// ```
#[must_use]
pub fn escape(value: &str) -> String {
    const SPECIAL: &[char] = &[
        '\\', '+', '-', '&', '|', '!', '(', ')', '{', '}', '[', ']', '^', '"', '~', '*', '?', ':',
        '/',
    ];
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        if SPECIAL.contains(&character) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// Parses a body, reporting a provider error rather than a serde one.
fn parse(body: &[u8]) -> Result<serde_json::Value> {
    serde_json::from_slice(body).map_err(|error| Error::Malformed {
        provider: ProviderId::MusicBrainz,
        detail: error.to_string(),
    })
}

/// The credited artist, joined with whatever the credit says between names.
///
/// MusicBrainz models a credit as a list of names with the literal joining text
/// between them, so `Autechre` and `Boards of Canada & Autechre` both come out
/// spelled the way the sleeve spells them rather than with a comma inserted.
fn credit(value: &serde_json::Value) -> String {
    let Some(parts) = value.as_array() else {
        return String::new();
    };
    let mut out = String::new();
    for part in parts {
        if let Some(name) = part["name"]
            .as_str()
            .or_else(|| part["artist"]["name"].as_str())
        {
            out.push_str(name.trim());
        }
        if let Some(join) = part["joinphrase"].as_str() {
            out.push_str(join);
        }
    }
    out.trim().to_string()
}

/// Turns a recording body into a recording.
///
/// The id is passed in rather than read out of the body, because the body's `id` is
/// the one the service redirected to: ask for a merged recording's old MBID and
/// MusicBrainz answers with the surviving one. Keeping the id that was asked for
/// means a caller can match the answer to its question; the body's own id is the
/// interesting one only to something following merges, which nothing here does.
fn recording_of(id: &str, value: &serde_json::Value) -> Recording {
    Recording {
        id: id.to_string(),
        title: value["title"].as_str().unwrap_or("").trim().to_string(),
        artist: credit(&value["artist-credit"]),
        duration: length_of(value),
        releases: value["releases"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|release| {
                let medium = release["media"].as_array().and_then(|m| m.first());
                RecordingRelease {
                    id: release["id"].as_str().unwrap_or("").to_string(),
                    title: release["title"].as_str().unwrap_or("").trim().to_string(),
                    medium: medium.and_then(|m| count(&m["position"])),
                    track_count: medium
                        .and_then(|m| count(&m["track-count"]))
                        .or_else(|| count(&release["track-count"])),
                    position: medium
                        .and_then(|m| m["tracks"].as_array())
                        .and_then(|t| t.first())
                        .and_then(|t| count(&t["position"])),
                    format: medium
                        .and_then(|m| m["format"].as_str())
                        .unwrap_or("")
                        .to_string(),
                }
            })
            .collect(),
    }
}

/// A JSON number as a `u32`, or `None` if it was absent or absurd.
fn count(value: &serde_json::Value) -> Option<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// The year out of a MusicBrainz date, which may be `1994`, `1994-11` or full.
fn year_of(date: Option<&str>) -> Option<u32> {
    let year: u32 = date?.trim().get(..4)?.parse().ok()?;
    (year > 0).then_some(year)
}

/// The genre names in a `genres` array, most-voted first.
///
/// MusicBrainz genres are voted on, and the count is the only signal about which
/// of five tags is the one to write on the record. Ties keep the service's order.
fn genre_names(value: &serde_json::Value) -> Vec<String> {
    let Some(array) = value.as_array() else {
        return Vec::new();
    };
    let mut genres: Vec<(i64, String)> = array
        .iter()
        .filter_map(|genre| {
            let name = genre["name"].as_str()?.trim();
            (!name.is_empty()).then(|| (genre["count"].as_i64().unwrap_or(0), name.to_string()))
        })
        .collect();
    // `Reverse` rather than a flipped comparator: same stable descending
    // order, and clippy asks for it from 1.98 on.
    genres.sort_by_key(|left| std::cmp::Reverse(left.0));
    genres.into_iter().map(|(_, name)| name).collect()
}

/// Title-cases a lowercase MusicBrainz tag.
///
/// Every word's first letter, and nothing else touched. Two things it does not do,
/// both on purpose:
///
/// - It does not lowercase anything. `IDM` typed as `IDM` stays `IDM`, and a tag
///   the mapping table produced has already been spelled the way it should be.
/// - It does not know about `hip-hop` or `j-pop`. Splitting on punctuation would
///   turn `rock 'n' roll` into `Rock 'N' Roll`, and the mapping table is the right
///   place for a genre whose capitalisation is a fact rather than a rule.
///
/// ```
/// # use vcw_metadata::musicbrainz::title_case;
/// assert_eq!(title_case("ambient techno"), "Ambient Techno");
/// assert_eq!(title_case("IDM"), "IDM");
/// assert_eq!(title_case("hip-hop"), "Hip-hop");
/// ```
#[must_use]
pub fn title_case(name: &str) -> String {
    name.split(' ')
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A one-line format description for a candidate: `2 x 12" Vinyl`.
fn media_summary(value: &serde_json::Value) -> String {
    let Some(media) = value.as_array() else {
        return String::new();
    };
    let mut counts: Vec<(String, usize)> = Vec::new();
    for medium in media {
        let format = medium["format"].as_str().unwrap_or("").trim().to_string();
        match counts.iter_mut().find(|(seen, _)| *seen == format) {
            Some((_, count)) => *count += 1,
            None => counts.push((format, 1)),
        }
    }
    counts
        .into_iter()
        .map(|(format, count)| {
            if count > 1 {
                format!("{count} x {format}")
            } else {
                format
            }
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" + ")
}

/// The artwork the Cover Art Archive holds, by URL convention.
///
/// MusicBrainz returns no image URLs at all. `cover-art-archive.front` says a
/// front cover exists, and its address is then a fixed shape. No dimensions,
/// because nothing in the response states any.
fn artwork_of(id: &str, cover_art: &serde_json::Value) -> Vec<ArtworkRef> {
    let mut artwork = Vec::new();
    if cover_art["front"].as_bool() == Some(true) {
        artwork.push(ArtworkRef {
            url: format!("{COVER_ART}/{id}/front"),
            primary: true,
            width: None,
            height: None,
        });
    }
    if cover_art["back"].as_bool() == Some(true) {
        artwork.push(ArtworkRef {
            url: format!("{COVER_ART}/{id}/back"),
            primary: false,
            width: None,
            height: None,
        });
    }
    artwork
}

/// The media in a release, each with its own tracklist.
///
/// This is the part MusicBrainz does better than Discogs: the medium is given, so
/// the side letters can be checked against it rather than being the only evidence.
fn media_of(value: &serde_json::Value) -> Vec<Medium> {
    let Some(media) = value.as_array() else {
        return Vec::new();
    };
    media
        .iter()
        .enumerate()
        .map(|(index, medium)| {
            let position = medium["position"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
                .unwrap_or_else(|| u32::try_from(index + 1).unwrap_or(1));
            Medium {
                position,
                format: medium["format"].as_str().unwrap_or("").trim().to_string(),
                tracks: tracks_of(&medium["tracks"], position),
            }
        })
        .collect()
}

/// The tracks on one medium.
///
/// `number` is the label position (`A1`, `C2`) and `position` is the ordinal
/// within the medium; the label position is preferred because that is what is
/// pressed into the record. A wholly numeric medium gets the halfway split, with
/// the sides taken from this medium rather than from the release.
fn tracks_of(value: &serde_json::Value, medium: u32) -> Vec<TrackEntry> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut entries: Vec<TrackEntry> = Vec::new();
    let mut numeric = 0usize;

    for item in items {
        let written = item["number"]
            .as_str()
            .map(str::trim)
            .filter(|number| !number.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| {
                item["position"]
                    .as_u64()
                    .map(|p| p.to_string())
                    .unwrap_or_default()
            });
        let reading = positions::read(&written, medium);
        if matches!(reading, Reading::Numeric(_)) {
            numeric += 1;
        }
        entries.push(TrackEntry {
            position: written,
            resolved: match reading {
                Reading::Exact(resolved) => Some(resolved),
                _ => None,
            },
            title: item["title"]
                .as_str()
                .or_else(|| item["recording"]["title"].as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            artist: item["artist-credit"]
                .as_array()
                .filter(|credits| !credits.is_empty())
                .map(|_| credit(&item["artist-credit"]))
                .filter(|name| !name.is_empty()),
            duration: length_of(item),
        });
    }

    if numeric > 0 && numeric == entries.len() {
        let inferred = positions::split_numeric(entries.len(), medium);
        for (entry, resolved) in entries.iter_mut().zip(inferred) {
            entry.resolved = Some(resolved);
        }
    }
    entries
}

/// A track length in milliseconds, from the track or its recording.
///
/// MusicBrainz states lengths in milliseconds and sometimes only on the recording
/// rather than on the track. A `null` length means nobody timed it, which is not
/// the same as zero.
fn length_of(item: &serde_json::Value) -> Option<Duration> {
    let millis = item["length"]
        .as_u64()
        .or_else(|| item["recording"]["length"].as_u64())?;
    (millis > 0).then(|| Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::Recorded;
    use crate::net::Offline;
    use crate::policy::{Limiter, TestClock};
    use vcw_types::Side;

    const SEARCH: &str = include_str!("../tests/fixtures/musicbrainz_search_amber_vinyl.json");
    const RELEASE: &str = include_str!("../tests/fixtures/musicbrainz_release_amber_1994.json");
    const RECORDING: &str = include_str!("../tests/fixtures/musicbrainz_recording_silverside.json");
    const AMBER_1994: &str = "bd5b1270-7468-47f0-9c9a-928199f9e4ad";
    const SILVERSIDE_MBID: &str = "91fa0fd1-60d9-4536-8316-2e4489812fa4";

    fn brainz(transport: Arc<dyn Transport>) -> MusicBrainz {
        let client = Client::new(ProviderId::MusicBrainz, transport)
            .with_clock(Arc::new(TestClock::new()))
            .with_limiter(Limiter::unlimited());
        MusicBrainz::new(Arc::new(Offline)).with_client(client)
    }

    #[test]
    fn a_lucene_query_is_fielded_and_quoted() {
        let lucene = MusicBrainz::lucene(&Query::new().artist("Autechre").album("Amber"));
        assert_eq!(
            lucene,
            concat!(
                r#"artist:"Autechre" AND release:"Amber" AND ("#,
                r#"format:"Vinyl" OR format:"12\" Vinyl" OR "#,
                r#"format:"7\" Vinyl" OR format:"10\" Vinyl")"#
            )
        );
    }

    #[test]
    fn the_vinyl_filter_names_every_format_musicbrainz_actually_uses() {
        // `format:vinyl` returns nothing from the live service. Measured, not
        // assumed, and the reason this is a disjunction of four exact names.
        let lucene = MusicBrainz::lucene(&Query::new().artist("x"));
        for format in VINYL_FORMATS {
            assert!(
                lucene.contains(&escape(format)),
                "{format} missing from {lucene}"
            );
        }
        assert!(
            !lucene.contains("format:\"vinyl\""),
            "lowercase would match nothing: {lucene}"
        );
    }

    #[test]
    fn a_search_that_is_not_vinyl_only_has_no_format_filter() {
        let lucene = MusicBrainz::lucene(&Query::new().artist("x").vinyl_only(false));
        assert_eq!(lucene, r#"artist:"x""#);
    }

    #[test]
    fn lucene_syntax_in_a_value_is_escaped() {
        let lucene = MusicBrainz::lucene(
            &Query::new()
                .artist("AC/DC")
                .album("Who Made Who?")
                .vinyl_only(false),
        );
        assert_eq!(
            lucene, r#"artist:"AC\/DC" AND release:"Who Made Who\?""#,
            "an unescaped ? is a Lucene wildcard and a / opens a regex"
        );
    }

    #[test]
    fn the_search_url_is_percent_encoded_and_asks_for_json() {
        let url = MusicBrainz::search_url(&Query::new().artist("Autechre").limit(5));
        assert!(url.starts_with(&format!("{API}/release?query=")));
        assert!(url.contains("&limit=5&fmt=json"), "{url}");
        assert!(!url.contains(' '), "a raw space is not a URL: {url}");
        assert!(url.contains("artist%3A%22Autechre%22"), "{url}");
    }

    #[test]
    fn a_release_fetch_asks_for_everything_it_needs() {
        let url = MusicBrainz::release_url(AMBER_1994);
        assert!(url.contains(AMBER_1994));
        for include in [
            "recordings",
            "artist-credits",
            "labels",
            "release-groups",
            "genres",
        ] {
            assert!(url.contains(include), "{include} missing from {url}");
        }
        assert!(url.ends_with("&fmt=json"));
    }

    #[test]
    fn no_credential_is_needed_and_the_user_agent_identifies_us() {
        let transport = Arc::new(Recorded::new().json_matching("/release?query=", SEARCH));
        let provider = brainz(transport.clone());
        provider
            .search(
                &Query::new().artist("Autechre").album("Amber"),
                &Cancel::new(),
            )
            .expect("a search");
        let sent = transport.requests().remove(0);
        assert!(sent.header("authorization").is_none(), "no token exists");
        assert!(
            sent.header("user-agent")
                .is_some_and(|ua| ua.contains("VCW/")),
            "MusicBrainz requires identification and will block a client without it"
        );
    }

    #[test]
    fn the_real_search_fixture_gives_two_pressings_of_the_same_record() {
        let provider = brainz(Arc::new(
            Recorded::new().json_matching("/release?query=", SEARCH),
        ));
        let found = provider
            .search(
                &Query::new().artist("Autechre").album("Amber"),
                &Cancel::new(),
            )
            .expect("a search");
        assert_eq!(found.len(), 2, "captured from the live service");

        let original = found
            .iter()
            .find(|c| c.id == AMBER_1994)
            .expect("the 1994 pressing");
        assert_eq!(original.artist, "Autechre");
        assert_eq!(original.album, "Amber");
        assert_eq!(original.year, Some(1994));
        assert_eq!(original.catalog, "WARPLP25");
        assert_eq!(original.country, "GB");
        assert_eq!(
            original.format, "2 x 12\" Vinyl",
            "the thing that tells a person it is a double album"
        );
        assert!(
            original
                .url
                .as_deref()
                .is_some_and(|u| u.contains(AMBER_1994))
        );

        let repress = found
            .iter()
            .find(|c| c.catalog == "WARPLP25R")
            .expect("the repress");
        assert_ne!(
            repress.id, original.id,
            "two pressings, and the catalog number is what separates them"
        );
        assert_eq!(repress.year, Some(2016));
    }

    #[test]
    fn the_real_release_fixture_gives_four_sides_across_two_records() {
        let provider = brainz(Arc::new(
            Recorded::new().json_matching(format!("/release/{AMBER_1994}"), RELEASE),
        ));
        let release = provider
            .fetch(AMBER_1994, &Cancel::new())
            .expect("a release");

        assert_eq!(release.album, "Amber");
        assert_eq!(release.album_artist, "Autechre");
        assert_eq!(release.year, Some(1994));
        assert_eq!(release.catalog, "WARPLP25");
        assert_eq!(
            release.label, "Warp",
            "MusicBrainz names the label Warp; Discogs calls the same label Warp Records"
        );
        assert_eq!(release.country, "GB");
        assert_eq!(release.barcode, None, "the fixture has a null barcode");
        assert_eq!(release.musicbrainz_id.as_deref(), Some(AMBER_1994));
        assert_eq!(release.discogs_id, None);

        assert_eq!(release.media.len(), 2);
        assert_eq!(release.media[0].format, "12\" Vinyl");
        assert!(release.media.iter().all(Medium::is_vinyl));
        assert!(release.has_vinyl());

        let sides: Vec<char> = release.sides().iter().map(|s| s.letter()).collect();
        assert_eq!(
            sides,
            ['A', 'B', 'C', 'D'],
            "the second medium is lettered C and D in the fixture itself"
        );
        let side_a = release.side_tracks(Side::A);
        assert_eq!(side_a[0].position, "A1");
        assert_eq!(side_a[0].resolved.map(|p| p.alpha()).as_deref(), Some("A1"));
        assert!(
            release.side_seconds(Side::A).is_some_and(|s| s > 60.0),
            "lengths are milliseconds in the response and seconds here"
        );
        assert_eq!(
            release.tracks().len(),
            11,
            "6 on the first record, 5 on the second"
        );
    }

    #[test]
    fn a_release_with_no_genres_of_its_own_takes_the_groups() {
        let provider = brainz(Arc::new(
            Recorded::new().json_matching(format!("/release/{AMBER_1994}"), RELEASE),
        ));
        let release = provider
            .fetch(AMBER_1994, &Cancel::new())
            .expect("a release");
        assert_eq!(
            release.genres,
            ["IDM", "Ambient Techno", "Ambient"],
            "the release has no genres of its own; the group has three, most-voted first"
        );
    }

    #[test]
    fn a_lowercase_tag_the_table_does_not_know_is_still_presentable() {
        // `ambient techno` is not in the mapping table, so the pass-through rule
        // applies and the only thing standing between it and the record sleeve is
        // the title-casing. `idm` is in the table and comes out as the table
        // spells it, which is not the same operation.
        let genres = MusicBrainz::new(Arc::new(Offline));
        assert_eq!(title_case("ambient techno"), "Ambient Techno");
        assert_eq!(
            genres.genres.normalize("idm"),
            ["IDM"],
            "the table, not the title-caser"
        );
    }

    #[test]
    fn a_release_with_its_own_genres_still_takes_the_groups_after_them() {
        // One vote on the release does not outweigh 99 on the group, and it does
        // not have to: both are kept and the release's own leads. The counts are
        // not comparable across the two arrays, so they are not compared.
        let body = r#"{
            "id": "x", "title": "T",
            "genres": [{"name": "krautrock", "count": 1}],
            "release-group": {"genres": [{"name": "rock", "count": 99}]},
            "media": []
        }"#;
        let provider = brainz(Arc::new(Recorded::new().json_matching("/release/x", body)));
        let release = provider.fetch("x", &Cancel::new()).expect("a release");
        assert_eq!(
            release.genres,
            ["Krautrock", "Rock"],
            "specific leads, popular follows"
        );
    }

    #[test]
    fn a_genre_on_both_the_release_and_the_group_is_written_once() {
        // Tomorrow's Harvest, reduced to the shape that mattered: `electronic`
        // appears in both arrays and comes out once, and the five the group
        // knows about are not lost to the release having had a single thin one
        // of its own.
        //
        // `Electronica` stays beside `Electronic` rather than folding onto it:
        // genre.dat has both as keys of their own (rows 327 and 330), which is
        // VRipr's judgment that they are different genres, and §32 says the
        // table decides. The dedupe here is of names, not of neighbors.
        let body = r#"{
            "id": "x", "title": "T",
            "genres": [{"name": "electronic", "count": 1}],
            "release-group": {"genres": [
                {"name": "electronic", "count": 4},
                {"name": "ambient", "count": 3},
                {"name": "idm", "count": 3},
                {"name": "downtempo", "count": 1},
                {"name": "electronica", "count": 1}
            ]},
            "media": []
        }"#;
        let provider = brainz(Arc::new(Recorded::new().json_matching("/release/x", body)));
        let release = provider.fetch("x", &Cancel::new()).expect("a release");
        assert_eq!(
            release.genres,
            ["Electronic", "Ambient", "IDM", "Downtempo", "Electronica"],
            "six mentions of five genres, release's own first, then the group most-voted first"
        );
    }

    #[test]
    fn artwork_is_synthesised_from_the_cover_art_flags() {
        let provider = brainz(Arc::new(
            Recorded::new().json_matching(format!("/release/{AMBER_1994}"), RELEASE),
        ));
        let release = provider
            .fetch(AMBER_1994, &Cancel::new())
            .expect("a release");
        assert_eq!(
            release.artwork.len(),
            2,
            "the fixture says front and back both exist"
        );
        assert!(release.artwork[0].primary);
        assert_eq!(
            release.artwork[0].url,
            format!("{COVER_ART}/{AMBER_1994}/front"),
            "MusicBrainz returns no URLs at all; this one is a convention"
        );
        assert_eq!(release.artwork[0].width, None, "and no dimensions either");
    }

    #[test]
    fn no_cover_art_means_no_artwork_reference() {
        let body =
            r#"{"id":"x","title":"T","cover-art-archive":{"front":false,"back":false},"media":[]}"#;
        let provider = brainz(Arc::new(Recorded::new().json_matching("/release/x", body)));
        let release = provider.fetch("x", &Cancel::new()).expect("a release");
        assert!(release.artwork.is_empty());
    }

    #[test]
    fn a_joined_credit_keeps_the_words_between_the_names() {
        let body = r#"{
            "id": "x", "title": "T", "media": [],
            "artist-credit": [
                {"name": "Boards of Canada", "joinphrase": " & "},
                {"name": "Autechre", "joinphrase": ""}
            ]
        }"#;
        let provider = brainz(Arc::new(Recorded::new().json_matching("/release/x", body)));
        let release = provider.fetch("x", &Cancel::new()).expect("a release");
        assert_eq!(release.album_artist, "Boards of Canada & Autechre");
    }

    #[test]
    fn a_track_with_no_length_has_none_rather_than_zero() {
        let body = r#"{
            "id": "x", "title": "T",
            "media": [{"position": 1, "format": "12\" Vinyl", "tracks": [
                {"number": "A1", "title": "Timed", "length": 273000},
                {"number": "A2", "title": "Untimed", "length": null}
            ]}]
        }"#;
        let provider = brainz(Arc::new(Recorded::new().json_matching("/release/x", body)));
        let release = provider.fetch("x", &Cancel::new()).expect("a release");
        let tracks = release.tracks();
        assert_eq!(tracks[0].seconds(), Some(273.0));
        assert_eq!(tracks[1].seconds(), None);
        assert_eq!(
            release.side_seconds(Side::A),
            None,
            "a side total with a guess in it is worse than no total"
        );
    }

    #[test]
    fn a_numeric_medium_splits_within_its_own_sides() {
        let body = r#"{
            "id": "x", "title": "T",
            "media": [
                {"position": 1, "format": "12\" Vinyl", "tracks": [
                    {"number": "1", "title": "One"}, {"number": "2", "title": "Two"}
                ]},
                {"position": 2, "format": "12\" Vinyl", "tracks": [
                    {"number": "1", "title": "Three"}, {"number": "2", "title": "Four"}
                ]}
            ]
        }"#;
        let provider = brainz(Arc::new(Recorded::new().json_matching("/release/x", body)));
        let release = provider.fetch("x", &Cancel::new()).expect("a release");
        let written: Vec<String> = release
            .tracks()
            .iter()
            .map(|t| t.resolved.map(|p| p.alpha()).unwrap_or_default())
            .collect();
        assert_eq!(written, ["A1", "B1", "C1", "D1"]);
    }

    #[test]
    fn an_empty_query_is_not_a_search() {
        let transport = Arc::new(Recorded::new());
        let provider = brainz(transport.clone());
        assert!(matches!(
            provider.search(&Query::new(), &Cancel::new()),
            Err(Error::NothingToSearch { .. })
        ));
        assert_eq!(transport.calls(), 0, "and it never left the process");
    }

    #[test]
    fn a_barcode_only_search_is_declared_unsupported_rather_than_returning_nothing() {
        let provider = brainz(Arc::new(Recorded::new()));
        let query = Query::new().barcode("5021603025011");
        assert_eq!(query.ignored_by(UNDERSTANDS), [Criterion::Barcode]);
        assert!(matches!(
            provider.search(&query, &Cancel::new()),
            Err(Error::NothingToSearch { .. })
        ));
    }

    #[test]
    fn offline_is_an_answer_with_a_message() {
        let provider = MusicBrainz::new(Arc::new(Offline));
        assert!(provider.is_offline());
        let error = provider
            .search(&Query::new().artist("Autechre"), &Cancel::new())
            .expect_err("no search");
        assert_eq!(
            error.to_string(),
            "networking is disabled, so MusicBrainz was not contacted"
        );
    }

    #[test]
    fn a_recording_comes_back_with_its_length_its_credit_and_its_pressings() {
        let provider = brainz(Arc::new(
            Recorded::new().json(MusicBrainz::recording_url(SILVERSIDE_MBID), RECORDING),
        ));
        let found = provider
            .recording(SILVERSIDE_MBID, &Cancel::new())
            .expect("the recorded answer");

        assert_eq!(found.id, SILVERSIDE_MBID);
        assert_eq!(found.title, "Silverside");
        assert_eq!(found.artist, "Autechre");
        // 331066 ms, and milliseconds here where AcoustID sends float seconds.
        assert_eq!(found.seconds(), Some(331.066));
        assert_eq!(found.releases.len(), 13);

        let records: Vec<&RecordingRelease> =
            found.releases.iter().filter(|r| r.is_vinyl()).collect();
        assert_eq!(records.len(), 2, "two of the thirteen are records");
        assert_eq!(records[0].title, "Amber");
        assert_eq!(records[0].format, "12\" Vinyl");
        assert_eq!(
            records[0].position,
            Some(3),
            "the third track of the medium"
        );
        assert_eq!(records[0].medium, Some(1), "one disc, so side A is disc 1");
    }

    #[test]
    fn a_recording_fetch_asks_for_the_media_without_which_releases_are_bare_titles() {
        let url = MusicBrainz::recording_url(SILVERSIDE_MBID);
        assert!(url.contains("/recording/"), "{url}");
        assert!(url.contains("inc=artists+releases+media"), "{url}");
        assert!(url.ends_with("&fmt=json"), "{url}");
    }

    #[test]
    fn a_recording_keeps_the_mbid_it_was_asked_about() {
        // MusicBrainz answers a merged recording's old MBID with the surviving
        // one's body. Keeping the id that was asked for is what lets a caller
        // match the answer to its question.
        let provider = brainz(Arc::new(
            Recorded::new().json_matching("/recording/", RECORDING),
        ));
        let found = provider
            .recording("00000000-0000-0000-0000-000000000000", &Cancel::new())
            .expect("an answer for a merged id");
        assert_eq!(found.id, "00000000-0000-0000-0000-000000000000");
        assert_eq!(found.title, "Silverside", "and the body that came back");
    }

    #[test]
    fn an_empty_mbid_is_refused_before_a_request_is_made() {
        let recorded = Arc::new(Recorded::new());
        let provider = brainz(recorded.clone());
        assert!(matches!(
            provider.recording("  ", &Cancel::new()),
            Err(Error::NothingToSearch { .. })
        ));
        assert_eq!(recorded.calls(), 0);
    }

    #[test]
    fn a_maintenance_page_is_malformed_rather_than_a_panic() {
        let provider = brainz(Arc::new(Recorded::new().answering(
            MusicBrainz::release_url("x"),
            crate::net::Response::ok("<html>503</html>"),
        )));
        assert!(matches!(
            provider.fetch("x", &Cancel::new()),
            Err(Error::Malformed { .. })
        ));
    }
}
