/*
 *  contend.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Reader threads hammering a project while the capture writer works (41).
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

//! Reader threads hammering a project while the capture writer works (§41).
//!
//! §41 asks for a contention test, and the thing worth contending over is the
//! one a user actually does during a recording: watch the waveform. The UI
//! redraws while the writer commits a block every 250 ms, and both go through
//! the same SQLite file. So the readers here are not synthetic - they run the
//! same [`vcw_project::waveform`] queries the window runs, at the same three
//! zoom levels, as fast as they can.
//!
//! # What a pass looks like
//!
//! Two things, and they are different claims:
//!
//! 1. **The readers get answers.** Not just "no error returned" - a query that
//!    covered no frames read nothing, and a thousand of those would pass a
//!    weaker test while proving the readers never touched the writer's data.
//!    [`Tally::covered`] is the evidence.
//! 2. **The writer does not slow down.** That one is not measured here: the
//!    soak already gates every commit against one block duration, so a writer
//!    that stalled behind a reader fails on the number it already reports.
//!    Running with and without `--readers` and comparing the commit tail is
//!    the experiment; this module's job is to supply the load and prove it was
//!    real.
//!
//! # Why WAL makes this interesting rather than trivial
//!
//! In WAL mode a reader does not block a writer and a writer does not block a
//! reader, which is exactly why the project uses it. That makes the expected
//! result "no contention at all" - and an expected-null test is still worth
//! running, because the ways it can fail are specific and silent: a checkpoint
//! needs every reader gone to finish, so a busy reader can hold the WAL open
//! and let it grow without bound, and that shows up as [`Outcome::peak_wal_bytes`]
//! rather than as an error anywhere.
//!
//! [`Outcome::peak_wal_bytes`]: vcw_project::persistence::Outcome::peak_wal_bytes

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use vcw_project::persistence::Latencies;
use vcw_project::sqlite::Project;
use vcw_project::waveform::{self, Shape};
use vcw_signal::waveform::{Level, Request};

/// What the readers managed while the writer ran.
#[derive(Debug, Default)]
pub(crate) struct Tally {
    /// Queries that returned a waveform.
    pub(crate) queries: u64,
    /// Frames those waveforms actually covered, summed.
    ///
    /// The non-vacuity number. Zero here means the readers ran and read
    /// nothing, whatever the query count says.
    pub(crate) covered: u64,
    /// Queries that returned an error of any kind.
    ///
    /// Not split by cause. `SQLITE_BUSY` is the one to expect if anything goes
    /// wrong, but a reader that cannot read while the writer works is the
    /// defect either way, so the first error's text is kept verbatim in
    /// [`Tally::first_error`] and the classification is left to whoever reads
    /// the report.
    pub(crate) failed: u64,
    /// The first error seen, as it printed.
    pub(crate) first_error: Option<String>,
    /// How long each query took, in microseconds.
    pub(crate) latency: Latencies,
    /// Queries answered from each rung, in [`Level`] order.
    ///
    /// Reported because a run whose queries all came from one level tested one
    /// query shape. The three spans below are chosen to spread them.
    pub(crate) by_level: [u64; 4],
}

impl Tally {
    /// Folds another thread's tally into this one.
    fn absorb(&mut self, other: Self) {
        self.queries += other.queries;
        self.covered += other.covered;
        self.failed += other.failed;
        self.first_error = self.first_error.take().or(other.first_error);
        for (mine, theirs) in self.by_level.iter_mut().zip(other.by_level) {
            *mine += theirs;
        }
        self.latency.absorb(&other.latency);
    }

    /// Whether the readers did their job: every query answered, and answered
    /// with data.
    ///
    /// `readers` is taken so that asking for readers and getting no queries out
    /// of them is a failure rather than a silent pass. A run that spawned four
    /// threads and recorded nothing tested nothing, and it looks identical in
    /// the report to a run that was never asked for readers at all.
    pub(crate) fn clean(&self, readers: usize) -> bool {
        if self.failed > 0 {
            return false;
        }
        readers == 0 || (self.queries > 0 && self.covered > 0)
    }

    /// One line for the report.
    pub(crate) fn line(&self, readers: usize) -> String {
        if readers == 0 {
            return "none (--readers 0)".to_owned();
        }
        let mut line = format!(
            "{readers} reader(s), {} queries, {} frames covered",
            self.queries, self.covered,
        );
        if let Some((p50, _, p99, max)) = self.latency.summary() {
            line += &format!(
                ", p50 {:.1} ms, p99 {:.1} ms, max {:.1} ms",
                p50 as f64 / 1_000.0,
                p99 as f64 / 1_000.0,
                max as f64 / 1_000.0,
            );
        }
        let rungs: Vec<String> = LEVELS
            .iter()
            .zip(self.by_level)
            .filter(|(_, n)| *n > 0)
            .map(|(level, n)| format!("{} {n}", level.as_str()))
            .collect();
        if !rungs.is_empty() {
            line += &format!(" ({})", rungs.join(", "));
        }
        if self.failed > 0 {
            line += &format!(
                ", {} FAILED: {}",
                self.failed,
                self.first_error.as_deref().unwrap_or("?"),
            );
        } else if self.queries > 0 && self.covered == 0 {
            line += ", but covered nothing - the readers read no audio";
        }
        line
    }
}

/// The rungs, in the order [`Tally::by_level`] counts them.
const LEVELS: [Level; 4] = [
    Level::Samples,
    Level::Summary256,
    Level::Block,
    Level::Summary64k,
];

/// Where a [`Level`] lands in [`Tally::by_level`].
fn slot(level: Level) -> usize {
    LEVELS.iter().position(|l| *l == level).unwrap_or(0)
}

/// Reader threads, running until [`Readers::stop`].
pub(crate) struct Readers {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<Tally>>,
}

impl Readers {
    /// Starts `count` readers against a project that is being written.
    ///
    /// Opening read-only is the point: it is the same `?mode=ro` connection the
    /// UI gets, so a lock the UI would hit is a lock these hit.
    ///
    /// `hz` is redraws per second per reader, and it is not a detail. A reader
    /// looping flat out issues around eleven thousand queries a second, which
    /// is not a window - it is a fuzzer, and it holds a read snapshot open
    /// essentially all the time. The window redraws at frame rate and each
    /// query costs about 0.2 ms, so its duty cycle is under 2%. The difference
    /// decides whether a starved checkpoint is a product defect or an artefact
    /// of the instrument, so the instrument is adjustable and the default
    /// models the window. Zero means flat out.
    pub(crate) fn start(
        path: &Path,
        capture_id: i64,
        channels: u16,
        count: usize,
        hz: u32,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let threads = (0..count)
            .map(|n| {
                let stop = Arc::clone(&stop);
                let path: PathBuf = path.to_path_buf();
                std::thread::spawn(move || {
                    read_until_stopped(&path, capture_id, channels, n, hz, &stop)
                })
            })
            .collect();
        Self { stop, threads }
    }

    /// Stops them and folds their tallies together.
    pub(crate) fn stop(self) -> Tally {
        self.stop.store(true, Ordering::Relaxed);
        let mut total = Tally::default();
        for thread in self.threads {
            if let Ok(tally) = thread.join() {
                total.absorb(tally);
            }
        }
        total
    }
}

/// One reader's loop.
///
/// An open that fails is counted and returned rather than retried: if the
/// project cannot be opened read-only while it is being written, that is the
/// finding, and retrying would bury it.
fn read_until_stopped(
    path: &Path,
    capture_id: i64,
    channels: u16,
    seed: usize,
    hz: u32,
    stop: &AtomicBool,
) -> Tally {
    let mut tally = Tally::default();
    let project = match Project::open_read_only(path) {
        Ok(project) => project,
        Err(error) => {
            tally.failed += 1;
            tally.first_error = Some(format!("opening the project read-only: {error}"));
            return tally;
        }
    };
    // A counter, not a random number generator. The spans need to vary so the
    // queries land on different rungs and different parts of the file; they do
    // not need to be unpredictable, and a dependency-free counter keeps the run
    // reproducible.
    let mut tick = seed as u64;
    // The gap between redraws. Sleeping is what makes the duty cycle realistic,
    // and the duty cycle is the whole question for the WAL.
    let period = (hz > 0).then(|| Duration::from_secs_f64(1.0 / f64::from(hz)));
    while !stop.load(Ordering::Relaxed) {
        tick = tick.wrapping_add(1);
        if let Some(period) = period {
            std::thread::sleep(period);
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }
        let Ok(shape) = Shape::of(project.conn(), capture_id) else {
            // Before the first commit there is no shape to read. That is not a
            // failure, it is the first few hundred milliseconds.
            std::thread::yield_now();
            continue;
        };
        if shape.frames == 0 {
            std::thread::yield_now();
            continue;
        }
        let channel = (tick % u64::from(channels.max(1))) as u16;
        let request = span(&shape, tick);
        let at = Instant::now();
        match waveform::read(project.conn(), capture_id, channel, &request) {
            Ok(drawn) => {
                tally.latency.record(at.elapsed().as_micros() as u64);
                tally.queries += 1;
                tally.covered += drawn.covered;
                tally.by_level[slot(drawn.level)] += 1;
            }
            Err(error) => {
                tally.failed += 1;
                if tally.first_error.is_none() {
                    tally.first_error = Some(error.to_string());
                }
            }
        }
    }
    tally
}

/// The span for one query: a whole-capture redraw, a window, or a close zoom.
///
/// Three shapes rather than one, because they are answered from three different
/// rungs and therefore three different query plans - and the pyramid's whole
/// claim is that the plan changes with the zoom. The window is pinned to the
/// *end* of the capture on purpose: that is where the writer is committing, so
/// it is the only span where a reader and the writer are looking at the same
/// blocks at the same time.
fn span(shape: &Shape, tick: u64) -> Request {
    const WIDTH: u32 = 1_200;
    match tick % 3 {
        0 => shape.whole(WIDTH),
        1 => {
            // The last ten seconds, where the writer is.
            let window = u64::from(shape.levels.block) * 40;
            Request::new(shape.frames.saturating_sub(window), shape.frames, WIDTH)
        }
        _ => {
            // A quarter of a second somewhere in the middle, close enough in to
            // reach the samples themselves.
            let window = u64::from(shape.levels.block);
            let start = (tick.wrapping_mul(7_919) % shape.frames.max(1))
                .min(shape.frames.saturating_sub(window));
            Request::new(start, start + window, WIDTH)
        }
    }
}
