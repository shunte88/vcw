/*
 *  side.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Sides, and the capture that produced each one (§29).
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

//! Sides, and the capture that produced each one (§29).
//!
//! A side is the natural unit of vinyl capture, which is why it - not the release -
//! is what a recording attaches to. You put the record on, you record a side, you
//! get up and flip it. Everything the project knows about track positions is
//! relative to a side's capture, so the side row is where the two halves of the
//! schema meet: `capture_id` points into the capture tables, `side_index` into
//! §29's topology.
//!
//! # Two sides may share one capture
//!
//! `capture_id` is not unique. Recording both faces in a single take is a real
//! thing people do - leave the machine running, flip, carry on - and the frames
//! are then what tells the sides apart. Allowing it costs nothing here and makes
//! [`crate::track::move_to_side`] meaningful: a track that turned out to be on
//! side B can move there, because side B's audio is the same audio.
//!
//! # Why a side is created deliberately
//!
//! [`ensure`] is explicit rather than implied by a capture. A capture that arrived
//! with no side named is the common case during a session - the operator has not
//! said yet, or is recording something that is not a record at all - and inventing
//! side A for it would quietly relabel a mislabelled recording instead of leaving
//! the question open.

use rusqlite::{Connection, OptionalExtension, params};
use vcw_types::vinyl::{Face, Side};

use crate::error::{Error, Result};
use crate::release;
use crate::sqlite::Project;

/// A side's row, as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Row id, referenced by this side's boundaries and tracks.
    pub id: i64,
    /// Which side it is. A is 0.
    pub side: Side,
    /// The capture holding its audio, where one has been recorded.
    pub capture: Option<i64>,
    /// A title, where the label prints one for the side.
    pub title: Option<String>,
    /// Unix seconds at creation.
    pub created_at: i64,
}

impl Record {
    /// The one-based disc this side is on (§29).
    #[must_use]
    pub const fn disc(&self) -> u32 {
        self.side.disc()
    }

    /// Which face of that disc it is.
    #[must_use]
    pub const fn face(&self) -> Face {
        self.side.face()
    }

    /// The side letter.
    #[must_use]
    pub const fn letter(&self) -> char {
        self.side.letter()
    }

    /// Whether anything has been recorded for this side.
    #[must_use]
    pub const fn is_recorded(&self) -> bool {
        self.capture.is_some()
    }
}

/// Reads a side, creating it - and the release it belongs to - if it is absent.
///
/// # Errors
///
/// If the project is read-only, or the write fails.
pub fn ensure(project: &mut Project, side: Side) -> Result<Record> {
    if let Some(record) = load(project.conn(), side)? {
        return Ok(record);
    }
    release::ensure(project)?;
    let conn = project.conn_mut();
    conn.execute(
        "INSERT INTO sides (release_id, side_index, capture_id, title, created_at)
         VALUES (?1, ?2, NULL, NULL, ?3)",
        params![release::RELEASE_ID, i64::from(side.index()), crate::now()],
    )?;
    load(conn, side)?.map_or_else(|| unreachable!("the row was just inserted"), Ok)
}

/// Reads a side by letter, or `None` if the project has no row for it.
///
/// # Errors
///
/// If the query fails.
pub fn load(conn: &Connection, side: Side) -> Result<Option<Record>> {
    let found = conn
        .query_row(
            "SELECT side_id, side_index, capture_id, title, created_at
               FROM sides WHERE release_id = ?1 AND side_index = ?2",
            params![release::RELEASE_ID, i64::from(side.index())],
            read_row,
        )
        .optional()?;
    Ok(found)
}

/// Reads a side by row id.
///
/// # Errors
///
/// If the query fails.
pub fn by_id(conn: &Connection, side_id: i64) -> Result<Option<Record>> {
    let found = conn
        .query_row(
            "SELECT side_id, side_index, capture_id, title, created_at
               FROM sides WHERE side_id = ?1",
            params![side_id],
            read_row,
        )
        .optional()?;
    Ok(found)
}

/// Every side in the project, in playing order.
///
/// # Errors
///
/// If the query fails.
pub fn list(conn: &Connection) -> Result<Vec<Record>> {
    let mut stmt = conn.prepare(
        "SELECT side_id, side_index, capture_id, title, created_at
           FROM sides WHERE release_id = ?1 ORDER BY side_index",
    )?;
    let rows = stmt
        .query_map(params![release::RELEASE_ID], read_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The sides recorded from a capture, in playing order.
///
/// Usually one. Two when a single take covered both faces.
///
/// # Errors
///
/// If the query fails.
pub fn for_capture(conn: &Connection, capture_id: i64) -> Result<Vec<Record>> {
    let mut stmt = conn.prepare(
        "SELECT side_id, side_index, capture_id, title, created_at
           FROM sides WHERE capture_id = ?1 ORDER BY side_index",
    )?;
    let rows = stmt
        .query_map(params![capture_id], read_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Points a side at the capture that recorded it.
///
/// Creates the side if it is absent, so "record side C" is one call rather than
/// two. Replacing an existing capture is allowed and is what re-recording a side
/// looks like: the previous capture stays in the project, because §4.1 does not
/// delete audio, and the side simply stops pointing at it.
///
/// # Errors
///
/// [`Error::NoSuchCapture`] if the capture is not in this project, or if the write
/// fails.
pub fn attach(project: &mut Project, side: Side, capture_id: i64) -> Result<Record> {
    if !capture_exists(project.conn(), capture_id)? {
        return Err(Error::NoSuchCapture { capture_id });
    }
    let record = ensure(project, side)?;
    project.conn_mut().execute(
        "UPDATE sides SET capture_id = ?2 WHERE side_id = ?1",
        params![record.id, capture_id],
    )?;
    Ok(Record {
        capture: Some(capture_id),
        ..record
    })
}

/// Forgets which capture recorded a side, keeping the audio.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if the side is not in the project.
pub fn detach(project: &mut Project, side: Side) -> Result<()> {
    let record = require(project.conn(), side)?;
    project.conn_mut().execute(
        "UPDATE sides SET capture_id = NULL WHERE side_id = ?1",
        params![record.id],
    )?;
    Ok(())
}

/// Sets or clears a side's title.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if the side is not in the project.
pub fn set_title(project: &mut Project, side: Side, title: Option<&str>) -> Result<()> {
    let record = require(project.conn(), side)?;
    project.conn_mut().execute(
        "UPDATE sides SET title = ?2 WHERE side_id = ?1",
        params![record.id, title],
    )?;
    Ok(())
}

/// Moves a side to another letter, taking its capture, boundaries and tracks (§31).
///
/// §31's "assign side/disc" at the level it is usually meant: the sides were
/// recorded out of order, or a two-disc set turned out to be numbered A/B/C/D
/// rather than A/D/B/C. Nothing about the audio changes, and neither does anything
/// about the tracks - they are attached to the row, not to the letter.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if `from` is absent, [`Error::SideOccupied`] if `to` is
/// taken. Swapping two sides is therefore two calls with a spare letter between
/// them, which is deliberate: an implicit swap would have to decide what happens to
/// the second side's tracks, and the caller knows.
pub fn relabel(project: &mut Project, from: Side, to: Side) -> Result<Record> {
    let record = require(project.conn(), from)?;
    if from == to {
        return Ok(record);
    }
    if load(project.conn(), to)?.is_some() {
        return Err(Error::SideOccupied { side: to.letter() });
    }
    project.conn_mut().execute(
        "UPDATE sides SET side_index = ?2 WHERE side_id = ?1",
        params![record.id, i64::from(to.index())],
    )?;
    Ok(Record { side: to, ..record })
}

/// Removes an empty side.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if it is absent, [`Error::SideNotEmpty`] if it still holds
/// tracks or boundaries. Refusing is the §4.1 answer: the caller can see what is on
/// it and decide, and this layer cannot.
pub fn remove(project: &mut Project, side: Side) -> Result<()> {
    let record = require(project.conn(), side)?;
    let tracks = count(project.conn(), "tracks", record.id)?;
    let boundaries = count(project.conn(), "track_boundaries", record.id)?;
    if tracks > 0 || boundaries > 0 {
        return Err(Error::SideNotEmpty {
            side: side.letter(),
            tracks,
            boundaries,
        });
    }
    project
        .conn_mut()
        .execute("DELETE FROM sides WHERE side_id = ?1", params![record.id])?;
    Ok(())
}

/// Reads a side, or reports that it is not there.
///
/// # Errors
///
/// [`Error::NoSuchSide`] if the project has no row for it.
pub fn require(conn: &Connection, side: Side) -> Result<Record> {
    load(conn, side)?.ok_or(Error::NoSuchSide {
        side: side.letter(),
    })
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
    let index: i64 = row.get(1)?;
    Ok(Record {
        id: row.get(0)?,
        // A stored index past Z would be a corrupt row rather than a side; Z is
        // thirteen discs, so clamping is the harmless reading.
        side: Side::from_index(index.clamp(0, 25) as u8).unwrap_or(Side::A),
        capture: row.get(2)?,
        title: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn capture_exists(conn: &Connection, capture_id: i64) -> Result<bool> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT capture_id FROM captures WHERE capture_id = ?1",
            params![capture_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

fn count(conn: &Connection, table: &str, side_id: i64) -> Result<usize> {
    // The table name is one of two literals chosen above, never caller input.
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE side_id = ?1");
    let n: i64 = conn.query_row(&sql, params![side_id], |r| r.get(0))?;
    Ok(n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcw_types::{CaptureEq, CaptureInfo, CaptureMode, SampleRate, StorageFormat};

    fn project(dir: &tempfile::TempDir) -> Project {
        Project::create(dir.path().join("sides.vcw")).expect("create")
    }

    fn info() -> CaptureInfo {
        CaptureInfo {
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
        }
    }

    fn a_capture(project: &mut Project) -> i64 {
        crate::session::Session::begin(project, &info())
            .expect("begin")
            .id()
    }

    fn side(letter: char) -> Side {
        Side::from_letter(letter).expect("letter")
    }

    #[test]
    fn a_side_knows_its_disc_and_face() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let c = ensure(&mut p, side('C')).expect("ensure");
        assert_eq!(c.letter(), 'C');
        assert_eq!(c.disc(), 2);
        assert_eq!(c.face(), Face::First);
        assert!(!c.is_recorded());
    }

    #[test]
    fn ensure_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let first = ensure(&mut p, Side::A).expect("first");
        let second = ensure(&mut p, Side::A).expect("second");
        assert_eq!(first, second);
        assert_eq!(list(p.conn()).expect("list").len(), 1);
    }

    #[test]
    fn sides_come_back_in_playing_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        for letter in ['D', 'A', 'C', 'B'] {
            ensure(&mut p, side(letter)).expect("ensure");
        }
        let letters: Vec<char> = list(p.conn())
            .expect("list")
            .iter()
            .map(Record::letter)
            .collect();
        assert_eq!(letters, ['A', 'B', 'C', 'D']);
    }

    #[test]
    fn attaching_a_capture_creates_the_side_if_it_has_to() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let capture = a_capture(&mut p);
        let b = attach(&mut p, side('B'), capture).expect("attach");
        assert_eq!(b.capture, Some(capture));
        assert!(b.is_recorded());
        assert_eq!(
            load(p.conn(), side('B'))
                .expect("load")
                .expect("row")
                .capture,
            Some(capture)
        );
    }

    #[test]
    fn a_capture_that_is_not_in_the_project_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let err = attach(&mut p, Side::A, 99).expect_err("no such capture");
        assert!(matches!(err, Error::NoSuchCapture { capture_id: 99 }));
    }

    #[test]
    fn one_capture_can_hold_two_sides() {
        // Both faces in a single take, which is a thing people do.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let capture = a_capture(&mut p);
        attach(&mut p, Side::A, capture).expect("A");
        attach(&mut p, side('B'), capture).expect("B");
        let sides = for_capture(p.conn(), capture).expect("for_capture");
        assert_eq!(sides.len(), 2);
        assert_eq!(sides[0].letter(), 'A');
        assert_eq!(sides[1].letter(), 'B');
    }

    #[test]
    fn re_recording_a_side_leaves_the_old_capture_in_the_project() {
        // §4.1: the audio stays, the side stops pointing at it.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let first = a_capture(&mut p);
        let second = a_capture(&mut p);
        attach(&mut p, Side::A, first).expect("first");
        attach(&mut p, Side::A, second).expect("second");

        let record = load(p.conn(), Side::A).expect("load").expect("row");
        assert_eq!(record.capture, Some(second));
        let captures: i64 = p
            .conn()
            .query_row("SELECT COUNT(*) FROM captures", [], |r| r.get(0))
            .expect("count");
        assert_eq!(captures, 2, "the first take is still there");
    }

    #[test]
    fn detaching_and_titling_a_side() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let capture = a_capture(&mut p);
        attach(&mut p, Side::A, capture).expect("attach");
        set_title(&mut p, Side::A, Some("The Quiet Side")).expect("title");
        detach(&mut p, Side::A).expect("detach");

        let record = load(p.conn(), Side::A).expect("load").expect("row");
        assert_eq!(record.capture, None);
        assert_eq!(record.title.as_deref(), Some("The Quiet Side"));
    }

    #[test]
    fn a_missing_side_is_reported_rather_than_created() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let err = detach(&mut p, side('D')).expect_err("no such side");
        assert!(matches!(err, Error::NoSuchSide { side: 'D' }));
        assert!(require(p.conn(), Side::A).is_err());
    }

    #[test]
    fn relabelling_moves_a_side_and_refuses_to_overwrite_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let capture = a_capture(&mut p);
        attach(&mut p, Side::A, capture).expect("A");
        ensure(&mut p, side('B')).expect("B");

        let err = relabel(&mut p, Side::A, side('B')).expect_err("occupied");
        assert!(matches!(err, Error::SideOccupied { side: 'B' }));

        let moved = relabel(&mut p, Side::A, side('C')).expect("relabel");
        assert_eq!(moved.letter(), 'C');
        assert_eq!(moved.disc(), 2, "the disc follows the letter");
        assert_eq!(moved.capture, Some(capture), "the audio came with it");
        assert!(load(p.conn(), Side::A).expect("load").is_none());
    }

    #[test]
    fn relabelling_a_side_to_itself_is_not_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        ensure(&mut p, Side::A).expect("ensure");
        assert_eq!(
            relabel(&mut p, Side::A, Side::A).expect("relabel").letter(),
            'A'
        );
    }

    #[test]
    fn an_empty_side_can_be_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        ensure(&mut p, side('B')).expect("ensure");
        remove(&mut p, side('B')).expect("remove");
        assert!(list(p.conn()).expect("list").is_empty());
    }
}
