/*
 *  query.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What can be asked of a metadata provider (§28).
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

//! What can be asked of a metadata provider (§28).
//!
//! §28 lists the search criteria: artist, album, catalog number, barcode, label,
//! year, country, provider release id, and fingerprint evidence. All of them are
//! optional and all of them are here, because a provider that cannot use one should
//! say so rather than force the caller to build a different query per provider.
//!
//! # Catalog number is the one that matters
//!
//! For vinyl, the catalog number is usually the difference between finding *a*
//! release and finding *the pressing in your hands*. Artist and title identify the
//! record; the catalog number, the country and the year identify which stamping
//! of it, and those are the fields that decide whether the tracklist you are about
//! to apply has the right number of tracks on side B.
//!
//! # Vinyl-only by default
//!
//! [`Query::vinyl_only`] starts `true`. A CD pressing's tracklist is a different
//! tracklist - different order, different splits, no sides - so offering one for a
//! vinyl capture is offering a wrong answer. It is a flag rather than a rule
//! because a test pressing or an unusual release may not be cataloged as vinyl at
//! all, and a person who has looked should be able to say so.

use std::fmt;

/// Audio evidence for a fingerprint lookup.
///
/// Carried by [`Query`] so that a provider trait implemented for AcoustID needs no
/// second query type; the two providers built today ignore it. Not a credential,
/// but derived from a recording, so it does not print itself either.
#[derive(Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The compressed fingerprint, in whatever encoding the service expects.
    pub code: String,
    /// How many seconds of audio it was computed over, which the service needs.
    pub seconds: u32,
}

impl Fingerprint {
    /// A fingerprint and the length of audio it was taken over.
    #[must_use]
    pub fn new(code: impl Into<String>, seconds: u32) -> Self {
        Self {
            code: code.into(),
            seconds,
        }
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Fingerprint({} bytes over {} s)",
            self.code.len(),
            self.seconds
        )
    }
}

/// One of §28's search criteria.
///
/// Exists so a provider can report what it ignored: a caller that searched on a
/// barcode deserves to know that the provider it asked cannot see barcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Criterion {
    /// The performing artist.
    Artist,
    /// The release title.
    Album,
    /// The label's catalog number.
    Catalog,
    /// The barcode printed on the sleeve.
    Barcode,
    /// The record label.
    Label,
    /// Year of release.
    Year,
    /// Country of release.
    Country,
    /// The provider's own release identifier.
    ReleaseId,
    /// Fingerprint evidence taken from the audio.
    Fingerprint,
}

impl Criterion {
    /// The name a person would recognize, for "this was ignored" messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Catalog => "catalog number",
            Self::Barcode => "barcode",
            Self::Label => "label",
            Self::Year => "year",
            Self::Country => "country",
            Self::ReleaseId => "release id",
            Self::Fingerprint => "fingerprint",
        }
    }
}

impl fmt::Display for Criterion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A search, built from any subset of §28's criteria.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// The performing artist.
    pub artist: Option<String>,
    /// The release title.
    pub album: Option<String>,
    /// The label's catalog number.
    pub catalog: Option<String>,
    /// The barcode printed on the sleeve.
    pub barcode: Option<String>,
    /// The record label.
    pub label: Option<String>,
    /// Year of release.
    pub year: Option<u32>,
    /// Country of release, as the provider spells it.
    pub country: Option<String>,
    /// The provider's own release identifier, which makes this a fetch.
    pub release_id: Option<String>,
    /// Fingerprint evidence, for a provider that can use it.
    pub fingerprint: Option<Fingerprint>,
    /// Whether to ask only about records.
    pub vinyl_only: bool,
    /// How many candidates to ask for.
    pub limit: usize,
}

/// How many candidates a search asks for when the caller does not say.
///
/// Enough that the right pressing is almost always on the first page, few enough
/// that a person can read the list. Providers cap it lower than this sometimes and
/// are allowed to.
pub const DEFAULT_LIMIT: usize = 25;

impl Default for Query {
    fn default() -> Self {
        Self {
            artist: None,
            album: None,
            catalog: None,
            barcode: None,
            label: None,
            year: None,
            country: None,
            release_id: None,
            fingerprint: None,
            vinyl_only: true,
            limit: DEFAULT_LIMIT,
        }
    }
}

/// Trims and discards empty strings, so `--artist ""` is the same as not saying.
fn clean(value: impl Into<String>) -> Option<String> {
    let value = value.into();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

impl Query {
    /// An empty query. Every provider refuses it, which is the point.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the artist.
    #[must_use]
    pub fn artist(mut self, artist: impl Into<String>) -> Self {
        self.artist = clean(artist);
        self
    }

    /// Sets the release title.
    #[must_use]
    pub fn album(mut self, album: impl Into<String>) -> Self {
        self.album = clean(album);
        self
    }

    /// Sets the catalog number.
    #[must_use]
    pub fn catalog(mut self, catalog: impl Into<String>) -> Self {
        self.catalog = clean(catalog);
        self
    }

    /// Sets the barcode.
    #[must_use]
    pub fn barcode(mut self, barcode: impl Into<String>) -> Self {
        self.barcode = clean(barcode);
        self
    }

    /// Sets the label.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = clean(label);
        self
    }

    /// Sets the year.
    #[must_use]
    pub const fn year(mut self, year: u32) -> Self {
        self.year = Some(year);
        self
    }

    /// Sets the country.
    #[must_use]
    pub fn country(mut self, country: impl Into<String>) -> Self {
        self.country = clean(country);
        self
    }

    /// Sets the provider's release identifier.
    #[must_use]
    pub fn release_id(mut self, id: impl Into<String>) -> Self {
        self.release_id = clean(id);
        self
    }

    /// Attaches fingerprint evidence.
    #[must_use]
    pub fn fingerprint(mut self, fingerprint: Fingerprint) -> Self {
        self.fingerprint = Some(fingerprint);
        self
    }

    /// Whether to restrict the search to records.
    #[must_use]
    pub const fn vinyl_only(mut self, only: bool) -> Self {
        self.vinyl_only = only;
        self
    }

    /// How many candidates to ask for. Zero is treated as one.
    #[must_use]
    pub const fn limit(mut self, limit: usize) -> Self {
        self.limit = if limit == 0 { 1 } else { limit };
        self
    }

    /// Which criteria this query actually carries, in a stable order.
    #[must_use]
    pub fn criteria(&self) -> Vec<Criterion> {
        let mut set = Vec::new();
        if self.artist.is_some() {
            set.push(Criterion::Artist);
        }
        if self.album.is_some() {
            set.push(Criterion::Album);
        }
        if self.catalog.is_some() {
            set.push(Criterion::Catalog);
        }
        if self.barcode.is_some() {
            set.push(Criterion::Barcode);
        }
        if self.label.is_some() {
            set.push(Criterion::Label);
        }
        if self.year.is_some() {
            set.push(Criterion::Year);
        }
        if self.country.is_some() {
            set.push(Criterion::Country);
        }
        if self.release_id.is_some() {
            set.push(Criterion::ReleaseId);
        }
        if self.fingerprint.is_some() {
            set.push(Criterion::Fingerprint);
        }
        set
    }

    /// Whether nothing at all was asked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.criteria().is_empty()
    }

    /// The criteria in this query that `understood` does not contain.
    ///
    /// A caller shows these to the user as "ignored by MusicBrainz", which is the
    /// honest thing to do: silently dropping a barcode the user typed makes the
    /// result look like an answer to a question nobody asked.
    #[must_use]
    pub fn ignored_by(&self, understood: &[Criterion]) -> Vec<Criterion> {
        self.criteria()
            .into_iter()
            .filter(|c| !understood.contains(c))
            .collect()
    }

    /// The free-text form of the query: artist, album, catalog number, label.
    ///
    /// Discogs' `q=` parameter wants one string, and this is how VRipr built it.
    #[must_use]
    pub fn free_text(&self) -> String {
        [
            self.artist.as_deref(),
            self.album.as_deref(),
            self.catalog.as_deref(),
            self.label.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_reports_only_the_criteria_it_carries() {
        let query = Query::new()
            .artist("Autechre")
            .album("Amber")
            .catalog("WARP LP 25");
        assert_eq!(
            query.criteria(),
            [Criterion::Artist, Criterion::Album, Criterion::Catalog]
        );
        assert!(!query.is_empty());
        assert!(Query::new().is_empty());
    }

    #[test]
    fn an_empty_field_is_the_same_as_an_unset_one() {
        let query = Query::new().artist("   ").album("Amber").country("");
        assert_eq!(query.artist, None);
        assert_eq!(query.country, None);
        assert_eq!(query.criteria(), [Criterion::Album]);
        assert_eq!(
            Query::new().album(" Amber ").album,
            Some("Amber".to_owned()),
            "and it is trimmed"
        );
    }

    #[test]
    fn the_defaults_are_the_vinyl_defaults() {
        let query = Query::new();
        assert!(query.vinyl_only, "this is a vinyl application");
        assert_eq!(query.limit, DEFAULT_LIMIT);
        assert_eq!(
            Query::new().limit(0).limit,
            1,
            "zero results is not a search"
        );
    }

    #[test]
    fn a_provider_can_be_told_what_it_ignored() {
        let query = Query::new().artist("Coil").barcode("5016027601712");
        let understood = [Criterion::Artist, Criterion::Album];
        assert_eq!(query.ignored_by(&understood), [Criterion::Barcode]);
        assert_eq!(
            query.ignored_by(&[Criterion::Artist, Criterion::Barcode]),
            [],
            "nothing ignored when the provider sees both"
        );
        assert_eq!(Criterion::Catalog.to_string(), "catalog number");
    }

    #[test]
    fn free_text_is_the_fields_a_search_box_would_take() {
        let query = Query::new()
            .artist("Pole")
            .album("1")
            .catalog("KITTY 05")
            .label("Kiff SM")
            .year(1998);
        assert_eq!(query.free_text(), "Pole 1 KITTY 05 Kiff SM");
        assert_eq!(
            Query::new().year(1998).free_text(),
            "",
            "a year alone is not a search string"
        );
    }

    #[test]
    fn fingerprint_evidence_does_not_print_itself() {
        let query = Query::new().fingerprint(Fingerprint {
            code: "AQADtEmkRGkkRUmSJEmSJEkSJUmSJEmSJEmSJEmS".into(),
            seconds: 120,
        });
        let debug = format!("{query:?}");
        assert!(!debug.contains("AQADtEmk"), "{debug}");
        assert!(debug.contains("40 bytes over 120 s"), "{debug}");
        assert_eq!(query.criteria(), [Criterion::Fingerprint]);
    }
}
