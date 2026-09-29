/*
 *  recovery.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Detecting and reconstructing an unfinished session (§15).
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

//! Detecting and reconstructing an unfinished session (§15).
//!
//! # What a crash actually leaves
//!
//! Nothing here reconstructs audio. The audio is already on disk: every block
//! was written inside a transaction that `synchronous=FULL` fsynced before it
//! returned, so a committed block survives the process dying between one commit
//! and the next. What a crash destroys is the *bookkeeping* - the row that says
//! how long the capture was, how it ended, and when. Recovery's job is to
//! rebuild that from the blocks, which are the only witnesses left.
//!
//! S2 established the floor this cannot beat, and [S1 corrected it]: worst-case
//! loss is commit granularity *plus the driver buffer*, rounded up to a block
//! boundary. Ring size is irrelevant. Anything claiming better is measuring
//! something other than a power cut.
//!
//! # Why `finished_at IS NULL` and not the state column
//!
//! A process killed mid-capture leaves `state` reading `recording`, but so
//! would a bug that forgot to update it, and so would a future version with a
//! different idea of the vocabulary. `finished_at` is only ever written by
//! [`Session::finish`](crate::session::Session::finish), in the same transaction as the state. Its absence is
//! the absence of a write, which is the one thing a crash cannot forge.
//!
//! # Why the blocks outrank the row
//!
//! [`Session::advance`](crate::session::Session::advance) moves `captures.frames` forward *inside the block
//! transaction*, so the two cannot disagree - and recovery still recomputes the
//! count from the blocks rather than trusting it. The invariant is the thing
//! being checked; a recovery tool that assumes it can only confirm what it
//! already believed. In practice the numbers always match, and the day they do
//! not is the day this module earns its place.
//!
//! # What recovery will not do without being asked
//!
//! Blocks are immutable and are never deleted to tidy up (D4). If a capture
//! somehow holds blocks past the point where every channel agrees - a ragged
//! tail, or blocks stranded after a gap - recovery reports them and stops,
//! and only removes them under [`Plan::Repair`]. Destroying captured audio is
//! an operator's decision, not a library's.
//!
//! [S1 corrected it]: https://github.com/shunte88/vcw/blob/main/docs/spikes/S1-cpal-bitperfect.md

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use vcw_types::{CaptureInfo, CaptureState, Diagnostics};

use crate::error::{Error, Result};
use crate::session::{self, Record};
use crate::sqlite::{Access, Project};

/// The `-wal` and `-shm` files beside a project, as they are on disk.
///
/// Worth reading *before* opening the project, because opening it is what makes
/// the evidence disappear: SQLite replays and may checkpoint a hot log during
/// connection setup, so by the time there is a [`Project`] to ask, the question
/// has already been answered and forgotten.
///
/// A non-empty `-wal` means the last process to hold this file did not close it
/// (§15). That is a hint and not a verdict - a reader that crashed leaves the
/// same trace as a writer that did - so it informs the report and never drives
/// the decision. [`session::unfinished`] drives the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecars {
    /// The project itself.
    pub project: PathBuf,
    /// Size of `-wal` in bytes, zero if absent.
    pub wal_bytes: u64,
    /// Size of `-shm` in bytes, zero if absent.
    pub shm_bytes: u64,
}

impl Sidecars {
    /// Reads the sidecars' sizes without opening anything.
    #[must_use]
    pub fn inspect(project: impl AsRef<Path>) -> Self {
        let project = project.as_ref().to_path_buf();
        Self {
            wal_bytes: size(&sidecar(&project, "-wal")),
            shm_bytes: size(&sidecar(&project, "-shm")),
            project,
        }
    }

    /// Whether the last process to hold the project left a log behind.
    #[must_use]
    pub const fn log_left_behind(&self) -> bool {
        self.wal_bytes > 0
    }

    /// Whether either sidecar exists at all.
    #[must_use]
    pub const fn present(&self) -> bool {
        self.wal_bytes > 0 || self.shm_bytes > 0
    }
}

fn sidecar(project: &Path, suffix: &str) -> PathBuf {
    let mut name = project.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Something recovery noticed that the operator should see.
///
/// Not an error: every one of these is survivable, and a recovery that refused
/// to proceed on the first oddity would be useless on exactly the files it
/// exists for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Stable machine-readable identifier, for tests and for the UI.
    pub code: &'static str,
    /// Human-readable detail, naming the rows and the numbers.
    pub detail: String,
}

/// Everything recovery could work out about one unfinished capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    /// The capture this is about.
    pub capture_id: i64,
    /// What was being recorded and through what (§38), as the row has it.
    pub info: CaptureInfo,
    /// The state the row claims, which for a killed process is `recording`.
    pub state: CaptureState,
    /// Unix seconds at stream start.
    pub started_at: i64,
    /// Frames the `captures` row declares.
    pub declared_frames: u64,
    /// Frames every channel actually has committed, contiguously, from zero.
    ///
    /// The number recovery will write, and the honest length of the recording.
    pub usable_frames: u64,
    /// Block rows belonging to this capture.
    pub blocks: u64,
    /// Blocks past the point every channel agrees on, by `blockid`.
    ///
    /// Empty in every capture a correct writer produced, because blocks for all
    /// channels are committed in one transaction. Non-empty means either a bug
    /// or a file that has been edited by something else, and either way the
    /// audio in them is real and is not thrown away without being asked.
    pub surplus: Vec<i64>,
    /// Distinct channels that have at least one block.
    pub channels_present: u16,
    /// Unix seconds of the newest block's commit: when the recording really
    /// stopped, as opposed to when anyone noticed.
    pub last_committed_at: Option<i64>,
    /// The counters as last persisted, which is not necessarily as at the crash.
    pub diagnostics: Diagnostics,
    /// Unix seconds at which those counters were written.
    pub diagnostics_at: i64,
    /// What recovery noticed.
    pub notes: Vec<Note>,
}

impl Assessment {
    /// Length of the recoverable audio in seconds.
    #[must_use]
    pub fn duration_secs(&self) -> f64 {
        if self.info.rate.hz() == 0 {
            return 0.0;
        }
        self.usable_frames as f64 / f64::from(self.info.rate.hz())
    }

    /// Whether the blocks and the row already agree and nothing is stranded.
    ///
    /// True for essentially every real crash: the writer's transaction covers
    /// the blocks *and* the frame count, so a kill lands between transactions
    /// and leaves a consistent, merely unfinished, capture. All recovery has to
    /// do then is say so.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.surplus.is_empty() && self.declared_frames == self.usable_frames
    }

    /// Whether there is any audio at all to keep.
    #[must_use]
    pub const fn has_audio(&self) -> bool {
        self.usable_frames > 0
    }

    /// How far behind the last block the counters are, in seconds.
    ///
    /// The honest measure of how much to trust them. A large number does not
    /// mean the capture was bad; it means nobody was told whether it was.
    #[must_use]
    pub fn counter_lag_secs(&self) -> Option<i64> {
        self.last_committed_at
            .map(|last| (last - self.diagnostics_at).max(0))
    }

    /// Whether any note carries this code.
    #[must_use]
    pub fn has(&self, code: &str) -> bool {
        self.notes.iter().any(|n| n.code == code)
    }
}

/// How much recovery is allowed to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Plan {
    /// Work out what would happen and write nothing.
    #[default]
    DryRun,
    /// Write the truth into the `captures` row. Refuses if blocks are stranded,
    /// because tidying those away is [`Plan::Repair`]'s decision to make.
    Commit,
    /// Write the row and delete stranded blocks.
    Repair,
}

/// What recovery did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// The capture recovered.
    pub capture_id: i64,
    /// The frame count written.
    pub frames: u64,
    /// Blocks deleted, which is zero unless [`Plan::Repair`] was used.
    pub blocks_removed: u64,
    /// The state written.
    pub state: CaptureState,
    /// The `finished_at` written: the last block's commit time, not now.
    pub finished_at: i64,
    /// Whether anything was actually written.
    pub applied: bool,
}

/// Every unfinished capture in the project, assessed (§15).
///
/// # Errors
///
/// If the project cannot be read.
pub fn survey(conn: &Connection) -> Result<Vec<Assessment>> {
    session::unfinished(conn)?
        .into_iter()
        .map(|record| assess_record(conn, record))
        .collect()
}

/// Assesses one capture, whether or not it is unfinished.
///
/// # Errors
///
/// If the project cannot be read.
pub fn assess(conn: &Connection, capture_id: i64) -> Result<Option<Assessment>> {
    match session::load(conn, capture_id)? {
        Some(record) => assess_record(conn, record).map(Some),
        None => Ok(None),
    }
}

fn assess_record(conn: &Connection, record: Record) -> Result<Assessment> {
    let mut notes = Vec::new();
    let coverage = walk(conn, record.id, &mut notes)?;

    let declared_channels = record.info.channels;
    let usable = (0..declared_channels)
        .map(|ch| coverage.prefix.get(&ch).copied().unwrap_or(0))
        .min()
        .unwrap_or(0);

    for ch in 0..declared_channels {
        if !coverage.prefix.contains_key(&ch) {
            notes.push(Note {
                code: "missing-channel",
                detail: format!(
                    "capture {} declares {declared_channels} channels but channel {ch} has no blocks",
                    record.id
                ),
            });
        }
    }

    // Anything starting at or after the agreed end, plus anything the walk
    // already stranded behind a gap. A block cannot straddle the boundary:
    // `usable` is a minimum over per-channel prefixes and every prefix ends on
    // a block boundary.
    let mut surplus = coverage.stranded;
    for (blockid, start) in coverage.starts {
        if start >= usable && !surplus.contains(&blockid) {
            surplus.push(blockid);
        }
    }
    surplus.sort_unstable();

    if !surplus.is_empty() {
        notes.push(Note {
            code: "surplus-blocks",
            detail: format!(
                "capture {} has {} block(s) past frame {usable}, where the channels stop agreeing",
                record.id,
                surplus.len()
            ),
        });
    }
    if record.frames != usable {
        notes.push(Note {
            code: if record.frames > usable {
                "frame-count-ahead"
            } else {
                "frame-count-behind"
            },
            detail: format!(
                "capture {} declares {} frames; the blocks hold {usable}",
                record.id, record.frames
            ),
        });
    }
    if coverage.blocks == 0 {
        notes.push(Note {
            code: "no-blocks",
            detail: format!(
                "capture {} committed nothing; it died inside its first block",
                record.id
            ),
        });
    }

    let mut assessment = Assessment {
        capture_id: record.id,
        info: record.info,
        state: record.state,
        started_at: record.started_at,
        declared_frames: record.frames,
        usable_frames: usable,
        blocks: coverage.blocks,
        surplus,
        channels_present: coverage.prefix.len() as u16,
        last_committed_at: coverage.last_committed_at,
        diagnostics: record.diagnostics,
        diagnostics_at: diagnostics_at(conn, record.id)?,
        notes,
    };

    // Reported last so the number is against the final assessment, and only
    // where it means something: a capture with no blocks has nothing to be
    // stale relative to.
    if let Some(lag) = assessment.counter_lag_secs()
        && lag > STALE_COUNTERS_SECS
    {
        assessment.notes.push(Note {
            code: "stale-counters",
            detail: format!(
                "capture {}'s counters were last written {lag} s before the final block, \
                 so overruns after that point were never recorded",
                assessment.capture_id
            ),
        });
    }
    Ok(assessment)
}

/// How far behind the audio the counters may fall before it is worth saying so.
///
/// Four times the writer's 2 s default, so an ordinary capture never trips it
/// and one written by a build with the timer disabled always does.
const STALE_COUNTERS_SECS: i64 = 8;

#[derive(Debug, Default)]
struct Coverage {
    /// Contiguous frames from zero, per channel.
    prefix: std::collections::BTreeMap<u16, u64>,
    /// `(blockid, start_frame)` for every block, for the surplus sweep.
    starts: Vec<(i64, u64)>,
    /// Blocks after a discontinuity: real audio at an unknown offset.
    stranded: Vec<i64>,
    blocks: u64,
    last_committed_at: Option<i64>,
}

/// Walks a capture's blocks in timeline order and measures what is contiguous.
///
/// Deliberately a walk and not an aggregate. `SUM(frame_count)` would say a
/// capture with a hole in it has all its frames, which is the one answer that
/// must never be given: the blocks after the hole are at the wrong offsets, and
/// a recovery that counted them would place every sample after the gap
/// somewhere it was not recorded.
fn walk(conn: &Connection, capture_id: i64, notes: &mut Vec<Note>) -> Result<Coverage> {
    let mut stmt = conn.prepare(
        "SELECT channel, sequence, start_frame, frame_count, blockid, committed_at
           FROM capture_blocks WHERE capture_id = ?1 ORDER BY channel, sequence",
    )?;
    let mut rows = stmt.query(params![capture_id])?;

    let mut coverage = Coverage::default();
    let mut channel_now: Option<u16> = None;
    let mut next_frame: u64 = 0;
    let mut next_sequence: i64 = 0;
    let mut broken = false;

    while let Some(row) = rows.next()? {
        let channel: i64 = row.get(0)?;
        let sequence: i64 = row.get(1)?;
        let start: i64 = row.get(2)?;
        let frames: i64 = row.get(3)?;
        let blockid: i64 = row.get(4)?;
        let committed: Option<i64> = row.get(5)?;
        let channel = channel.clamp(0, i64::from(u16::MAX)) as u16;

        if channel_now != Some(channel) {
            if let Some(previous) = channel_now {
                coverage.prefix.insert(previous, next_frame);
            }
            channel_now = Some(channel);
            next_frame = 0;
            next_sequence = 0;
            broken = false;
        }

        coverage.blocks += 1;
        coverage.starts.push((blockid, start.max(0) as u64));
        coverage.last_committed_at = match (coverage.last_committed_at, committed) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };

        if broken {
            coverage.stranded.push(blockid);
            continue;
        }
        if sequence != next_sequence || start.max(0) as u64 != next_frame || frames <= 0 {
            notes.push(Note {
                code: "timeline-gap",
                detail: format!(
                    "capture {capture_id} channel {channel} breaks at sequence {sequence}: \
                     block {blockid} starts at frame {start} with {frames} frame(s), \
                     expected sequence {next_sequence} at frame {next_frame}"
                ),
            });
            broken = true;
            coverage.stranded.push(blockid);
            continue;
        }
        next_frame += frames as u64;
        next_sequence += 1;
    }
    if let Some(last) = channel_now {
        coverage.prefix.insert(last, next_frame);
    }
    Ok(coverage)
}

fn diagnostics_at(conn: &Connection, capture_id: i64) -> Result<i64> {
    let at = conn
        .query_row(
            "SELECT updated_at FROM capture_diagnostics WHERE capture_id = ?1",
            params![capture_id],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(at.unwrap_or(0))
}

/// Writes the assessment into the project, closing the capture honestly.
///
/// The `captures` row gets the frame count the blocks support,
/// [`CaptureState::Recovered`], and a `finished_at` of the **last block's commit
/// time** rather than now. Now is when someone opened the file, which may be
/// days later; the last commit is when the audio stopped, and it is the only
/// answer the file can support.
///
/// The counters are left exactly as the writer last wrote them. Recovery knows
/// they may be stale and says so in [`Assessment::notes`]; overwriting them with
/// zeros to look tidy would turn "we do not know" into "nothing went wrong".
///
/// # Errors
///
/// [`Error::ReadOnly`] if the project cannot be written,
/// [`Error::StrandedBlocks`] under [`Plan::Commit`] when there are surplus
/// blocks, or whatever the write failed on.
pub fn recover(project: &mut Project, assessment: &Assessment, plan: Plan) -> Result<Recovered> {
    let finished_at = assessment
        .last_committed_at
        .unwrap_or(assessment.started_at);
    let mut outcome = Recovered {
        capture_id: assessment.capture_id,
        frames: assessment.usable_frames,
        blocks_removed: 0,
        state: CaptureState::Recovered,
        finished_at,
        applied: false,
    };

    if plan == Plan::DryRun {
        return Ok(outcome);
    }
    if project.access() != Access::ReadWrite {
        return Err(Error::ReadOnly {
            path: project.path().to_path_buf(),
        });
    }
    if plan == Plan::Commit && !assessment.surplus.is_empty() {
        return Err(Error::StrandedBlocks {
            capture_id: assessment.capture_id,
            blocks: assessment.surplus.len(),
        });
    }

    let tx = project.conn_mut().transaction()?;
    if plan == Plan::Repair {
        for blockid in &assessment.surplus {
            // capture_blocks first: it holds the foreign key into sampleblocks,
            // and the other order fails with foreign_keys on, which is how we
            // want it to fail if this is ever reordered by accident.
            tx.execute(
                "DELETE FROM capture_blocks WHERE blockid = ?1",
                params![blockid],
            )?;
            tx.execute(
                "DELETE FROM sampleblocks WHERE blockid = ?1",
                params![blockid],
            )?;
            outcome.blocks_removed += 1;
        }
    }
    tx.execute(
        "UPDATE captures SET frames = ?2, state = ?3, finished_at = ?4 WHERE capture_id = ?1",
        params![
            assessment.capture_id,
            session::clamp(assessment.usable_frames),
            CaptureState::Recovered.as_str(),
            finished_at,
        ],
    )?;
    tx.commit()?;

    // `warn`, not `info`. Recovery is a repair, and the fact that a project
    // needed one is the interesting part of the story afterwards - "how much did
    // I lose in that crash" is answered by these two numbers.
    tracing::warn!(
        capture_id = assessment.capture_id,
        frames = outcome.frames,
        blocks_removed = outcome.blocks_removed,
        "recovered an unfinished capture"
    );
    outcome.applied = true;
    Ok(outcome)
}

/// Recovers every unfinished capture in the project.
///
/// # Errors
///
/// As [`recover`], on the first capture that cannot be recovered. Captures
/// already written are left written: a partial recovery of a multi-capture
/// project is better than none, and the ones that succeeded are now correct.
pub fn recover_all(project: &mut Project, plan: Plan) -> Result<Vec<Recovered>> {
    let assessments = survey(project.conn())?;
    if !assessments.is_empty() {
        tracing::warn!(
            captures = assessments.len(),
            dry_run = plan == Plan::DryRun,
            "the project holds captures that never finished"
        );
    }
    let mut done = Vec::with_capacity(assessments.len());
    for assessment in &assessments {
        done.push(recover(project, assessment, plan)?);
    }
    Ok(done)
}

/// Folds the write-ahead log back into the project and truncates it (§15).
///
/// Returns the size of the log beforehand, which is what was at risk. Part of
/// the lifecycle rather than an optimisation: a project left with a large log
/// is a project whose next reader has more to replay and whose next backup is
/// incomplete if it copies only the one file.
///
/// # Errors
///
/// If the project is read-only, or the checkpoint fails.
pub fn checkpoint(project: &Project) -> Result<u64> {
    if project.access() != Access::ReadWrite {
        return Err(Error::ReadOnly {
            path: project.path().to_path_buf(),
        });
    }
    let before = size(&sidecar(project.path(), "-wal"));
    project
        .conn()
        .pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .or_else(|_| {
            project
                .conn()
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        })?;
    Ok(before)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{Config, Writer};
    use crate::session::Session;
    use crate::validate::{Options, validate};
    use vcw_types::{CaptureMode, SampleRate, StorageFormat};

    const RATE: u32 = 48_000;
    const WIDTH: usize = 4;
    const CHANNELS: u16 = 2;

    fn info() -> CaptureInfo {
        CaptureInfo {
            rate: SampleRate(RATE),
            channels: CHANNELS,
            storage_format: StorageFormat::Int24Padded,
            capture_mode: CaptureMode::Exclusive,
            host_api: Some("ALSA".into()),
            device_id: Some("hw:CARD=0,DEV=0".into()),
            device_name: Some("Cirrus Analog".into()),
            os_verified: false,
            os_report: None,
        }
    }

    /// Writes `frames` of audio and then walks away without finishing, which is
    /// what a killed process leaves behind: committed blocks, an open row, and
    /// nobody left to close it.
    fn abandoned(path: &Path, frames: u64) -> i64 {
        let mut project = Project::create(path).expect("create");
        let info = info();
        let session = Session::begin(&mut project, &info).expect("begin");
        let id = session.id();
        let mut writer =
            Writer::resume(project, session, &info, Config::default()).expect("resume");
        let bytes = frames as usize * CHANNELS as usize * WIDTH;
        let data: Vec<u8> = (0..bytes).map(|i| (i % 251) as u8).collect();
        writer.push(&data).expect("push");
        // No finish(), no close(): the Drop impls run and that is all a crash
        // gets either.
        drop(writer);
        id
    }

    fn open(path: &Path) -> Project {
        Project::open(path).expect("open")
    }

    #[test]
    fn an_unfinished_capture_is_found_and_a_finished_one_is_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("killed.vcw");
        let id = abandoned(&path, 30_000);

        let project = open(&path);
        let found = survey(project.conn()).expect("survey");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].capture_id, id);
        assert_eq!(found[0].state, CaptureState::Recording);

        let mut project = project;
        let assessment = found.into_iter().next().expect("one");
        recover(&mut project, &assessment, Plan::Commit).expect("recover");
        assert!(
            survey(project.conn()).expect("survey").is_empty(),
            "a recovered capture is not offered for recovery again"
        );
    }

    #[test]
    fn the_blocks_decide_the_length_not_the_row() {
        // The whole method of this module. Corrupt the row deliberately and
        // check that recovery believes the audio instead.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("liar.vcw");
        let id = abandoned(&path, 24_000);

        let mut project = open(&path);
        project
            .conn()
            .execute(
                "UPDATE captures SET frames = 999999 WHERE capture_id = ?1",
                [id],
            )
            .expect("lie");

        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        assert_eq!(assessment.declared_frames, 999_999);
        assert_eq!(assessment.usable_frames, 24_000);
        assert!(assessment.has("frame-count-ahead"));
        assert!(!assessment.is_consistent());

        recover(&mut project, &assessment, Plan::Commit).expect("recover");
        let record = session::load(project.conn(), id)
            .expect("load")
            .expect("row");
        assert_eq!(record.frames, 24_000);
        assert_eq!(record.state, CaptureState::Recovered);
    }

    #[test]
    fn an_ordinary_kill_leaves_a_capture_that_already_agrees_with_itself() {
        // The common case, and the reason the exit criterion is achievable:
        // WP-05 moves the frame count inside the block transaction, so a kill
        // lands between transactions and there is nothing to reconcile.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ordinary.vcw");
        let id = abandoned(&path, 37_000);

        let project = open(&path);
        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        assert!(assessment.is_consistent(), "{:?}", assessment.notes);
        assert!(assessment.surplus.is_empty());
        assert!(assessment.has_audio());
        // 37000 frames is three whole 250 ms blocks at 48 kHz plus a remainder
        // the writer was still holding when it died. That remainder is the loss.
        assert_eq!(assessment.usable_frames, 36_000);
        assert!((assessment.duration_secs() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn finished_at_is_when_the_audio_stopped_not_when_recovery_ran() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("when.vcw");
        let id = abandoned(&path, 24_000);

        let mut project = open(&path);
        // Backdate every block by an hour: the file has been sitting unopened.
        let then = crate::now() - 3_600;
        project
            .conn()
            .execute("UPDATE capture_blocks SET committed_at = ?1", [then])
            .expect("backdate");

        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        let done = recover(&mut project, &assessment, Plan::Commit).expect("recover");
        assert_eq!(done.finished_at, then);

        let record = session::load(project.conn(), id)
            .expect("load")
            .expect("row");
        assert_eq!(record.finished_at, Some(then));
        assert!(
            record.finished_at.expect("some") < crate::now() - 3_000,
            "recovery stamped its own clock onto the recording"
        );
    }

    #[test]
    fn a_capture_that_died_before_its_first_block_is_still_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nothing.vcw");
        let id = abandoned(&path, 10); // far less than one block

        let mut project = open(&path);
        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        assert_eq!(assessment.usable_frames, 0);
        assert_eq!(assessment.blocks, 0);
        assert!(!assessment.has_audio());
        assert!(assessment.has("no-blocks"));
        assert!(assessment.has("missing-channel"));

        let done = recover(&mut project, &assessment, Plan::Commit).expect("recover");
        assert_eq!(done.frames, 0);
        // §15 wants the attempt on record even when it produced nothing: the
        // row is the only evidence the recording was tried at all.
        assert_eq!(done.finished_at, assessment.started_at);
        assert!(survey(project.conn()).expect("survey").is_empty());
    }

    #[test]
    fn a_dry_run_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dry.vcw");
        let id = abandoned(&path, 24_000);

        let mut project = open(&path);
        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        let done = recover(&mut project, &assessment, Plan::DryRun).expect("dry run");
        assert!(!done.applied);
        assert_eq!(done.frames, 24_000);

        let record = session::load(project.conn(), id)
            .expect("load")
            .expect("row");
        assert_eq!(record.state, CaptureState::Recording);
        assert_eq!(record.finished_at, None);
        assert_eq!(survey(project.conn()).expect("survey").len(), 1);
    }

    #[test]
    fn stranded_blocks_are_refused_and_then_removed_only_when_asked() {
        // A ragged tail cannot come from the writer, whose transaction covers
        // every channel. Make one by hand and check recovery will not silently
        // delete audio to make the numbers tidy.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ragged.vcw");
        let id = abandoned(&path, 36_000);

        let mut project = open(&path);
        let last: i64 = project
            .conn()
            .query_row(
                "SELECT blockid FROM capture_blocks WHERE capture_id = ?1 AND channel = 1
                 ORDER BY sequence DESC LIMIT 1",
                [id],
                |r| r.get(0),
            )
            .expect("blockid");
        // Both rows, the way a real ragged tail would be: leaving the
        // sampleblock behind would be an orphan, which is a different fault
        // and one validate() already has a code for.
        project
            .conn()
            .execute("DELETE FROM capture_blocks WHERE blockid = ?1", [last])
            .expect("delete");
        project
            .conn()
            .execute("DELETE FROM sampleblocks WHERE blockid = ?1", [last])
            .expect("delete");

        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        assert_eq!(assessment.usable_frames, 24_000);
        assert_eq!(assessment.surplus.len(), 1, "{:?}", assessment.surplus);
        assert!(assessment.has("surplus-blocks"));

        match recover(&mut project, &assessment, Plan::Commit) {
            Err(Error::StrandedBlocks { blocks: 1, .. }) => {}
            other => panic!("Commit should have refused: {other:?}"),
        }

        let done = recover(&mut project, &assessment, Plan::Repair).expect("repair");
        assert_eq!(done.blocks_removed, 1);
        assert_eq!(done.frames, 24_000);
        assert!(
            validate(&project, Options::default())
                .expect("validate")
                .is_clean(),
            "a repaired project has to be a valid one"
        );
    }

    #[test]
    fn a_block_stranded_behind_a_gap_is_not_counted_as_length() {
        // The reason assessment walks instead of summing: SUM(frame_count)
        // would report 36000 here and place 12000 frames of audio at an offset
        // they were never recorded at.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gap.vcw");
        let id = abandoned(&path, 36_000);

        let project = open(&path);
        project
            .conn()
            .execute(
                "DELETE FROM capture_blocks WHERE capture_id = ?1 AND sequence = 1",
                [id],
            )
            .expect("punch");

        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        assert_eq!(assessment.usable_frames, 12_000);
        assert!(assessment.has("timeline-gap"));
        // Both channels' third blocks are stranded behind the hole.
        assert_eq!(assessment.surplus.len(), 2);
    }

    #[test]
    fn the_counters_survive_the_crash_and_their_staleness_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("counters.vcw");
        let id = abandoned(&path, 24_000);

        let project = open(&path);
        // What the writer's timer would have left: real counters, written some
        // time before the end.
        project
            .conn()
            .execute(
                "UPDATE capture_diagnostics SET overruns = 3, dropped_frames = 480,
                        updated_at = ?2 WHERE capture_id = ?1",
                params![id, crate::now() - 100],
            )
            .expect("counters");

        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        assert_eq!(assessment.diagnostics.overruns, 3);
        assert_eq!(assessment.diagnostics.dropped_frames, 480);
        assert!(assessment.counter_lag_secs().expect("lag") >= 99);
        assert!(assessment.has("stale-counters"));

        let mut project = project;
        recover(&mut project, &assessment, Plan::Commit).expect("recover");
        let record = session::load(project.conn(), id)
            .expect("load")
            .expect("row");
        assert_eq!(
            record.diagnostics.overruns, 3,
            "recovery must not tidy the counters away"
        );
        assert_eq!(record.diagnostics.dropped_frames, 480);
    }

    #[test]
    fn a_read_only_project_is_refused_rather_than_panicked_over() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ro.vcw");
        let id = abandoned(&path, 24_000);
        open(&path).close().expect("close");

        let mut project = Project::open_read_only(&path).expect("ro");
        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        // A dry run is always allowed: inspecting a project is the reason to
        // have opened it read-only in the first place.
        assert!(recover(&mut project, &assessment, Plan::DryRun).is_ok());
        match recover(&mut project, &assessment, Plan::Commit) {
            Err(Error::ReadOnly { .. }) => {}
            other => panic!("expected ReadOnly, got {other:?}"),
        }
        assert!(matches!(checkpoint(&project), Err(Error::ReadOnly { .. })));
    }

    #[test]
    fn dropping_a_project_still_clears_the_log_because_sqlite_closes_it() {
        // Worth pinning, because it is the opposite of what the phrase
        // "simulating a crash by dropping the writer" suggests. Dropping a
        // `Project` in-process runs `sqlite3_close`, and SQLite checkpoints and
        // *deletes* the sidecars when the last connection to a file goes. So a
        // dropped writer reproduces a crash's database state - committed
        // blocks, an open row - and not its filesystem state. Only a process
        // that is actually killed leaves a hot log, which is why the exit
        // criterion for WP-06 is an out-of-process kill and not this.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dropped.vcw");
        abandoned(&path, 24_000);
        assert!(!Sidecars::inspect(&path).present());
    }

    #[test]
    fn a_log_left_behind_is_visible_before_anything_opens_the_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hot.vcw");

        // A second connection held open is what a killed process leaves from
        // SQLite's point of view: the file has a live reference, so the log is
        // not folded back when the writer's own connection goes. It has to be
        // taken after the project exists, because opening one would create the
        // file and `Project::create` refuses to overwrite.
        let mut project = Project::create(&path).expect("create");
        let info = info();
        let session = Session::begin(&mut project, &info).expect("begin");
        let mut writer =
            Writer::resume(project, session, &info, Config::default()).expect("resume");
        let bytes = 24_000usize * CHANNELS as usize * WIDTH;
        writer
            .push(&(0..bytes).map(|i| (i % 251) as u8).collect::<Vec<u8>>())
            .expect("push");
        let keep_open = Connection::open(&path).expect("keep open");
        // It has to actually read: SQLite attaches to the `-wal` and `-shm`
        // lazily, so a connection that has touched nothing is not a reference
        // and does not stop the close from cleaning them up.
        let _: i64 = keep_open
            .query_row("SELECT COUNT(*) FROM captures", [], |r| r.get(0))
            .expect("read");
        drop(writer);

        // Reading the sidecars has to happen before any *new* connection,
        // because connecting is what replays and may remove them.
        let hot = Sidecars::inspect(&path);
        assert!(hot.log_left_behind(), "{hot:?}");
        assert!(hot.present());
        assert_eq!(hot.project, path);
        // Checkpointed while the log is still hot, which is the case that has
        // a number worth returning: `checkpoint` reports what it found, so the
        // caller can say how much was at risk.
        //
        // A floor rather than an identity, and the difference is a real one.
        // `Project::open` stamps `last_written_at` on its way in, so by the
        // time the checkpoint runs the log can hold one more frame than the
        // inspection saw - 64 KiB, one page. Whether it does depends on the
        // checkpoint SQLite attempts when the writer's connection closes, which
        // `keep_open` can only partly block: an equality here passes most runs
        // and fails perhaps one in twenty-five. The claim worth making is that
        // the checkpoint found everything the inspection did.
        let project = open(&path);
        let folded = checkpoint(&project).expect("checkpoint");
        assert!(
            folded >= hot.wal_bytes,
            "the checkpoint folded {folded} B but the log already held {} B",
            hot.wal_bytes
        );
        assert!(folded > 0);

        drop(keep_open);
        project.close().expect("close");
        assert!(!Sidecars::inspect(&path).log_left_behind());
    }

    #[test]
    fn recovering_everything_leaves_a_project_that_validates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("all.vcw");
        abandoned(&path, 30_000);

        let mut project = open(&path);
        // A second unfinished capture in the same file, to prove the sweep is
        // per capture rather than per project.
        let info = info();
        let second = Session::begin(&mut project, &info).expect("begin");
        let id2 = second.id();
        drop(project);

        let mut project = open(&path);
        let done = recover_all(&mut project, Plan::Repair).expect("recover all");
        assert_eq!(done.len(), 2);
        assert!(done.iter().all(|d| d.applied));
        assert_eq!(done[1].capture_id, id2);
        assert_eq!(done[1].frames, 0);

        assert!(survey(project.conn()).expect("survey").is_empty());
        let report = validate(
            &project,
            Options {
                verify_checksums: true,
            },
        )
        .expect("validate");
        assert!(report.is_clean(), "{:?}", report.findings);
    }

    #[test]
    fn a_clean_capture_is_never_offered_for_recovery() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("clean.vcw");
        let mut project = Project::create(&path).expect("create");
        let info = info();
        let session = Session::begin(&mut project, &info).expect("begin");
        let id = session.id();
        let mut writer =
            Writer::resume(project, session, &info, Config::default()).expect("resume");
        let bytes = 24_000usize * CHANNELS as usize * WIDTH;
        writer
            .push(&(0..bytes).map(|i| (i % 251) as u8).collect::<Vec<u8>>())
            .expect("push");
        let (_, project, _) = writer
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");

        assert!(survey(project.conn()).expect("survey").is_empty());
        let assessment = assess(project.conn(), id).expect("assess").expect("some");
        assert!(assessment.is_consistent());
        assert_eq!(assessment.state, CaptureState::Finalised);
        assert_eq!(assessment.usable_frames, 24_000);
    }
}
