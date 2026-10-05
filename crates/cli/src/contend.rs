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

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use vcw_audio::capture::Counters;
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
    /// How far behind the device the drawable waveform was, in microseconds.
    ///
    /// §37's *sub-second waveform latency*, measured end to end on the Rust
    /// clock: the newest frame the device has handed to the ring, minus the
    /// newest frame a read-only connection can draw, converted at the capture
    /// rate. So it contains the ring dwell, the commit interval and SQLite's
    /// visibility, which is the whole of what stands between a stylus and a
    /// pixel except the drawing itself.
    pub(crate) freshness: Latencies,
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
        self.freshness.absorb(&other.freshness);
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

    /// §37's waveform latency, as a line of its own.
    ///
    /// Not folded into the readers' line: this is a claim about the product -
    /// how stale the picture a person is watching can get - and the readers are
    /// only the instrument that measures it.
    pub(crate) fn freshness_line(
        &self,
        budget_millis: u64,
        readers: usize,
        real_time: bool,
    ) -> String {
        let Some((p50, _, p99, max)) = self.freshness.summary() else {
            return if readers == 0 {
                "not measured (--readers 0, so nothing drew the waveform)".to_owned()
            } else {
                "not measured (the readers never saw a committed block)".to_owned()
            };
        };
        let ms = |micros: u64| micros as f64 / 1_000.0;
        format!(
            "behind the device by p50 {:.0} ms, p99 {:.0} ms, max {:.0} ms over {} redraw(s){}",
            ms(p50),
            ms(p99),
            ms(max),
            self.freshness.count(),
            match (real_time, budget_millis) {
                (false, _) =>
                    ", not gated: at a metered pace this is production speed, not staleness"
                        .to_owned(),
                (true, 0) => ", not gated".to_owned(),
                (true, budget) => format!(", budget {budget} ms (§37)"),
            },
        )
    }

    /// Whether the waveform stayed inside §37's sub-second claim.
    ///
    /// The p99 rather than the maximum, and the difference is the point: one
    /// redraw held up behind a checkpoint is not a product that feels slow,
    /// while one redraw in a hundred arriving a second late is. The maximum is
    /// reported beside it either way, and zero measures without gating.
    ///
    /// `real_time` is the other way this returns true regardless. At a metered
    /// pace the source is throttled but still outruns the wall clock, so the
    /// frames between the device and the drawable end are a ratio of production
    /// speed rather than a duration anybody experiences - the same reason
    /// `--fast` cannot make a commit-latency claim either. The figure is still
    /// printed, with the reason on the end of the line.
    pub(crate) fn fresh_enough(&self, budget_millis: u64, real_time: bool) -> bool {
        !real_time
            || budget_millis == 0
            || self
                .freshness
                .quantile(0.99)
                .is_none_or(|p99| p99 <= budget_millis * 1_000)
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

/// What the readers read, and what to compare it against.
///
/// A struct rather than six arguments, and it carries the device's own frame
/// counter because §37's waveform latency is a difference between two clocks
/// that only this process can see both of: what the device has produced, and
/// what a read-only connection can draw.
#[derive(Clone)]
pub(crate) struct Subject {
    /// The project file, opened `?mode=ro` by each reader.
    pub(crate) path: PathBuf,
    /// The capture being written.
    pub(crate) capture_id: i64,
    /// Its channel count, so the readers spread over the channels.
    pub(crate) channels: u16,
    /// Its rate, for turning a frame count into a duration.
    pub(crate) rate: u32,
    /// Frames the device has handed to the ring, live.
    pub(crate) produced: Arc<Counters>,
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
    /// decides whether a starved checkpoint is a product defect or an artifact
    /// of the instrument, so the instrument is adjustable and the default
    /// models the window. Zero means flat out.
    pub(crate) fn start(subject: &Subject, count: usize, hz: u32) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let threads = (0..count)
            .map(|n| {
                let stop = Arc::clone(&stop);
                let subject = subject.clone();
                std::thread::spawn(move || read_until_stopped(&subject, n, hz, &stop))
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
fn read_until_stopped(subject: &Subject, seed: usize, hz: u32, stop: &AtomicBool) -> Tally {
    let Subject {
        path,
        capture_id,
        channels,
        rate,
        produced,
    } = subject;
    let (capture_id, channels, rate) = (*capture_id, *channels, *rate);
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
        // Read before the query, not after: the query costs a fraction of a
        // millisecond against a lag of hundreds, and reading it first means the
        // instrument can only ever understate itself.
        let produced_now = produced.frames();
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
        // What the window could draw, against what the stylus has delivered.
        if rate > 0 && produced_now > shape.frames {
            let behind = produced_now - shape.frames;
            tally.freshness.record(behind * 1_000_000 / u64::from(rate));
        } else if rate > 0 {
            // Not an impossibility: a commit can land between the two reads.
            tally.freshness.record(0);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A tally whose readers were behind the device by these many milliseconds.
    fn behind(millis: &[u64]) -> Tally {
        let mut tally = Tally::default();
        for ms in millis {
            tally.freshness.record(ms * 1_000);
        }
        tally
    }

    #[test]
    fn the_waveform_gate_fails_a_run_that_fell_behind() {
        // §37. The p99 and not the maximum: a hundred redraws, one of
        // them stuck behind a checkpoint for two seconds, is a product that
        // feels fine, and the maximum is printed beside the verdict either way.
        let mut millis: Vec<u64> = (0..99).map(|_| 200).collect();
        millis.push(2_000);
        let one_bad_redraw = behind(&millis);
        assert_eq!(one_bad_redraw.freshness.max(), Some(2_000_000));
        assert!(
            one_bad_redraw.fresh_enough(1_000, true),
            "one redraw in a hundred should not fail §37's claim"
        );

        // Where it does fail: behind by a second and a half, consistently.
        let slow = behind(&[1_500; 100]);
        assert!(
            !slow.fresh_enough(1_000, true),
            "1.5 s of lag passed a 1 s budget"
        );
        assert!(
            slow.fresh_enough(2_000, true),
            "1.5 s of lag failed a 2 s budget"
        );
        assert!(
            slow.fresh_enough(0, true),
            "a zero budget is supposed to measure without gating"
        );

        // And the pace, which is the one that would have bitten a real run:
        // `--fast` produces hours of audio in minutes, so the frames between
        // the device and the drawable end are a throughput ratio. Gating on it
        // would fail every metered leg in the harness.
        assert!(
            slow.fresh_enough(1_000, false),
            "a metered run cannot make a staleness claim, so it cannot fail one"
        );
        assert!(
            slow.freshness_line(1_000, 4, false).contains("not gated"),
            "and the line has to say why: {}",
            slow.freshness_line(1_000, 4, false)
        );
    }

    #[test]
    fn a_run_that_measured_nothing_says_so_rather_than_passing_quietly() {
        // The failure this guards against is a silent one: a reader that never
        // saw a committed block records no freshness at all, and a gate on an
        // empty quantile would have called that a sub-second waveform.
        let nothing = Tally::default();
        assert!(nothing.freshness.summary().is_none());
        assert!(
            nothing
                .freshness_line(1_000, 0, true)
                .contains("--readers 0"),
            "with no readers the line should name the reason"
        );
        assert!(
            nothing
                .freshness_line(1_000, 4, true)
                .contains("never saw a committed block"),
            "with readers the line should name the other reason: {}",
            nothing.freshness_line(1_000, 4, true)
        );

        // And `clean` is the gate that catches it, because `fresh_enough` on an
        // empty measurement cannot: there is no percentile to compare.
        assert!(nothing.fresh_enough(1_000, true));
        assert!(
            !nothing.clean(4),
            "four readers and no queries is a failure"
        );
    }

    #[test]
    fn the_waveform_line_reports_the_percentiles_it_gated_on() {
        let tally = behind(&[100, 200, 300, 400]);
        let line = tally.freshness_line(1_000, 4, true);
        assert!(line.contains("4 redraw(s)"), "{line}");
        assert!(line.contains("max 400 ms"), "{line}");
        assert!(line.contains("budget 1000 ms (§37)"), "{line}");
        assert!(
            tally.freshness_line(0, 4, true).contains("not gated"),
            "a zero budget should say so in the report, not just in the verdict"
        );
    }
}
