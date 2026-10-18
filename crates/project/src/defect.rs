/*
 *  defect.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The defect list: spans of damage on a capture, and the repair proposed for each.
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

//! The defect list (§46 Phase 3, WP-30).
//!
//! A defect is a span of damage on a *capture*, not on a track. Frames index a
//! capture, so a row written here survives re-splitting, re-adoption and a side
//! being laid out again, none of which change what frame 4,812,330 is.
//!
//! Nothing in this module repairs anything. §4.1 keeps the sample blocks immutable
//! and WP-30 computes the repair from these rows every time audio leaves the
//! project, so what is stored is a proposal and a verdict: where the damage is, how
//! sure the detector was, and what somebody decided to do about it.
//!
//! **Re-running a detector** is [`replace`], and it is the reason this module exists
//! rather than four `execute` calls in `vcw-core`. A second pass has to be able to
//! *remove* a span the first pass found and the second one does not, or raising the
//! threshold could never shorten the list; and it must not touch a row a person has
//! locked, or their veto would be undone by the next analysis. Both halves are one
//! transaction, so a detector that fails part way through leaves the previous
//! answer rather than half of two.
//!
//! Requirements: §4.1 (immutable PCM), §23 (observations, not edits), §24
//! (provenance and locking).

use rusqlite::{Connection, OptionalExtension, params};
use vcw_types::{Evidence, Provenance, Repair};

use crate::error::{Error, Result};
use crate::sqlite::Project;
use crate::track::{frame_to_sql, read_evidence, read_provenance, write_evidence};

/// A defect row, as read back.
#[derive(Debug, Clone, PartialEq)]
pub struct Defect {
    /// Row id. Stable across re-detection, which is what lets an edit to one row
    /// survive another analysis pass.
    pub id: i64,
    /// The capture the damage is in.
    pub capture_id: i64,
    /// First damaged frame, from the start of the capture.
    pub start_frame: u64,
    /// One past the last. Exclusive, so the length is a subtraction.
    pub end_frame: u64,
    /// Which channels are damaged, as a bitmask: bit 0 is channel 0.
    pub channels: u32,
    /// What kind of damage: `click`, `crackle`, `dropout`. Free text, because
    /// whether crackle is the same detector as clicks is still open (WP-30 §6).
    pub kind: String,
    /// How sure the detector was, in `0.0..=1.0`.
    pub confidence: f32,
    /// Which detector found it, or [`Provenance::User`].
    pub provenance: Provenance,
    /// The measurements behind it (§24).
    pub evidence: Vec<Evidence>,
    /// What to do about it.
    pub repair: Repair,
    /// Whether automatic analysis may change it (§24).
    pub locked: bool,
    /// Unix seconds at creation.
    pub created_at: i64,
    /// Unix seconds at the last change.
    pub updated_at: i64,
}

impl Defect {
    /// Length in frames.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.end_frame.saturating_sub(self.start_frame)
    }

    /// Whether this row would change the audio on its way out.
    #[must_use]
    pub const fn is_audible(&self) -> bool {
        self.repair.is_audible()
    }

    /// How many measurements the row carries (§24).
    #[must_use]
    pub fn measurement_count(&self) -> usize {
        self.evidence.len()
    }

    /// Whether the damage touches a channel.
    #[must_use]
    pub const fn touches(&self, channel: u16) -> bool {
        channel < 32 && self.channels & (1 << channel) != 0
    }
}

/// What to write for a new defect.
#[derive(Debug, Clone, PartialEq)]
pub struct NewDefect {
    /// First damaged frame.
    pub start_frame: u64,
    /// One past the last.
    pub end_frame: u64,
    /// Damaged channels, as a bitmask.
    pub channels: u32,
    /// What kind of damage.
    pub kind: String,
    /// How sure the detector was.
    pub confidence: f32,
    /// Who found it.
    pub provenance: Provenance,
    /// The measurements behind it.
    pub evidence: Vec<Evidence>,
    /// What to do about it. Stated, never defaulted.
    pub repair: Repair,
    /// Whether analysis may change it. A user row is locked whatever this says.
    pub locked: bool,
}

/// Adds one defect, or updates the row already covering exactly that span.
///
/// The same upsert [`crate::track::add_boundary_to`] uses, and for the same reason:
/// two detectors agreeing about the same damage is one defect. A locked row keeps
/// its repair, its provenance and its lock, because those three are what a person
/// set; its confidence and evidence are still refreshed, since how sure the detector
/// was this time is worth knowing about a span somebody has already ruled on.
///
/// # Errors
///
/// If the write fails, or the span is empty or touches no channel - both of which
/// the schema refuses and neither of which is audio.
pub fn add(project: &mut Project, capture_id: i64, new: &NewDefect) -> Result<i64> {
    let now = crate::now();
    let conn = project.conn_mut();
    write(conn, capture_id, new, now)
}

fn write(conn: &Connection, capture_id: i64, new: &NewDefect, now: i64) -> Result<i64> {
    let locked = new.locked || new.provenance.is_locked();
    conn.execute(
        "INSERT INTO defects
             (capture_id, start_frame, end_frame, channels, kind, confidence,
              provenance, evidence, repair, locked, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
         ON CONFLICT (capture_id, start_frame, end_frame, channels) DO UPDATE SET
             -- A locked row keeps what a person decided: the repair is their
             -- verdict and the provenance is their credit, and a detector arriving
             -- at the same span must not overwrite either.
             kind       = CASE WHEN locked THEN kind       ELSE excluded.kind       END,
             provenance = CASE WHEN locked THEN provenance ELSE excluded.provenance END,
             repair     = CASE WHEN locked THEN repair     ELSE excluded.repair     END,
             -- Recorded either way: how sure this pass was about a span somebody
             -- has already ruled on is worth having.
             confidence = excluded.confidence,
             evidence   = excluded.evidence,
             -- A second opinion may pin a defect but never unpin one.
             locked     = locked OR excluded.locked,
             updated_at = excluded.updated_at",
        params![
            capture_id,
            frame_to_sql(new.start_frame),
            frame_to_sql(new.end_frame),
            new.channels,
            new.kind,
            f64::from(new.confidence.clamp(0.0, 1.0)),
            new.provenance.as_str(),
            write_evidence(&new.evidence),
            new.repair.as_str(),
            i64::from(locked),
            now,
        ],
    )?;
    let id: i64 = conn.query_row(
        "SELECT defect_id FROM defects
           WHERE capture_id = ?1 AND start_frame = ?2 AND end_frame = ?3 AND channels = ?4",
        params![
            capture_id,
            frame_to_sql(new.start_frame),
            frame_to_sql(new.end_frame),
            new.channels
        ],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// Replaces one detector's unlocked findings on a capture with a fresh set.
///
/// What makes a detector re-runnable. Three things have to be true at once and they
/// are only true together, which is why this is one transaction and one function:
///
/// 1. A span the previous pass found and this one did not **goes**. Otherwise
///    raising the threshold could only ever lengthen the list.
/// 2. A locked row **stays**, untouched, whatever this pass thinks. That is §24,
///    and it is what a person's veto is made of.
/// 3. A span this pass found that a locked row already covers exactly keeps the
///    locked row's verdict - handled by the upsert in [`add`], not by a second rule
///    here, so there is one place where locking is decided.
///
/// Returns how many rows were removed and how many were written, in that order.
///
/// # Errors
///
/// If the transaction fails. Nothing is written in that case: the previous answer
/// survives, which is better than half of two.
pub fn replace(
    project: &mut Project,
    capture_id: i64,
    provenance: Provenance,
    found: &[NewDefect],
) -> Result<(usize, usize)> {
    let now = crate::now();
    let conn = project.conn_mut();
    let transaction = conn.transaction()?;
    let removed = transaction.execute(
        "DELETE FROM defects
          WHERE capture_id = ?1 AND provenance = ?2 AND locked = 0",
        params![capture_id, provenance.as_str()],
    )?;
    for defect in found {
        write(&transaction, capture_id, defect, now)?;
    }
    transaction.commit()?;
    Ok((removed, found.len()))
}

/// Every defect on a capture, in frame order.
///
/// # Errors
///
/// If the read fails, or a row stores a repair nothing can carry out.
pub fn of_capture(conn: &Connection, capture_id: i64) -> Result<Vec<Defect>> {
    let mut statement = conn.prepare(
        "SELECT defect_id, capture_id, start_frame, end_frame, channels, kind,
                confidence, provenance, evidence, repair, locked, created_at, updated_at
           FROM defects
          WHERE capture_id = ?1
          ORDER BY start_frame, end_frame, channels",
    )?;
    let rows = statement.query_map(params![capture_id], read)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .collect()
}

/// One defect by id.
///
/// # Errors
///
/// If the read fails, or the row stores a repair nothing can carry out.
pub fn get(conn: &Connection, defect_id: i64) -> Result<Option<Defect>> {
    let found = conn
        .query_row(
            "SELECT defect_id, capture_id, start_frame, end_frame, channels, kind,
                    confidence, provenance, evidence, repair, locked, created_at, updated_at
               FROM defects WHERE defect_id = ?1",
            params![defect_id],
            read,
        )
        .optional()?;
    found.transpose()
}

/// Sets what to do about one defect, and locks it.
///
/// Locking is not optional here and is not a separate call: a person choosing a
/// repair - including [`Repair::None`], the veto - is a decision §24 reserves, and
/// a decision the next detection pass would silently undo is not one.
///
/// # Errors
///
/// [`Error::NoSuchDefect`] if it is not there.
pub fn set_repair(project: &mut Project, defect_id: i64, repair: Repair) -> Result<()> {
    let now = crate::now();
    let changed = project.conn_mut().execute(
        "UPDATE defects SET repair = ?2, locked = 1, provenance = ?3, updated_at = ?4
          WHERE defect_id = ?1",
        params![defect_id, repair.as_str(), Provenance::User.as_str(), now],
    )?;
    if changed == 0 {
        return Err(Error::NoSuchDefect { defect_id });
    }
    Ok(())
}

/// Pins a defect against analysis, or releases it.
///
/// # Errors
///
/// [`Error::NoSuchDefect`] if it is not there.
pub fn set_lock(project: &mut Project, defect_id: i64, locked: bool) -> Result<()> {
    let now = crate::now();
    let changed = project.conn_mut().execute(
        "UPDATE defects SET locked = ?2, updated_at = ?3 WHERE defect_id = ?1",
        params![defect_id, i64::from(locked), now],
    )?;
    if changed == 0 {
        return Err(Error::NoSuchDefect { defect_id });
    }
    Ok(())
}

/// Removes a defect.
///
/// Refuses a locked one, because unlocking is the decision and deleting is not a
/// way around it. Note that deleting a detector's row is rarely what somebody
/// wants: the next pass will find the same damage again, where [`Repair::None`]
/// records the verdict and survives.
///
/// # Errors
///
/// [`Error::NoSuchDefect`] if it is not there, [`Error::DefectLocked`] if it is
/// pinned.
pub fn remove(project: &mut Project, defect_id: i64) -> Result<()> {
    let locked: Option<i64> = project
        .conn()
        .query_row(
            "SELECT locked FROM defects WHERE defect_id = ?1",
            params![defect_id],
            |r| r.get(0),
        )
        .optional()?;
    match locked {
        None => Err(Error::NoSuchDefect { defect_id }),
        Some(flag) if flag != 0 => Err(Error::DefectLocked { defect_id }),
        Some(_) => {
            project.conn_mut().execute(
                "DELETE FROM defects WHERE defect_id = ?1",
                params![defect_id],
            )?;
            Ok(())
        }
    }
}

/// Removes a locked defect anyway.
///
/// The pair [`crate::track::delete_boundary_forced`] has, for the same caller: a
/// person who placed a row is allowed to take it away again, and the lock exists to
/// stop *analysis*, not them.
///
/// # Errors
///
/// [`Error::NoSuchDefect`] if it is not there.
pub fn remove_forced(project: &mut Project, defect_id: i64) -> Result<()> {
    let changed = project.conn_mut().execute(
        "DELETE FROM defects WHERE defect_id = ?1",
        params![defect_id],
    )?;
    if changed == 0 {
        return Err(Error::NoSuchDefect { defect_id });
    }
    Ok(())
}

/// Reads a row, refusing a repair nothing can carry out.
///
/// The inner `Result` is the refusal: `query_map` wants a `rusqlite::Result`, and a
/// column that says `Interpolat` is not a SQLite error, so it is carried out as a
/// value and unwrapped by the caller.
fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Defect>> {
    let id: i64 = row.get(0)?;
    let start: i64 = row.get(2)?;
    let end: i64 = row.get(3)?;
    let confidence: f64 = row.get(6)?;
    let provenance: String = row.get(7)?;
    let evidence: String = row.get(8)?;
    let repair: String = row.get(9)?;
    let locked: i64 = row.get(10)?;
    let Some(repair) = Repair::parse(&repair) else {
        return Ok(Err(Error::UnknownRepair {
            defect_id: id,
            repair,
        }));
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a confidence is a 0..=1 quantity, stored as REAL and read as f32"
    )]
    Ok(Ok(Defect {
        id,
        capture_id: row.get(1)?,
        start_frame: start.max(0) as u64,
        end_frame: end.max(0) as u64,
        channels: row.get(4)?,
        kind: row.get(5)?,
        confidence: confidence as f32,
        provenance: read_provenance(&provenance),
        evidence: read_evidence(&evidence),
        repair,
        locked: locked != 0,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    }))
}
