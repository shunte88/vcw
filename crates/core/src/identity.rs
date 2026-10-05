/*
 *  Writing a release a person accepted into the project (§26, §32).
 *
 *  Writing a release a person accepted into the project (§26, §32).
 *  Writing a release a person accepted into the project (§26, §32).
 *
 *  Writing a release a person accepted into the project (§26, §32).
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

//! Writing a release a person accepted into the project (§26, §32).
//!
//! The bridge between [`vcw_metadata`], which knows what Discogs and
//! MusicBrainz say, and [`vcw_project`], which knows what this record is. It
//! lives in `vcw-core` because it is the only crate that depends on both, and
//! because deciding *which* provider track is which project track is
//! application behaviour: a shell command that worked it out would be a second
//! answer the moment `vcw release` needs the same one.
//!
//! # The matching rule, and why it is this dull
//!
//! A provider track carries a position string and
//! [`positions::read`](vcw_metadata::positions::read) has already resolved it
//! into a side and a number. A project track carries a side and a number
//! because that is what §29 makes it. So the match is exact: `A3` names the
//! third track on side A, and nothing else.
//!
//! No duration matching, no fuzzy titles. §26's confidence-weighted
//! identification is Phase 2 and lives in `vcw-identify`; what this does is the
//! part that is not a guess. The cost of the dull rule is visible rather than
//! hidden: [`Applied::unmatched`] lists the provider's positions that named no
//! track, and [`Applied::unnamed`] lists the project's tracks the provider did
//! not cover, so a tracklist that does not line up is reported instead of being
//! forced.
//!
//! # Where the sides come from, and why the release wins
//!
//! The exact rule above is only honest if the project's side letters mean
//! something, and until a release is accepted they very often do not. Capture
//! writes one side because one capture is one take, so a person who recorded a
//! double album in a single pass has seventeen tracks called `A1` to `A17`.
//! That is not a layout the record could have: a 12-inch side holds about
//! twenty-two minutes, and seventeen tracks over an hour do not fit on one
//! face. The old behaviour matched `A1` to `A4` against a `A`/`B`/`C`/`D`
//! tracklist, named four tracks, and reported the other thirteen as a
//! disappointment - which blamed the provider for being right.
//!
//! So `relay` runs first. When the release's tracklist has exactly as many
//! tracks as the project does and every one of them resolved to a position, the
//! release's layout is adopted: the sides it names are created against the
//! capture the tracks already live on, and each track is moved to the side its
//! ordinal falls on. §31 allows this because two faces sharing one capture is
//! the case the schema was built for - `sides.capture_id` is deliberately not
//! unique - and [`vcw_project::track::move_to_side`] carries the boundaries
//! across unchanged, because they are frames of the same audio either way.
//!
//! A person chose this release from a list of candidates, which is §26's
//! confirmation. Having confirmed it, its tracklist is the better authority on
//! how many faces this record has than a side letter nothing has yet had cause
//! to set. Equal counts is the whole condition, and it is a strong one: a
//! tracklist that does not have the same number of tracks is not this pressing,
//! and nothing moves.
//!
//! # What accepting does not overwrite
//!
//! A track a person has marked confirmed is left alone. Accepting a release is
//! a person saying "this is the record"; it is not a person saying "throw away
//! the titles I typed". The release row itself *is* overwritten, because every
//! field on it is one the provider is authoritative about - with two
//! exceptions, `composer` and `comments`, which no provider reports and which
//! are therefore a person's own.

use vcw_metadata::release::Release;
use vcw_project::error::Result;
use vcw_project::{Project, release, side, track};

/// What accepting a release wrote, and what it could not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// The track rows retitled, in the order they were written.
    pub tracks: Vec<i64>,
    /// Tracks skipped because a person had already confirmed them.
    pub confirmed: Vec<i64>,
    /// Provider positions that named no track in this project.
    ///
    /// The usual cause is a side that has not been recorded yet, which is
    /// normal in the middle of a rip and worth saying rather than hiding.
    pub unmatched: Vec<String>,
    /// Project tracks the provider's tracklist did not cover, as §29 positions.
    pub unnamed: Vec<String>,
    /// Tracks the release's own layout moved to another side, as their new
    /// §29 positions.
    ///
    /// Empty when the project was already laid out the way the release
    /// describes, which is the case once a release has been accepted once.
    pub relaid: Vec<String>,
}

impl Applied {
    /// Whether the tracklist lined up with the project exactly.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.unmatched.is_empty() && self.unnamed.is_empty()
    }
}

/// Writes a release into the project and names what tracks it can.
///
/// Sets [`release::Record::confirmed`], because a person choosing a candidate
/// from a list is §26's confirmation. `composer` and `comments` are carried
/// over from what was already there for the reason in the module header.
///
/// # Errors
///
/// If the project is read-only or a row will not read or write.
pub fn accept(project: &mut Project, found: &Release) -> Result<Applied> {
    let existing = release::ensure(project)?;
    let record = release::Record {
        album: found.album.clone(),
        album_artist: found.album_artist.clone(),
        year: found.year,
        genres: found.genres.clone(),
        label: found.label.clone(),
        catalog: found.catalog.clone(),
        country: found.country.clone(),
        barcode: found.barcode.clone(),
        // Not the provider's, because no provider reports them.
        composer: existing.composer.clone(),
        comments: existing.comments.clone(),
        // A provider that lists media knows how many discs there are better
        // than a default does; one that lists none leaves the count alone
        // rather than setting it to zero.
        discs: if found.media.is_empty() {
            existing.discs
        } else {
            u32::try_from(found.media.len()).unwrap_or(existing.discs)
        },
        // §29's presentation, which is a property of how a person wants to see
        // this record and not something a provider has an opinion about.
        numbering: existing.numbering,
        musicbrainz_id: found.musicbrainz_id.clone(),
        discogs_id: found.discogs_id.clone(),
        confirmed: true,
        // The operator's, not the provider's, and not derivable from either: a
        // Discogs release says nothing about whether this copy is to be folded
        // to mono on export or played through a curve. Carried over for the
        // same reason as composer and comments, and §26's reason besides -
        // identification does not get to silently undo what a person stated.
        is_mono: existing.is_mono,
        riaa_eq: existing.riaa_eq,
        updated_at: existing.updated_at,
    };
    release::store(project, &record)?;
    let relaid = relay(project, found)?;
    let mut applied = name_tracks(project, found)?;
    applied.relaid = relaid;
    Ok(applied)
}

/// Lays the project's tracks out the way the release says the record is cut.
///
/// Returns the new positions of the tracks that moved, and an empty vector
/// when nothing did - which covers every case the condition does not hold for,
/// because this is a correction and not a demand. See the module header for
/// why the release is the authority on the layout.
///
/// Nothing moves unless all of:
///
/// - every provider track resolved to a position, so there is a full layout to
///   adopt rather than a partial one;
/// - the release lists exactly as many tracks as the project holds;
/// - the project is not already laid out that way;
/// - every side holding tracks points at the same capture, since a track's
///   boundaries are frames into its side's audio and §31 only lets it move
///   within one recording;
/// - no side the release names is already attached to a *different* capture,
///   which would mean that face was recorded separately and the counts lining
///   up was a coincidence.
fn relay(project: &mut Project, found: &Release) -> Result<Vec<String>> {
    let mut wanted: Vec<vcw_types::vinyl::Position> = Vec::new();
    for medium in &found.media {
        for entry in &medium.tracks {
            let Some(position) = entry.resolved else {
                return Ok(Vec::new());
            };
            wanted.push(position);
        }
    }
    let have = track::listing(project.conn())?;
    if wanted.is_empty() || wanted.len() != have.len() {
        return Ok(Vec::new());
    }
    if have
        .iter()
        .zip(&wanted)
        .all(|((side, row), want)| *side == want.side && row.number == want.number)
    {
        return Ok(Vec::new());
    }

    // One capture under every track, and no side the release names already
    // spoken for by another.
    let sides = side::list(project.conn())?;
    let mut capture: Option<i64> = None;
    for record in &sides {
        if track::tracks_of(project.conn(), record.id)?.is_empty() {
            continue;
        }
        match (capture, record.capture) {
            (_, None) => return Ok(Vec::new()),
            (None, Some(id)) => capture = Some(id),
            (Some(already), Some(id)) if already == id => {}
            _ => return Ok(Vec::new()),
        }
    }
    let Some(capture) = capture else {
        return Ok(Vec::new());
    };
    for record in &sides {
        if wanted.iter().any(|want| want.side == record.side)
            && record.capture.is_some_and(|id| id != capture)
        {
            return Ok(Vec::new());
        }
    }

    // Create the faces before moving anything on to them. Attaching a side
    // that already points at this capture is the same row written twice.
    let mut faces: Vec<vcw_types::vinyl::Side> = wanted.iter().map(|want| want.side).collect();
    faces.sort_unstable();
    faces.dedup();
    for face in faces {
        side::attach(project, face, capture)?;
    }

    // In tracklist order, which is timeline order, which is the order
    // `move_to_side` renumbers each face into. The number therefore does not
    // need setting: moving the first five tracks of side B on to it in turn
    // numbers them one to five.
    let mut relaid = Vec::new();
    for ((side, row), want) in have.iter().zip(&wanted) {
        if *side == want.side {
            continue;
        }
        track::move_to_side(project, row.id, want.side)?;
        relaid.push(want.alpha());
    }
    Ok(relaid)
}

/// The tracklist half: match on side and number, write titles.
fn name_tracks(project: &mut Project, found: &Release) -> Result<Applied> {
    // Every project track, indexed by the thing a provider position resolves
    // to. Read once: a lookup per provider track would be a query per track.
    let mut by_position: std::collections::BTreeMap<(vcw_types::vinyl::Side, u32), track::Record> =
        std::collections::BTreeMap::new();
    for record in side::list(project.conn())? {
        for row in track::tracks_of(project.conn(), record.id)? {
            by_position.insert((record.side, row.number), row);
        }
    }

    let mut applied = Applied::default();
    let mut covered: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    let mut updates: Vec<(i64, track::Update)> = Vec::new();

    for medium in &found.media {
        for entry in &medium.tracks {
            let Some(position) = entry.resolved else {
                applied.unmatched.push(entry.position.clone());
                continue;
            };
            let Some(row) = by_position.get(&(position.side, position.number)) else {
                applied.unmatched.push(entry.position.clone());
                continue;
            };
            covered.insert(row.id);
            if row.confirmed {
                applied.confirmed.push(row.id);
                continue;
            }
            updates.push((
                row.id,
                track::Update {
                    title: Some(entry.title.clone()),
                    // An entry with no artist of its own clears the column back
                    // to the release's, which is what `""` means here and what
                    // is right for the common case of a record by one artist.
                    artist: Some(entry.artist.clone().unwrap_or_default()),
                    ..track::Update::default()
                },
            ));
        }
    }

    // Collected first and written after, because `track::update` takes the
    // project mutably and the index above borrows it.
    for (id, change) in updates {
        track::update(project, id, &change)?;
        applied.tracks.push(id);
    }

    for ((side, number), row) in &by_position {
        if !covered.contains(&row.id) {
            applied.unnamed.push(format!("{}{}", side.letter(), number));
        }
    }
    Ok(applied)
}
