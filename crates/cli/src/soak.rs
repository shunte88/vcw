/*
 *  soak.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The long-run writer soak: the measurement D3 and WP-05 both depend on.
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

//! The long-run writer soak: the measurement D3 and WP-05 both depend on.
//!
//! WP-05's exit criterion is "90-minute 24/192 soak, zero loss, bounded WAL",
//! and D3's outstanding item is the same soak run against the *firmed*
//! configuration rather than the spike harness's defaults. This verb is where
//! both get satisfied, with the product code, on whatever machine it is pointed
//! at - which matters, because S2's numbers are x86_64 and the Pi 5 and Windows
//! runs are still open.
//!
//! The source is [`Simulated`] with [`Pattern::Deterministic`], which is the
//! only reason the run proves anything. Every sample is a pure function of its
//! frame and channel index, so after 90 minutes the verifier can recompute all
//! 8 GB of what should be there and compare it against what is, byte for byte.
//! A soak that only checked that the frame count looked right would pass just as
//! happily with the channels swapped.
//!
//! No hardware is involved and none is claimed: a simulated capture can never be
//! called bit-perfect, and the writer cannot tell the difference between this and
//! a turntable, which is the whole point of the `PcmSource` boundary.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use vcw_audio::capture::Negotiated;
use vcw_audio::source::{Faults, Pace, Pattern, Simulated, Source};
use vcw_project::persistence::{self, Checkpoint};
use vcw_project::{Project, validate};
use vcw_types::{CaptureState, Diagnostics, SampleFormat, SampleRate, StorageFormat};

use crate::capture::Format;

/// WAL policies selectable on the command line.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Wal {
    /// SQLite's own autocheckpoint. What S2 measured, and D3's choice.
    Automatic,
    /// Writer-issued `PASSIVE` checkpoints.
    Passive,
    /// Writer-issued `TRUNCATE` checkpoints.
    Truncate,
    /// None at all, to see how far the log actually grows.
    Never,
}

impl From<Wal> for Checkpoint {
    fn from(w: Wal) -> Self {
        match w {
            Wal::Automatic => Self::Automatic,
            Wal::Passive => Self::Passive,
            Wal::Truncate => Self::Truncate,
            Wal::Never => Self::Never,
        }
    }
}

/// Everything the soak was asked to do.
pub(crate) struct Options {
    /// Project to write. Must not already exist.
    pub(crate) project: PathBuf,
    /// Sample rate.
    pub(crate) rate: u32,
    /// Channel count.
    pub(crate) channels: u16,
    /// Sample format.
    pub(crate) format: Format,
    /// How long to run, in minutes.
    pub(crate) minutes: f64,
    /// Block duration. The D3 parameter under test.
    pub(crate) block_millis: u32,
    /// Blocks per transaction. The other D3 parameter under test.
    pub(crate) batch_blocks: usize,
    /// WAL policy.
    pub(crate) wal: Wal,
    /// Blocks between writer-issued checkpoints.
    pub(crate) checkpoint_blocks: u64,
    /// WAL ceiling in mebibytes, for the automatic policy.
    pub(crate) wal_mib: u64,
    /// Ring capacity in milliseconds.
    pub(crate) ring_millis: u32,
    /// Run flat out instead of in real time, at [`Pace::Metered`] - so nothing
    /// is dropped and the byte-for-byte readback still means something.
    ///
    /// Useless as a *timing* measurement and labeled as such in the output.
    /// Until WP-17 this ran at [`Pace::Fast`] and could not pass: the source
    /// outran the writer, the ring overran, and every overrun moved the written
    /// frame index away from the source's, so the verifier compared frame n
    /// against the sample belonging to some later frame. The flag's own doc
    /// comment claimed it was a smoke test; a first run of it produced
    /// 3,578,279 overruns and a mismatch.
    pub(crate) fast: bool,
    /// Skip the byte-for-byte readback. Only sensible when the run is being
    /// killed deliberately.
    pub(crate) no_verify: bool,
    /// Feed a real WAV rip through the capture path instead of the generated
    /// pattern. §41's file-backed capture.
    ///
    /// The file's own rate, channel count and sample format win over the flags,
    /// because reinterpreting a 44.1 kHz 16-bit rip as 192 kHz 24-bit would
    /// produce a project that verified perfectly against the wrong bytes.
    pub(crate) from_file: Option<PathBuf>,
    /// Go silent after this many seconds of audio, saying nothing - an
    /// unplugged device as it actually presents itself.
    pub(crate) vanish_after: Option<f64>,
    /// The same, with a stream error reported first. [`Faults::unplug_after`].
    pub(crate) unplug_after: Option<f64>,
    /// Report a stream error after this many seconds and keep going.
    pub(crate) error_after: Option<f64>,
    /// Deliver one empty callback after this many seconds.
    pub(crate) starve_after: Option<f64>,
    /// Cap the project at this many pages, so the writer meets `SQLITE_FULL`
    /// without needing a filesystem anybody can fill. §41's disk-full
    /// injection, and the reproduction for the piCorePlayer report: the
    /// capture stops committing and the transport has to notice by itself.
    pub(crate) max_pages: u32,
    /// Fail the run if resident memory grows by more than this many MiB.
    ///
    /// Growth from the settled baseline to the peak, so a long run is the one
    /// that makes it mean anything: a leak of a few KiB a second is invisible
    /// in ten seconds and obvious in an hour.
    pub(crate) max_growth_mib: u64,
    /// Reader threads drawing the waveform while the writer works. §41's
    /// contention test; see [`crate::contend`].
    pub(crate) readers: usize,
    /// Redraws a second per reader. Zero for flat out. See
    /// [`crate::contend::Readers::start`].
    pub(crate) reader_hz: u32,
    /// How stale the drawable waveform may get, in milliseconds. §37's
    /// sub-second claim, and zero to measure without gating.
    pub(crate) waveform_budget_millis: u64,
    /// How many times its budget the WAL may reach before the run fails. Zero
    /// to measure without gating.
    pub(crate) wal_slack: u32,
    /// Report the commit latency without gating on it. For shared machines,
    /// where the tail measures the machine rather than the writer.
    pub(crate) ignore_commit_budget: bool,
    /// How often to print a progress line, in seconds. Zero for silence.
    pub(crate) every: u64,
    /// Machine-readable output.
    pub(crate) json: bool,
}

impl Options {
    /// Whether any fault was asked for.
    ///
    /// Worth a method rather than a chain at the call site, because it decides
    /// which of two opposite pass conditions applies: a clean run has to report
    /// no loss, and a fault run has to report the fault. See [`Evidence`].
    fn injects_a_fault(&self) -> bool {
        self.vanish_after.is_some()
            || self.unplug_after.is_some()
            || self.error_after.is_some()
            || self.starve_after.is_some()
    }

    /// The faults, with the seconds turned into frame indices.
    ///
    /// Seconds of *audio*, not of wall clock, which is what makes the flags mean
    /// the same thing at either pace: a metered run produces 30 s of audio in
    /// well under 30 s, and a fault asked for at 30 s should still land 30 s
    /// into the recording.
    ///
    /// `rate` is passed rather than read off `self` because a file-backed run
    /// takes its rate from the file, and a fault asked for at 30 s has to land
    /// 30 s into *that* recording.
    fn faults(&self, rate: u32) -> Faults {
        let at = |secs: Option<f64>| secs.map(|s| (s * f64::from(rate)) as u64);
        // `unplug` is vanish and error together, so it fills both and a
        // separately-given value for either still wins if it is earlier.
        let earliest = |a: Option<u64>, b: Option<u64>| match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (found, None) | (None, found) => found,
        };
        let unplug = at(self.unplug_after);
        Faults {
            vanish_after: earliest(at(self.vanish_after), unplug),
            error_after: earliest(at(self.error_after), unplug),
            starve_after: at(self.starve_after),
        }
    }
}

/// Runs the soak.
pub(crate) fn run(options: &Options) -> Result<()> {
    if options.project.exists() {
        // An 8 GB append into a project that already holds a side is not a
        // thing anyone means to ask for.
        bail!(
            "{} already exists; the soak writes a fresh project",
            options.project.display()
        );
    }

    // Checked before anything is created, like the file below: a soak that
    // cannot honor the gate it was asked for should cost nothing to find out.
    check_growth_gate(options.max_growth_mib, resident_bytes())?;

    // A file, if one was named, and then the format it dictates. Sniffed before
    // anything is created, so a file that cannot be read costs nothing.
    let layout = options
        .from_file
        .as_deref()
        .map(crate::wavfile::layout)
        .transpose()?;
    let (rate, channels, format) = match layout {
        Some(l) => (l.rate.hz(), l.channels, l.format),
        None => (
            options.rate,
            options.channels,
            SampleFormat::from(options.format),
        ),
    };
    let pattern = match (&options.from_file, layout) {
        (Some(path), Some(l)) => Pattern::File {
            path: path.clone(),
            offset: l.offset,
            bytes: Some(l.bytes),
        },
        _ => Pattern::Deterministic,
    };
    let negotiated = Negotiated::simulated(SampleRate(rate), channels, format);
    let storage = negotiated.storage;
    let frame_bytes = negotiated.frame_bytes();
    let pace = if options.fast {
        Pace::Metered
    } else {
        Pace::RealTime
    };

    let config = persistence::Config {
        block_millis: options.block_millis,
        batch_blocks: options.batch_blocks,
        checkpoint: options.wal.into(),
        checkpoint_blocks: options.checkpoint_blocks,
        wal_bytes: options.wal_mib * 1024 * 1024,
        summaries: true,
        max_pages: options.max_pages,
        ..persistence::Config::default()
    };

    let faults = options.faults(rate);
    let (source, reader) = Simulated::start(
        negotiated.clone(),
        &pattern,
        pace,
        faults,
        options.ring_millis,
    )
    .context("starting the simulated source")?;
    let info = source.info();

    let project = Project::create(&options.project)
        .with_context(|| format!("creating {}", options.project.display()))?;
    let handle = persistence::spawn(project, &info, config, reader)
        .context("starting the capture writer")?;
    let capture_id = handle.capture_id();
    let progress = std::sync::Arc::clone(handle.progress());

    if !options.json {
        println!(
            "soaking {} Hz, {} ch, {:?} ({} B/frame) for {:.1} min into {}",
            rate,
            channels,
            storage,
            frame_bytes,
            options.minutes,
            options.project.display(),
        );
        println!(
            "  config      {} ms blocks, batch {}, {:?} checkpointing at {} MiB, \
             {} ms ring, {} pace",
            config.block_millis,
            config.batch_blocks,
            config.checkpoint,
            options.wal_mib,
            options.ring_millis,
            if options.fast { "metered" } else { "real-time" },
        );
        if let (Some(path), Some(l)) = (&options.from_file, layout) {
            println!(
                "  source      {} ({:.1} s of audio at offset {}, cycled; its format \
                 wins over the flags)",
                path.display(),
                l.seconds(),
                l.offset,
            );
        }
    }

    // After the writer, because they read the capture it created, and before
    // the clock, because the load is supposed to cover the whole run.
    let subject = crate::contend::Subject {
        path: options.project.clone(),
        capture_id,
        channels,
        rate,
        produced: std::sync::Arc::clone(source.counters()),
    };
    let readers = crate::contend::Readers::start(&subject, options.readers, options.reader_hz);

    let started = Instant::now();
    let run_for = Duration::from_secs_f64(options.minutes * 60.0);
    let mut next_report = Duration::from_secs(options.every);
    let mut growth = Growth::default();
    while started.elapsed() < run_for {
        std::thread::sleep(Duration::from_millis(200));
        // §41's memory-growth test rides the tick that already exists. Five
        // samples a second over a multi-hour run is thousands of them, which
        // is why `Growth` keeps four numbers and not a histogram.
        growth.sample(started.elapsed());
        // Hand the writer the device counters so they reach the project on
        // its own timer. Without this a soak killed mid-run leaves four
        // zeros behind, and four zeros is how a flawless capture is
        // spelled (§15).
        handle.note(source.diagnostics());
        // A writer that has stopped on a failed commit keeps its error until
        // it is joined, so without this the loop runs its full term against a
        // dead writer and prints a progress line every tick while it does -
        // measured at `--max-pages 40`: sixty seconds of reports for 0.8 s of
        // audio. A harness built to find faults must not be deaf to the one
        // it just injected. Breaking out leaves the diagnosis to `stop` below,
        // which is where it already lives.
        if !handle.is_running() {
            eprintln!(
                "  {:>6.0} s   the writer has stopped - draining for the reason",
                started.elapsed().as_secs_f64()
            );
            break;
        }
        if options.every == 0 || options.json || started.elapsed() < next_report {
            continue;
        }
        next_report += Duration::from_secs(options.every);
        let elapsed = started.elapsed().as_secs_f64();
        println!(
            "  {:>6.0} s   {:.1} s written, rtf {:.5}, worst commit {:.1} ms, \
             wal {:.2} MiB, rss {:.1} MiB, {} overrun(s)",
            elapsed,
            progress.frames() as f64 / f64::from(rate),
            progress.frames() as f64 / f64::from(rate) / elapsed,
            progress.worst_commit_micros() as f64 / 1_000.0,
            progress.peak_wal_bytes() as f64 / (1024.0 * 1024.0),
            growth.last as f64 / (1024.0 * 1024.0),
            source.diagnostics().overruns,
        );
    }
    let wall = started.elapsed();

    let diagnostics = source.stop();
    let delivered = diagnostics.dropped_frames;
    let state = if diagnostics.is_clean() {
        CaptureState::Finalised
    } else {
        CaptureState::Interrupted
    };
    handle.set_result(state, diagnostics);
    // Stopped *after* the writer, not before, so the writer's final checkpoint
    // happens with the readers still holding the WAL open. That is the case
    // worth provoking: a checkpoint needs every reader gone to truncate, so a
    // busy reader is the one thing that can leave the WAL behind. Joined before
    // the `?` so a writer that failed to stop does not leak the threads.
    let outcome = handle.stop();
    let contention = readers.stop();
    let outcome = outcome.context("stopping the capture writer")?;

    let project = Project::open(&options.project)
        .with_context(|| format!("reopening {}", options.project.display()))?;
    // Checksums on: they read every sample byte, which is exactly what a soak
    // has time for and what a routine open does not.
    let report = validate(
        &project,
        vcw_project::Options {
            verify_checksums: !options.no_verify,
        },
    )
    .context("validating")?;
    let verified = if options.no_verify {
        None
    } else {
        // Up to the starve point, if one was asked for: see `verify`. The other
        // faults lose nothing, so they are verified to the last frame written.
        Some(
            verify(
                &project,
                capture_id,
                storage,
                channels,
                faults.starve_after,
                Expected::open(options.from_file.as_deref(), layout)?,
            )
            .context("verifying")?,
        )
    };
    let file_bytes = std::fs::metadata(&options.project)
        .map(|m| m.len())
        .unwrap_or(0);
    project.close().context("closing the project")?;

    let audio_secs = outcome.frames as f64 / f64::from(rate);
    let rtf = if wall.as_secs_f64() > 0.0 {
        audio_secs / wall.as_secs_f64()
    } else {
        0.0
    };
    let (p50, p95, p99, worst) = outcome.commit.summary().unwrap_or((0, 0, 0, 0));
    // Every gate, each one naming itself when it fails.
    //
    // A list rather than a conjunction because the verdict has to say which
    // one: there are ten of these now, the report is twenty lines long, and a
    // bare "FAIL" leaves whoever reads the nightly to diff the lines and guess.
    let mut broke: Vec<&'static str> = Vec::new();
    // The damage must be confined whether a fault was asked for or not: a
    // project that no longer validates, or a byte that does not match what the
    // source produced, is a failure in either mode.
    if !report.is_clean() {
        broke.push("the project does not validate");
    }
    if verified == Some(false) {
        broke.push("a byte does not match what the source generated");
    }
    // Optional, and the only gate here that is. A hosted CI runner shares its
    // CPU and its disk with whatever else is on the box, so its commit tail
    // measures the runner and not the writer: the first nightly saw 736 ms
    // against a 250 ms budget with zero loss and every byte verified. Endurance
    // and correctness still hold there; the tail does not, so CI turns this one
    // off and the report says so.
    if !options.ignore_commit_budget && !outcome.commits_within_budget(&config) {
        broke.push("a commit took longer than its budget");
    }
    // A leak is a failure whether a fault was asked for or not, which is what
    // puts it here rather than in the clean-run arm below. So is a reader that
    // could not read: the writer is allowed to be interrupted, never to lock
    // the window out.
    if !growth.within(growth_allowance_mib(
        options.max_growth_mib,
        options.readers,
    )) {
        broke.push("resident memory grew past its allowance");
    }
    if !contention.clean(options.readers) {
        broke.push("a reader could not draw the waveform");
    }
    // §37's sub-second waveform latency, which nothing measured until now.
    // Gated with the readers rather than beside them because a stale picture is
    // a defect in the product and not in the instrument.
    if !contention.fresh_enough(options.waveform_budget_millis, !options.fast) {
        broke.push("the waveform fell further behind the device than §37 allows");
    }
    // An unbounded WAL is confinement failing in the one place that costs disk
    // rather than audio, and until WP-17 nothing checked it: a run that reached
    // 90 MiB against a 4 MiB budget printed "pass".
    if !outcome.wal_within_budget(&config, options.wal_slack) {
        broke.push("the WAL grew past its budget");
    }
    let evidence = Evidence::read(&faults, &diagnostics, outcome.frames, rate);
    if options.injects_a_fault() {
        if !evidence.every_fault_showed_up() {
            broke.push("an injected fault never showed up");
        }
    } else {
        if !diagnostics.is_clean() {
            broke.push("the device reported a loss");
        }
        // `stalls` is checked separately from `is_clean`, because it is the one
        // defect the source's counters structurally cannot report: a device
        // that stops delivering leaves all four of them at zero. See
        // `persistence::Config::stall_millis`.
        if outcome.stalls > 0 {
            broke.push("the source went quiet mid-capture");
        }
    }
    let passed = broke.is_empty();

    if options.json {
        let payload = serde_json::json!({
            "config": {
                "rate": rate, "channels": channels,
                "storage": format!("{storage:?}"), "frame_bytes": frame_bytes,
                "block_millis": config.block_millis, "batch_blocks": config.batch_blocks,
                "checkpoint": format!("{:?}", config.checkpoint),
                "checkpoint_blocks": config.checkpoint_blocks,
                "wal_bytes": config.wal_bytes,
                "ring_millis": options.ring_millis,
                "pace": if options.fast { "metered" } else { "real-time" },
                "requested_minutes": options.minutes,
            },
            "wall_secs": wall.as_secs_f64(),
            "audio_secs": audio_secs,
            "real_time_factor": rtf,
            "written": {
                "blocks": outcome.blocks, "frames": outcome.frames, "bytes": outcome.bytes,
                "commits": outcome.commits, "checkpoints": outcome.checkpoints,
                "file_bytes": file_bytes, "peak_wal_bytes": outcome.peak_wal_bytes,
            },
            "wal_slack": options.wal_slack,
            "wal_within_budget": outcome.wal_within_budget(&config, options.wal_slack),
            "stalls": outcome.stalls,
            "recorded_state": format!("{:?}", outcome.state),
            "readers": {
                "threads": options.readers,
                "hz": options.reader_hz,
                "queries": contention.queries,
                "frames_covered": contention.covered,
                "failed": contention.failed,
                "first_error": contention.first_error,
                "query_micros": contention.latency.summary()
                    .map(|(a, b, c, d)| serde_json::json!({ "p50": a, "p95": b, "p99": c, "max": d })),
                "by_level": contention.by_level,
                "waveform_lag_micros": contention.freshness.summary()
                    .map(|(a, b, c, d)| serde_json::json!({ "p50": a, "p95": b, "p99": c, "max": d })),
                "waveform_budget_millis": options.waveform_budget_millis,
                "waveform_gated": !options.fast && options.waveform_budget_millis > 0,
            },
            "memory": {
                "sampled": growth.sampled,
                "baseline_bytes": growth.baseline,
                "peak_bytes": growth.peak,
                "end_bytes": growth.last,
                "grew_bytes": growth.grew_by(),
                "limit_mib": options.max_growth_mib,
                "allowance_mib": growth_allowance_mib(options.max_growth_mib, options.readers),
            },
            "commit_micros": { "p50": p50, "p95": p95, "p99": p99, "max": worst,
                               "budget": config.commit_granularity_millis() * 1_000,
                               // So a consumer can tell a timing claim from a
                               // run that only measured the timing.
                               "gated": !options.ignore_commit_budget },
            "prepare_micros": outcome.prepare.summary()
                .map(|(a, b, c, d)| serde_json::json!({ "p50": a, "p95": b, "p99": c, "max": d })),
            "checkpoint_micros": outcome.checkpoint.summary()
                .map(|(a, b, c, d)| serde_json::json!({ "p50": a, "p95": b, "p99": c, "max": d })),
            "final_checkpoint_micros": outcome.final_checkpoint_micros,
            "faults": {
                "vanish_after_frames": faults.vanish_after,
                "error_after_frames": faults.error_after,
                "starve_after_frames": faults.starve_after,
                "observed": evidence.lines(),
                "all_showed_up": evidence.every_fault_showed_up(),
            },
            "diagnostics": diagnostics,
            "dropped_frames": delivered,
            "validate_clean": report.is_clean(),
            "bytes_verified": verified,
            "passed": passed,
            // Named, not just counted: a nightly's JSON is the only record of
            // a red leg once the log has scrolled away.
            "broke": broke,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        // The same exit status as the human-readable report, and it used to be
        // `Ok(())` unconditionally: `--json` printed `"passed": false` and left
        // the shell a zero. Anything driving the soak from a script - which is
        // the only reason the flag exists - would have read a red run as green.
        return if passed {
            Ok(())
        } else {
            bail!("the soak did not pass")
        };
    }

    println!(
        "  ran         {:.1} s wall, {:.1} s of audio, real-time factor {rtf:.5}",
        wall.as_secs_f64(),
        audio_secs,
    );
    println!(
        "  written     {} frames, {} blocks, {:.2} GiB of samples in {} commits",
        outcome.frames,
        outcome.blocks,
        outcome.bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        outcome.commits,
    );
    println!(
        "  commit      p50 {:.1} ms, p95 {:.1} ms, p99 {:.1} ms, max {:.1} ms, budget {} ms{}",
        p50 as f64 / 1_000.0,
        p95 as f64 / 1_000.0,
        p99 as f64 / 1_000.0,
        worst as f64 / 1_000.0,
        config.commit_granularity_millis(),
        if options.ignore_commit_budget {
            " (NOT GATED, so this is not a timing claim)"
        } else {
            ""
        },
    );
    if let Some((a, _, _, d)) = outcome.prepare.summary() {
        println!(
            "  prepare     p50 {:.1} ms, max {:.1} ms (deinterleave, summaries, crc)",
            a as f64 / 1_000.0,
            d as f64 / 1_000.0,
        );
    }
    println!(
        "  wal         peak {:.2} MiB of {} MiB budget{}, {} writer checkpoint(s){}",
        outcome.peak_wal_bytes as f64 / (1024.0 * 1024.0),
        options.wal_mib,
        if options.wal_slack == 0 {
            " (not gated)".to_owned()
        } else {
            format!(" x{} allowed", options.wal_slack)
        },
        outcome.checkpoints,
        outcome
            .checkpoint
            .max()
            .map_or_else(String::new, |m| format!(
                ", worst {:.1} ms",
                m as f64 / 1_000.0
            )),
    );
    println!(
        "  memory      {}",
        growth.line(options.max_growth_mib, options.readers)
    );
    println!("  readers     {}", contention.line(options.readers));
    println!(
        "  waveform    {}",
        contention.freshness_line(
            options.waveform_budget_millis,
            options.readers,
            !options.fast
        )
    );
    println!(
        "  file        {:.2} GiB",
        file_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    println!(
        "  stalls      {} (source quiet for over {} ms), capture recorded as {:?}",
        outcome.stalls, config.stall_millis, outcome.state,
    );
    println!(
        "  counters    {} overruns, {} underruns, {} dropped frames, {} stream errors",
        diagnostics.overruns,
        diagnostics.underruns,
        diagnostics.dropped_frames,
        diagnostics.stream_errors,
    );
    println!(
        "  validate    {}",
        if report.is_clean() {
            "clean".to_owned()
        } else {
            format!("{} problem(s)", report.findings.len())
        }
    );
    for finding in report.findings.iter().take(5) {
        println!("              - {}: {}", finding.code, finding.detail);
    }
    println!(
        "  bytes       {}",
        match verified {
            None => "not checked (--no-verify)".to_owned(),
            Some(true) => match faults.starve_after {
                None => format!(
                    "every one of {} matches what the source generated",
                    outcome.bytes
                ),
                Some(at) => format!(
                    "every one up to the starve at frame {at} matches what the source \
                     generated; past a loss the frame index is shifted and cannot be \
                     compared"
                ),
            },
            Some(false) => "MISMATCH - see above".to_owned(),
        }
    );
    for (n, line) in evidence.lines().iter().enumerate() {
        println!("  {:<11} {line}", if n == 0 { "faults" } else { "" });
    }
    println!(
        "  verdict     {}",
        match (passed, options.injects_a_fault()) {
            (true, false) => "pass: zero loss, bounded WAL, every byte accounted for".to_owned(),
            (true, true) => {
                "pass: the fault landed, the damage stopped there, and the project is intact"
                    .to_owned()
            }
            (false, _) => format!("FAIL - {}", broke.join("; ")),
        }
    );

    if passed {
        Ok(())
    } else {
        bail!("the soak did not pass")
    }
}

/// Resident set size of this process, in bytes.
///
/// From `/proc/self/status` rather than `/proc/self/statm`, because `VmRSS` is
/// already in kilobytes and needs no page size - and getting a page size means
/// either a `libc` dependency or an assumption, one of which is a cost and the
/// other of which is wrong on aarch64 with 16 KiB pages.
///
/// `None` anywhere without a procfs, which today means anywhere that is not
/// Linux. §41 asks for memory-growth tests and this is the platform they run
/// on; the soak says so in its output rather than reporting a zero that reads
/// like a measurement.
#[cfg(target_os = "linux")]
fn resident_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

/// Windows and macOS have no procfs, and asking them costs a dependency.
///
/// Returning `None` here is only safe because [`check_growth_gate`] refuses a
/// gated soak on a platform it cannot measure. Before it did, WP-17's memory
/// gate was live on one of three Tier 1 platforms and silently absent on the
/// other two, which CI found the day after it shipped.
#[cfg(not(target_os = "linux"))]
fn resident_bytes() -> Option<u64> {
    None
}

/// The growth allowance for a run, in MiB: the leak margin the operator asked
/// for, plus the page cache every connection in the run is entitled to fill.
///
/// Zero stays zero, because zero means no gate at all.
///
/// # Why a page cache is not growth
///
/// [`vcw_project::PAGE_CACHE_KIB`] gives the writer's connection 31.25 MiB of
/// page cache, and SQLite never asks for it back. It fills over the first
/// minutes of a run as the writer appends, and then it stops - which is the
/// shape a bounded cache has and a leak does not.
///
/// The baseline cannot absorb it. It is taken one [`Growth::SETTLE`] in, which
/// is long enough for the binary to fault in and SQLite to allocate, and far
/// too early for a cache that fills over minutes. So every MiB the cache claims
/// after the settle point is counted as growth by a measurement that is working
/// exactly as designed.
///
/// # What that cost, and what filled it, measured
///
/// CI's `hour-contended` nightly failed at 39.1 MiB of growth against a flat
/// 32 MiB gate - baseline 19.9, peak 59.1, end 56.8 - on an hour that was clean
/// on every other count: zero loss, every one of 1,036,846,080 bytes verified,
/// WAL bounded, real-time factor 1.00000, with four readers making 679,478
/// queries. Two six-minute runs on the dev box, same audio, one variable:
///
/// | readers | baseline | peak     | grew     |
/// |---------|----------|----------|----------|
/// | 0       | 14.5 MiB | 41.7 MiB | 27.2 MiB |
/// | 4       | 22.8 MiB | 52.0 MiB | 29.2 MiB |
///
/// So the writer's own connection is nearly the whole of the old gate, and four
/// readers are 2 MiB between them. That is not a reader being frugal: it is
/// [`vcw_project::Project::open_read_only`] not applying the pragma at all, so
/// a read-only connection gets SQLite's default cache rather than the
/// entitlement the writer has. Hence the two terms below rather than one
/// multiplied by connections, which is what the first version of this function
/// did on the strength of the nightly alone.
///
/// That is also why the 90-minute soak has always passed the flat gate: the
/// same run plateaus at about 42 MiB and then grows 0.5 MiB an hour, so an
/// hour-long leg spends most of its length flat and the number the gate reads
/// depends on where the baseline happened to land. A gate on a knife edge is
/// not a gate.
///
/// What is left over is the leak margin, and it is the operator's number: a
/// leak has to exceed every cache in the run *plus* `--max-growth-mib` before
/// this fails. That is weaker than the figure it replaces looked, and exactly
/// as strong as the figure it replaces was.
fn growth_allowance_mib(limit_mib: u64, readers: usize) -> u64 {
    if limit_mib == 0 {
        return 0;
    }
    // Rounded up: the entitlement is 31.25 MiB and a gate is not the place to
    // be strict about three quarters of a mebibyte.
    let writer = vcw_project::PAGE_CACHE_KIB as u64 / 1024 + 1;
    limit_mib + writer + READER_PAGE_CACHE_MIB * readers as u64
}

/// What one read-only connection's page cache may come to, in MiB.
///
/// SQLite's own default, because that is what a reader gets:
/// [`vcw_project::Project::open_read_only`] applies `foreign_keys` and nothing
/// else, so `cache_size` stays at the compiled-in `-2000`, i.e. 2000 KiB.
/// Measured, four readers over six minutes claimed 2 MiB between them - well
/// inside this - so this is a ceiling and not a prediction.
const READER_PAGE_CACHE_MIB: u64 = 2;

/// Refuses a memory gate this platform cannot honor.
///
/// `rss` is passed in rather than read here so that both answers are reachable
/// from a test on any platform.
///
/// # Why this is an error and not a warning
///
/// The alternative is what WP-17 shipped first: `--max-growth-mib 32` on a
/// machine with no procfs reported "not measured" and then printed `pass`. A
/// gate that cannot fail is worse than no gate at all, because the report reads
/// the same whether it held or was never applied. Refusing puts the choice in
/// the operator's hands and records it in the command line.
fn check_growth_gate(limit_mib: u64, rss: Option<u64>) -> Result<()> {
    if limit_mib > 0 && rss.is_none() {
        bail!(
            "this platform cannot report resident memory, so --max-growth-mib {limit_mib} \
             cannot be honored. Pass --max-growth-mib 0 to soak without the memory gate; \
             every other check still applies."
        );
    }
    Ok(())
}

/// Resident memory over the run. §41's memory-growth test.
///
/// # Why the baseline is not the first sample
///
/// Resident size climbs while the process starts: the binary faults in, SQLite
/// allocates its page cache, the ring and the block buffers are allocated. All
/// of that is setup, none of it is growth, and measuring from zero would report
/// a leak on every run. So the baseline is the first sample taken at least
/// [`Growth::SETTLE`] in, and a run shorter than that reports no baseline at all
/// rather than a misleading one.
#[derive(Debug, Default)]
struct Growth {
    /// The settled baseline, once one has been taken.
    baseline: Option<u64>,
    /// The largest sample seen.
    peak: u64,
    /// The most recent sample.
    last: u64,
    /// Whether any sample was taken at all.
    sampled: bool,
}

impl Growth {
    /// How long to let the process settle before taking the baseline.
    const SETTLE: Duration = Duration::from_secs(5);

    /// Takes a sample, reading this process's own resident size.
    fn sample(&mut self, elapsed: Duration) {
        self.sample_at(elapsed, resident_bytes());
    }

    /// The half of [`Growth::sample`] that does not touch the operating system,
    /// so which sample becomes the baseline is testable off Linux too.
    fn sample_at(&mut self, elapsed: Duration, rss: Option<u64>) {
        let Some(rss) = rss else {
            return;
        };
        self.sampled = true;
        self.last = rss;
        self.peak = self.peak.max(rss);
        if self.baseline.is_none() && elapsed >= Self::SETTLE {
            self.baseline = Some(rss);
        }
    }

    /// Bytes of growth from the settled baseline to the peak.
    ///
    /// `None` when there is no baseline, which is the honest answer for a run
    /// too short to have one - not zero, which would read as "it did not grow".
    fn grew_by(&self) -> Option<u64> {
        self.baseline.map(|base| self.peak.saturating_sub(base))
    }

    /// Whether the growth is inside the budget, in MiB. Zero disables the gate.
    ///
    /// A run with no baseline passes. It has to: there is nothing to compare,
    /// and failing a short run for being short would make `--minutes 0.1`
    /// useless for everything else it is good for.
    fn within(&self, limit_mib: u64) -> bool {
        if limit_mib == 0 {
            return true;
        }
        self.grew_by()
            .is_none_or(|grew| grew <= limit_mib * 1024 * 1024)
    }

    /// One line for the human report.
    fn line(&self, limit_mib: u64, readers: usize) -> String {
        if !self.sampled {
            return "not measured on this platform, so no memory gate ran".to_owned();
        }
        let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
        let Some(grew) = self.grew_by() else {
            return format!(
                "peak {:.1} MiB, end {:.1} MiB (run too short for a baseline)",
                mib(self.peak),
                mib(self.last),
            );
        };
        let base = self.baseline.unwrap_or_default();
        format!(
            "baseline {:.1} MiB, peak {:.1} MiB, end {:.1} MiB, grew {:.1} MiB{}",
            mib(base),
            mib(self.peak),
            mib(self.last),
            mib(grew),
            if limit_mib == 0 {
                " (no limit set)".to_owned()
            } else {
                {
                    let allowance = growth_allowance_mib(limit_mib, readers);
                    format!(
                        " of {allowance} allowed ({limit_mib} plus the {} MiB of page cache \
                         this run's connections may fill)",
                        allowance - limit_mib
                    )
                }
            },
        )
    }
}

/// What a fault run observed, against what it asked for.
///
/// # A fault run's pass condition is the opposite of a clean run's
///
/// With nothing injected, passing means nothing went wrong - clean counters, a
/// project that validates, every byte matching, no commit over budget.
///
/// With a fault injected, "nothing went wrong" is a *failure*: it means the
/// injection never happened and the run proved nothing. §41 asks for
/// dropped-frame tests, and a dropped-frame test that cannot tell whether a
/// frame was dropped is not one. So the condition becomes: the damage is
/// confined, **and** every fault asked for left a trace.
///
/// # The silent vanish is the one that does not fit
///
/// Three of the four faults announce themselves in a counter. A device that
/// simply stops delivering does not: [`vcw_types::Diagnostics`] has nothing for
/// "no data arrived for a while", so its trace is the *frame count* stopping
/// short of where the run should have reached. That is recorded here rather
/// than papered over, because it is also a product gap - a stalled device
/// currently produces a capture that looks finished.
struct Evidence {
    /// Where a vanish was asked for, and the frame the capture actually reached.
    vanish: Option<(u64, u64)>,
    /// Whether a stream error was asked for, and whether one was counted.
    error: Option<bool>,
    /// Whether a starvation was asked for, and whether an underrun was counted.
    starve: Option<bool>,
    /// Frames of slack allowed between the fault point and the last frame in.
    slack: u64,
}

impl Evidence {
    /// Reads the evidence off the faults asked for and the counters that came
    /// back.
    fn read(faults: &Faults, diagnostics: &Diagnostics, frames: u64, rate: u32) -> Self {
        Self {
            vanish: faults.vanish_after.map(|at| (at, frames)),
            error: faults.error_after.map(|_| diagnostics.stream_errors > 0),
            starve: faults.starve_after.map(|_| diagnostics.underruns > 0),
            // Half a second. The feeder pushes in 10 ms chunks and stops at the
            // first chunk at or past the fault, and the ring and the last part
            // block still drain, so the capture lands a little beyond the fault
            // point and never before it.
            slack: u64::from(rate) / 2,
        }
    }

    /// Whether the capture stopped where a vanish should have stopped it.
    ///
    /// Short of the fault point is impossible and would mean loss the vanish did
    /// not cause; far past it means the source kept delivering and the fault
    /// never landed.
    fn vanish_showed_up(&self) -> bool {
        self.vanish
            .is_none_or(|(at, frames)| frames >= at && frames <= at.saturating_add(self.slack))
    }

    /// Whether every fault asked for left a trace.
    fn every_fault_showed_up(&self) -> bool {
        self.vanish_showed_up() && self.error.unwrap_or(true) && self.starve.unwrap_or(true)
    }

    /// One line per fault, saying what was asked and what was seen.
    fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some((at, frames)) = self.vanish {
            out.push(format!(
                "vanish at frame {at}: capture reached {frames} ({}), \
                 and the counters cannot see a silent stall at all",
                if self.vanish_showed_up() {
                    "stopped where it should have"
                } else {
                    "NOT where the fault was injected"
                },
            ));
        }
        if let Some(seen) = self.error {
            out.push(format!(
                "stream error: {}",
                if seen { "counted" } else { "NONE COUNTED" }
            ));
        }
        if let Some(seen) = self.starve {
            out.push(format!(
                "starvation: {}",
                if seen {
                    "counted as an underrun"
                } else {
                    "NO UNDERRUN COUNTED"
                }
            ));
        }
        out
    }
}

/// Where the verifier gets the bytes it expects.
///
/// Two sources of truth, and the point is that both are *recomputable*: a
/// generated pattern from the frame index, or a real rip re-read from the file
/// the capture was fed. Neither is a copy of what the writer wrote, which is
/// what stops the check from being a tautology.
enum Expected {
    /// [`Simulated::expected_sample`], from the frame and channel index.
    Generated,
    /// A byte range of a WAV, cycled exactly as the feeder cycled it.
    File {
        handle: File,
        /// First byte of audio.
        offset: u64,
        /// Bytes of audio, clamped to the file's real size the same way
        /// [`Pattern::File`] clamps it. If this disagreed with the feeder by one
        /// byte, every frame after the first wrap would mismatch.
        length: u64,
    },
}

impl Expected {
    /// Opens the source a run was fed from.
    fn open(path: Option<&Path>, layout: Option<crate::wavfile::Layout>) -> Result<Self> {
        let (Some(path), Some(layout)) = (path, layout) else {
            return Ok(Self::Generated);
        };
        let handle = File::open(path)
            .with_context(|| format!("reopening {} to verify against it", path.display()))?;
        let on_disk = handle
            .metadata()
            .with_context(|| format!("sizing {}", path.display()))?
            .len();
        Ok(Self::File {
            handle,
            offset: layout.offset,
            length: layout.bytes.min(on_disk.saturating_sub(layout.offset)),
        })
    }

    /// The interleaved source bytes for a block, or `None` for a generated run.
    ///
    /// The feeder is a byte stream with a wrap, so stream byte *p* is file byte
    /// `offset + p % length` and a block starting at frame *f* starts at stream
    /// byte `f * frame_bytes`. That identity is the whole verification: it holds
    /// only if nothing was dropped, which is why a run at
    /// [`vcw_audio::source::Pace::Fast`] cannot be verified at all.
    fn span(
        &mut self,
        start_frame: u64,
        frames: u64,
        frame_bytes: usize,
    ) -> Result<Option<Vec<u8>>> {
        let Self::File {
            handle,
            offset,
            length,
        } = self
        else {
            return Ok(None);
        };
        if *length == 0 {
            return Ok(None);
        }
        let mut out = vec![0u8; frames as usize * frame_bytes];
        let from = start_frame * frame_bytes as u64;
        let mut filled = 0;
        while filled < out.len() {
            let at = (from + filled as u64) % *length;
            // Up to the wrap, then round again. One or two reads for any block
            // shorter than the file, which is every block of every real rip.
            let room = ((*length - at) as usize).min(out.len() - filled);
            handle.seek(SeekFrom::Start(*offset + at))?;
            handle.read_exact(&mut out[filled..filled + room])?;
            filled += room;
        }
        Ok(Some(out))
    }
}

/// Recomputes every sample the source should have produced and compares it with
/// what landed in the project.
///
/// Block by block rather than all at once: a 90-minute 24/192 capture is 8 GB,
/// and a verifier that needed it in memory could not check the run it was
/// written for. The frame index is taken from `start_frame`, so a block written
/// out of order or at the wrong offset fails here rather than passing on its own
/// internal consistency.
///
/// # `until`, and why a comparison has to stop at a loss
///
/// The whole method rests on written frame *n* holding the sample the source
/// produced for frame *n*, and a lost callback breaks that permanently: a
/// starved device produces nothing for a chunk while its frame index moves on,
/// so the audio for those frames never existed and every frame after the gap is
/// shifted. Comparing past that point does not detect the loss - the loss is
/// already counted as an underrun - it just reports a mismatch on every
/// remaining frame of the run.
///
/// So a run that injected a loss verifies up to the loss and stops. That is not
/// a weaker check than it looks: "the damage was confined to the fault" is
/// exactly the claim being made about a fault run, and this is what makes it
/// checkable rather than asserted.
fn verify(
    project: &Project,
    capture_id: i64,
    storage: StorageFormat,
    channels: u16,
    until: Option<u64>,
    mut expected: Expected,
) -> Result<bool> {
    let width = storage.bytes_per_sample();
    let mut stmt = project.conn().prepare(
        "SELECT b.channel, b.start_frame, b.frame_count, s.samples
         FROM capture_blocks b JOIN sampleblocks s ON s.blockid = b.blockid
         WHERE b.capture_id = ?1 ORDER BY b.sequence, b.channel",
    )?;
    let mut rows = stmt.query([capture_id])?;
    let mut expected_next = vec![0u64; channels as usize];
    let mut ok = true;
    while let Some(row) = rows.next()? {
        let channel: i64 = row.get(0)?;
        let start_frame: i64 = row.get(1)?;
        let frame_count: i64 = row.get(2)?;
        let samples: Vec<u8> = row.get(3)?;

        if channel < 0 || channel as usize >= expected_next.len() {
            println!("  !           a block claims channel {channel} of {channels}");
            return Ok(false);
        }
        let slot = &mut expected_next[channel as usize];
        if start_frame as u64 != *slot {
            println!(
                "  !           channel {channel} block starts at frame {start_frame}, \
                 expected {slot}"
            );
            ok = false;
        }
        if samples.len() != frame_count as usize * width {
            println!(
                "  !           channel {channel} block at {start_frame} holds {} bytes for \
                 {frame_count} frames",
                samples.len()
            );
            ok = false;
        }
        // One read per block for a file-backed run, not one per sample: the
        // block's frames are a contiguous span of the source stream even though
        // one channel's samples inside it are not.
        let span = expected
            .span(
                start_frame as u64,
                frame_count as u64,
                width * channels as usize,
            )
            .with_context(|| format!("reading the source for frame {start_frame}"))?;
        for i in 0..frame_count as usize {
            let frame = start_frame as u64 + i as u64;
            if until.is_some_and(|last| frame >= last) {
                return Ok(ok);
            }
            let generated = Simulated::expected_sample(frame, channel as u16).to_le_bytes();
            let want: &[u8] = match &span {
                // Interleaved, so this channel's sample sits `channel` samples
                // into the frame.
                Some(bytes) => {
                    let at = i * width * channels as usize + channel as usize * width;
                    &bytes[at..at + width]
                }
                None => &generated[..width],
            };
            let got = &samples[i * width..(i + 1) * width];
            if got != want {
                println!(
                    "  !           channel {channel} frame {frame}: stored {got:02X?}, \
                     source produced {want:02X?}"
                );
                return Ok(false);
            }
        }
        *slot = start_frame as u64 + frame_count as u64;
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Growth` as the sampler would have left it, without a `/proc` read.
    fn growth(baseline: Option<u64>, peak: u64) -> Growth {
        Growth {
            baseline,
            peak,
            last: peak,
            sampled: true,
        }
    }

    #[test]
    fn the_memory_gate_fails_a_run_that_grew_past_its_budget() {
        // WP-17, §41. The gate exists because the measurement alone is not a
        // test - a soak that printed a rising number and passed anyway would
        // have reported the leak and shipped it.
        let mib = 1024 * 1024;
        let leaked = growth(Some(40 * mib), 140 * mib);
        assert_eq!(leaked.grew_by(), Some(100 * mib));
        assert!(
            !leaked.within(32),
            "100 MiB of growth passed a 32 MiB budget"
        );
        assert!(
            leaked.within(128),
            "100 MiB of growth failed a 128 MiB budget"
        );
        assert!(
            leaked.within(0),
            "a zero budget is supposed to measure without gating"
        );

        // What the real soaks measure: flat, a fraction of a MiB either way.
        assert!(growth(Some(40 * mib), 40 * mib + 105_000).within(32));
    }

    #[test]
    fn a_page_cache_is_an_allowance_and_not_growth() {
        // CI's nightly and the dev box's answer to it, in assertions. 39.1 MiB
        // of growth with four readers failed the flat 32 MiB gate on an hour
        // clean on every other count, and a six-minute run with no readers at
        // all grew 27.2 MiB - so the cache the configuration already promised
        // every connection is most of what the gate was reading.
        let mib = 1024 * 1024;
        let contended = growth(Some(19 * mib), 59 * mib);
        assert!(
            !contended.within(32),
            "40 MiB of growth passed a flat 32 MiB gate"
        );
        assert!(
            contended.within(growth_allowance_mib(32, 4)),
            "growth inside five connections' page cache failed the gate"
        );

        // The writer's connection is nearly all of it, which is the correction
        // the uncontended run forced: 27.2 MiB of growth with no readers at
        // all.
        // It passed the flat gate, which is the knife edge rather than a
        // defense: 27.2 MiB of cache against a 32 MiB budget leaves under 5
        // MiB, so on that gate a run's verdict turned on where the baseline
        // landed. Against the allowance it has the whole budget to spare.
        let uncontended = growth(Some(14 * mib), 41 * mib + 717 * mib / 1000);
        assert!(uncontended.within(32));
        assert!(
            uncontended.grew_by().unwrap() > 32 * mib * 3 / 4,
            "the fixture no longer sits near the flat gate it exposed"
        );
        assert!(
            uncontended.within(growth_allowance_mib(32, 0)),
            "the writer's page cache failed the gate"
        );

        // A leak is still a leak: past every cache in the run plus the margin.
        assert!(
            !growth(Some(19 * mib), 319 * mib).within(growth_allowance_mib(32, 4)),
            "300 MiB of growth passed"
        );

        // Zero means no gate, and adding connections to no gate must not make
        // one.
        assert_eq!(growth_allowance_mib(0, 4), 0);

        // The arithmetic itself, so a changed cache size is a failing test
        // rather than a quietly different gate. The writer is entitled to the
        // configured cache; a reader gets SQLite's default and is charged for
        // that instead.
        let writer = vcw_project::PAGE_CACHE_KIB as u64 / 1024 + 1;
        assert_eq!(growth_allowance_mib(32, 0), 32 + writer);
        assert_eq!(
            growth_allowance_mib(32, 4),
            32 + writer + 4 * READER_PAGE_CACHE_MIB
        );
        // A reader is worth a fraction of the writer, not a copy of it. The
        // first version of this gate multiplied and allowed 187 MiB.
        assert!(READER_PAGE_CACHE_MIB * 8 < writer);
    }

    #[test]
    fn the_memory_line_says_what_it_allowed_and_why() {
        // A report that prints one number while the gate uses another is how a
        // gate stops being checkable from its own output.
        let line = growth(Some(19 * 1024 * 1024), 59 * 1024 * 1024).line(32, 4);
        assert!(
            line.contains(&format!("of {} allowed", growth_allowance_mib(32, 4))),
            "{line}"
        );
        assert!(
            line.contains(&format!(
                "{} MiB of page cache",
                growth_allowance_mib(32, 4) - 32
            )),
            "{line}"
        );
        // And with no readers the cache it names is the writer's alone.
        let solo = growth(Some(19 * 1024 * 1024), 20 * 1024 * 1024).line(32, 0);
        assert!(
            solo.contains(&format!(
                "{} MiB of page cache",
                growth_allowance_mib(32, 0) - 32
            )),
            "{solo}"
        );
    }

    #[test]
    fn a_run_with_no_settled_baseline_reports_nothing_rather_than_zero() {
        // Growth from a baseline taken before the process settled would be
        // startup allocation counted as a leak, so a run too short to have one
        // has to say so - and pass, because there is nothing to judge.
        let short = growth(None, 40 * 1024 * 1024);
        assert_eq!(short.grew_by(), None);
        assert!(short.within(1), "a run with no baseline was failed");
        assert!(short.line(1, 0).contains("too short"));

        // And a platform without procfs is not a flawless run, it is an
        // unmeasured one.
        assert!(Growth::default().line(32, 0).contains("not measured"));
    }

    #[test]
    fn the_baseline_is_the_first_sample_after_the_settle_window() {
        // `sample_at` rather than `sample`: the assertions are about which
        // sample became the baseline, and reading a real RSS to make that point
        // is what made this test fail on Windows and macOS, where there is no
        // procfs and every sample was `None`.
        let mut g = Growth::default();
        g.sample_at(Duration::from_secs(1), Some(40 * 1024 * 1024));
        assert!(g.sampled, "an early sample should still be recorded");
        assert_eq!(g.baseline, None, "the baseline was taken before the settle");
        g.sample_at(Growth::SETTLE, Some(41 * 1024 * 1024));
        assert_eq!(
            g.baseline,
            Some(41 * 1024 * 1024),
            "no baseline at the settle boundary"
        );
        g.sample_at(
            Growth::SETTLE + Duration::from_secs(60),
            Some(50 * 1024 * 1024),
        );
        assert_eq!(
            g.baseline,
            Some(41 * 1024 * 1024),
            "the baseline moved after it was set"
        );
        assert_eq!(g.peak, 50 * 1024 * 1024, "the peak did not follow the run");
    }

    #[test]
    fn a_sample_the_platform_cannot_give_leaves_the_growth_unmeasured() {
        let mut g = Growth::default();
        g.sample_at(Growth::SETTLE, None);
        assert!(!g.sampled, "an absent reading counted as a sample");
        assert_eq!(g.baseline, None);
        assert!(g.line(32, 0).contains("no memory gate ran"));
    }

    #[test]
    fn a_memory_gate_the_platform_cannot_honor_is_refused() {
        // Both ends, which is the whole point: the gate must be refusable and
        // it must not refuse a run that never asked for it.
        let refused = check_growth_gate(32, None);
        let message = refused
            .expect_err("an unmeasurable gate was accepted")
            .to_string();
        assert!(
            message.contains("--max-growth-mib 0"),
            "no remedy: {message}"
        );

        assert!(
            check_growth_gate(0, None).is_ok(),
            "an ungated soak was refused for being ungated"
        );
        assert!(
            check_growth_gate(32, Some(40 * 1024 * 1024)).is_ok(),
            "a measurable gate was refused"
        );
    }
}
