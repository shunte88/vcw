/*
 *  release.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The provider-neutral release model (§28, §32).
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

//! The provider-neutral release model (§28, §32).
//!
//! Discogs and MusicBrainz describe the same object in different vocabularies, and
//! the rest of VCW should not have to know which one answered. Everything in this
//! module is the shape a *pressing* has, not the shape a provider's JSON has: the
//! provider modules translate into it, and the translation is where each provider's
//! peculiarities are documented and tested.
//!
//! # Why a medium rather than a disc
//!
//! §29's data model says disc, and a disc is what a person holds. A provider says
//! *medium*, and a medium may be a CD in a release that also has vinyl, or a
//! "file" for a digital edition. [`Medium`] keeps the provider's word because it
//! keeps the provider's meaning: turning one into a §29 disc means deciding it is
//! vinyl at all, and that decision belongs to the data model, not to a parser.
//!
//! # Why the positions are resolved here
//!
//! A track arrives from a provider with a position string - `A1`, `AA`, `3`, or
//! nothing at all. [`TrackEntry::position`] keeps that string verbatim, because it
//! is what the label says and a user comparing the screen to the record will look
//! for it. Beside it sits a resolved [`Position`], which is what the data model and
//! the exporter use. The two can disagree, and when they do the raw string wins the
//! argument about what is printed and the resolved one wins about what is played.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use vcw_types::vinyl::Position;

/// Which service an answer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// Discogs, the pressing-level database. Needs a token (§39).
    Discogs,
    /// MusicBrainz, the release-level database. Needs no credential, only a
    /// user agent that identifies the application (§40).
    MusicBrainz,
    /// AcoustID, which answers fingerprints rather than text. Landing with the
    /// fingerprinting work; named here because §28 requires it and because
    /// [`crate::query::Query`] already carries the evidence it needs.
    AcoustId,
}

impl ProviderId {
    /// The lowercase token used in JSON, on the command line and in cache keys.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discogs => "discogs",
            Self::MusicBrainz => "musicbrainz",
            Self::AcoustId => "acoustid",
        }
    }

    /// The name as the service spells it, for anything a person reads.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Discogs => "Discogs",
            Self::MusicBrainz => "MusicBrainz",
            Self::AcoustId => "AcoustID",
        }
    }

    /// Whether the provider will refuse to answer without a credential (§39).
    #[must_use]
    pub const fn needs_credential(self) -> bool {
        match self {
            Self::Discogs | Self::AcoustId => true,
            Self::MusicBrainz => false,
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// A search hit: enough to choose between pressings without fetching each one.
///
/// §28 asks that results expose enough to distinguish vinyl pressings, which is
/// why `format`, `country`, `catalog` and `year` are all here and all optional.
/// Two pressings of the same record differ in exactly these fields.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Candidate {
    /// The provider's own identifier, as a string because MusicBrainz uses UUIDs
    /// and Discogs uses integers.
    pub id: String,
    /// The artist as the provider credits it.
    pub artist: String,
    /// The release title.
    pub album: String,
    /// Release year, when the provider gives one.
    pub year: Option<u32>,
    /// The label, first one only when there are several.
    pub label: String,
    /// Catalogue number, the field that most often identifies one pressing.
    pub catalog: String,
    /// Country of release.
    pub country: String,
    /// Format as the provider describes it: `Vinyl, LP, Album, 180g`.
    pub format: String,
    /// How many tracks, when the search result says.
    pub tracks: Option<usize>,
    /// Where the release can be read by a person, for a "view on the web" action.
    pub url: Option<String>,
}

impl Candidate {
    /// One line for a picker list, skipping whatever the provider did not say.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.artist.is_empty() {
            parts.push(self.artist.clone());
        }
        if !self.album.is_empty() {
            parts.push(self.album.clone());
        }
        if let Some(year) = self.year {
            parts.push(format!("({year})"));
        }
        if !self.label.is_empty() {
            parts.push(format!("[{}]", self.label));
        }
        if !self.catalog.is_empty() {
            parts.push(self.catalog.clone());
        }
        if !self.format.is_empty() {
            parts.push(self.format.clone());
        }
        if !self.country.is_empty() {
            parts.push(self.country.clone());
        }
        parts.join(" ")
    }
}

/// One track as a provider describes it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TrackEntry {
    /// The position string exactly as the provider gave it, `A1` or `AA` or `3`.
    pub position: String,
    /// The position resolved into a side and a number, when it could be.
    ///
    /// `None` means the provider's string made no sense as vinyl and no side
    /// could be inferred. The track is still listed: a title with no position is
    /// more use than no title.
    pub resolved: Option<Position>,
    /// The track title.
    pub title: String,
    /// The track artist, when it differs from the release artist.
    pub artist: Option<String>,
    /// Stated duration, when the provider knows one.
    ///
    /// Vinyl durations are frequently wrong by a second or two and occasionally
    /// missing altogether, so nothing may *require* this. It is evidence for
    /// matching detected boundaries, not a boundary.
    pub duration: Option<Duration>,
}

impl TrackEntry {
    /// The stated duration in seconds, for display and for matching.
    #[must_use]
    pub fn seconds(&self) -> Option<f64> {
        self.duration.map(|d| d.as_secs_f64())
    }
}

/// One recording, which is what a fingerprint lookup answers (§27).
///
/// A *recording* is not a release and not a track: it is the performance, and the
/// same performance appears on many pressings. That is exactly why this is a
/// separate type from [`Candidate`] - a fingerprint identifies the performance, and
/// which pressing the user is holding is a question the performance alone cannot
/// answer. [`Self::releases`] is the evidence for that second question.
///
/// Produced by both providers: AcoustID returns it inline with a lookup, and
/// [`crate::MusicBrainz::recording`] fetches the canonical version of one by MBID.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Recording {
    /// The MusicBrainz recording MBID. Both providers use the same identifier
    /// here, because AcoustID's recordings *are* MusicBrainz recordings.
    pub id: String,
    /// The recording title.
    pub title: String,
    /// The artist credit, joined when there are several.
    pub artist: String,
    /// Stated length, when the provider knows one.
    ///
    /// Evidence for §26's duration agreement, not a boundary: a recording's
    /// length is the studio master's, and a pressing's groove is often a second or
    /// two off it.
    pub duration: Option<Duration>,
    /// The releases this recording is known to appear on.
    pub releases: Vec<RecordingRelease>,
}

impl Recording {
    /// The stated length in seconds, for matching against a detected region.
    #[must_use]
    pub fn seconds(&self) -> Option<f64> {
        self.duration.map(|d| d.as_secs_f64())
    }
}

/// Where a [`Recording`] sits on one release it appears on.
///
/// The position fields are the point. A fingerprint that matches a recording which
/// is track 3 of side B on one pressing and track 1 of a CD on another is evidence
/// about *which* pressing, and §26 says several such matches together constrain
/// the release, the side and the position. Anything the provider did not say is
/// `None` rather than zero, because a missing position and the first position are
/// not the same claim.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RecordingRelease {
    /// The release MBID, which [`crate::Provider::fetch`] accepts for MusicBrainz.
    pub id: String,
    /// The release title.
    pub title: String,
    /// Which medium of the release, one-based: the second disc of a 2xLP is `2`.
    ///
    /// The disc, not the side. MusicBrainz models a vinyl release as one medium per
    /// disc, so the side lives in the track *number* - `A3` - and neither provider
    /// sends that string here: a recording lookup gives the numeric position within
    /// the medium. The side comes from [`crate::Provider::fetch`] on [`Self::id`],
    /// which returns [`TrackEntry::resolved`] for every track at once, so a resolver
    /// holding several matches on one release fetches it once rather than per track.
    pub medium: Option<u32>,
    /// How many tracks that medium carries.
    pub track_count: Option<u32>,
    /// This recording's one-based position on that medium.
    pub position: Option<u32>,
    /// The medium's format as the provider spells it: `12" Vinyl`, `CD`.
    ///
    /// The field that separates the pressing the user is holding from the CD
    /// reissue it shares a title with. Thirteen releases came back for one Autechre
    /// recording and two of them were records.
    pub format: String,
}

impl RecordingRelease {
    /// Whether this appearance is on a record rather than a CD, a file or a tape.
    #[must_use]
    pub fn is_vinyl(&self) -> bool {
        looks_like_vinyl(&self.format)
    }
}

/// One physical carrier in a release: a disc, in §29's vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Medium {
    /// One-based position in the release, so disc 2 of 3 is `2`.
    pub position: u32,
    /// The provider's format string: `Vinyl`, `12" Vinyl`, `CD`.
    pub format: String,
    /// The tracks on it, in the order the provider listed them.
    pub tracks: Vec<TrackEntry>,
}

impl Medium {
    /// Whether this medium is a record rather than a CD, a file or a cassette.
    ///
    /// A substring test on the provider's own word, because both providers spell
    /// it several ways (`Vinyl`, `12" Vinyl`, `Vinyl, LP`) and neither offers a
    /// machine-readable flag.
    #[must_use]
    pub fn is_vinyl(&self) -> bool {
        looks_like_vinyl(&self.format)
    }
}

/// Whether a provider's format string describes a record.
///
/// A substring test on the provider's own word, because both providers spell it
/// several ways (`Vinyl`, `12" Vinyl`, `Vinyl, LP`) and neither offers a
/// machine-readable flag. Shared by [`Medium`] and [`RecordingRelease`] so the two
/// cannot drift: a format one of them calls vinyl and the other does not would show
/// up as a release appearing and disappearing between two views of the same record.
#[must_use]
pub fn looks_like_vinyl(format: &str) -> bool {
    let lower = format.to_ascii_lowercase();
    lower.contains("vinyl") || lower.contains("lp") || lower.contains('"')
}

/// A reference to artwork, which may or may not have been downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkRef {
    /// Where the image lives.
    pub url: String,
    /// Whether the provider calls this the front cover.
    pub primary: bool,
    /// Pixel width, when stated.
    pub width: Option<u32>,
    /// Pixel height, when stated.
    pub height: Option<u32>,
}

/// A full release, with its tracklist.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Release {
    /// The provider's identifier for it.
    pub id: String,
    /// The release title.
    pub album: String,
    /// The credited album artist.
    pub album_artist: String,
    /// Release year, when stated.
    pub year: Option<u32>,
    /// Genres and styles, normalised through the §32 mapping table.
    pub genres: Vec<String>,
    /// The label, first one only when there are several.
    pub label: String,
    /// Catalogue number.
    pub catalog: String,
    /// Country of release.
    pub country: String,
    /// Barcode, when stated.
    pub barcode: Option<String>,
    /// The MusicBrainz release id, whoever answered (§32).
    pub musicbrainz_id: Option<String>,
    /// The Discogs release id, whoever answered (§32).
    pub discogs_id: Option<String>,
    /// Artwork references, primary first.
    pub artwork: Vec<ArtworkRef>,
    /// The carriers, in release order.
    pub media: Vec<Medium>,
}

impl Release {
    /// Every track on every medium, in release order.
    #[must_use]
    pub fn tracks(&self) -> Vec<&TrackEntry> {
        self.media.iter().flat_map(|m| m.tracks.iter()).collect()
    }

    /// The tracks on one side, in order, across every medium.
    #[must_use]
    pub fn side_tracks(&self, side: vcw_types::vinyl::Side) -> Vec<&TrackEntry> {
        let mut tracks: Vec<&TrackEntry> = self
            .tracks()
            .into_iter()
            .filter(|t| t.resolved.is_some_and(|p| p.side == side))
            .collect();
        tracks.sort_by_key(|t| t.resolved.map(|p| p.number).unwrap_or(0));
        tracks
    }

    /// The sides the tracklist mentions, in playing order.
    #[must_use]
    pub fn sides(&self) -> Vec<vcw_types::vinyl::Side> {
        let mut sides: Vec<vcw_types::vinyl::Side> = self
            .tracks()
            .into_iter()
            .filter_map(|t| t.resolved.map(|p| p.side))
            .collect();
        sides.sort_unstable();
        sides.dedup();
        sides
    }

    /// The stated running time of one side, or `None` if any track is missing one.
    ///
    /// All-or-nothing on purpose: a side total assembled from three known
    /// durations and one guess is worse than no total, because it looks precise.
    #[must_use]
    pub fn side_seconds(&self, side: vcw_types::vinyl::Side) -> Option<f64> {
        let tracks = self.side_tracks(side);
        if tracks.is_empty() {
            return None;
        }
        tracks
            .iter()
            .try_fold(0.0, |total, track| track.seconds().map(|s| total + s))
    }

    /// Whether any medium is a record.
    #[must_use]
    pub fn has_vinyl(&self) -> bool {
        self.media.iter().any(Medium::is_vinyl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcw_types::vinyl::Side;

    fn track(position: &str, title: &str, seconds: Option<f64>) -> TrackEntry {
        TrackEntry {
            position: position.to_owned(),
            resolved: position.parse().ok(),
            title: title.to_owned(),
            artist: None,
            duration: seconds.map(Duration::from_secs_f64),
        }
    }

    fn release() -> Release {
        Release {
            id: "12345".into(),
            album: "Selected Ambient Works".into(),
            album_artist: "Aphex Twin".into(),
            media: vec![Medium {
                position: 1,
                format: "2 x Vinyl, LP".into(),
                tracks: vec![
                    track("A1", "Xtal", Some(294.0)),
                    track("A2", "Tha", Some(543.0)),
                    track("B1", "Pulsewidth", Some(230.0)),
                    track("C1", "Heliosphan", None),
                ],
            }],
            ..Release::default()
        }
    }

    #[test]
    fn a_release_reads_out_by_side() {
        let release = release();
        let a = Side::A;
        let titles: Vec<&str> = release
            .side_tracks(a)
            .iter()
            .map(|t| t.title.as_str())
            .collect();
        assert_eq!(titles, ["Xtal", "Tha"]);
        assert_eq!(
            release
                .sides()
                .iter()
                .map(|s| s.letter())
                .collect::<String>(),
            "ABC"
        );
        assert_eq!(release.tracks().len(), 4);
    }

    #[test]
    fn a_side_total_is_all_or_nothing() {
        let release = release();
        assert_eq!(release.side_seconds(Side::A), Some(837.0));
        assert_eq!(
            release.side_seconds(Side::from_letter('C').unwrap()),
            None,
            "one unknown duration makes the total unknown"
        );
        assert_eq!(
            release.side_seconds(Side::from_letter('Z').unwrap()),
            None,
            "a side with no tracks has no total"
        );
    }

    #[test]
    fn vinyl_is_recognised_however_the_provider_spells_it() {
        for format in ["Vinyl", "2 x Vinyl, LP", "12\" Vinyl", "LP", "vinyl"] {
            let medium = Medium {
                format: format.into(),
                ..Medium::default()
            };
            assert!(medium.is_vinyl(), "{format:?} is a record");
        }
        for format in ["CD", "File, FLAC", "Cassette"] {
            let medium = Medium {
                format: format.into(),
                ..Medium::default()
            };
            assert!(!medium.is_vinyl(), "{format:?} is not a record");
        }
    }

    #[test]
    fn a_candidate_summary_skips_what_the_provider_did_not_say() {
        let full = Candidate {
            artist: "Boards of Canada".into(),
            album: "Geogaddi".into(),
            year: Some(2002),
            label: "Warp".into(),
            catalog: "WARPLP101".into(),
            format: "3 x Vinyl, LP".into(),
            country: "UK".into(),
            ..Candidate::default()
        };
        assert_eq!(
            full.summary(),
            "Boards of Canada Geogaddi (2002) [Warp] WARPLP101 3 x Vinyl, LP UK"
        );
        let bare = Candidate {
            album: "Untitled".into(),
            ..Candidate::default()
        };
        assert_eq!(
            bare.summary(),
            "Untitled",
            "no empty brackets, no stray gaps"
        );
    }

    #[test]
    fn a_provider_says_whether_it_needs_a_credential() {
        assert!(ProviderId::Discogs.needs_credential());
        assert!(ProviderId::AcoustId.needs_credential());
        assert!(
            !ProviderId::MusicBrainz.needs_credential(),
            "MusicBrainz asks for identification, not authorisation"
        );
        assert_eq!(ProviderId::MusicBrainz.as_str(), "musicbrainz");
        assert_eq!(ProviderId::MusicBrainz.to_string(), "MusicBrainz");
    }

    #[test]
    fn an_unresolvable_position_keeps_its_title() {
        let odd = track("", "Untitled", None);
        assert_eq!(odd.resolved, None);
        assert_eq!(odd.title, "Untitled");
        let release = Release {
            media: vec![Medium {
                tracks: vec![odd],
                ..Medium::default()
            }],
            ..Release::default()
        };
        assert_eq!(release.tracks().len(), 1);
        assert_eq!(release.sides(), [], "and claims no side");
    }
}
