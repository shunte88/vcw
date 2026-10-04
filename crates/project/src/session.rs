/*
 *  session.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Capture sessions: one per contiguous recording, with its diagnostics
 *  counters.
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

//! Capture sessions: one per contiguous recording, with its diagnostics counters.
//!
//! §10 requires overruns, underruns, dropped frames and stream errors counted
//! *and persisted*, and §38 adds the provenance around them: device, backend,
//! format, rate, channels, duration, frame count. This is where all of that
//! lands, in the `captures` and `capture_diagnostics` tables.
//!
//! The counters belong to the recording, not to the process that made it. A
//! capture that overran in 2026 is still a capture that overran when someone
//! opens the project in 2031, and a diagnostics row that only existed in memory
//! would have told them nothing.
//!
//! # Why `frames` is updated as it goes
//!
//! [`Session::advance`] moves the frame count forward as blocks land, rather
//! than writing it once at the end. A session killed mid-capture has no end, and
//! recovery (WP-06) needs to know how far it got from committed rows alone. The
//! same reasoning makes `finished_at IS NULL` the signal for an unfinished
//! session: it is the absence of a write, which is the one thing a crash cannot
//! forge.

use rusqlite::{Connection, OptionalExtension, params};
use vcw_types::{
    CaptureEq, CaptureInfo, CaptureMode, CaptureState, Diagnostics, SampleRate, StorageFormat,
};

use crate::error::Result;
use crate::sqlite::Project;

/// A capture session's row, as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The session's id, referenced by every block it wrote.
    pub id: i64,
    /// What was recorded and through what (§38).
    pub info: CaptureInfo,
    /// How far the session got.
    pub state: CaptureState,
    /// Unix seconds at stream start.
    pub started_at: i64,
    /// Unix seconds at clean stop, or `None` for a session that never finished.
    pub finished_at: Option<i64>,
    /// Frames committed per channel.
    pub frames: u64,
    /// The four counters.
    pub diagnostics: Diagnostics,
}

impl Record {
    /// Duration in seconds, from the frame count and the rate. §38 asks for
    /// duration; storing it as well as the frames it is derived from would be
    /// one more thing that can disagree with itself.
    pub fn duration_secs(&self) -> f64 {
        if self.info.rate.hz() == 0 {
            return 0.0;
        }
        self.frames as f64 / f64::from(self.info.rate.hz())
    }

    /// Whether recovery has to look at this one.
    pub const fn needs_recovery(&self) -> bool {
        self.finished_at.is_none()
    }
}

/// A capture session that is open for writing.
///
/// Holds nothing but the id. The connection is passed in on every call, because
/// the writer thread owns it and a session that captured one would fight the
/// writer for it at exactly the wrong moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Session {
    id: i64,
}

impl Session {
    /// Opens a session and its diagnostics row, in one transaction.
    ///
    /// Both rows or neither: a capture whose diagnostics row is missing has
    /// nowhere to put the counters, and finding that out at the end of a
    /// 90-minute side would be a poor time to learn it.
    ///
    /// # Errors
    ///
    /// If the project is read-only, or the insert fails.
    pub fn begin(project: &mut Project, info: &CaptureInfo) -> Result<Self> {
        let started = crate::now();
        let tx = project.conn_mut().transaction()?;
        tx.execute(
            "INSERT INTO captures (
                 sample_rate, channels, storage_format, capture_mode, host_api,
                 device_id, device_name, os_verified, os_report, started_at,
                 finished_at, frames, state, capture_eq
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, 0, ?11, ?12)",
            params![
                info.rate.hz(),
                info.channels,
                info.storage_format.code(),
                info.capture_mode.as_str(),
                info.host_api,
                info.device_id,
                info.device_name,
                i64::from(info.os_verified),
                info.os_report,
                started,
                CaptureState::Recording.as_str(),
                info.eq.as_str(),
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO capture_diagnostics (capture_id, updated_at) VALUES (?1, ?2)",
            params![id, started],
        )?;
        tx.commit()?;
        Ok(Self { id })
    }

    /// Re-opens an existing session, for a writer resuming after recovery.
    pub const fn with_id(id: i64) -> Self {
        Self { id }
    }

    /// The session's id.
    pub const fn id(&self) -> i64 {
        self.id
    }

    /// Moves the committed frame count forward.
    ///
    /// Absolute rather than incremental: the writer knows how many frames it has
    /// committed, and an increment that is applied twice after a retry would
    /// quietly inflate the count.
    ///
    /// # Errors
    ///
    /// If the update fails.
    pub fn advance(&self, conn: &Connection, frames: u64) -> Result<()> {
        conn.execute(
            "UPDATE captures SET frames = ?2 WHERE capture_id = ?1",
            params![self.id, clamp(frames)],
        )?;
        Ok(())
    }

    /// Writes the counters. Call it whenever they change enough to matter, and
    /// once at the end whatever happens.
    ///
    /// # Errors
    ///
    /// If the update fails.
    pub fn record(&self, conn: &Connection, diagnostics: Diagnostics) -> Result<()> {
        conn.execute(
            "UPDATE capture_diagnostics
                SET overruns = ?2, underruns = ?3, dropped_frames = ?4,
                    stream_errors = ?5, updated_at = ?6
              WHERE capture_id = ?1",
            params![
                self.id,
                clamp(diagnostics.overruns),
                clamp(diagnostics.underruns),
                clamp(diagnostics.dropped_frames),
                clamp(diagnostics.stream_errors),
                crate::now(),
            ],
        )?;
        Ok(())
    }

    /// Records the verification outcome, once the OS has been asked.
    ///
    /// Separate from [`Session::begin`] because on ALSA the answer only exists
    /// while the stream is running, which is after the row has to be written.
    ///
    /// # Errors
    ///
    /// If the update fails.
    pub fn record_verification(
        &self,
        conn: &Connection,
        verified: bool,
        report: Option<&str>,
    ) -> Result<()> {
        conn.execute(
            "UPDATE captures SET os_verified = ?2, os_report = ?3 WHERE capture_id = ?1",
            params![self.id, i64::from(verified), report],
        )?;
        Ok(())
    }

    /// Closes the session: frames, counters, state and `finished_at`, atomically.
    ///
    /// `state` is the caller's honest assessment. A capture that ended because
    /// the device went away is [`CaptureState::Interrupted`] even though the
    /// stop was orderly, and calling it finalised would lose the one fact a
    /// later reader most needs.
    ///
    /// # Errors
    ///
    /// If the project is read-only, or the update fails.
    pub fn finish(
        &self,
        project: &mut Project,
        state: CaptureState,
        frames: u64,
        diagnostics: Diagnostics,
    ) -> Result<()> {
        let finished = crate::now();
        let tx = project.conn_mut().transaction()?;
        tx.execute(
            "UPDATE captures SET frames = ?2, state = ?3, finished_at = ?4
              WHERE capture_id = ?1",
            params![self.id, clamp(frames), state.as_str(), finished],
        )?;
        tx.execute(
            "UPDATE capture_diagnostics
                SET overruns = ?2, underruns = ?3, dropped_frames = ?4,
                    stream_errors = ?5, updated_at = ?6
              WHERE capture_id = ?1",
            params![
                self.id,
                clamp(diagnostics.overruns),
                clamp(diagnostics.underruns),
                clamp(diagnostics.dropped_frames),
                clamp(diagnostics.stream_errors),
                finished,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
}

/// Reads one session back.
///
/// # Errors
///
/// If the query fails.
pub fn load(conn: &Connection, id: i64) -> Result<Option<Record>> {
    let record = conn
        .query_row(
            &format!("{SELECT} WHERE c.capture_id = ?1"),
            params![id],
            row,
        )
        .optional()?;
    Ok(record)
}

/// Every session in the project, oldest first.
///
/// # Errors
///
/// If the query fails.
pub fn all(conn: &Connection) -> Result<Vec<Record>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY c.capture_id"))?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Sessions that never finished: what recovery looks for on the next launch (§15).
///
/// Keyed on `finished_at IS NULL` rather than on the state column, because the
/// absence of a write is the one thing a crash cannot forge. A process killed
/// mid-capture leaves `state` saying `recording`, but so would a bug that forgot
/// to update it; only the missing timestamp is evidence.
///
/// # Errors
///
/// If the query fails.
pub fn unfinished(conn: &Connection) -> Result<Vec<Record>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE c.finished_at IS NULL ORDER BY c.capture_id"
    ))?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

const SELECT: &str = "SELECT c.capture_id, c.sample_rate, c.channels, c.storage_format,
        c.capture_mode, c.host_api, c.device_id, c.device_name, c.os_verified,
        c.os_report, c.state, c.started_at, c.finished_at, c.frames,
        COALESCE(d.overruns, 0), COALESCE(d.underruns, 0),
        COALESCE(d.dropped_frames, 0), COALESCE(d.stream_errors, 0),
        c.capture_eq
   FROM captures c LEFT JOIN capture_diagnostics d ON d.capture_id = c.capture_id";

/// SQLite integers are signed, and every counter here is a `u64`. Saturating
/// rather than wrapping: at 192 kHz a frame count reaches `i64::MAX` after
/// roughly 1.5 million years, so the clamp is unreachable in practice, and if
/// it ever is reached a pinned maximum reads as obviously wrong where a
/// negative number would read as a small one.
pub(crate) const fn clamp(n: u64) -> i64 {
    if n > i64::MAX as u64 {
        i64::MAX
    } else {
        n as i64
    }
}

/// The inverse. A negative value can only come from a row VCW did not write;
/// zero is the least misleading thing to report for it.
pub(crate) const fn widen(n: i64) -> u64 {
    if n < 0 { 0 } else { n as u64 }
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
    let storage_code: u32 = r.get(3)?;
    let mode: String = r.get(4)?;
    let state: String = r.get(10)?;
    let eq: String = r.get(18)?;
    Ok(Record {
        id: r.get(0)?,
        info: CaptureInfo {
            rate: SampleRate(r.get(1)?),
            channels: r.get(2)?,
            // A code no version of VCW writes means a project written by
            // something else, or a damaged row. `validate()` reports it as
            // `unknown-format`; here it has to read as *something*, and the
            // widest integer is the least destructive guess to carry forward.
            storage_format: StorageFormat::from_code(storage_code).unwrap_or(StorageFormat::Int32),
            capture_mode: CaptureMode::parse(&mode).unwrap_or(CaptureMode::Shared),
            // A spelling no version of VCW wrote reads as unknown, which is both
            // the safe answer and the honest one: the column says something this
            // build cannot interpret, so the provenance is not known to it.
            eq: CaptureEq::parse(&eq).unwrap_or_default(),
            host_api: r.get(5)?,
            device_id: r.get(6)?,
            device_name: r.get(7)?,
            os_verified: r.get::<_, i64>(8)? != 0,
            os_report: r.get(9)?,
        },
        state: CaptureState::parse(&state).unwrap_or(CaptureState::Interrupted),
        started_at: r.get(11)?,
        finished_at: r.get(12)?,
        frames: widen(r.get(13)?),
        diagnostics: Diagnostics {
            overruns: widen(r.get(14)?),
            underruns: widen(r.get(15)?),
            dropped_frames: widen(r.get(16)?),
            stream_errors: widen(r.get(17)?),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::{Options, validate};
    use vcw_types::SampleRate;

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::create(dir.path().join("session.vcw")).expect("create");
        (dir, project)
    }

    fn info() -> CaptureInfo {
        CaptureInfo {
            rate: SampleRate(96_000),
            channels: 2,
            storage_format: StorageFormat::Int24Padded,
            capture_mode: CaptureMode::Exclusive,
            host_api: Some("ALSA".into()),
            device_id: Some("hw:CARD=0,DEV=0".into()),
            device_name: Some("Cirrus Analog".into()),
            os_verified: false,
            os_report: None,
            // Deliberately not the default. Every test below that round-trips
            // this record through SQLite would pass with `capture_eq` never
            // written at all if this said `Unknown`, because `Unknown` is what
            // the column defaults to and what a parse failure reads as.
            eq: CaptureEq::Riaa,
        }
    }

    #[test]
    fn a_new_session_starts_unfinished_and_at_zero() {
        let (_dir, mut p) = project();
        let s = Session::begin(&mut p, &info()).expect("begin");
        let r = load(p.conn(), s.id()).expect("load").expect("row");
        assert_eq!(r.state, CaptureState::Recording);
        assert_eq!(r.frames, 0);
        assert!(r.finished_at.is_none());
        assert!(r.needs_recovery());
        assert!(r.diagnostics.is_clean());
    }

    #[test]
    fn every_field_of_the_provenance_survives_the_round_trip() {
        let (_dir, mut p) = project();
        let want = info();
        let s = Session::begin(&mut p, &want).expect("begin");
        let got = load(p.conn(), s.id()).expect("load").expect("row").info;
        assert_eq!(got, want);
    }

    #[test]
    fn the_counters_are_persisted_not_merely_held() {
        // WP-04's exit criterion, stated as a test: a diagnostics count that
        // only exists in the process that made it has not been recorded.
        let (_dir, mut p) = project();
        let s = Session::begin(&mut p, &info()).expect("begin");
        let d = Diagnostics {
            overruns: 3,
            underruns: 1,
            dropped_frames: 1_440,
            stream_errors: 2,
        };
        s.record(p.conn(), d).expect("record");

        let path = p.path().to_path_buf();
        p.close().expect("close");
        let reopened = Project::open(&path).expect("reopen");
        assert_eq!(
            load(reopened.conn(), s.id())
                .expect("load")
                .expect("row")
                .diagnostics,
            d
        );
    }

    #[test]
    fn advancing_is_absolute_so_a_repeated_write_cannot_inflate_it() {
        let (_dir, mut p) = project();
        let s = Session::begin(&mut p, &info()).expect("begin");
        s.advance(p.conn(), 48_000).expect("advance");
        s.advance(p.conn(), 48_000).expect("again");
        assert_eq!(load(p.conn(), s.id()).unwrap().unwrap().frames, 48_000);
    }

    #[test]
    fn finishing_writes_frames_state_counters_and_the_timestamp_together() {
        let (_dir, mut p) = project();
        let s = Session::begin(&mut p, &info()).expect("begin");
        s.finish(
            &mut p,
            CaptureState::Finalised,
            96_000 * 60,
            Diagnostics::default(),
        )
        .expect("finish");

        let r = load(p.conn(), s.id()).expect("load").expect("row");
        assert_eq!(r.state, CaptureState::Finalised);
        assert_eq!(r.frames, 96_000 * 60);
        assert!(r.finished_at.is_some_and(|t| t >= r.started_at));
        assert!(!r.needs_recovery());
        assert!((r.duration_secs() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn an_orderly_stop_after_the_device_vanished_is_still_interrupted() {
        // R9. The stop was clean; the capture was not. Calling it finalised
        // would lose the one fact a later reader most needs.
        let (_dir, mut p) = project();
        let s = Session::begin(&mut p, &info()).expect("begin");
        s.finish(
            &mut p,
            CaptureState::Interrupted,
            1_024,
            Diagnostics {
                stream_errors: 1,
                ..Diagnostics::default()
            },
        )
        .expect("finish");
        let r = load(p.conn(), s.id()).expect("load").expect("row");
        assert_eq!(r.state, CaptureState::Interrupted);
        assert!(!r.diagnostics.is_clean());
        // Interrupted but *ended*: recovery has nothing left to do here.
        assert!(!r.needs_recovery());
    }

    #[test]
    fn recovery_finds_the_session_that_never_ended() {
        let (_dir, mut p) = project();
        let killed = Session::begin(&mut p, &info()).expect("begin");
        killed.advance(p.conn(), 4_096).expect("advance");
        let clean = Session::begin(&mut p, &info()).expect("begin");
        clean
            .finish(&mut p, CaptureState::Finalised, 8, Diagnostics::default())
            .expect("finish");

        let open = unfinished(p.conn()).expect("unfinished");
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, killed.id());
        assert_eq!(open[0].frames, 4_096, "how far it got, from committed rows");
        assert_eq!(all(p.conn()).expect("all").len(), 2);
    }

    #[test]
    fn verification_is_recorded_after_the_fact_because_that_is_when_it_is_known() {
        let (_dir, mut p) = project();
        let s = Session::begin(&mut p, &info()).expect("begin");
        assert!(!load(p.conn(), s.id()).unwrap().unwrap().info.os_verified);

        s.record_verification(p.conn(), true, Some("format: S32_LE\nrate: 96000"))
            .expect("record");
        let got = load(p.conn(), s.id()).unwrap().unwrap().info;
        assert!(got.os_verified);
        assert!(got.os_report.expect("report").contains("96000"));
    }

    #[test]
    fn a_persisted_session_passes_validation() {
        let (_dir, mut p) = project();
        let s = Session::begin(&mut p, &info()).expect("begin");
        s.finish(&mut p, CaptureState::Finalised, 480, Diagnostics::default())
            .expect("finish");
        let report = validate(&p, Options::default()).expect("validate");
        assert!(report.is_clean(), "{:?}", report.findings);
        assert_eq!(report.captures, 1);
    }

    #[test]
    fn an_unfinished_session_passes_validation_too() {
        // A capture in progress is not a damaged project. If validation
        // rejected it, every crash would look like corruption and recovery
        // would have nothing sound to work from.
        let (_dir, mut p) = project();
        Session::begin(&mut p, &info()).expect("begin");
        let report = validate(&p, Options::default()).expect("validate");
        assert!(report.is_clean(), "{:?}", report.findings);
    }

    #[test]
    fn a_missing_session_is_none_rather_than_an_error() {
        let (_dir, p) = project();
        assert!(load(p.conn(), 99).expect("load").is_none());
        assert!(all(p.conn()).expect("all").is_empty());
    }

    #[test]
    fn counters_saturate_rather_than_wrap_through_the_database() {
        let (_dir, mut p) = project();
        let s = Session::begin(&mut p, &info()).expect("begin");
        s.record(
            p.conn(),
            Diagnostics {
                overruns: u64::MAX,
                ..Diagnostics::default()
            },
        )
        .expect("record");
        let got = load(p.conn(), s.id()).unwrap().unwrap().diagnostics;
        assert_eq!(got.overruns, i64::MAX as u64);
        assert!(!got.is_clean(), "a pinned maximum still reads as damage");
    }
}
