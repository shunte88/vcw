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
//! No duration matching, no fuzzy titles, no reordering. §26's confidence-
//! weighted identification is Phase 2 and lives in `vcw-identify`; what this
//! does is the part that is not a guess. The cost of the dull rule is visible
//! rather than hidden: [`Applied::unmatched`] lists the provider's positions
//! that named no track, and [`Applied::unnamed`] lists the project's tracks the
//! provider did not cover, so a tracklist that does not line up is reported
//! instead of being forced.
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
        updated_at: existing.updated_at,
    };
    release::store(project, &record)?;
    name_tracks(project, found)
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
