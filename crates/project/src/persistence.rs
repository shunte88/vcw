/*
 *  persistence.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The capture writer thread: batched transactions and checkpoint policy (§13, §14).
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

//! The capture writer thread: batched transactions and checkpoint policy (§13, §14).
//!
//! §14 states the rule this module exists to keep: **SQLite writes must never
//! occur on the CPAL callback.** The callback fills a bounded ring
//! ([`vcw_types::PcmSource`] is its reading end) and this is the only thing that
//! drains it. Everything expensive - deinterleaving, summaries, CRC, the
//! transaction, the checkpoint - happens here, where being late costs ring space
//! rather than audio.
//!
//! # The parameters are measured, not chosen
//!
//! D3, from S2: **per-channel blocks of 250 ms, batch 1, WAL,
//! `synchronous=FULL`.** The reasoning inverted the starting hypothesis.
//! Throughput turned out to be a non-issue - a 90-minute 24/192 soak ran at a
//! real-time factor of 0.99996 with a commit p99 of 63 ms against a 250 ms
//! budget - so the budget buys *recovery granularity* instead of headroom.
//! Worst-case loss on a power cut is commit granularity plus the driver buffer,
//! and S1 finding 4 measured that driver buffer at 170 ms on a real `hw:`
//! device. 250 ms sits at that floor rather than inside it; going smaller stops
//! buying durability and starts costing tail margin.
//!
//! `synchronous=FULL` cost 3.7 ms at the median and was *better* at the tail
//! than NORMAL, which for a recording that cannot be repeated without replaying
//! the side makes maximum durability an easy call. Both pragmas are applied by
//! [`crate::sqlite`] on every connection, so they are not this module's to set.
//!
//! # Why the frame count moves inside the block transaction
//!
//! `captures.frames` is advanced in the same transaction that commits the
//! blocks. If it were a separate write, a crash between the two would leave a
//! project claiming more audio than it holds, and recovery would have to decide
//! which of two committed facts to believe. Written together, the count can
//! never run ahead of the data.
//!
//! # Why a failed commit stops the writer
//!
//! Blocks tile each channel's timeline with no gaps - [`validate`](mod@crate::validate)
//! enforces it and recovery depends on it. Carrying on after a failed commit
//! would punch a hole in that tiling which no later write could close, so the
//! writer stops, the session is marked interrupted, and the error is surfaced.
//! A short capture that says why it is short beats a long one with a hole in it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rusqlite::params;
use vcw_types::{CaptureInfo, CaptureState, Diagnostics, PcmSource, StorageFormat};

use crate::error::{Error, Result};
use crate::schema::{BLOCK_MILLIS, SUMMARY_64K_STRIDE, SUMMARY_256_STRIDE};
use crate::session::Session;
use crate::sqlite::{Project, block_checksum};

/// What to do about the write-ahead log as blocks accumulate.
///
/// S2 soaked [`Checkpoint::Automatic`] and saw a peak WAL of 4.57 MiB against an
/// 8.41 GB database, with a single 21.6 ms stall in 90 minutes. That is bounded
/// and cheap, so it is the default; the explicit modes exist because the Pi 5
/// and Windows runs may not agree and the policy has to be changeable without a
/// rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Checkpoint {
    /// SQLite's own autocheckpoint, with the threshold set from
    /// [`Config::wal_bytes`]. Measured, and the default.
    #[default]
    Automatic,
    /// Disable autocheckpoint and issue `PASSIVE` checkpoints on a block count.
    /// Never blocks a reader, and may do nothing if one is active.
    Passive,
    /// Disable autocheckpoint and issue `TRUNCATE` checkpoints on a block count.
    /// Keeps the WAL smallest, at the cost of the longest stall.
    Truncate,
    /// No checkpointing at all. Present because a policy nobody can turn off is
    /// a policy nobody has measured; the WAL grows without bound.
    Never,
}

/// How the writer is tuned. [`Config::default`] is D3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Block duration. D3 says 250 ms, and the number is a recovery-granularity
    /// decision rather than a throughput one.
    pub block_millis: u32,
    /// Blocks per transaction. D3 says 1: worst-case loss is
    /// `block_millis * batch_blocks` plus the driver buffer.
    pub batch_blocks: usize,
    /// WAL policy.
    pub checkpoint: Checkpoint,
    /// Blocks between writer-issued checkpoints. Ignored when
    /// [`Checkpoint::Automatic`] or [`Checkpoint::Never`].
    pub checkpoint_blocks: u64,
    /// Roughly how large the write-ahead log is allowed to get before
    /// [`Checkpoint::Automatic`] folds it back, in bytes.
    ///
    /// Stated in bytes on purpose. SQLite's `wal_autocheckpoint` counts *pages*,
    /// and VCW's page size is 64 KiB, so the stock threshold of 1000 pages is a
    /// 64 MiB log rather than the 4 MiB one S2 measured on a default-page-size
    /// harness. Measured as a pair on ext4 at 192 kHz: the log plateaued at
    /// 64.71 MiB under the stock threshold against 4.50 MiB under this one, and
    /// the commit tail went with it - p99 84.0 ms against 31.9 ms, worst 100.1 ms
    /// against 54.3 ms - because a checkpoint folding back 4 MiB finishes inside
    /// a block period and one folding back 64 MiB does not. Expressed in bytes
    /// the policy means the same thing whatever the page size, which is what it
    /// was always meant to.
    pub wal_bytes: u64,
    /// Whether to build the AUP4 waveform pyramids as blocks are written.
    ///
    /// On by default. S2 measured summary building as the largest per-block CPU
    /// cost - about 3 % of real time at 192 kHz - which is affordable here and
    /// saves a second pass over every byte later.
    pub summaries: bool,
    /// How long to wait when the source has nothing ready.
    pub poll: Duration,
    /// How often to write the device counters out during a capture (§15).
    ///
    /// Without this a process killed mid-capture leaves `capture_diagnostics`
    /// exactly as [`Session::begin`] created it - four zeros - and four zeros
    /// is the spelling of a flawless capture. Recovery would then certify a
    /// recording that had been overrunning for an hour. Writing them on a timer
    /// means the row is wrong by at most this interval rather than by the whole
    /// session, and it keeps `updated_at` meaningful, which is how recovery
    /// reports the staleness instead of hiding it.
    ///
    /// The cost is one small `UPDATE` every interval: at 2 s against eight
    /// commits a second it is under 2 % of the transactions and, at
    /// `synchronous=FULL`, under 2 % of the fsyncs. Zero disables it.
    pub diagnostics_millis: u32,

    /// Whether the writer begins paused rather than committing.
    ///
    /// §11 puts `Armed` before `Recording`: the device is open and the levels
    /// are being set, and nothing should be written yet. Starting the writer
    /// paused is how that is arranged - the ring is drained from the moment the
    /// device opens, so the counters never report an overrun the transport
    /// caused by not listening, and the first `RECORD` is a flag flip rather
    /// than a thread spawn and a session insert.
    pub start_paused: bool,

    /// How long the source may deliver nothing before the capture is called
    /// stalled, in milliseconds. Zero disables the watchdog.
    ///
    /// # Why the writer has to be the one to notice
    ///
    /// A device that is unplugged mid-capture does not report an error and does
    /// not end the stream - it simply stops calling the callback. Every counter
    /// in [`Diagnostics`] describes something that *happened*, so none of them
    /// can describe data that never arrived, and
    /// [`Diagnostics::is_clean`] therefore returns true for a capture that lost
    /// twenty minutes of a side. Three separate callers turned that into
    /// [`CaptureState::Finalised`], so an operator whose interface dropped out
    /// ten seconds in was handed a ten-second project marked as a complete,
    /// flawless capture.
    ///
    /// The source cannot see it, by definition. The writer can: it is the thing
    /// waiting for bytes. So the watchdog lives here, and the downgrade to
    /// [`CaptureState::Interrupted`] happens where the capture row is written
    /// rather than at each caller - which is what stops the next caller getting
    /// it wrong again.
    ///
    /// Two seconds. A pause does not trip it, because a paused writer still
    /// drains a running device and still sees bytes. Found by WP-17's fault
    /// harness; `--vanish-after` is the reproduction.
    pub stall_millis: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            block_millis: BLOCK_MILLIS,
            batch_blocks: 1,
            checkpoint: Checkpoint::Automatic,
            checkpoint_blocks: 64,
            wal_bytes: 4 * 1024 * 1024,
            summaries: true,
            poll: Duration::from_millis(5),
            diagnostics_millis: 2_000,
            start_paused: false,
            stall_millis: 2_000,
        }
    }
}

impl Config {
    /// Frames in one block at this rate. At least one, so a nonsense rate cannot
    /// produce a zero-length block and an infinite loop.
    pub const fn block_frames(&self, rate: u32) -> u64 {
        let frames = (rate as u64 * self.block_millis as u64) / 1_000;
        if frames == 0 { 1 } else { frames }
    }

    /// Worst-case audio lost to a power cut, in milliseconds, *excluding* the
    /// driver buffer.
    ///
    /// S1 finding 4 is why the exclusion is called out: on a real converter the
    /// driver holds audio that never reached a callback, and that is lost too.
    /// This number is the part the writer controls, not the whole loss.
    pub const fn commit_granularity_millis(&self) -> u64 {
        // A batch of zero is a batch of one: `push` commits as soon as the
        // batch is at least as long as the limit, so zero and one behave
        // identically, and reporting a budget of 0 ms would make every run look
        // as though it had blown it.
        let blocks = if self.batch_blocks == 0 {
            1
        } else {
            self.batch_blocks as u64
        };
        self.block_millis as u64 * blocks
    }
}

/// Re-exported so the writer's callers need not learn a second path to it.
/// The type belongs to [`vcw_types`] because the writer produces these and the
/// renderer in `vcw-signal` consumes them; see its module doc for why merging
/// them is exact.
pub use vcw_types::Summary;

/// Builds one level of the waveform pyramid: a triplet per `stride` samples.
///
/// The last group is summarised over the samples it actually has, and no
/// triplet is emitted for a group with none.
///
/// # Divergence from Audacity, measured 2026-09-25
///
/// Audacity does two things here that we deliberately do not. It sizes the
/// arrays for the block's *capacity* and pads the unused tail with
/// `(FLT_MAX, -FLT_MAX, 0)`, which is an artefact of a fixed maximum block size
/// that we have no equivalent of. And it builds the 64k level from the 256 level
/// by weighting every group as if it held a full 256 samples, then dividing by
/// the true sample count - so its 64k rms is slightly high wherever a block does
/// not divide evenly by 256. Both were confirmed against
/// `/data2/vinyl_rips/simples_test.aup3`. We compute each level from the samples
/// directly, which is the same answer everywhere Audacity's arithmetic is exact
/// and a more accurate one where it is not. Imported audio is summarised here
/// too, since WP-20 re-blocks it through this writer, and the source project is
/// never opened for writing - so the divergence is a difference between two
/// files rather than something either of them has to be made to agree with.
#[must_use]
pub fn pyramid(format: StorageFormat, samples: &[u8], stride: u32) -> Vec<u8> {
    if stride == 0 {
        return Vec::new();
    }
    let width = format.bytes_per_sample();
    let stride_bytes = stride as usize * width;
    let n = format.samples_in(samples.len());
    let groups = n.div_ceil(stride as usize);
    let mut out = Vec::with_capacity(groups * 12);
    for g in 0..groups {
        let lo = g * stride_bytes;
        let hi = ((g + 1) * stride_bytes).min(n * width);
        out.extend_from_slice(&Summary::of(format, &samples[lo..hi]).to_le_bytes());
    }
    out
}

/// A distribution, kept as raw samples because the tail is the whole question.
///
/// S2's acceptance was never "is it fast on average" but "does the tail ever
/// exceed one block duration", so a mean would answer the wrong question.
#[derive(Debug, Clone, Default)]
pub struct Latencies(Vec<u64>);

impl Latencies {
    /// Records one observation, in microseconds.
    pub fn record(&mut self, micros: u64) {
        self.0.push(micros);
    }

    /// Observations recorded.
    #[must_use]
    pub fn count(&self) -> usize {
        self.0.len()
    }

    /// The quantile at `q` in 0.0..=1.0, in microseconds, or `None` if nothing
    /// was recorded.
    #[must_use]
    pub fn quantile(&self, q: f64) -> Option<u64> {
        if self.0.is_empty() {
            return None;
        }
        let mut sorted = self.0.clone();
        sorted.sort_unstable();
        let idx = (((sorted.len() - 1) as f64) * q.clamp(0.0, 1.0)).round() as usize;
        Some(sorted[idx])
    }

    /// The worst observation, in microseconds.
    #[must_use]
    pub fn max(&self) -> Option<u64> {
        self.0.iter().copied().max()
    }

    /// Folds another distribution into this one.
    ///
    /// Raw samples rather than merged quantiles, because merging quantiles is
    /// wrong and the tail is the whole question: the p99 of two p99s is not the
    /// p99 of the union.
    pub fn absorb(&mut self, other: &Self) {
        self.0.extend_from_slice(&other.0);
    }

    /// p50, p95, p99 and max, in microseconds, for a report.
    #[must_use]
    pub fn summary(&self) -> Option<(u64, u64, u64, u64)> {
        Some((
            self.quantile(0.50)?,
            self.quantile(0.95)?,
            self.quantile(0.99)?,
            self.max()?,
        ))
    }
}

/// Live counters a caller can watch while the writer runs.
#[derive(Debug, Default)]
pub struct Progress {
    blocks: AtomicU64,
    frames: AtomicU64,
    bytes: AtomicU64,
    commits: AtomicU64,
    checkpoints: AtomicU64,
    worst_commit_micros: AtomicU64,
    peak_wal_bytes: AtomicU64,
    stopped: AtomicBool,
    paused: AtomicBool,
    stalls: AtomicU64,
    stalled: AtomicBool,
    /// Whether the current quiet period has already been counted.
    ///
    /// Internal. Without it a device that vanishes is counted again on every
    /// poll for the rest of the run, and `stalls` becomes a measure of how long
    /// the soak was left running rather than of how many times the device went
    /// away.
    stalled_now: AtomicBool,
}

impl Progress {
    /// Blocks committed, across every channel.
    pub fn blocks(&self) -> u64 {
        self.blocks.load(Ordering::Relaxed)
    }
    /// Frames committed, per channel.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }
    /// Sample bytes committed.
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
    /// Transactions committed.
    pub fn commits(&self) -> u64 {
        self.commits.load(Ordering::Relaxed)
    }
    /// Whether the writer is draining the ring and committing nothing.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }
    /// Checkpoints issued by the writer.
    pub fn checkpoints(&self) -> u64 {
        self.checkpoints.load(Ordering::Relaxed)
    }
    /// The worst commit seen so far, in microseconds. The number that decides
    /// whether the architecture holds.
    pub fn worst_commit_micros(&self) -> u64 {
        self.worst_commit_micros.load(Ordering::Relaxed)
    }
    /// Largest `-wal` sidecar seen, in bytes.
    pub fn peak_wal_bytes(&self) -> u64 {
        self.peak_wal_bytes.load(Ordering::Relaxed)
    }
    /// Times the source went quiet for longer than [`Config::stall_millis`].
    ///
    /// Counted per episode, not per lost frame: how much was lost is exactly
    /// what a vanished device does not tell us.
    pub fn stalls(&self) -> u64 {
        self.stalls.load(Ordering::Relaxed)
    }
    /// Whether the source has *ever* gone quiet for longer than the threshold.
    ///
    /// Latching rather than live, because a device that stalled and recovered
    /// left a hole in the timeline either way, and the capture has to say so
    /// after the fact. A caller wanting to know whether it is happening *now*
    /// should watch [`Progress::stalls`] move.
    pub fn has_stalled(&self) -> bool {
        self.stalled.load(Ordering::Relaxed)
    }
    /// Whether the writer has stopped, for whatever reason.
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

/// What a completed capture wrote.
#[derive(Debug)]
pub struct Outcome {
    /// The session that was written.
    pub capture_id: i64,
    /// Blocks committed, across every channel.
    pub blocks: u64,
    /// Frames committed, per channel.
    pub frames: u64,
    /// Sample bytes committed.
    pub bytes: u64,
    /// Transactions committed.
    pub commits: u64,
    /// Checkpoints issued by the writer.
    pub checkpoints: u64,
    /// Largest `-wal` sidecar seen, in bytes.
    pub peak_wal_bytes: u64,
    /// Commit durations.
    pub commit: Latencies,
    /// Summary and deinterleave durations, per block group.
    pub prepare: Latencies,
    /// Writer-issued checkpoint durations. Does not include the final one.
    pub checkpoint: Latencies,
    /// The closing `TRUNCATE` checkpoint, in microseconds.
    ///
    /// Separate because it is not part of the steady state: it happens once,
    /// after the last block, and folding it into the distribution would make
    /// every run look as though it had one anomalous stall.
    pub final_checkpoint_micros: u64,
    /// Times the source went quiet for longer than [`Config::stall_millis`].
    pub stalls: u64,
    /// The state the session was closed in.
    pub state: CaptureState,
}

impl Outcome {
    /// Seconds of audio committed, from the frame count and the rate.
    #[must_use]
    pub fn duration_secs(&self, rate: u32) -> f64 {
        if rate == 0 {
            return 0.0;
        }
        self.frames as f64 / f64::from(rate)
    }

    /// Whether every commit finished inside one block's worth of time.
    ///
    /// The acceptance question from S2. False does not mean audio was lost - the
    /// ring absorbs a late commit - but it means the margin has gone, which is
    /// the thing worth knowing before it does.
    #[must_use]
    pub fn commits_within_budget(&self, config: &Config) -> bool {
        self.commit
            .max()
            .is_none_or(|worst| worst < config.commit_granularity_millis() * 1_000)
    }

    /// Whether the WAL stayed near its budget, allowing `slack` times it.
    ///
    /// # Why the budget needs slack at all
    ///
    /// [`Config::wal_bytes`] is a *threshold*, not a ceiling: SQLite checks it
    /// after a commit and checkpoints from there, so the WAL always overshoots
    /// by at least the commit that crossed the line, and a checkpoint that
    /// finds a reader active does nothing and leaves the next commit to try
    /// again. A 4 MiB budget measured 4.94 MiB idle and 6.56 MiB with four
    /// waveform readers redrawing at 60 Hz, so the overshoot is real, small and
    /// bounded.
    ///
    /// What this catches is the unbounded case, which is not hypothetical:
    /// readers querying flat out hold a read snapshot open essentially all the
    /// time, no checkpoint ever completes, and the same run reached 51 MiB. See
    /// `cli::contend` - `--reader-hz 0` reproduces it, and is expected to fail
    /// this check. That is what makes it a demonstration rather than a claim.
    ///
    /// Always true under [`Checkpoint::Never`], which grows the WAL without
    /// bound on purpose, and under a `slack` of zero, which turns the check off.
    #[must_use]
    pub fn wal_within_budget(&self, config: &Config, slack: u32) -> bool {
        if slack == 0 || config.checkpoint == Checkpoint::Never {
            return true;
        }
        self.peak_wal_bytes <= config.wal_bytes.saturating_mul(u64::from(slack))
    }
}

/// One channel's slice of one block, ready to commit.
struct Row {
    channel: u16,
    samples: Vec<u8>,
    checksum: u32,
    whole: Summary,
    summary256: Option<Vec<u8>>,
    summary64k: Option<Vec<u8>>,
}

/// One block: the same span of time on every channel.
struct Block {
    sequence: u64,
    start_frame: u64,
    frames: u64,
    rows: Vec<Row>,
}

/// The writer, without a thread around it.
///
/// Separated from [`spawn`] so that every decision it makes - where a block
/// boundary falls, what goes in a transaction, when a checkpoint fires - is
/// testable by pushing bytes at it and reading the database back, with no
/// timing involved. A writer that can only be tested through a thread can only
/// be tested flakily.
pub struct Writer {
    project: Project,
    session: Session,
    config: Config,
    format: StorageFormat,
    channels: u16,
    frame_bytes: usize,
    block_frames: u64,
    /// Interleaved bytes not yet long enough to make a block.
    pending: Vec<u8>,
    /// Blocks prepared but not yet committed.
    batch: Vec<Block>,
    sequence: u64,
    start_frame: u64,
    frames: u64,
    diagnostics: Diagnostics,
    outcome: Outcome,
    progress: Arc<Progress>,
    wal_path: std::path::PathBuf,
}

impl Writer {
    /// Opens a session on `project` and prepares to write into it.
    ///
    /// # Errors
    ///
    /// If the session cannot be created, or the checkpoint policy cannot be
    /// applied.
    pub fn begin(mut project: Project, info: &CaptureInfo, config: Config) -> Result<Self> {
        let session = Session::begin(&mut project, info)?;
        Self::resume(project, session, info, config)
    }

    /// Attaches to a session that already exists, for a caller that opened it
    /// before the first byte arrived, or for recovery to continue one.
    ///
    /// # Errors
    ///
    /// If the checkpoint policy cannot be applied.
    pub fn resume(
        project: Project,
        session: Session,
        info: &CaptureInfo,
        config: Config,
    ) -> Result<Self> {
        // Expressed in bytes and converted here, because SQLite counts pages and
        // VCW's are 64 KiB: the stock 1000-page threshold is a 64 MiB log.
        let pages: i64 = match config.checkpoint {
            Checkpoint::Automatic => {
                let page_size: i64 = project
                    .conn()
                    .query_row("PRAGMA page_size", [], |r| r.get(0))?;
                let wanted = config.wal_bytes / (page_size.max(1) as u64);
                crate::session::clamp(wanted).max(1)
            }
            // SQLite's own autocheckpoint would otherwise fire alongside ours,
            // which makes the policy unmeasurable.
            _ => 0,
        };
        project
            .conn()
            .pragma_update(None, "wal_autocheckpoint", pages)?;

        let frame_bytes = info.frame_bytes();
        if info.channels == 0 || frame_bytes == 0 {
            return Err(Error::Unwritable {
                channels: info.channels,
                frame_bytes,
            });
        }
        let block_frames = config.block_frames(info.rate.hz());
        let wal_path = {
            let mut p = project.path().as_os_str().to_os_string();
            p.push("-wal");
            std::path::PathBuf::from(p)
        };
        Ok(Self {
            config,
            format: info.storage_format,
            channels: info.channels,
            frame_bytes,
            block_frames,
            pending: Vec::with_capacity(block_frames as usize * frame_bytes),
            batch: Vec::with_capacity(config.batch_blocks),
            sequence: 0,
            start_frame: 0,
            frames: 0,
            diagnostics: Diagnostics::default(),
            outcome: Outcome {
                capture_id: session.id(),
                blocks: 0,
                frames: 0,
                bytes: 0,
                commits: 0,
                checkpoints: 0,
                peak_wal_bytes: 0,
                commit: Latencies::default(),
                prepare: Latencies::default(),
                checkpoint: Latencies::default(),
                final_checkpoint_micros: 0,
                stalls: 0,
                state: CaptureState::Recording,
            },
            progress: Arc::new(Progress::default()),
            wal_path,
            project,
            session,
        })
    }

    /// Records how many times the source went quiet, for the outcome.
    fn record_stalls(&mut self, stalls: u64) {
        self.outcome.stalls = stalls;
    }

    /// The live counters, shareable with a caller on another thread.
    #[must_use]
    pub fn progress(&self) -> Arc<Progress> {
        Arc::clone(&self.progress)
    }

    /// The session being written.
    pub const fn session(&self) -> Session {
        self.session
    }

    /// Frames in one block at the configured rate.
    pub const fn block_frames(&self) -> u64 {
        self.block_frames
    }

    /// Hands the writer the counters the audio side is keeping, to be stored
    /// when the session closes.
    ///
    /// The writer cannot know these. Overruns and underruns happen upstream of
    /// it, in the callback and the ring, and a writer that reported its own view
    /// of them would be reporting that it was never asked to write the frames
    /// that went missing - which is true and useless.
    pub fn set_diagnostics(&mut self, diagnostics: Diagnostics) {
        self.diagnostics = diagnostics;
    }

    /// Writes the current counters to the project now (§15).
    ///
    /// Outside the block transaction on purpose: it is a one-row `UPDATE` to a
    /// table no block touches, and putting it inside would make the commit that
    /// carries audio wait on it. It is called on a timer by the writer thread so
    /// that a process killed mid-capture leaves counters that are nearly true
    /// rather than counters that are zero.
    ///
    /// # Errors
    ///
    /// If the update fails.
    pub fn persist_diagnostics(&mut self, diagnostics: Diagnostics) -> Result<()> {
        self.diagnostics = diagnostics;
        self.session.record(self.project.conn(), diagnostics)
    }

    /// Accepts interleaved capture bytes, committing whole blocks as they form.
    ///
    /// Partial frames are held over rather than written: a block that started
    /// half a frame late would shift every channel in it.
    ///
    /// # Errors
    ///
    /// If a transaction fails. The writer should not be used afterwards.
    pub fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.pending.extend_from_slice(bytes);
        let block_bytes = self.block_frames as usize * self.frame_bytes;
        while self.pending.len() >= block_bytes {
            let raw: Vec<u8> = self.pending.drain(..block_bytes).collect();
            self.prepare(raw);
            if self.batch.len() >= self.config.batch_blocks {
                self.commit()?;
            }
        }
        Ok(())
    }

    /// Commits everything held, including a final short block.
    ///
    /// # Errors
    ///
    /// If a transaction fails.
    pub fn flush(&mut self) -> Result<()> {
        // Whole frames only. A trailing partial frame is audio the device never
        // finished delivering, and inventing the rest of it would be a lie in
        // the one place §9 cares most about.
        let whole = (self.pending.len() / self.frame_bytes) * self.frame_bytes;
        if whole > 0 {
            let raw: Vec<u8> = self.pending.drain(..whole).collect();
            self.prepare(raw);
        }
        if !self.batch.is_empty() {
            self.commit()?;
        }
        Ok(())
    }

    /// Flushes, closes the session in `state`, and reports what was written.
    ///
    /// # Errors
    ///
    /// If the final flush or the session update fails.
    pub fn finish(mut self, state: CaptureState) -> Result<Outcome> {
        self.flush()?;
        self.session
            .finish(&mut self.project, state, self.frames, self.diagnostics)?;
        self.close_the_log();
        self.outcome.state = state;
        self.progress.stopped.store(true, Ordering::Relaxed);
        Ok(self.outcome)
    }

    /// Closes the session and hands back the project, for a caller that wants to
    /// keep reading it.
    ///
    /// # Errors
    ///
    /// If the final flush or the session update fails.
    pub fn finish_with_project(
        mut self,
        state: CaptureState,
    ) -> Result<(Outcome, Project, Session)> {
        self.flush()?;
        self.session
            .finish(&mut self.project, state, self.frames, self.diagnostics)?;
        self.close_the_log();
        self.outcome.state = state;
        self.progress.stopped.store(true, Ordering::Relaxed);
        let Self {
            project,
            session,
            outcome,
            ..
        } = self;
        Ok((outcome, project, session))
    }

    /// Folds the write-ahead log back into the database, once, at the end.
    ///
    /// So that the file a user copies is the whole file rather than a database
    /// plus a sidecar they may not think to take with it (§12). Failure is not
    /// an error: the data is committed either way, and a `-wal` left beside it
    /// is an inconvenience, not a loss.
    fn close_the_log(&mut self) {
        let started = Instant::now();
        let _ = self
            .project
            .conn()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        self.outcome.final_checkpoint_micros = started.elapsed().as_micros() as u64;
        tracing::debug!(
            micros = self.outcome.final_checkpoint_micros,
            "folded the write-ahead log back into the project"
        );
    }

    /// Splits one block's interleaved bytes per channel and summarises each.
    fn prepare(&mut self, raw: Vec<u8>) {
        let started = Instant::now();
        let width = self.format.bytes_per_sample();
        let channels = self.channels as usize;
        let frames = (raw.len() / self.frame_bytes) as u64;

        let mut rows = Vec::with_capacity(channels);
        for channel in 0..channels {
            // Per-channel layout, which is AUP4's and which S2 measured as
            // kinder to the WAL than interleaved at no throughput cost.
            let mut samples = Vec::with_capacity(frames as usize * width);
            for frame in 0..frames as usize {
                let at = frame * self.frame_bytes + channel * width;
                samples.extend_from_slice(&raw[at..at + width]);
            }
            let (s256, s64k) = if self.config.summaries {
                (
                    Some(pyramid(self.format, &samples, SUMMARY_256_STRIDE)),
                    Some(pyramid(self.format, &samples, SUMMARY_64K_STRIDE)),
                )
            } else {
                (None, None)
            };
            rows.push(Row {
                channel: channel as u16,
                checksum: block_checksum(&samples),
                whole: Summary::of(self.format, &samples),
                summary256: s256,
                summary64k: s64k,
                samples,
            });
        }

        self.outcome
            .prepare
            .record(started.elapsed().as_micros() as u64);
        self.batch.push(Block {
            sequence: self.sequence,
            start_frame: self.start_frame,
            frames,
            rows,
        });
        self.sequence += 1;
        self.start_frame += frames;
    }

    /// Writes the batch, and the frame count, in one transaction.
    fn commit(&mut self) -> Result<()> {
        let started = Instant::now();
        let committed_at = crate::now();
        let frames_after = self.start_frame;
        let format = self.format.code();

        let tx = self.project.conn_mut().transaction()?;
        let mut blocks = 0u64;
        let mut bytes = 0u64;
        for block in &self.batch {
            for row in &block.rows {
                tx.execute(
                    "INSERT INTO sampleblocks
                         (sampleformat, summin, summax, sumrms, summary256, summary64k, samples)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        format,
                        f64::from(row.whole.min),
                        f64::from(row.whole.max),
                        f64::from(row.whole.rms),
                        row.summary256,
                        row.summary64k,
                        row.samples,
                    ],
                )?;
                let blockid = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO capture_blocks
                         (blockid, capture_id, channel, sequence, start_frame, frame_count,
                          checksum, committed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        blockid,
                        self.session.id(),
                        row.channel,
                        crate::session::clamp(block.sequence),
                        crate::session::clamp(block.start_frame),
                        crate::session::clamp(block.frames),
                        row.checksum,
                        committed_at,
                    ],
                )?;
                blocks += 1;
                bytes += row.samples.len() as u64;
            }
        }
        // In the same transaction, so the count can never run ahead of the data.
        self.session.advance(&tx, frames_after)?;
        tx.commit()?;

        let micros = started.elapsed().as_micros() as u64;
        self.batch.clear();
        self.frames = frames_after;
        self.outcome.blocks += blocks;
        self.outcome.bytes += bytes;
        self.outcome.frames = frames_after;
        self.outcome.commits += 1;
        self.outcome.commit.record(micros);

        self.progress
            .blocks
            .store(self.outcome.blocks, Ordering::Relaxed);
        self.progress.frames.store(frames_after, Ordering::Relaxed);
        self.progress
            .bytes
            .store(self.outcome.bytes, Ordering::Relaxed);
        self.progress
            .commits
            .store(self.outcome.commits, Ordering::Relaxed);
        self.progress
            .worst_commit_micros
            .fetch_max(micros, Ordering::Relaxed);

        let wal = std::fs::metadata(&self.wal_path)
            .map(|m| m.len())
            .unwrap_or(0);
        self.outcome.peak_wal_bytes = self.outcome.peak_wal_bytes.max(wal);
        self.progress
            .peak_wal_bytes
            .fetch_max(wal, Ordering::Relaxed);

        self.maybe_checkpoint();
        Ok(())
    }

    /// Issues a checkpoint if the policy says so.
    ///
    /// A failed checkpoint is deliberately not an error: the WAL is a
    /// performance concern, and losing the capture because the log could not be
    /// folded back would be the wrong trade by a wide margin.
    fn maybe_checkpoint(&mut self) {
        let mode = match self.config.checkpoint {
            Checkpoint::Automatic | Checkpoint::Never => return,
            Checkpoint::Passive => "PASSIVE",
            Checkpoint::Truncate => "TRUNCATE",
        };
        if self.config.checkpoint_blocks == 0
            || !self
                .outcome
                .commits
                .is_multiple_of(self.config.checkpoint_blocks)
        {
            return;
        }
        let started = Instant::now();
        let _ = self
            .project
            .conn()
            .execute_batch(&format!("PRAGMA wal_checkpoint({mode});"));
        self.outcome
            .checkpoint
            .record(started.elapsed().as_micros() as u64);
        self.outcome.checkpoints += 1;
        self.progress
            .checkpoints
            .store(self.outcome.checkpoints, Ordering::Relaxed);
    }
}

/// A writer running on its own thread.
pub struct Handle {
    thread: Option<JoinHandle<Result<Outcome>>>,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    progress: Arc<Progress>,
    capture_id: i64,
    result: Arc<std::sync::Mutex<(CaptureState, Diagnostics)>>,
}

impl Handle {
    /// Live counters, safe to poll from anywhere.
    #[must_use]
    pub fn progress(&self) -> &Arc<Progress> {
        &self.progress
    }

    /// The session the writer is filling.
    pub const fn capture_id(&self) -> i64 {
        self.capture_id
    }

    /// Records how the capture ended, before stopping.
    ///
    /// Separate from [`Handle::stop`] because the caller learns it from the
    /// audio side, not from the writer: a capture whose device vanished is
    /// interrupted even though the writer shut down tidily, and the overrun
    /// count belongs to the ring rather than to anything this thread did.
    ///
    /// May be called either side of releasing the device. The writer does not
    /// decide how the capture ended until it is told to stop, so a result set
    /// after the ring's writing end has gone is still the one that reaches the
    /// row - which matters because the device's final counters are only final
    /// once the device has been released.
    pub fn set_result(&self, state: CaptureState, diagnostics: Diagnostics) {
        if let Ok(mut slot) = self.result.lock() {
            *slot = (state, diagnostics);
        }
    }

    /// Hands the writer the current device counters, mid-capture.
    ///
    /// The writer cannot read them for itself: the counters belong to the audio
    /// callback and the ring, and [`PcmSource`] deliberately exposes neither, so
    /// whoever owns the device has to pass them along. Call it from the same
    /// loop that already polls them for a progress display; the writer persists
    /// them on its own timer rather than on every call.
    ///
    /// Leaves the recorded end state alone - that is [`Handle::set_result`]'s
    /// job, and it is not known yet.
    pub fn note(&self, diagnostics: Diagnostics) {
        if let Ok(mut slot) = self.result.lock() {
            slot.1 = diagnostics;
        }
    }

    /// Keeps draining the ring but stops committing (§11's `PAUSE`).
    ///
    /// The ring must still be emptied or it fills within milliseconds and every
    /// callback after that counts as an overrun - the diagnostics would then
    /// report a fault the pause invented. So the frames keep arriving and are
    /// discarded, which is also what makes the recorded timeline *contiguous*
    /// across a pause: the audio either side is adjacent, and the minutes spent
    /// flipping the record are simply not in the capture.
    ///
    /// The writer commits whatever partial block it is holding as the pause
    /// begins. Carrying it across would leave up to a block of audio unwritten
    /// for as long as the operator takes, which is the one interval where
    /// nothing else is protecting it.
    ///
    /// Idempotent, and safe to call from any thread.
    pub fn pause(&self) {
        self.pause.store(true, Ordering::Relaxed);
    }

    /// Starts committing again (§11's `RESUME`). Idempotent.
    pub fn resume(&self) {
        self.pause.store(false, Ordering::Relaxed);
    }

    /// Whether the writer is currently discarding what it reads.
    ///
    /// Reads the writer's own flag rather than the request, so it is false
    /// until the pause has actually taken effect.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.progress.is_paused()
    }

    /// Asks the writer to drain what is left and stop, and waits for it.
    ///
    /// # Errors
    ///
    /// Whatever the writer failed on, or [`Error::WriterLost`] if its thread
    /// died without reporting.
    pub fn stop(mut self) -> Result<Outcome> {
        self.stop.store(true, Ordering::Relaxed);
        match self.thread.take() {
            Some(thread) => thread.join().unwrap_or(Err(Error::WriterLost)),
            None => Err(Error::WriterLost),
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // A dropped handle must not leave a thread holding the database open.
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts a writer thread draining `source` into `project`.
///
/// The thread owns the write connection for its lifetime, which is not a
/// limitation so much as the shape SQLite already imposes: there is one writer.
/// Readers open the same file separately, and WAL is what makes that work.
///
/// # Errors
///
/// If the session cannot be opened or the thread cannot be spawned.
pub fn spawn<S: PcmSource + 'static>(
    project: Project,
    info: &CaptureInfo,
    config: Config,
    source: S,
) -> Result<Handle> {
    let mut project = project;
    let session = Session::begin(&mut project, info)?;
    spawn_on(project, session, info, config, source)
}

/// Starts a writer thread on a session that already exists.
///
/// For a caller that wanted the session row on disk before the device was even
/// opened, which §15 asks for: a capture killed in its first second should still
/// leave evidence that it was attempted.
///
/// # Errors
///
/// If the writer cannot be prepared or the thread cannot be spawned.
pub fn spawn_on<S: PcmSource + 'static>(
    project: Project,
    session: Session,
    info: &CaptureInfo,
    config: Config,
    mut source: S,
) -> Result<Handle> {
    let writer = Writer::resume(project, session, info, config)?;
    let progress = writer.progress();
    let capture_id = writer.session().id();
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(config.start_paused));
    let result = Arc::new(std::sync::Mutex::new((
        CaptureState::Finalised,
        Diagnostics::default(),
    )));

    let thread_pause = Arc::clone(&pause);
    let thread_stop = Arc::clone(&stop);
    let thread_result = Arc::clone(&result);
    let thread_progress = Arc::clone(&progress);
    let thread = std::thread::Builder::new()
        .name("vcw-capture-writer".to_owned())
        .spawn(move || {
            let mut writer = writer;
            // One block's worth at a time: large enough that a commit does not
            // wait on the next read, small enough to stay off the heap's radar.
            let mut scratch = vec![0u8; writer.block_frames() as usize * writer.frame_bytes];
            let every = Duration::from_millis(u64::from(config.diagnostics_millis));
            let mut counters_written = Instant::now();
            // Starts now rather than at the first byte, so a device that opens
            // and never delivers at all is caught too - which is a stall with
            // nothing before it, and the failure a person sees as "it recorded
            // silence".
            let mut last_data = Instant::now();
            let stall_after = Duration::from_millis(u64::from(config.stall_millis));
            let mut was_paused = config.start_paused;
            thread_progress.paused.store(was_paused, Ordering::Relaxed);
            let outcome = loop {
                let paused = thread_pause.load(Ordering::Relaxed);
                if paused != was_paused {
                    // Entering a pause: commit the partial block rather than
                    // hold it for however long the operator takes. Leaving one:
                    // nothing to do, the next read simply gets pushed again.
                    if paused && let Err(e) = writer.flush() {
                        thread_progress.stopped.store(true, Ordering::Relaxed);
                        let _ = writer.finish(CaptureState::Interrupted);
                        break Err(e);
                    }
                    was_paused = paused;
                    thread_progress.paused.store(paused, Ordering::Relaxed);
                }
                if config.diagnostics_millis > 0 && counters_written.elapsed() >= every {
                    counters_written = Instant::now();
                    let latest = thread_result.lock().map(|r| r.1).unwrap_or_default();
                    // A counter that cannot be written is not worth abandoning a
                    // capture over; the audio is the part that cannot be redone.
                    let _ = writer.persist_diagnostics(latest);
                }
                let n = source.read(&mut scratch);
                if n > 0 {
                    // Bytes arrived, so the device is alive. Reset before the
                    // pause check, not after: a paused writer reads and discards
                    // from a running device, and treating that as silence would
                    // report a stall for every pause an operator takes.
                    last_data = Instant::now();
                    thread_progress.stalled_now.store(false, Ordering::Relaxed);
                    if paused {
                        // Read and dropped. The ring stays empty, the counters
                        // stay honest, and the frame index does not advance.
                        continue;
                    }
                    if let Err(e) = writer.push(&scratch[..n]) {
                        thread_progress.stopped.store(true, Ordering::Relaxed);
                        // The session is left interrupted deliberately: the
                        // timeline has a hole in it and a later reader must know.
                        let _ = writer.finish(CaptureState::Interrupted);
                        break Err(e);
                    }
                    continue;
                }
                // Nothing ready, and the producer has not gone. A device that
                // is unplugged looks exactly like this and looks like it
                // forever, so this is the only place the difference between a
                // quiet moment and a dead device can be told - by how long it
                // lasts. Counted once per episode; the flag latches so the
                // capture row can be downgraded however the run ends.
                if config.stall_millis > 0
                    && !thread_stop.load(Ordering::Relaxed)
                    && !source.is_finished()
                    && last_data.elapsed() >= stall_after
                    && !thread_progress.stalled_now.swap(true, Ordering::Relaxed)
                {
                    thread_progress.stalls.fetch_add(1, Ordering::Relaxed);
                    thread_progress.stalled.store(true, Ordering::Relaxed);
                }
                // Stop only when the producer has also gone, so a quiet moment
                // is not mistaken for the end of the side.
                if thread_stop.load(Ordering::Relaxed) || source.is_finished() {
                    if !thread_stop.load(Ordering::Relaxed) {
                        // The producer going away is not the same thing as the
                        // owner having said how the capture ended, and a caller
                        // that reads the device's final counters has to release
                        // the device to get them - so the result usually arrives
                        // in the moment *after* the ring's writing end is gone.
                        // Finalising here would race that and record a vanished
                        // device as a clean capture. Commit what is held, so the
                        // wait costs nothing that a crash could take, and then
                        // wait for the stop that `Handle::stop` and `Handle::drop`
                        // both guarantee.
                        if let Err(e) = writer.flush() {
                            thread_progress.stopped.store(true, Ordering::Relaxed);
                            let _ = writer.finish(CaptureState::Interrupted);
                            break Err(e);
                        }
                        while !thread_stop.load(Ordering::Relaxed) {
                            std::thread::sleep(config.poll);
                        }
                    }
                    let (state, diagnostics) = thread_result
                        .lock()
                        .map(|r| *r)
                        .unwrap_or((CaptureState::Interrupted, Diagnostics::default()));
                    writer.set_diagnostics(diagnostics);
                    // The caller's verdict, overruled if the source went quiet.
                    // Overruled here rather than returned for the caller to
                    // apply, because three callers compute this state from
                    // `Diagnostics::is_clean` alone and a fourth would too.
                    let state = if thread_progress.stalled.load(Ordering::Relaxed) {
                        CaptureState::Interrupted
                    } else {
                        state
                    };
                    writer.record_stalls(thread_progress.stalls.load(Ordering::Relaxed));
                    break writer.finish(state);
                }
                std::thread::sleep(config.poll);
            };
            thread_progress.stopped.store(true, Ordering::Relaxed);
            outcome
        })
        .map_err(Error::Io)?;

    Ok(Handle {
        thread: Some(thread),
        stop,
        pause,
        progress,
        capture_id,
        result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::{Options, validate};
    use rusqlite::Connection;
    use vcw_types::{CaptureEq, CaptureMode, SampleRate};

    const RATE: u32 = 96_000;

    fn info() -> CaptureInfo {
        CaptureInfo {
            rate: SampleRate(RATE),
            channels: 2,
            storage_format: StorageFormat::Int24Padded,
            capture_mode: CaptureMode::Exclusive,
            host_api: Some("ALSA".into()),
            device_id: Some("hw:CARD=0,DEV=0".into()),
            device_name: Some("Cirrus Analog".into()),
            os_verified: false,
            os_report: None,
            eq: CaptureEq::Unknown,
        }
    }

    fn project(dir: &tempfile::TempDir) -> Project {
        Project::create(dir.path().join("writer.vcw")).expect("create")
    }

    /// A recognisable byte stream. Every byte is a function of its own offset,
    /// so a single wrong byte anywhere identifies where it came from.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    /// Reads the capture back and reinterleaves it, which is the only comparison
    /// that means anything: the writer split the stream, so the test has to put
    /// it together again rather than check the halves it happens to have made.
    fn readback(conn: &Connection, capture_id: i64, channels: u16, width: usize) -> Vec<u8> {
        let mut per_channel: Vec<Vec<u8>> = vec![Vec::new(); channels as usize];
        let mut stmt = conn
            .prepare(
                "SELECT b.channel, s.samples FROM capture_blocks b
                 JOIN sampleblocks s ON s.blockid = b.blockid
                 WHERE b.capture_id = ?1 ORDER BY b.sequence, b.channel",
            )
            .expect("prepare");
        let rows = stmt
            .query_map([capture_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .expect("query");
        for row in rows {
            let (channel, samples) = row.expect("row");
            per_channel[channel as usize].extend_from_slice(&samples);
        }
        let frames = per_channel[0].len() / width;
        let mut out = Vec::with_capacity(frames * channels as usize * width);
        for frame in 0..frames {
            for channel in &per_channel {
                out.extend_from_slice(&channel[frame * width..(frame + 1) * width]);
            }
        }
        out
    }

    /// A [`PcmSource`] that hands over a fixed buffer and then says so.
    struct Canned {
        data: Vec<u8>,
        at: usize,
    }

    impl PcmSource for Canned {
        fn read(&mut self, dst: &mut [u8]) -> usize {
            let n = dst.len().min(self.data.len() - self.at);
            dst[..n].copy_from_slice(&self.data[self.at..self.at + n]);
            self.at += n;
            n
        }

        fn is_finished(&self) -> bool {
            self.at >= self.data.len()
        }
    }

    #[test]
    fn the_default_configuration_is_the_one_s2_measured() {
        let c = Config::default();
        assert_eq!(c.block_millis, 250);
        assert_eq!(c.batch_blocks, 1);
        assert_eq!(c.checkpoint, Checkpoint::Automatic);
        // Bytes, not pages: with 64 KiB pages the stock 1000-page threshold is a
        // 64 MiB log, which is not what S2 measured and not what anyone chose.
        assert_eq!(c.wal_bytes, 4 * 1024 * 1024);
        // The number D3 actually turns on: worst case loss from the writer.
        assert_eq!(c.commit_granularity_millis(), 250);
        assert_eq!(c.block_frames(192_000), 48_000);
        assert_eq!(c.block_frames(44_100), 11_025);
    }

    #[test]
    fn a_capture_with_no_channels_is_refused_rather_than_spun_on() {
        // Zero channels means zero-length blocks, and the block loop would drain
        // nothing for ever. Unreachable from a device, reachable from the API.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = CaptureInfo {
            channels: 0,
            ..info()
        };
        match Writer::begin(project(&dir), &info, Config::default()) {
            Err(Error::Unwritable { channels: 0, .. }) => {}
            Err(other) => panic!("wrong error: {other}"),
            Ok(_) => panic!("a zero-channel capture was accepted"),
        }
        // And a pyramid with no stride is empty rather than a division by zero.
        assert!(pyramid(StorageFormat::Int16, &[0; 64], 0).is_empty());
    }

    #[test]
    fn a_nonsense_rate_cannot_produce_a_zero_length_block() {
        // Not a real rate, but a zero-frame block would spin forever, and a
        // writer that hangs is worse than one that writes odd blocks.
        assert_eq!(Config::default().block_frames(1), 1);
        assert_eq!(Config::default().block_frames(0), 1);
    }

    #[test]
    fn the_bytes_that_come_back_are_the_bytes_that_went_in() {
        // §9, end to end through the writer: no conversion, no reordering, no
        // loss. Deinterleaving is a layout change and must be perfectly undone.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let mut w = Writer::begin(project(&dir), &info, Config::default()).expect("begin");
        let sent = pattern(w.block_frames() as usize * info.frame_bytes() * 3 + 999 * 8);
        w.push(&sent).expect("push");
        let (outcome, p, _) = w
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");

        let got = readback(p.conn(), outcome.capture_id, 2, 4);
        assert_eq!(got.len(), sent.len());
        assert_eq!(got, sent, "the capture path altered a byte");
        assert_eq!(outcome.frames, (sent.len() / info.frame_bytes()) as u64);
    }

    #[test]
    fn a_short_final_block_is_written_rather_than_discarded() {
        // The end of a side is not a multiple of 250 ms. Dropping the remainder
        // would silently truncate every recording ever made.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let mut w = Writer::begin(project(&dir), &info, Config::default()).expect("begin");
        let block = w.block_frames();
        w.push(&pattern((block as usize + 7) * info.frame_bytes()))
            .expect("push");
        let (outcome, p, _) = w
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");

        assert_eq!(outcome.frames, block + 7);
        let counts: Vec<i64> = p
            .conn()
            .prepare("SELECT frame_count FROM capture_blocks WHERE channel = 0 ORDER BY sequence")
            .expect("prepare")
            .query_map([], |r| r.get(0))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        assert_eq!(counts, vec![block as i64, 7]);
    }

    #[test]
    fn a_partial_frame_is_held_over_rather_than_written() {
        // Half a frame is not audio. Padding it would put a click on one channel
        // and shift the other, which is exactly the fabrication §9 forbids.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let mut w = Writer::begin(project(&dir), &info, Config::default()).expect("begin");
        let frame = info.frame_bytes();
        w.push(&pattern(10 * frame + 3)).expect("push");
        let (outcome, p, _) = w
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");

        assert_eq!(outcome.frames, 10);
        assert_eq!(
            readback(p.conn(), outcome.capture_id, 2, 4).len(),
            10 * frame
        );
    }

    #[test]
    fn the_frame_count_never_runs_ahead_of_the_blocks() {
        // The invariant recovery depends on. Checked after every commit, not
        // just at the end, because the crash it guards against is mid-capture.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let mut w = Writer::begin(project(&dir), &info, Config::default()).expect("begin");
        let block_bytes = w.block_frames() as usize * info.frame_bytes();
        let id = w.session().id();
        for _ in 0..5 {
            w.push(&pattern(block_bytes)).expect("push");
            let claimed: i64 = w
                .project
                .conn()
                .query_row(
                    "SELECT frames FROM captures WHERE capture_id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .expect("frames");
            let held: i64 = w
                .project
                .conn()
                .query_row(
                    "SELECT COALESCE(SUM(frame_count), 0) FROM capture_blocks
                     WHERE capture_id = ?1 AND channel = 0",
                    [id],
                    |r| r.get(0),
                )
                .expect("sum");
            assert_eq!(claimed, held);
        }
        let _ = w.finish(CaptureState::Finalised).expect("finish");
    }

    #[test]
    fn a_written_capture_validates_clean() {
        // WP-02's schema has only ever held synthetic blocks. This is the first
        // time the validator sees blocks a capture actually produced.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let mut w = Writer::begin(project(&dir), &info, Config::default()).expect("begin");
        w.push(&pattern(
            w.block_frames() as usize * info.frame_bytes() * 2 + 4_096,
        ))
        .expect("push");
        let (_, p, _) = w
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");

        let report = validate(&p, Options::default()).expect("validate");
        assert!(report.is_clean(), "{:?}", report.findings);
    }

    #[test]
    fn every_block_carries_the_checksum_of_its_own_samples() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let mut w = Writer::begin(project(&dir), &info, Config::default()).expect("begin");
        w.push(&pattern(w.block_frames() as usize * info.frame_bytes() * 2))
            .expect("push");
        let (_, p, _) = w
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");

        let mut stmt = p
            .conn()
            .prepare(
                "SELECT b.checksum, s.samples FROM capture_blocks b
                 JOIN sampleblocks s ON s.blockid = b.blockid",
            )
            .expect("prepare");
        let mut seen = 0;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))
            .expect("query");
        for row in rows {
            let (stored, samples) = row.expect("row");
            assert_eq!(stored as u32, block_checksum(&samples));
            seen += 1;
        }
        assert_eq!(seen, 4, "two blocks on each of two channels");
    }

    #[test]
    fn batching_changes_the_transaction_count_and_nothing_else() {
        // The batch size is a durability dial. It must not be a correctness one.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let single = Config {
            batch_blocks: 1,
            ..Config::default()
        };
        let batched = Config {
            batch_blocks: 4,
            ..Config::default()
        };

        let mut wrote = Vec::new();
        for (name, config) in [("one", single), ("four", batched)] {
            let p = Project::create(dir.path().join(format!("{name}.vcw"))).expect("create");
            let mut w = Writer::begin(p, &info, config).expect("begin");
            w.push(&pattern(w.block_frames() as usize * info.frame_bytes() * 8))
                .expect("push");
            let (outcome, p, _) = w
                .finish_with_project(CaptureState::Finalised)
                .expect("finish");
            wrote.push((
                outcome.commits,
                readback(p.conn(), outcome.capture_id, 2, 4),
            ));
        }
        assert_eq!(wrote[0].0, 8);
        assert_eq!(wrote[1].0, 2);
        assert_eq!(wrote[0].1, wrote[1].1);
    }

    #[test]
    fn summaries_can_be_turned_off_and_the_audio_is_unaffected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let config = Config {
            summaries: false,
            ..Config::default()
        };
        let mut w = Writer::begin(project(&dir), &info, config).expect("begin");
        w.push(&pattern(w.block_frames() as usize * info.frame_bytes()))
            .expect("push");
        let (outcome, p, _) = w
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");

        let missing: i64 = p
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sampleblocks WHERE summary256 IS NULL",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(missing, 2);
        // The whole-block summary is not a pyramid level and is always written:
        // it is one triplet, and the waveform view needs something at every zoom.
        let rms: f64 = p
            .conn()
            .query_row("SELECT sumrms FROM sampleblocks LIMIT 1", [], |r| r.get(0))
            .expect("rms");
        assert!(rms > 0.0);
        assert_eq!(
            readback(p.conn(), outcome.capture_id, 2, 4).len(),
            outcome.bytes as usize
        );
    }

    #[test]
    fn a_full_scale_square_wave_summarises_to_plus_and_minus_one() {
        // A value we can check by hand, in the format the corpus work showed is
        // easiest to get wrong: padded 24-bit is not left-justified.
        let mut samples = Vec::new();
        for i in 0..512 {
            let v: i32 = if i % 2 == 0 { 8_388_607 } else { -8_388_608 };
            samples.extend_from_slice(&v.to_le_bytes());
        }
        let s = Summary::of(StorageFormat::Int24Padded, &samples);
        assert!((s.max - 0.999_999_9).abs() < 1e-6, "{}", s.max);
        assert!((s.min + 1.0).abs() < 1e-6, "{}", s.min);
        assert!((s.rms - 1.0).abs() < 1e-6, "{}", s.rms);
    }

    #[test]
    fn a_pyramid_has_one_triplet_per_group_and_none_for_a_group_that_is_not_there() {
        // Where we part company with Audacity, deliberately: it sizes these to
        // block capacity and pads with (FLT_MAX, -FLT_MAX, 0). We emit exactly
        // the groups that exist, so a reader never has to know the sentinel.
        let format = StorageFormat::Int16;
        let samples = vec![0u8; 600 * 2];
        let level = pyramid(format, &samples, SUMMARY_256_STRIDE);
        assert_eq!(level.len(), 3 * 12, "256, 256 and a final 88");
        assert_eq!(pyramid(format, &[], SUMMARY_256_STRIDE).len(), 0);
        assert_eq!(pyramid(format, &samples, SUMMARY_64K_STRIDE).len(), 12);
    }

    #[test]
    fn a_short_final_group_is_summarised_over_what_it_holds() {
        // Audacity's 64k rms weights every group as a full 256 and is slightly
        // high on a short tail. Ours uses the true count, and this pins that.
        let format = StorageFormat::Int16;
        let mut samples = Vec::new();
        for _ in 0..256 {
            samples.extend_from_slice(&0i16.to_le_bytes());
        }
        for _ in 0..4 {
            samples.extend_from_slice(&16_384i16.to_le_bytes());
        }
        let level = pyramid(format, &samples, SUMMARY_256_STRIDE);
        assert_eq!(level.len(), 2 * 12);
        let tail = f32::from_le_bytes(level[20..24].try_into().expect("rms"));
        assert!((tail - 0.5).abs() < 1e-6, "{tail}");
    }

    /// A source that delivers, then goes quiet without ever saying it is done.
    ///
    /// An unplugged USB interface, exactly: the stream is not closed and no
    /// error is raised, the callback simply stops being called. `is_finished`
    /// staying false is the whole point - it is what makes this different from
    /// [`Canned`], and what the writer has to notice for itself.
    struct Vanishing {
        data: Vec<u8>,
        at: usize,
    }

    impl PcmSource for Vanishing {
        fn read(&mut self, dst: &mut [u8]) -> usize {
            let n = dst.len().min(self.data.len() - self.at);
            dst[..n].copy_from_slice(&self.data[self.at..self.at + n]);
            self.at += n;
            n
        }

        fn is_finished(&self) -> bool {
            false
        }
    }

    /// Runs a vanishing source to a stop and returns what the writer recorded.
    ///
    /// `Finalised` is passed in on purpose: the caller here is the one being
    /// corrected. Every real caller computes that verdict from
    /// `Diagnostics::is_clean`, and after a silent vanish all four counters are
    /// zero, so `Finalised` is precisely what the writer will be told.
    fn after_vanishing(stall_millis: u32) -> (Outcome, CaptureState) {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let config = Config {
            stall_millis,
            ..Config::default()
        };
        let source = Vanishing {
            data: pattern(config.block_frames(RATE) as usize * info.frame_bytes()),
            at: 0,
        };
        let handle = spawn(project(&dir), &info, config, source).expect("spawn");
        let capture_id = handle.capture_id();
        // Long enough to cross a 100 ms threshold several times over, and short
        // enough that the test is not a delay in the suite.
        std::thread::sleep(Duration::from_millis(400));
        handle.set_result(CaptureState::Finalised, Diagnostics::default());
        let outcome = handle.stop().expect("stop");

        let p = Project::open(dir.path().join("writer.vcw")).expect("reopen");
        let on_disk: String = p
            .conn()
            .query_row(
                "SELECT state FROM captures WHERE capture_id = ?1",
                [capture_id],
                |row| row.get(0),
            )
            .expect("reading the capture state");
        (
            outcome,
            match on_disk.as_str() {
                "finalised" => CaptureState::Finalised,
                "interrupted" => CaptureState::Interrupted,
                other => panic!("unexpected state on disk: {other}"),
            },
        )
    }

    #[test]
    fn a_source_that_goes_quiet_without_a_word_is_not_a_finished_capture() {
        // WP-17. Found by `vcw soak --vanish-after`: a device that stops
        // delivering leaves every counter in `Diagnostics` at zero, because all
        // four describe something that happened and this is data that never
        // arrived. `is_clean` therefore returned true and three separate callers
        // turned that into `Finalised` - so ten seconds of a twenty-two-minute
        // side was recorded as a complete, flawless capture.
        let (stalled, on_disk) = after_vanishing(100);
        assert!(
            stalled.stalls >= 1,
            "the writer did not notice a source that stopped delivering"
        );
        assert_eq!(
            stalled.state,
            CaptureState::Interrupted,
            "the writer was told Finalised and should have overruled it"
        );
        assert_eq!(
            on_disk,
            CaptureState::Interrupted,
            "the project still claims a clean capture, which is the whole defect"
        );

        // The control arm. With the watchdog off, the same source through the
        // same path produces the old answer - which is what proves the
        // downgrade above came from the watchdog and not from something else
        // about a source that stops early.
        let (ignored, on_disk) = after_vanishing(0);
        assert_eq!(ignored.stalls, 0);
        assert_eq!(ignored.state, CaptureState::Finalised);
        assert_eq!(on_disk, CaptureState::Finalised);
    }

    #[test]
    fn the_writer_thread_drains_a_source_to_its_last_byte() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let sent = pattern(
            Config::default().block_frames(RATE) as usize * info.frame_bytes() * 3
                + 137 * info.frame_bytes(),
        );
        let source = Canned {
            data: sent.clone(),
            at: 0,
        };
        let handle = spawn(project(&dir), &info, Config::default(), source).expect("spawn");
        let capture_id = handle.capture_id();
        let outcome = handle.stop().expect("stop");

        assert_eq!(outcome.state, CaptureState::Finalised);
        assert_eq!(outcome.frames, (sent.len() / info.frame_bytes()) as u64);

        let p = Project::open(dir.path().join("writer.vcw")).expect("reopen");
        assert_eq!(readback(p.conn(), capture_id, 2, 4), sent);
        assert!(
            validate(&p, Options::default())
                .expect("validate")
                .is_clean()
        );
    }

    #[test]
    fn a_capture_that_ends_badly_is_recorded_as_interrupted() {
        // The writer shutting down tidily is not the same as the capture going
        // well. The state comes from the caller, who is watching the device.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let source = Canned {
            data: pattern(Config::default().block_frames(RATE) as usize * info.frame_bytes()),
            at: 0,
        };
        let handle = spawn(project(&dir), &info, Config::default(), source).expect("spawn");
        let lost = Diagnostics {
            overruns: 2,
            underruns: 0,
            dropped_frames: 960,
            stream_errors: 1,
        };
        handle.set_result(CaptureState::Interrupted, lost);
        let outcome = handle.stop().expect("stop");
        assert_eq!(outcome.state, CaptureState::Interrupted);

        let p = Project::open(dir.path().join("writer.vcw")).expect("reopen");
        let record = crate::session::load(p.conn(), outcome.capture_id)
            .expect("load")
            .expect("row");
        assert_eq!(record.state, CaptureState::Interrupted);
        // The ring's counters, not the writer's: what went missing upstream has
        // to reach the file, or the project claims a clean capture it never had.
        assert_eq!(record.diagnostics, lost);
        // Interrupted, but finished: recovery is for captures nobody closed.
        assert!(!record.needs_recovery());
    }

    #[test]
    fn a_result_set_after_the_source_went_quiet_is_still_the_one_recorded() {
        // R9, and the one ordering that is easy to get wrong: the caller cannot
        // read a device's final counters until it has released the device, and
        // releasing it takes the ring's writing end with it. The writer must not
        // have decided the capture was clean by then.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let done = Arc::new(AtomicBool::new(false));
        let source = Tap {
            next: 0,
            done: Arc::clone(&done),
        };
        let handle = spawn(project(&dir), &info, Config::default(), source).expect("spawn");
        std::thread::sleep(Duration::from_millis(50));

        // The device is gone: nothing more will be read and `is_finished` is true.
        done.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(150));

        let lost = Diagnostics {
            overruns: 0,
            underruns: 0,
            dropped_frames: 1_440,
            stream_errors: 1,
        };
        handle.set_result(CaptureState::Interrupted, lost);
        let outcome = handle.stop().expect("stop");
        assert_eq!(outcome.state, CaptureState::Interrupted);

        let p = Project::open(dir.path().join("writer.vcw")).expect("reopen");
        let record = crate::session::load(p.conn(), outcome.capture_id)
            .expect("load")
            .expect("row");
        assert_eq!(record.state, CaptureState::Interrupted);
        assert_eq!(record.diagnostics, lost);
        assert!(
            validate(&p, Options::default())
                .expect("validate")
                .is_clean()
        );
    }

    #[test]
    fn the_log_is_bounded_by_the_ceiling_it_was_given() {
        // The WAL ceiling is the half of "zero loss, bounded WAL" that is easy
        // to believe without checking. SQLite counts pages and VCW's are 64 KiB,
        // so an unset threshold is a 64 MiB log.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let config = Config {
            wal_bytes: 2 * 1024 * 1024,
            ..Config::default()
        };
        let mut w = Writer::begin(project(&dir), &info, config).expect("begin");
        let pages: i64 = w
            .project
            .conn()
            .query_row("PRAGMA wal_autocheckpoint", [], |r| r.get(0))
            .expect("pragma");
        assert_eq!(pages, 32, "2 MiB of 64 KiB pages");

        let block_bytes = w.block_frames() as usize * info.frame_bytes();
        for _ in 0..60 {
            w.push(&pattern(block_bytes)).expect("push");
        }
        let outcome = w.finish(CaptureState::Finalised).expect("finish");
        // Generous: the file is a high-water mark and SQLite never shrinks it
        // mid-run, so the check is that the ceiling is respected in order of
        // magnitude, not to the page.
        assert!(
            outcome.peak_wal_bytes < 8 * 1024 * 1024,
            "the log reached {} bytes against a 2 MiB ceiling",
            outcome.peak_wal_bytes
        );
    }

    #[test]
    fn a_truncating_checkpoint_policy_actually_checkpoints() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let config = Config {
            checkpoint: Checkpoint::Truncate,
            checkpoint_blocks: 2,
            ..Config::default()
        };
        let mut w = Writer::begin(project(&dir), &info, config).expect("begin");
        w.push(&pattern(w.block_frames() as usize * info.frame_bytes() * 6))
            .expect("push");
        let (outcome, p, _) = w
            .finish_with_project(CaptureState::Finalised)
            .expect("finish");

        assert_eq!(outcome.checkpoints, 3);
        assert_eq!(outcome.checkpoint.count(), 3);
        assert!(outcome.final_checkpoint_micros > 0);
        assert!(
            validate(&p, Options::default())
                .expect("validate")
                .is_clean()
        );
    }

    #[test]
    fn the_commit_budget_is_reported_against_the_block_duration() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let config = Config::default();
        let mut w = Writer::begin(project(&dir), &info, config).expect("begin");
        w.push(&pattern(w.block_frames() as usize * info.frame_bytes() * 4))
            .expect("push");
        let outcome = w.finish(CaptureState::Finalised).expect("finish");

        assert_eq!(outcome.commit.count(), 4);
        let (p50, p95, p99, max) = outcome.commit.summary().expect("percentiles");
        assert!(p50 <= p95 && p95 <= p99 && p99 <= max);
        assert!(
            outcome.commits_within_budget(&config),
            "a 250 ms budget was blown by a {max} us commit"
        );
        assert!(outcome.duration_secs(RATE) > 0.0);
        assert_eq!(outcome.duration_secs(0), 0.0);
    }

    #[test]
    fn the_wal_budget_allows_an_overshoot_and_not_an_unbounded_one() {
        // WP-17, §41. Until the contention test there was no WAL gate at all,
        // and a run whose WAL reached 90 MiB against a 4 MiB budget printed
        // "pass". These are the numbers that run measured.
        let dir = tempfile::tempdir().expect("tempdir");
        let config = Config::default();
        let mut w = Writer::begin(project(&dir), &info(), config).expect("begin");
        w.push(&pattern(w.block_frames() as usize * info().frame_bytes()))
            .expect("push");
        let mut outcome = w.finish(CaptureState::Finalised).expect("finish");

        let mib = 1024 * 1024;
        assert_eq!(config.wal_bytes, 4 * mib, "the budget these numbers assume");

        // Idle, and with four readers redrawing at 60 Hz: an overshoot, bounded.
        for measured in [4 * mib, 4_940_000, 6_560_000] {
            outcome.peak_wal_bytes = measured;
            assert!(
                outcome.wal_within_budget(&config, 4),
                "{measured} bytes should be inside four times a 4 MiB budget"
            );
        }

        // Four readers querying flat out, which starves the checkpoint.
        outcome.peak_wal_bytes = 51 * mib;
        assert!(
            !outcome.wal_within_budget(&config, 4),
            "51 MiB against a 4 MiB budget passed, so the gate is not a gate"
        );
        // Slack of zero measures without gating, and `Never` grows on purpose.
        assert!(outcome.wal_within_budget(&config, 0));
        let unbounded = Config {
            checkpoint: Checkpoint::Never,
            ..config
        };
        assert!(
            outcome.wal_within_budget(&unbounded, 4),
            "a policy that grows the WAL by design cannot fail for growing it"
        );
    }

    /// A [`PcmSource`] that keeps producing until it is switched off.
    ///
    /// [`Canned`] finishes when its buffer runs out, which ends the capture -
    /// no use for testing a pause, where the whole question is what happens
    /// while audio keeps arriving and nothing is being written.
    struct Tap {
        next: u8,
        done: Arc<AtomicBool>,
    }

    impl PcmSource for Tap {
        fn read(&mut self, dst: &mut [u8]) -> usize {
            if self.done.load(Ordering::Relaxed) {
                return 0;
            }
            // A slow trickle, so the test can pause between reads rather than
            // racing a writer that has already swallowed the whole capture.
            let n = dst.len().min(4_096);
            for byte in &mut dst[..n] {
                *byte = self.next;
                self.next = self.next.wrapping_add(1);
            }
            std::thread::sleep(Duration::from_millis(1));
            n
        }

        fn is_finished(&self) -> bool {
            self.done.load(Ordering::Relaxed)
        }
    }

    #[test]
    fn a_paused_writer_drains_the_ring_and_commits_none_of_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let done = Arc::new(AtomicBool::new(false));
        let source = Tap {
            next: 0,
            done: Arc::clone(&done),
        };
        let handle = spawn(
            project(&dir),
            &info,
            Config {
                // Small blocks, so a short test still crosses several commits.
                block_millis: 20,
                ..Config::default()
            },
            source,
        )
        .expect("spawn");

        std::thread::sleep(Duration::from_millis(150));
        let before = handle.progress().frames();
        assert!(before > 0, "the writer committed nothing while running");

        handle.pause();
        // The flag is read at the top of the writer's loop, so give it one.
        std::thread::sleep(Duration::from_millis(50));
        assert!(handle.is_paused());
        let at_pause = handle.progress().frames();

        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            handle.progress().frames(),
            at_pause,
            "the writer committed audio while paused"
        );

        handle.resume();
        std::thread::sleep(Duration::from_millis(150));
        assert!(!handle.is_paused());
        assert!(
            handle.progress().frames() > at_pause,
            "the writer did not start again"
        );

        done.store(true, Ordering::Relaxed);
        let outcome = handle.stop().expect("stop");
        assert!(outcome.frames > 0);

        // The point of the whole exercise: the timeline has no hole in it. The
        // audio either side of the pause is adjacent, and the wall-clock time
        // the operator spent paused is simply not in the capture.
        let project = Project::open(dir.path().join("writer.vcw")).expect("reopen");
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
    fn a_writer_that_starts_paused_writes_nothing_until_it_is_told_to() {
        // §11's `Armed`: the device is open and the ring is being emptied, so
        // the counters stay honest, but the project is untouched.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let done = Arc::new(AtomicBool::new(false));
        let handle = spawn(
            project(&dir),
            &info,
            Config {
                block_millis: 20,
                start_paused: true,
                ..Config::default()
            },
            Tap {
                next: 0,
                done: Arc::clone(&done),
            },
        )
        .expect("spawn");

        std::thread::sleep(Duration::from_millis(200));
        assert!(handle.is_paused());
        assert_eq!(
            handle.progress().frames(),
            0,
            "an armed transport wrote audio before it was asked to"
        );

        handle.resume();
        std::thread::sleep(Duration::from_millis(150));
        assert!(handle.progress().frames() > 0);

        done.store(true, Ordering::Relaxed);
        let outcome = handle.stop().expect("stop");
        assert!(outcome.frames > 0);
    }

    #[test]
    fn progress_is_visible_while_the_writer_runs_not_only_after() {
        let dir = tempfile::tempdir().expect("tempdir");
        let info = info();
        let mut w = Writer::begin(project(&dir), &info, Config::default()).expect("begin");
        let progress = w.progress();
        assert_eq!(progress.frames(), 0);
        assert!(!progress.is_stopped());

        let block = w.block_frames();
        w.push(&pattern(block as usize * info.frame_bytes() * 2))
            .expect("push");
        assert_eq!(progress.frames(), block * 2);
        assert_eq!(progress.blocks(), 4);
        assert_eq!(progress.commits(), 2);
        assert_eq!(progress.checkpoints(), 0);
        assert_eq!(progress.bytes(), (block * 2 * 4 * 2) as u64);

        let _ = w.finish(CaptureState::Finalised).expect("finish");
        assert!(progress.is_stopped());
    }
}
