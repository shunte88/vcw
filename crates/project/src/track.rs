/*
 *  track.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Track boundaries, tracks, and the non-destructive edits of §31.
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

//! Track boundaries, tracks, and the non-destructive edits of §31.
//!
//! # A track is its two boundaries
//!
//! `tracks` has no frame columns. A track row points at two `track_boundaries`
//! rows and takes its extent from them, so there is no second copy of a frame
//! number anywhere and nothing can disagree with itself. Moving a boundary moves
//! whichever tracks it bounds, without a cascade, because there is nothing to
//! cascade to. Splitting adds one boundary and one track; merging drops one of
//! each.
//!
//! # Nothing here touches audio
//!
//! Every verb in this module writes to `sides`, `track_boundaries` and `tracks`.
//! None of them writes, rewrites or deletes a `sampleblocks` row: an edit is a
//! statement about where the music is, not a change to the recording, which is
//! §4.1's promise and the first half of WP-13's exit criterion. The test that
//! asserts it fingerprints the block tables before and after every verb.
//!
//! # Locking
//!
//! [`Provenance::User`] is locked by definition ([`Provenance::is_locked`]), and
//! §24 says a boundary a person placed is never moved by analysis. That is
//! enforced here rather than in each caller: [`move_boundary`] and
//! [`delete_boundary`] refuse a locked row with [`Error::BoundaryLocked`], and
//! [`move_boundary_forced`] is the deliberately awkward name for the operator's
//! own override. The `locked` column exists as well as the provenance so a person
//! can pin a boundary a *detector* found - "that one is right, stop reconsidering
//! it" - without claiming to have placed it.

use rusqlite::{Connection, OptionalExtension, params};
use vcw_types::observation::{Edge, Evidence, Provenance};
use vcw_types::vinyl::{Numbering, Position, Side};

use crate::error::{Error, Result};
use crate::side;
use crate::sqlite::Project;

/// A boundary row, as read back.
#[derive(Debug, Clone, PartialEq)]
pub struct Boundary {
    /// Row id, referenced by the tracks it bounds.
    pub id: i64,
    /// The side it is on.
    pub side_id: i64,
    /// The frame it sits at, in that side's capture timeline.
    pub at_frame: u64,
    /// Which way the audio crosses it.
    pub edge: Edge,
    /// How much to trust it, in `0.0..=1.0`.
    pub confidence: f32,
    /// What decided its position.
    pub provenance: Provenance,
    /// Every provenance that reported it, in order.
    pub sources: Vec<Provenance>,
    /// The measurements behind it.
    pub evidence: Vec<Evidence>,
    /// Whether analysis may move it.
    pub locked: bool,
    /// Unix seconds at creation.
    pub created_at: i64,
    /// Unix seconds at the last change.
    pub updated_at: i64,
}

impl Boundary {
    /// How many distinct detectors reported this boundary.
    ///
    /// The number §24's "n detectors agree" is about, and what a promotion policy
    /// thresholds on. A boundary a person placed reports zero sources and is not
    /// weaker for it.
    #[must_use]
    pub fn agreement(&self) -> usize {
        self.sources.len()
    }

    /// Looks a measurement up by name.
    #[must_use]
    pub fn measurement(&self, name: &str) -> Option<f64> {
        self.evidence
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.value)
    }
}

/// What to write for a new boundary.
///
/// A builder rather than eight arguments, because the two callers want different
/// halves of it: a person placing a boundary sets `at`, `edge` and nothing else,
/// and [`crate::release`]-style adoption from detection fills in all of it.
#[derive(Debug, Clone, PartialEq)]
pub struct NewBoundary {
    /// The frame it sits at.
    pub at: u64,
    /// Which way the audio crosses it.
    pub edge: Edge,
    /// How much to trust it.
    pub confidence: f32,
    /// What decided the position.
    pub provenance: Provenance,
    /// Every provenance that reported it.
    pub sources: Vec<Provenance>,
    /// The measurements behind it.
    pub evidence: Vec<Evidence>,
    /// Whether analysis may move it. [`Provenance::User`] forces this on.
    pub locked: bool,
}

impl NewBoundary {
    /// A boundary a person placed: full confidence, locked, no evidence.
    #[must_use]
    pub fn by_user(at: u64, edge: Edge) -> Self {
        Self {
            at,
            edge,
            confidence: 1.0,
            provenance: Provenance::User,
            sources: Vec::new(),
            evidence: Vec::new(),
            locked: true,
        }
    }

    /// A boundary a detector found.
    #[must_use]
    pub fn detected(at: u64, edge: Edge, confidence: f32, provenance: Provenance) -> Self {
        Self {
            at,
            edge,
            confidence: confidence.clamp(0.0, 1.0),
            provenance,
            sources: vec![provenance],
            evidence: Vec::new(),
            locked: false,
        }
    }

    /// Records which provenances reported it.
    #[must_use]
    pub fn with_sources(mut self, sources: Vec<Provenance>) -> Self {
        self.sources = sources;
        self
    }

    /// Records the measurements behind it.
    #[must_use]
    pub fn with_evidence(mut self, evidence: Vec<Evidence>) -> Self {
        self.evidence = evidence;
        self
    }

    /// Pins it against analysis without claiming a person placed it.
    #[must_use]
    pub const fn locked(mut self) -> Self {
        self.locked = true;
        self
    }
}

/// A track row, with the frames its boundaries put it at.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Row id.
    pub id: i64,
    /// The side it is on.
    pub side_id: i64,
    /// Its one-based number within the side, which is §29's alpha position.
    pub number: u32,
    /// The boundary it starts at.
    pub start_boundary: i64,
    /// The boundary it ends at.
    pub end_boundary: i64,
    /// The frame it starts at, from that boundary.
    pub start: u64,
    /// The frame it ends at, from that boundary.
    pub end: u64,
    /// Title, empty until something fills it in.
    pub title: String,
    /// Track artist, or `None` to take the release's (§32).
    ///
    /// The common case is `None`: on most records every track is by the album
    /// artist, and storing that name once per track would be storing it n times.
    /// A compilation is what this is for.
    pub artist: Option<String>,
    /// Composer, or `None` to take the release's (§32).
    pub composer: Option<String>,
    /// Free text a person added, or `None` for none.
    pub comments: Option<String>,
    /// The recording this track was identified as (§25/§28).
    pub musicbrainz_id: Option<String>,
    /// Whether a person has confirmed the metadata.
    pub confirmed: bool,
    /// Unix seconds at the last change.
    pub updated_at: i64,
}

impl Record {
    /// How long the track is, in frames.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// How long the track is, in seconds at a given rate.
    #[must_use]
    pub fn seconds(&self, rate: vcw_types::SampleRate) -> f64 {
        let hz = f64::from(rate.hz());
        if hz <= 0.0 {
            return 0.0;
        }
        self.frames() as f64 / hz
    }

    /// Whether a frame falls inside the track.
    #[must_use]
    pub const fn contains(&self, frame: u64) -> bool {
        frame >= self.start && frame < self.end
    }

    /// Its §29 position on a side.
    #[must_use]
    pub const fn position(&self, side: Side) -> Position {
        Position::new(side, self.number)
    }

    /// The track artist, falling back to the release's.
    ///
    /// What a tag writer wants (§33): every track needs an artist in the file
    /// even though most rows do not store one.
    #[must_use]
    pub fn artist_or<'a>(&'a self, release_artist: &'a str) -> &'a str {
        self.artist.as_deref().unwrap_or(release_artist)
    }

    /// The composer, falling back to the release's.
    #[must_use]
    pub fn composer_or<'a>(&'a self, release_composer: &'a str) -> &'a str {
        self.composer.as_deref().unwrap_or(release_composer)
    }
}

/// Adds a boundary to a side.
///
/// Idempotent on `(side, frame, edge)`: the schema's unique constraint means
/// re-detecting the same boundary updates the existing row rather than doubling
/// it, which is what re-analysis does on every pass. Updating will not unlock a
/// locked row and will not move it, since it is already where it is.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if the side is absent, or if the write fails.
pub fn add_boundary(project: &mut Project, side: Side, new: &NewBoundary) -> Result<i64> {
    let record = side::require(project.conn(), side)?;
    add_boundary_to(project, record.id, new)
}

/// Adds a boundary to a side already looked up.
///
/// # Errors
///
/// If the write fails.
pub fn add_boundary_to(project: &mut Project, side_id: i64, new: &NewBoundary) -> Result<i64> {
    let now = crate::now();
    let locked = new.locked || new.provenance.is_locked();
    let conn = project.conn_mut();
    conn.execute(
        "INSERT INTO track_boundaries
             (side_id, at_frame, edge, confidence, provenance, sources, evidence,
              locked, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT (side_id, at_frame, edge) DO UPDATE SET
             -- A locked row keeps its own attribution: §24 reserves the position
             -- *and* the credit for whoever pinned it, so a detector arriving at
             -- the same frame must not turn a user boundary into a silence one.
             -- Every unqualified name here reads the row as it was, so the order
             -- of these assignments does not matter.
             confidence = CASE WHEN locked THEN confidence ELSE excluded.confidence END,
             provenance = CASE WHEN locked THEN provenance ELSE excluded.provenance END,
             -- Sources and evidence are recorded either way, because how many
             -- detectors also found it is worth knowing about a boundary a
             -- person confirmed.
             sources    = excluded.sources,
             evidence   = excluded.evidence,
             -- A second opinion may pin a boundary but never unpin one.
             locked     = locked OR excluded.locked,
             updated_at = excluded.updated_at",
        params![
            side_id,
            frame_to_sql(new.at),
            new.edge.as_str(),
            f64::from(new.confidence.clamp(0.0, 1.0)),
            new.provenance.as_str(),
            write_sources(&new.sources),
            write_evidence(&new.evidence),
            i64::from(locked),
            now,
        ],
    )?;
    let id: i64 = conn.query_row(
        "SELECT boundary_id FROM track_boundaries
           WHERE side_id = ?1 AND at_frame = ?2 AND edge = ?3",
        params![side_id, frame_to_sql(new.at), new.edge.as_str()],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// Moves a boundary, and with it whichever tracks it bounds.
///
/// The whole point of a track being its boundaries: nothing else has to be
/// updated, because nothing else holds the frame.
///
/// # Errors
///
/// [`Error::NoSuchBoundary`] if it is absent, [`Error::BoundaryLocked`] if a person
/// placed or pinned it (§24).
pub fn move_boundary(project: &mut Project, boundary_id: i64, to: u64) -> Result<()> {
    let existing = require_boundary(project.conn(), boundary_id)?;
    if existing.locked {
        return Err(Error::BoundaryLocked {
            boundary_id,
            at_frame: existing.at_frame,
        });
    }
    write_position(project, &existing, to)
}

/// Moves a boundary whether or not it is locked, and takes the lock with it.
///
/// The operator's own override. Dragging a boundary one has already confirmed is a
/// normal thing to do and it stays confirmed afterwards - a person moving a
/// boundary is a person placing it - so this sets [`Provenance::User`] and leaves
/// the row locked. Analysis must never call this; that is what [`move_boundary`]
/// is for, and why the two are not one function with a flag.
///
/// # Errors
///
/// [`Error::NoSuchBoundary`] if it is absent.
pub fn move_boundary_forced(project: &mut Project, boundary_id: i64, to: u64) -> Result<()> {
    let existing = require_boundary(project.conn(), boundary_id)?;
    write_position(project, &existing, to)?;
    project.conn_mut().execute(
        "UPDATE track_boundaries
            SET provenance = ?2, confidence = 1.0, locked = 1, updated_at = ?3
          WHERE boundary_id = ?1",
        params![boundary_id, Provenance::User.as_str(), crate::now()],
    )?;
    Ok(())
}

/// Sets or clears a boundary's lock (§24).
///
/// Clearing it is how an operator hands a boundary back to analysis after deciding
/// their own guess was worse. The provenance is left alone: it is a record of what
/// put the boundary there, which unlocking does not change.
///
/// # Errors
///
/// [`Error::NoSuchBoundary`] if it is absent.
pub fn set_lock(project: &mut Project, boundary_id: i64, locked: bool) -> Result<()> {
    require_boundary(project.conn(), boundary_id)?;
    project.conn_mut().execute(
        "UPDATE track_boundaries SET locked = ?2, updated_at = ?3 WHERE boundary_id = ?1",
        params![boundary_id, i64::from(locked), crate::now()],
    )?;
    Ok(())
}

/// Deletes a boundary that no track depends on.
///
/// # Errors
///
/// [`Error::NoSuchBoundary`] if it is absent, [`Error::BoundaryLocked`] if a person
/// placed it, [`Error::BoundaryInUse`] if a track is bounded by it. That last one
/// is not a limitation: deleting one end of a track would leave a track with one
/// end, so the edit the caller wants is [`merge`] or [`remove`].
pub fn delete_boundary(project: &mut Project, boundary_id: i64) -> Result<()> {
    let existing = require_boundary(project.conn(), boundary_id)?;
    if existing.locked {
        return Err(Error::BoundaryLocked {
            boundary_id,
            at_frame: existing.at_frame,
        });
    }
    if let Some(track_id) = bounded_track(project.conn(), boundary_id)? {
        return Err(Error::BoundaryInUse {
            boundary_id,
            track_id,
        });
    }
    project.conn_mut().execute(
        "DELETE FROM track_boundaries WHERE boundary_id = ?1",
        params![boundary_id],
    )?;
    Ok(())
}

/// Every boundary on a side, in timeline order.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if the side is absent.
pub fn boundaries(conn: &Connection, side: Side) -> Result<Vec<Boundary>> {
    let record = side::require(conn, side)?;
    boundaries_of(conn, record.id)
}

/// Every boundary on a side already looked up, in timeline order.
///
/// # Errors
///
/// If the query fails.
pub fn boundaries_of(conn: &Connection, side_id: i64) -> Result<Vec<Boundary>> {
    let mut stmt = conn.prepare(
        "SELECT boundary_id, side_id, at_frame, edge, confidence, provenance, sources,
                evidence, locked, created_at, updated_at
           FROM track_boundaries WHERE side_id = ?1
          -- Two boundaries can share a frame, where one track ends and the next
          -- begins. The end comes first, because that is the order the audio
          -- crosses them. Spelled out rather than left to `edge`'s collation.
          ORDER BY at_frame, CASE edge WHEN 'end' THEN 0 ELSE 1 END",
    )?;
    let rows = stmt
        .query_map(params![side_id], read_boundary)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Reads one boundary by row id.
///
/// # Errors
///
/// If the query fails.
pub fn boundary(conn: &Connection, boundary_id: i64) -> Result<Option<Boundary>> {
    let found = conn
        .query_row(
            "SELECT boundary_id, side_id, at_frame, edge, confidence, provenance, sources,
                    evidence, locked, created_at, updated_at
               FROM track_boundaries WHERE boundary_id = ?1",
            params![boundary_id],
            read_boundary,
        )
        .optional()?;
    Ok(found)
}

/// Adds a track spanning two frames, creating the boundaries it needs.
///
/// The number is assigned after the side's existing tracks, and then the whole
/// side is renumbered in timeline order, so adding a track in a gap between two
/// others gets the number its position deserves rather than the next one free.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if the side is absent, [`Error::OutsideTrack`] with the
/// frames swapped if `end` is not after `start`, or if a write fails.
pub fn add_track(project: &mut Project, side: Side, start: u64, end: u64) -> Result<i64> {
    let record = side::require(project.conn(), side)?;
    if end <= start {
        // The one degenerate case: an empty or backwards track. Reported against
        // the side rather than a track, since there is no track yet.
        return Err(Error::OutsideTrack {
            track_id: 0,
            at: end,
            start,
            end,
        });
    }
    let start_id = add_boundary_to(
        project,
        record.id,
        &NewBoundary::by_user(start, Edge::Start),
    )?;
    let end_id = add_boundary_to(project, record.id, &NewBoundary::by_user(end, Edge::End))?;
    let id = insert_track(project, record.id, start_id, end_id)?;
    renumber_side(project, record.id)?;
    Ok(id)
}

/// Adds a track between two boundaries that already exist.
///
/// What adoption from detection uses: the boundaries were written from a
/// [`Provenance`] with its evidence, and pairing them into tracks must not replace
/// them with user-placed ones.
///
/// # Errors
///
/// [`Error::NoSuchBoundary`] if either is absent, or if they are on different
/// sides, or if a write fails.
pub fn add_track_between(project: &mut Project, start_id: i64, end_id: i64) -> Result<i64> {
    let start = require_boundary(project.conn(), start_id)?;
    let end = require_boundary(project.conn(), end_id)?;
    if start.side_id != end.side_id {
        return Err(Error::NoSuchBoundary {
            boundary_id: end_id,
        });
    }
    if end.at_frame <= start.at_frame {
        return Err(Error::OutsideTrack {
            track_id: 0,
            at: end.at_frame,
            start: start.at_frame,
            end: end.at_frame,
        });
    }
    let id = insert_track(project, start.side_id, start_id, end_id)?;
    renumber_side(project, start.side_id)?;
    Ok(id)
}

/// Every track on a side, in timeline order with numbers to match.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if the side is absent.
pub fn tracks(conn: &Connection, side: Side) -> Result<Vec<Record>> {
    let record = side::require(conn, side)?;
    tracks_of(conn, record.id)
}

/// Every track on a side already looked up, in timeline order.
///
/// # Errors
///
/// If the query fails.
pub fn tracks_of(conn: &Connection, side_id: i64) -> Result<Vec<Record>> {
    let mut stmt = conn.prepare(TRACK_SELECT)?;
    let rows = stmt
        .query_map(params![side_id], read_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Reads one track by row id.
///
/// # Errors
///
/// If the query fails.
pub fn track(conn: &Connection, track_id: i64) -> Result<Option<Record>> {
    let found = conn
        .query_row(TRACK_BY_ID, params![track_id], read_track)
        .optional()?;
    Ok(found)
}

/// Every track in the project, side by side, in playing order.
///
/// Paired with the side each one is on, because the number alone does not identify
/// a track across a double album and the export layer needs both (§33).
///
/// # Errors
///
/// If the query fails.
pub fn listing(conn: &Connection) -> Result<Vec<(Side, Record)>> {
    let mut all = Vec::new();
    for side in side::list(conn)? {
        for record in tracks_of(conn, side.id)? {
            all.push((side.side, record));
        }
    }
    Ok(all)
}

/// Every track edge on the sides a capture was recorded to, ascending.
///
/// Both ends of every track, not just the starts, because §21's `SKIP BACK` from
/// the middle of the last track should land at its top and `SKIP FORWARD` from
/// there should land at its end rather than run to the end of the side. Adjacent
/// tracks share a frame - one's end is the next one's start - so the list is
/// deduplicated, and a skip between them moves once.
///
/// Frames, and one flat list rather than one per side, because that is the shape
/// `vcw_core::playback::Audition::marks` wants: a capture can hold two faces
/// (§21) and a skip does not stop at the join between them any more than the
/// needle does.
///
/// Empty for a capture nothing has been analyzed from, which is the state a skip
/// falls back to a fixed step in.
///
/// # Errors
///
/// If a query fails.
pub fn edges_of_capture(conn: &Connection, capture_id: i64) -> Result<Vec<u64>> {
    let mut frames = Vec::new();
    for record in side::for_capture(conn, capture_id)? {
        for track in tracks_of(conn, record.id)? {
            frames.push(track.start);
            frames.push(track.end);
        }
    }
    frames.sort_unstable();
    frames.dedup();
    Ok(frames)
}

/// Splits a track at a frame, giving two tracks that meet there.
///
/// Two new boundaries and one new track, with no gap: the first track now ends at
/// the frame and the second begins there. Two rows for one instant rather than one
/// shared row, because `UNIQUE (start_boundary)` and `UNIQUE (end_boundary)` mean
/// a boundary bounds at most one track - and because an end and a start are not
/// interchangeable anyway, since the splitter pads them differently (§33). The two
/// are distinguishable by [`Edge`] and are reported end-first, which is the order
/// the audio crosses them.
///
/// Both are placed as the operator's, since asking for a split is placing a
/// boundary. [`merge`] will still put the two tracks back together - a lock binds
/// analysis, not the person who set it - but the boundary stays behind unless
/// [`set_lock`] clears it first.
///
/// # Errors
///
/// [`Error::NoSuchTrack`] if the track is absent, [`Error::OutsideTrack`] if the
/// frame is not strictly inside it.
pub fn split(project: &mut Project, track_id: i64, at: u64) -> Result<i64> {
    let existing = require_track(project.conn(), track_id)?;
    if at <= existing.start || at >= existing.end {
        return Err(Error::OutsideTrack {
            track_id,
            at,
            start: existing.start,
            end: existing.end,
        });
    }
    // One boundary, marked as an end, shared by the track that now finishes there
    // and the track that now starts there.
    let cut = add_boundary_to(
        project,
        existing.side_id,
        &NewBoundary::by_user(at, Edge::End),
    )?;
    let start_of_second = add_boundary_to(
        project,
        existing.side_id,
        &NewBoundary::by_user(at, Edge::Start),
    )?;
    project.conn_mut().execute(
        "UPDATE tracks SET end_boundary = ?2, updated_at = ?3 WHERE track_id = ?1",
        params![track_id, cut, crate::now()],
    )?;
    let second = insert_track(
        project,
        existing.side_id,
        start_of_second,
        existing.end_boundary,
    )?;
    renumber_side(project, existing.side_id)?;
    Ok(second)
}

/// Merges two adjacent tracks into the first, dropping the boundary between them.
///
/// The surviving track keeps the left one's metadata and row id, because that is
/// what merging a mis-split track means: the second half was never a track, so its
/// empty title should not win.
///
/// A locked boundary between them is no obstacle, unlike in [`move_boundary`]:
/// §24 protects a boundary from *analysis*, and an operator merging two tracks has
/// asked for this in as many words. The boundary itself is kept, though, where a
/// person placed it - "there is a transition here, and these are still one track"
/// is a coherent thing to say about a segue, and [`mod@crate::validate`] does not
/// count a boundary that bounds nothing as a fault.
///
/// # Errors
///
/// [`Error::NoSuchTrack`] if either is absent, [`Error::NotAdjacent`] if they are
/// on different sides or something sits between them.
pub fn merge(project: &mut Project, left_id: i64, right_id: i64) -> Result<()> {
    let left = require_track(project.conn(), left_id)?;
    let right = require_track(project.conn(), right_id)?;
    if left.side_id != right.side_id || left.end > right.start {
        return Err(Error::NotAdjacent {
            left: left_id,
            right: right_id,
        });
    }
    let between = tracks_of(project.conn(), left.side_id)?
        .into_iter()
        .any(|t| {
            t.start >= left.end && t.end <= right.start && t.id != left_id && t.id != right_id
        });
    if between {
        return Err(Error::NotAdjacent {
            left: left_id,
            right: right_id,
        });
    }
    let inner = [left.end_boundary, right.start_boundary];
    // The right row has to go first: `UNIQUE (end_boundary)` means the left track
    // cannot claim the end boundary while the right one still holds it.
    project
        .conn_mut()
        .execute("DELETE FROM tracks WHERE track_id = ?1", params![right_id])?;
    project.conn_mut().execute(
        "UPDATE tracks SET end_boundary = ?2, updated_at = ?3 WHERE track_id = ?1",
        params![left_id, right.end_boundary, crate::now()],
    )?;
    // The boundaries that bounded the join are now unreferenced, unless a person
    // locked them, in which case they stay as a marker of where the join was.
    for boundary_id in inner {
        if bounded_track(project.conn(), boundary_id)?.is_none() {
            let existing = require_boundary(project.conn(), boundary_id)?;
            if !existing.locked {
                project.conn_mut().execute(
                    "DELETE FROM track_boundaries WHERE boundary_id = ?1",
                    params![boundary_id],
                )?;
            }
        }
    }
    renumber_side(project, left.side_id)?;
    Ok(())
}

/// Deletes a track, and the boundaries nothing else needs.
///
/// The audio stays. A deleted track is a statement that the frames between those
/// boundaries are not a track - run-out groove, a false positive, a spoken
/// introduction someone does not want split out - and §4.1 means that statement
/// cannot cost samples.
///
/// # Errors
///
/// [`Error::NoSuchTrack`] if it is absent.
pub fn remove(project: &mut Project, track_id: i64) -> Result<()> {
    let existing = require_track(project.conn(), track_id)?;
    project
        .conn_mut()
        .execute("DELETE FROM tracks WHERE track_id = ?1", params![track_id])?;
    for boundary_id in [existing.start_boundary, existing.end_boundary] {
        if bounded_track(project.conn(), boundary_id)?.is_none() {
            let boundary = require_boundary(project.conn(), boundary_id)?;
            if !boundary.locked {
                project.conn_mut().execute(
                    "DELETE FROM track_boundaries WHERE boundary_id = ?1",
                    params![boundary_id],
                )?;
            }
        }
    }
    renumber_side(project, existing.side_id)?;
    Ok(())
}

/// What to change about a track's metadata (§32).
///
/// Every field is `None` for "leave it alone", so a UI that edits one cell sends
/// one field. For the four nullable columns, `Some("")` clears the row back to
/// NULL - which is the same statement as an empty string in a column whose NULL
/// means "take the release's", so one spelling is enough and a separate clear flag
/// per field is not needed. `title` is the exception: it is NOT NULL, and an empty
/// title is an untitled track rather than an inherited one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Update {
    /// The track title. Empty means untitled.
    pub title: Option<String>,
    /// The track artist. Empty clears it back to the release's.
    pub artist: Option<String>,
    /// The composer. Empty clears it back to the release's.
    pub composer: Option<String>,
    /// Free text. Empty clears it.
    pub comments: Option<String>,
    /// The recording it was identified as. Empty clears it.
    pub musicbrainz_id: Option<String>,
    /// Whether the metadata is confirmed.
    pub confirmed: Option<bool>,
}

impl Update {
    /// An update that sets the title and nothing else.
    #[must_use]
    pub fn title(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::default()
        }
    }

    /// Whether this update would change nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// Applies a metadata update to a track.
///
/// # Errors
///
/// [`Error::NoSuchTrack`] if it is absent.
pub fn update(project: &mut Project, track_id: i64, change: &Update) -> Result<()> {
    let existing = require_track(project.conn(), track_id)?;
    project.conn_mut().execute(
        "UPDATE tracks SET title = ?2, artist = ?3, composer = ?4, comments = ?5,
                           musicbrainz_id = ?6, confirmed = ?7, updated_at = ?8
           WHERE track_id = ?1",
        params![
            track_id,
            change.title.clone().unwrap_or(existing.title),
            settle(change.artist.as_deref(), existing.artist),
            settle(change.composer.as_deref(), existing.composer),
            settle(change.comments.as_deref(), existing.comments),
            settle(change.musicbrainz_id.as_deref(), existing.musicbrainz_id),
            i64::from(change.confirmed.unwrap_or(existing.confirmed)),
            crate::now(),
        ],
    )?;
    Ok(())
}

/// Renumbers a side's tracks 1..n in timeline order.
///
/// Called by every verb that changes the set of tracks, so a caller only needs it
/// after moving a boundary past a neighbor - the one edit that can reorder tracks
/// without adding or removing any.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if the side is absent.
pub fn renumber(project: &mut Project, side: Side) -> Result<()> {
    let record = side::require(project.conn(), side)?;
    renumber_side(project, record.id)
}

/// Moves a track to another side of the same capture (§31).
///
/// Only meaningful when the two sides share a capture - both faces recorded in one
/// take - because a track's boundaries are frames into its side's audio and
/// pointing them at a different recording would not be the same track.
///
/// # Errors
///
/// [`Error::NoSuchTrack`] or [`Error::NoSuchSide`] if either is absent,
/// [`Error::DifferentCapture`] if the sides hold different audio.
pub fn move_to_side(project: &mut Project, track_id: i64, to: Side) -> Result<()> {
    let existing = require_track(project.conn(), track_id)?;
    let target = side::require(project.conn(), to)?;
    if target.id == existing.side_id {
        return Ok(());
    }
    let from =
        side::by_id(project.conn(), existing.side_id)?.ok_or(Error::NoSuchTrack { track_id })?;
    if from.capture.is_none() || from.capture != target.capture {
        return Err(Error::DifferentCapture {
            track_id,
            from: from.letter(),
            to: to.letter(),
        });
    }
    let now = crate::now();
    // The boundaries move with the track: they are the same frames of the same
    // audio, and the side is the only thing that was wrong about them.
    project.conn_mut().execute(
        "UPDATE track_boundaries SET side_id = ?2, updated_at = ?3
           WHERE boundary_id IN (?1, ?4)",
        params![
            existing.start_boundary,
            target.id,
            now,
            existing.end_boundary
        ],
    )?;
    let number = next_number(project.conn(), target.id)?;
    project.conn_mut().execute(
        "UPDATE tracks SET side_id = ?2, number = ?3, updated_at = ?4 WHERE track_id = ?1",
        params![track_id, target.id, number, now],
    )?;
    renumber_side(project, from.id)?;
    renumber_side(project, target.id)?;
    Ok(())
}

/// The §29 positions of every track in the project, rendered for display.
///
/// The one place the [`Numbering`] scheme is applied, since it is a presentation
/// choice rather than a fact about the record: the rows store side and number, and
/// this turns them into `A1` or `01` or `06` on the way out.
///
/// The running number restarts on each disc, not on each release, because a
/// disc is what a track number is numbered within - sides A and B are disc 1,
/// C and D are disc 2, and side C's first track is `01` again.
///
/// # Errors
///
/// If the query fails.
pub fn positions(conn: &Connection, numbering: Numbering) -> Result<Vec<(Record, String)>> {
    let mut rendered = Vec::new();
    let mut per_disc: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for (side, record) in listing(conn)? {
        let within_disc = per_disc
            .entry(side.disc())
            .and_modify(|n| *n += 1)
            .or_insert(1);
        let text = numbering.render(record.position(side), *within_disc);
        rendered.push((record, text));
    }
    Ok(rendered)
}

/// A track and the frames of its two boundaries.
///
/// The join is what makes a track's extent a derived fact rather than a stored
/// one, and the reason `tracks` has no frame columns to fall out of step with. A
/// macro rather than two constants because `concat!` takes literals: the columns
/// and the joins are written once and only the tail differs.
macro_rules! track_query {
    ($tail:literal) => {
        concat!(
            "SELECT t.track_id, t.side_id, t.number, t.start_boundary, t.end_boundary,
                    s.at_frame, e.at_frame, t.title, t.artist, t.composer, t.comments,
                    t.musicbrainz_id, t.confirmed, t.updated_at
               FROM tracks t
               JOIN track_boundaries s ON s.boundary_id = t.start_boundary
               JOIN track_boundaries e ON e.boundary_id = t.end_boundary ",
            $tail
        )
    };
}

const TRACK_SELECT: &str = track_query!("WHERE t.side_id = ?1 ORDER BY s.at_frame");
const TRACK_BY_ID: &str = track_query!("WHERE t.track_id = ?1");

fn insert_track(project: &mut Project, side_id: i64, start: i64, end: i64) -> Result<i64> {
    let number = next_number(project.conn(), side_id)?;
    let conn = project.conn_mut();
    conn.execute(
        "INSERT INTO tracks (side_id, number, start_boundary, end_boundary, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![side_id, number, start, end, crate::now()],
    )?;
    Ok(conn.last_insert_rowid())
}

fn next_number(conn: &Connection, side_id: i64) -> Result<u32> {
    let highest: Option<i64> = conn.query_row(
        "SELECT MAX(number) FROM tracks WHERE side_id = ?1",
        params![side_id],
        |r| r.get(0),
    )?;
    Ok(u32::try_from(highest.unwrap_or(0) + 1).unwrap_or(1))
}

fn renumber_side(project: &mut Project, side_id: i64) -> Result<()> {
    let ordered = tracks_of(project.conn(), side_id)?;
    let now = crate::now();
    let tx = project.conn_mut().transaction()?;
    // Out of the way first: `UNIQUE (side_id, number)` would be violated halfway
    // through any in-place renumber, and a deferred constraint would only move the
    // problem to commit time. Negative numbers cannot collide with the real ones.
    for (index, record) in ordered.iter().enumerate() {
        tx.execute(
            "UPDATE tracks SET number = ?2 WHERE track_id = ?1",
            params![record.id, -(index as i64 + 1)],
        )?;
    }
    for (index, record) in ordered.iter().enumerate() {
        tx.execute(
            "UPDATE tracks SET number = ?2, updated_at = ?3 WHERE track_id = ?1",
            params![record.id, index as i64 + 1, now],
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn write_position(project: &mut Project, existing: &Boundary, to: u64) -> Result<()> {
    project.conn_mut().execute(
        "UPDATE track_boundaries SET at_frame = ?2, updated_at = ?3 WHERE boundary_id = ?1",
        params![existing.id, frame_to_sql(to), crate::now()],
    )?;
    Ok(())
}

fn bounded_track(conn: &Connection, boundary_id: i64) -> Result<Option<i64>> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT track_id FROM tracks
              WHERE start_boundary = ?1 OR end_boundary = ?1 LIMIT 1",
            params![boundary_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found)
}

fn require_boundary(conn: &Connection, boundary_id: i64) -> Result<Boundary> {
    boundary(conn, boundary_id)?.ok_or(Error::NoSuchBoundary { boundary_id })
}

fn require_track(conn: &Connection, track_id: i64) -> Result<Record> {
    track(conn, track_id)?.ok_or(Error::NoSuchTrack { track_id })
}

fn read_boundary(row: &rusqlite::Row<'_>) -> rusqlite::Result<Boundary> {
    let at: i64 = row.get(2)?;
    let edge: String = row.get(3)?;
    let confidence: f64 = row.get(4)?;
    let provenance: String = row.get(5)?;
    let sources: String = row.get(6)?;
    let evidence: String = row.get(7)?;
    let locked: i64 = row.get(8)?;
    Ok(Boundary {
        id: row.get(0)?,
        side_id: row.get(1)?,
        at_frame: at.max(0) as u64,
        edge: read_edge(&edge),
        confidence: confidence as f32,
        provenance: read_provenance(&provenance),
        sources: read_sources(&sources),
        evidence: read_evidence(&evidence),
        locked: locked != 0,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn read_track(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
    let number: i64 = row.get(2)?;
    let start: i64 = row.get(5)?;
    let end: i64 = row.get(6)?;
    let confirmed: i64 = row.get(12)?;
    Ok(Record {
        id: row.get(0)?,
        side_id: row.get(1)?,
        number: u32::try_from(number.max(0)).unwrap_or(0),
        start_boundary: row.get(3)?,
        end_boundary: row.get(4)?,
        start: start.max(0) as u64,
        end: end.max(0) as u64,
        title: row.get(7)?,
        artist: row.get(8)?,
        composer: row.get(9)?,
        comments: row.get(10)?,
        musicbrainz_id: row.get(11)?,
        confirmed: confirmed != 0,
        updated_at: row.get(13)?,
    })
}

/// Resolves one nullable field of an [`Update`] against the row it is changing.
///
/// `None` leaves the row alone, `Some("")` clears it, anything else sets it.
fn settle(change: Option<&str>, existing: Option<String>) -> Option<String> {
    match change {
        None => existing,
        Some("") => None,
        Some(text) => Some(text.to_owned()),
    }
}

/// Frames as SQLite sees them.
///
/// SQLite integers are signed, and a frame count that overflowed `i64` would be
/// six million years of audio, so saturating is the honest conversion rather than
/// a silent wrap.
fn frame_to_sql(frame: u64) -> i64 {
    i64::try_from(frame).unwrap_or(i64::MAX)
}

fn read_edge(text: &str) -> Edge {
    match text {
        "end" => Edge::End,
        // A row that says anything else is corrupt; a start is the reading that
        // keeps a track's extent positive.
        _ => Edge::Start,
    }
}

fn read_provenance(text: &str) -> Provenance {
    match text {
        "spectral-change" => Provenance::SpectralChange,
        "hmm" => Provenance::Hmm,
        "fingerprint" => Provenance::Fingerprint,
        "metadata-duration" => Provenance::MetadataDuration,
        "release-topology" => Provenance::ReleaseTopology,
        "user" => Provenance::User,
        // Not `User`: an unreadable provenance must not grant the one privilege
        // §24 reserves for a person.
        _ => Provenance::Silence,
    }
}

fn write_sources(sources: &[Provenance]) -> String {
    sources
        .iter()
        .map(|p| p.as_str())
        .collect::<Vec<_>>()
        .join("+")
}

fn read_sources(text: &str) -> Vec<Provenance> {
    text.split('+')
        .filter(|s| !s.is_empty())
        .map(read_provenance)
        .collect()
}

/// Evidence as `name=value;name=value`.
///
/// Text rather than rows because it is read whole by a person explaining a
/// decision after the fact and never queried by name - §24 asks for it to be
/// carried, not indexed. A name containing `;` or `=` would break the encoding,
/// so both are stripped; every producer uses kebab-case names.
fn write_evidence(evidence: &[Evidence]) -> String {
    evidence
        .iter()
        .map(|e| {
            let name = e.name.replace([';', '='], "");
            format!("{name}={}", e.value)
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn read_evidence(text: &str) -> Vec<Evidence> {
    text.split(';')
        .filter(|item| !item.is_empty())
        .filter_map(|item| {
            let (name, value) = item.split_once('=')?;
            Some(Evidence::new(name, value.parse::<f64>().ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcw_types::{CaptureEq, CaptureInfo, CaptureMode, SampleRate, StorageFormat};

    fn project(dir: &tempfile::TempDir, name: &str) -> Project {
        Project::create(dir.path().join(name)).expect("create")
    }

    fn a_capture(project: &mut Project) -> i64 {
        let info = CaptureInfo {
            rate: SampleRate(48_000),
            channels: 2,
            storage_format: StorageFormat::Int32,
            capture_mode: CaptureMode::Exclusive,
            host_api: Some("ALSA".into()),
            device_id: None,
            device_name: None,
            os_verified: false,
            os_report: None,
            eq: CaptureEq::Unknown,
        };
        crate::session::Session::begin(project, &info)
            .expect("begin")
            .id()
    }

    fn side_of(letter: char) -> Side {
        Side::from_letter(letter).expect("letter")
    }

    /// A side with three tracks at 0..10, 20..30 and 40..50.
    fn three_tracks(project: &mut Project) -> Vec<i64> {
        side::ensure(project, Side::A).expect("side");
        [(0, 10), (20, 30), (40, 50)]
            .into_iter()
            .map(|(start, end)| add_track(project, Side::A, start, end).expect("add"))
            .collect()
    }

    fn extents(project: &Project) -> Vec<(u32, u64, u64)> {
        tracks(project.conn(), Side::A)
            .expect("tracks")
            .into_iter()
            .map(|t| (t.number, t.start, t.end))
            .collect()
    }

    #[test]
    fn a_track_takes_its_extent_from_its_boundaries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "extent.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let id = add_track(&mut p, Side::A, 100, 48_100).expect("add");

        let record = track(p.conn(), id).expect("read").expect("row");
        assert_eq!((record.start, record.end), (100, 48_100));
        assert_eq!(record.frames(), 48_000);
        assert!((record.seconds(SampleRate(48_000)) - 1.0).abs() < f64::EPSILON);
        assert!(record.contains(100) && record.contains(48_099));
        assert!(!record.contains(48_100), "the end frame is not inside");
        assert_eq!(record.position(Side::A).alpha(), "A1");
        // No frame column on the track row at all: the extent is the join.
        let columns: Vec<String> = p
            .conn()
            .prepare("SELECT * FROM tracks")
            .expect("prepare")
            .column_names()
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert!(!columns.iter().any(|c| c.contains("frame")), "{columns:?}");
    }

    #[test]
    fn moving_a_boundary_moves_the_track_it_bounds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "move.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let id = add_boundary(
            &mut p,
            Side::A,
            &NewBoundary::detected(0, Edge::Start, 0.9, Provenance::Silence),
        )
        .expect("start");
        let end = add_boundary(
            &mut p,
            Side::A,
            &NewBoundary::detected(100, Edge::End, 0.9, Provenance::Silence),
        )
        .expect("end");
        let track_id = add_track_between(&mut p, id, end).expect("pair");

        move_boundary(&mut p, end, 200).expect("move");
        let record = track(p.conn(), track_id).expect("read").expect("row");
        assert_eq!(record.end, 200, "one write moved the track");
    }

    #[test]
    fn a_locked_boundary_refuses_to_move_or_be_deleted() {
        // §24, and the second half of WP-13's exit criterion in miniature.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "locked.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let id =
            add_boundary(&mut p, Side::A, &NewBoundary::by_user(500, Edge::Start)).expect("add");

        let err = move_boundary(&mut p, id, 600).expect_err("locked");
        assert!(matches!(err, Error::BoundaryLocked { at_frame: 500, .. }));
        assert!(matches!(
            delete_boundary(&mut p, id).expect_err("locked"),
            Error::BoundaryLocked { .. }
        ));
        assert_eq!(
            boundary(p.conn(), id).expect("read").expect("row").at_frame,
            500
        );

        // The operator's own override does move it, and it stays locked.
        move_boundary_forced(&mut p, id, 600).expect("forced");
        let after = boundary(p.conn(), id).expect("read").expect("row");
        assert_eq!(after.at_frame, 600);
        assert!(after.locked);
        assert_eq!(after.provenance, Provenance::User);

        // Unlocking hands it back to analysis.
        set_lock(&mut p, id, false).expect("unlock");
        move_boundary(&mut p, id, 700).expect("now movable");
        let handed_back = boundary(p.conn(), id).expect("read").expect("row");
        assert_eq!(handed_back.at_frame, 700);
        assert_eq!(
            handed_back.provenance,
            Provenance::User,
            "unlocking does not rewrite what put it there"
        );
    }

    #[test]
    fn re_detecting_a_boundary_updates_it_rather_than_doubling_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "again.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let first = add_boundary(
            &mut p,
            Side::A,
            &NewBoundary::detected(1_000, Edge::Start, 0.6, Provenance::Silence),
        )
        .expect("first");
        let second = add_boundary(
            &mut p,
            Side::A,
            &NewBoundary::detected(1_000, Edge::Start, 0.9, Provenance::Hmm)
                .with_sources(vec![Provenance::Silence, Provenance::Hmm]),
        )
        .expect("second");

        assert_eq!(first, second, "the same boundary, not a second one");
        let all = boundaries(p.conn(), Side::A).expect("boundaries");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].agreement(), 2);
        assert!((all[0].confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn a_second_opinion_may_pin_a_boundary_but_never_unpin_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "pin.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let id =
            add_boundary(&mut p, Side::A, &NewBoundary::by_user(42, Edge::Start)).expect("add");
        add_boundary(
            &mut p,
            Side::A,
            &NewBoundary::detected(42, Edge::Start, 0.3, Provenance::Silence)
                .with_sources(vec![Provenance::Silence, Provenance::Hmm])
                .with_evidence(vec![Evidence::new("silence.contrast-db", -40.0)]),
        )
        .expect("detected");
        let after = boundary(p.conn(), id).expect("read").expect("row");
        assert!(
            after.locked,
            "a detector cannot unlock what a person placed"
        );
        assert_eq!(
            after.provenance,
            Provenance::User,
            "nor take the credit for it"
        );
        assert!(
            (after.confidence - 1.0).abs() < 1e-6,
            "nor talk it down to 0.3: {}",
            after.confidence
        );
        assert_eq!(
            after.agreement(),
            2,
            "but the agreement is still worth recording"
        );
        assert_eq!(after.measurement("silence.contrast-db"), Some(-40.0));
    }

    #[test]
    fn evidence_and_sources_survive_a_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "evidence.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let id = add_boundary(
            &mut p,
            Side::A,
            &NewBoundary::detected(7, Edge::End, 0.75, Provenance::SpectralChange)
                .with_sources(vec![Provenance::Silence, Provenance::SpectralChange])
                .with_evidence(vec![
                    Evidence::new("silence.contrast-db", -42.5),
                    Evidence::new("hmm.posterior", 0.9375),
                ]),
        )
        .expect("add");

        let back = boundary(p.conn(), id).expect("read").expect("row");
        assert_eq!(back.edge, Edge::End);
        assert_eq!(back.provenance, Provenance::SpectralChange);
        assert_eq!(
            back.sources,
            [Provenance::Silence, Provenance::SpectralChange]
        );
        assert_eq!(back.measurement("silence.contrast-db"), Some(-42.5));
        assert_eq!(back.measurement("hmm.posterior"), Some(0.9375));
        assert_eq!(back.measurement("nothing-measured"), None);
        assert!(!back.locked, "a detector's boundary is not locked");
    }

    #[test]
    fn splitting_gives_two_tracks_that_meet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "split.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let first = add_track(&mut p, Side::A, 0, 1_000).expect("add");
        let second = split(&mut p, first, 400).expect("split");

        assert_eq!(extents(&p), [(1, 0, 400), (2, 400, 1_000)]);
        assert_ne!(first, second);
        // Two rows at one instant: the end of the first track and the start of the
        // second. Separate rows because the edges differ, and the splitter pads
        // an end and a start differently (§33). The end is reported first, which
        // is the order the audio crosses them.
        let at_400: Vec<Edge> = boundaries(p.conn(), Side::A)
            .expect("boundaries")
            .into_iter()
            .filter(|b| b.at_frame == 400)
            .map(|b| b.edge)
            .collect();
        assert_eq!(at_400, [Edge::End, Edge::Start]);
    }

    #[test]
    fn splitting_outside_the_track_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "outside.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let id = add_track(&mut p, Side::A, 100, 200).expect("add");
        for at in [0, 100, 200, 500] {
            let err = split(&mut p, id, at).expect_err("outside");
            assert!(
                matches!(err, Error::OutsideTrack { at: got, .. } if got == at),
                "{err} for {at}"
            );
        }
        assert_eq!(extents(&p), [(1, 100, 200)], "nothing changed");
    }

    #[test]
    fn merging_adjacent_tracks_keeps_the_first_ones_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "merge.vcw");
        let ids = three_tracks(&mut p);
        update(&mut p, ids[0], &Update::title("Part One")).expect("title");

        merge(&mut p, ids[0], ids[1]).expect("merge");
        assert_eq!(extents(&p), [(1, 0, 30), (2, 40, 50)]);
        let survivor = track(p.conn(), ids[0]).expect("read").expect("row");
        assert_eq!(survivor.title, "Part One");
        assert!(track(p.conn(), ids[1]).expect("read").is_none());
    }

    #[test]
    fn a_split_then_merged_track_is_back_where_it_started() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "roundtrip.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let first = add_track(&mut p, Side::A, 0, 1_000).expect("add");
        let second = split(&mut p, first, 400).expect("split");
        // The split boundaries are the operator's. Unlock them, so the merge
        // clears them too and the side is byte-for-byte where it started: the
        // locked-boundary case is
        // `merging_across_a_locked_boundary_keeps_the_boundary`.
        for id in [
            track(p.conn(), first)
                .expect("read")
                .expect("row")
                .end_boundary,
            track(p.conn(), second)
                .expect("read")
                .expect("row")
                .start_boundary,
        ] {
            set_lock(&mut p, id, false).expect("unlock");
        }
        merge(&mut p, first, second).expect("merge");

        assert_eq!(extents(&p), [(1, 0, 1_000)]);
        assert_eq!(
            boundaries(p.conn(), Side::A).expect("boundaries").len(),
            2,
            "the boundary between them went with the join"
        );
    }

    #[test]
    fn merging_across_a_track_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "notadjacent.vcw");
        let ids = three_tracks(&mut p);
        let err = merge(&mut p, ids[0], ids[2]).expect_err("not adjacent");
        assert!(matches!(err, Error::NotAdjacent { .. }));
        assert_eq!(extents(&p).len(), 3, "the middle track is still there");
    }

    #[test]
    fn merging_across_a_locked_boundary_keeps_the_boundary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "mergelock.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let first = add_track(&mut p, Side::A, 0, 400).expect("first");
        let second = add_track(&mut p, Side::A, 400, 1_000).expect("second");
        // `add_track` places user boundaries, so the join is locked already, and
        // that is not an obstacle: §24 is about analysis, not about the operator.
        merge(&mut p, first, second).expect("merge");
        assert_eq!(extents(&p), [(1, 0, 1_000)]);
        let kept = boundaries(p.conn(), Side::A).expect("boundaries");
        assert_eq!(
            kept.len(),
            4,
            "the locked join stays as a marker of where it was: {kept:?}"
        );
    }

    #[test]
    fn deleting_a_bounded_boundary_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "inuse.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let id = add_boundary(
            &mut p,
            Side::A,
            &NewBoundary::detected(0, Edge::Start, 0.9, Provenance::Silence),
        )
        .expect("start");
        let end = add_boundary(
            &mut p,
            Side::A,
            &NewBoundary::detected(100, Edge::End, 0.9, Provenance::Silence),
        )
        .expect("end");
        let track_id = add_track_between(&mut p, id, end).expect("pair");

        let err = delete_boundary(&mut p, end).expect_err("in use");
        assert!(matches!(err, Error::BoundaryInUse { track_id: t, .. } if t == track_id));

        // With the track gone, the boundary goes with it.
        remove(&mut p, track_id).expect("remove");
        assert!(
            boundaries(p.conn(), Side::A)
                .expect("boundaries")
                .is_empty()
        );
    }

    #[test]
    fn deleting_a_track_keeps_a_boundary_a_person_locked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "keeplock.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let id = add_track(&mut p, Side::A, 0, 100).expect("add");
        remove(&mut p, id).expect("remove");
        assert_eq!(
            boundaries(p.conn(), Side::A).expect("boundaries").len(),
            2,
            "user boundaries outlive the track"
        );
        assert!(tracks(p.conn(), Side::A).expect("tracks").is_empty());
    }

    #[test]
    fn tracks_renumber_themselves_in_timeline_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "renumber.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        // Added out of order, on purpose.
        add_track(&mut p, Side::A, 200, 300).expect("late");
        add_track(&mut p, Side::A, 0, 100).expect("early");
        add_track(&mut p, Side::A, 100, 200).expect("middle");
        assert_eq!(extents(&p), [(1, 0, 100), (2, 100, 200), (3, 200, 300)]);

        // Dragging the first track past the second reorders them, and `renumber`
        // is the one call a caller needs after moving a boundary.
        let first = tracks(p.conn(), Side::A).expect("tracks")[0].clone();
        set_lock(&mut p, first.start_boundary, false).expect("unlock");
        set_lock(&mut p, first.end_boundary, false).expect("unlock");
        move_boundary(&mut p, first.start_boundary, 400).expect("start");
        move_boundary(&mut p, first.end_boundary, 500).expect("end");
        renumber(&mut p, Side::A).expect("renumber");
        assert_eq!(extents(&p), [(1, 100, 200), (2, 200, 300), (3, 400, 500)]);
    }

    #[test]
    fn a_metadata_update_leaves_the_fields_it_was_not_given() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "update.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        let id = add_track(&mut p, Side::A, 0, 100).expect("add");

        update(
            &mut p,
            id,
            &Update {
                title: Some("Sister Ray".into()),
                artist: Some("The Velvet Underground".into()),
                musicbrainz_id: Some("5b11f4ce-a62d-471e-81fc-a69a8278c7da".into()),
                confirmed: Some(true),
                ..Update::default()
            },
        )
        .expect("first");
        update(&mut p, id, &Update::title("Sister Ray (edit)")).expect("second");

        let record = track(p.conn(), id).expect("read").expect("row");
        assert_eq!(record.title, "Sister Ray (edit)");
        assert_eq!(
            record.artist.as_deref(),
            Some("The Velvet Underground"),
            "artist survived"
        );
        assert!(record.confirmed, "confirmation survived");
        assert!(record.musicbrainz_id.is_some());

        // An empty string clears a nullable column back to the release's value.
        update(
            &mut p,
            id,
            &Update {
                musicbrainz_id: Some(String::new()),
                artist: Some(String::new()),
                ..Update::default()
            },
        )
        .expect("clear");
        let cleared = track(p.conn(), id).expect("read").expect("row");
        assert_eq!(cleared.musicbrainz_id, None);
        assert_eq!(cleared.artist, None);
        assert_eq!(
            cleared.artist_or("The Velvet Underground"),
            "The Velvet Underground",
            "and the release's artist is what a tag writer gets"
        );
        assert_eq!(cleared.title, "Sister Ray (edit)", "the title is untouched");
        assert!(Update::default().is_empty());
    }

    #[test]
    fn a_track_can_move_between_two_sides_of_one_capture() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "moveside.vcw");
        let capture = a_capture(&mut p);
        side::attach(&mut p, Side::A, capture).expect("A");
        side::attach(&mut p, side_of('B'), capture).expect("B");
        add_track(&mut p, Side::A, 0, 100).expect("first");
        let stray = add_track(&mut p, Side::A, 100, 200).expect("second");

        move_to_side(&mut p, stray, side_of('B')).expect("move");
        assert_eq!(
            tracks(p.conn(), Side::A).expect("A").len(),
            1,
            "it left side A"
        );
        let on_b = tracks(p.conn(), side_of('B')).expect("B");
        assert_eq!(on_b.len(), 1);
        assert_eq!((on_b[0].start, on_b[0].end), (100, 200), "same frames");
        assert_eq!(on_b[0].number, 1);
        // Its boundaries came with it, so side A no longer reports them.
        assert_eq!(boundaries(p.conn(), Side::A).expect("boundaries").len(), 2);
    }

    #[test]
    fn a_captures_edges_are_both_ends_of_every_track_on_both_its_faces() {
        // §21's skip marks. Two faces on one capture, adjacent tracks sharing a
        // frame, and one gap - which is what a real side looks like once the
        // run-in and the run-out are outside the tracks.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "edges.vcw");
        let capture = a_capture(&mut p);
        side::attach(&mut p, Side::A, capture).expect("A");
        side::attach(&mut p, side_of('B'), capture).expect("B");
        add_track(&mut p, Side::A, 100, 200).expect("A1");
        add_track(&mut p, Side::A, 200, 300).expect("A2");
        add_track(&mut p, side_of('B'), 400, 500).expect("B1");

        assert_eq!(
            edges_of_capture(p.conn(), capture).expect("edges"),
            vec![100, 200, 300, 400, 500],
            "ascending, deduplicated across the shared frame, and across the join"
        );
    }

    #[test]
    fn a_capture_nothing_has_been_analyzed_from_has_no_edges() {
        // Which is the state a skip falls back to a fixed step in, so it is
        // worth being sure it is empty rather than a zero.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "noedges.vcw");
        let capture = a_capture(&mut p);
        side::attach(&mut p, Side::A, capture).expect("A");
        assert!(
            edges_of_capture(p.conn(), capture)
                .expect("edges")
                .is_empty()
        );
        assert!(
            edges_of_capture(p.conn(), 4_242)
                .expect("absent")
                .is_empty(),
            "and a capture that is not there is not an error here - playback \
             refuses it at the door, with a better message than this could give"
        );
    }

    #[test]
    fn a_track_cannot_move_to_a_side_holding_different_audio() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "differing.vcw");
        let first = a_capture(&mut p);
        let second = a_capture(&mut p);
        side::attach(&mut p, Side::A, first).expect("A");
        side::attach(&mut p, side_of('B'), second).expect("B");
        let id = add_track(&mut p, Side::A, 0, 100).expect("add");

        let err = move_to_side(&mut p, id, side_of('B')).expect_err("different capture");
        assert!(matches!(
            err,
            Error::DifferentCapture {
                from: 'A',
                to: 'B',
                ..
            }
        ));
        assert_eq!(tracks(p.conn(), Side::A).expect("A").len(), 1);
    }

    #[test]
    fn a_degenerate_track_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "degenerate.vcw");
        side::ensure(&mut p, Side::A).expect("side");
        assert!(matches!(
            add_track(&mut p, Side::A, 100, 100).expect_err("empty"),
            Error::OutsideTrack { .. }
        ));
        assert!(matches!(
            add_track(&mut p, Side::A, 200, 100).expect_err("backwards"),
            Error::OutsideTrack { .. }
        ));
        assert!(tracks(p.conn(), Side::A).expect("tracks").is_empty());
    }

    #[test]
    fn a_missing_row_is_reported_rather_than_panicking() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "missing.vcw");
        assert!(matches!(
            split(&mut p, 99, 10).expect_err("no track"),
            Error::NoSuchTrack { track_id: 99 }
        ));
        assert!(matches!(
            move_boundary(&mut p, 99, 10).expect_err("no boundary"),
            Error::NoSuchBoundary { boundary_id: 99 }
        ));
        assert!(matches!(
            add_boundary(&mut p, Side::A, &NewBoundary::by_user(0, Edge::Start))
                .expect_err("no side"),
            Error::NoSuchSide { side: 'A' }
        ));
        assert_eq!(track(p.conn(), 99).expect("read"), None);
        assert_eq!(boundary(p.conn(), 99).expect("read"), None);
    }

    #[test]
    fn positions_are_rendered_across_sides_in_playing_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "positions.vcw");
        side::ensure(&mut p, Side::A).expect("A");
        side::ensure(&mut p, side_of('B')).expect("B");
        add_track(&mut p, Side::A, 0, 100).expect("a1");
        add_track(&mut p, Side::A, 100, 200).expect("a2");
        add_track(&mut p, side_of('B'), 0, 100).expect("b1");

        let rendered: Vec<String> = positions(p.conn(), Numbering::Alpha)
            .expect("positions")
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert_eq!(rendered, ["A1", "A2", "B1"]);

        let numeric: Vec<String> = positions(p.conn(), Numbering::Numeric)
            .expect("positions")
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert_eq!(numeric, ["01", "02", "01"], "numeric restarts on each side");

        // The two forms differ exactly where it matters: B1 is `01` as a side
        // number and `03` as a disc number. A test that only ever looked at
        // side A could not tell them apart.
        let sequence: Vec<String> = positions(p.conn(), Numbering::Sequence)
            .expect("positions")
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert_eq!(
            sequence,
            ["01", "02", "03"],
            "sequence runs across the disc"
        );

        let listed = listing(p.conn()).expect("listing");
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[2].0, side_of('B'));
        assert_eq!(listed[2].1.number, 1, "numbers are per side");
    }

    #[test]
    fn a_sequence_restarts_on_the_next_disc() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir, "discs.vcw");
        for letter in ['A', 'B', 'C'] {
            side::ensure(&mut p, side_of(letter)).expect("side");
            add_track(&mut p, side_of(letter), 0, 100).expect("track");
        }
        add_track(&mut p, side_of('C'), 100, 200).expect("c2");

        let rendered: Vec<String> = positions(p.conn(), Numbering::Sequence)
            .expect("positions")
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        // A and B are disc 1, C is disc 2 - the CD parlance the numbering
        // follows. A release-wide count would have said 03 and 04 here.
        assert_eq!(rendered, ["01", "02", "01", "02"]);
    }
}
