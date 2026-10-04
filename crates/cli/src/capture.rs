/*
 *  capture.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  `vcw capture` - open a device, record for a while, and say honestly what
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

//! `vcw capture` - open a device, record for a while, and say honestly what
//! happened.
//!
//! §4.5 wants the capture workflow drivable with no UI present, and this is the
//! smallest verb that exercises the whole of WP-04 end to end: negotiate, start,
//! drain the ring, ask the operating system what it really did, weigh the
//! evidence, and persist the counters.
//!
//! **With `--project`, the samples are written.** The ring's reading end is
//! handed to [`vcw_project::persistence`], which owns the writer thread, the
//! block layout and the transactions. Without `--project` there is nowhere to
//! put them, so they are drained and counted: draining still matters, because a
//! ring nobody reads fills in milliseconds and every callback after that is an
//! overrun, which would make the diagnostics report a problem the verb invented.
//!
//! The stream itself stays on the main thread, because a
//! [`vcw_audio::capture::Capture`] is not `Send`. Only the reading end moves,
//! which is the piece that was designed to.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use vcw_audio::capture::{BitPerfect, Capture, Request};
use vcw_audio::devices::{self, Direction};
use vcw_project::{Project, Session, persistence};
use vcw_types::{CaptureEq, CaptureMode, CaptureState, SampleFormat, SampleRate};

/// Sample formats selectable on the command line (§8).
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Format {
    /// 16-bit signed integer.
    S16,
    /// 24-bit signed integer.
    S24,
    /// 32-bit signed integer.
    S32,
    /// 32-bit float.
    F32,
}

impl From<Format> for SampleFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::S16 => Self::S16,
            Format::S24 => Self::S24,
            Format::S32 => Self::S32,
            Format::F32 => Self::F32,
        }
    }
}

/// Capture modes selectable on the command line (§9).
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Mode {
    /// Take the device exclusively. The only mode that can be bit-perfect.
    Exclusive,
    /// Open at the device's own rate without asking for exclusivity.
    Native,
    /// Go through the system mixer. Never bit-perfect.
    Shared,
}

impl From<Mode> for CaptureMode {
    fn from(m: Mode) -> Self {
        match m {
            Mode::Exclusive => Self::Exclusive,
            Mode::Native => Self::Native,
            Mode::Shared => Self::Shared,
        }
    }
}

/// Equalisation provenance selectable on the command line (§51).
///
/// A flag rather than a prompt, and with no default beyond `unknown`, because the
/// answer is a property of the operator's hifi and not of this run: it is the same
/// for every rip they will ever make, and the place it eventually belongs is the
/// settings file. Until then, stating it per session is better than guessing it
/// once.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Eq {
    /// Nothing applied upstream: a flat transfer.
    Flat,
    /// An ordinary phono stage applied the RIAA curve.
    Riaa,
    /// Not stated. What a capture gets when nobody says.
    Unknown,
}

impl From<Eq> for CaptureEq {
    fn from(eq: Eq) -> Self {
        match eq {
            Eq::Flat => Self::Flat,
            Eq::Riaa => Self::Riaa,
            Eq::Unknown => Self::Unknown,
        }
    }
}

/// Everything the verb was asked to do.
pub(crate) struct Options {
    /// Device id, or a name if it is unambiguous.
    pub(crate) device: String,
    /// Sample rate to pin, if any.
    pub(crate) rate: Option<u32>,
    /// Channel count to pin, if any.
    pub(crate) channels: Option<u16>,
    /// Sample format to pin, if any.
    pub(crate) format: Option<Format>,
    /// Mode to request.
    pub(crate) mode: Mode,
    /// How long to record.
    pub(crate) seconds: f64,
    /// Ring capacity in milliseconds.
    pub(crate) ring_millis: u32,
    /// Project to record the session in, created if it does not exist.
    pub(crate) project: Option<std::path::PathBuf>,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Runs one capture.
pub(crate) fn run(options: &Options) -> Result<()> {
    let snapshot = devices::enumerate();
    let device = snapshot
        .find(&options.device, Direction::Input)
        .with_context(|| format!("looking up {:?}", options.device))?;

    let mut request = Request::new(device.key.clone()).mode(options.mode.into());
    request.ring_millis = options.ring_millis;
    if let Some(hz) = options.rate {
        request = request.at(SampleRate(hz));
    }
    if let Some(ch) = options.channels {
        request = request.channels(ch);
    }
    if let Some(f) = options.format {
        request = request.format(f.into());
    }

    let (capture, reader) = Capture::start_on(device, &request)
        .with_context(|| format!("opening {}", device.label()))?;

    // A session row before a single frame is drained, so that a capture killed
    // in the next second still leaves evidence that it happened (§15). Only
    // then does the writer take the project and the ring away.
    let info = capture.info();
    let config = persistence::Config::default();
    let (mut writing, mut discarding, session) = match &options.project {
        Some(path) => {
            let mut project = open_or_create(path)?;
            let session = Session::begin(&mut project, &info).context("beginning the session")?;
            let handle = persistence::spawn_on(project, session, &info, config, reader)
                .context("starting the capture writer")?;
            (Some(handle), None, Some(session))
        }
        None => (None, Some(reader), None),
    };

    let run_for = Duration::from_secs_f64(options.seconds);
    let read = match &mut discarding {
        Some(reader) => drain(reader, run_for),
        // The writer thread is doing the draining. Sleeping here is the honest
        // spelling of "this thread has nothing to do but hold the stream open".
        None => {
            std::thread::sleep(run_for);
            0
        }
    };

    let frames = capture.frames();
    let verdict = capture.verdict();
    let negotiated = capture.negotiated().clone();
    let verification = capture.verification().evidence();
    let verified = capture.verification().confirms();
    // Drops the stream, which drops the ring's writing end, which is how the
    // writer thread learns that no more audio is coming.
    let diagnostics = capture.stop();
    // Interrupted, not finalised, if anything was lost. The stop was orderly
    // either way; the capture was not.
    let state = if diagnostics.is_clean() {
        CaptureState::Finalised
    } else {
        CaptureState::Interrupted
    };

    let outcome = match writing.take() {
        Some(handle) => {
            handle.set_result(state, diagnostics);
            Some(handle.stop().context("stopping the capture writer")?)
        }
        None => None,
    };

    if let (Some(path), Some(s)) = (&options.project, session) {
        if outcome.is_none() {
            s.finish(&mut open_or_create(path)?, state, frames, diagnostics)
                .context("finishing the session")?;
        }
        // The verification only exists once the stream has run, and by then the
        // writer owns the connection, so this is a second open rather than a
        // held one. §9's confirmation, recorded when it is actually known.
        let project =
            Project::open(path).with_context(|| format!("reopening {}", path.display()))?;
        s.record_verification(project.conn(), verified, Some(verification.as_str()))
            .context("recording the verification")?;
        project.close().context("closing the project")?;
    }

    if options.json {
        let payload = serde_json::json!({
            "device": { "label": device.label(), "id": device.key.to_string(),
                        "host": device.key.host() },
            "requested": {
                "rate": options.rate, "channels": options.channels,
                "format": options.format.map(|f| format!("{:?}", SampleFormat::from(f))),
                "mode": CaptureMode::from(options.mode).as_str(),
            },
            "negotiated": {
                "rate": negotiated.rate.hz(), "channels": negotiated.channels,
                "format": format!("{:?}", negotiated.format),
                "storage": format!("{:?}", negotiated.storage),
                "mode": negotiated.mode.as_str(),
                "transport": negotiated.transport.to_string(),
                "buffer": negotiated.buffer,
                "divergences": negotiated.divergences.iter()
                    .map(ToString::to_string).collect::<Vec<_>>(),
            },
            "os": { "verified": verified, "evidence": verification },
            "frames": frames,
            "bytes_read": read,
            "diagnostics": diagnostics,
            "bit_perfect": verdict.is_confirmed(),
            "verdict": verdict.summary(),
            "project": options.project.as_ref().map(|p| p.display().to_string()),
            "capture_id": session.map(|s| s.id()),
            "written": outcome.as_ref().map(|o| serde_json::json!({
                "blocks": o.blocks,
                "frames": o.frames,
                "bytes": o.bytes,
                "commits": o.commits,
                "checkpoints": o.checkpoints,
                "peak_wal_bytes": o.peak_wal_bytes,
                "commit_micros": o.commit.summary()
                    .map(|(p50, p95, p99, max)| serde_json::json!({
                        "p50": p50, "p95": p95, "p99": p99, "max": max })),
                "within_budget": o.commits_within_budget(&config),
                "state": o.state.as_str(),
            })),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("{}  {}", device.label(), device.key);
    println!(
        "  requested   {}, {}, {}, {}",
        options
            .rate
            .map_or_else(|| "any rate".to_owned(), |r| format!("{r} Hz")),
        options
            .channels
            .map_or_else(|| "any channels".to_owned(), |c| format!("{c} ch")),
        options.format.map_or_else(
            || "any format".to_owned(),
            |f| format!("{:?}", SampleFormat::from(f))
        ),
        CaptureMode::from(options.mode).as_str(),
    );
    println!(
        "  negotiated  {} Hz, {} ch, {:?}, {}, {}, buffer {}",
        negotiated.rate.hz(),
        negotiated.channels,
        negotiated.format,
        negotiated.mode.as_str(),
        negotiated.transport,
        negotiated.buffer,
    );
    for divergence in &negotiated.divergences {
        println!("  !           {divergence}");
    }
    println!(
        "  storage     {:?}, {} bytes per frame, {} B/s",
        negotiated.storage,
        negotiated.frame_bytes(),
        negotiated.bytes_per_second(),
    );
    for (n, line) in verification.lines().enumerate() {
        println!(
            "  {}  {line}",
            if n == 0 { "os says   " } else { "          " }
        );
    }
    if let Some(o) = &outcome {
        println!(
            "  captured    {frames} frames from the device in {:.1} s",
            options.seconds
        );
        println!(
            "  written     {} frames, {} blocks, {} bytes, {} commits",
            o.frames, o.blocks, o.bytes, o.commits,
        );
        if let Some((p50, p95, p99, max)) = o.commit.summary() {
            println!(
                "  commit      p50 {:.1} ms, p95 {:.1} ms, p99 {:.1} ms, max {:.1} ms \
                 against a {} ms budget",
                p50 as f64 / 1_000.0,
                p95 as f64 / 1_000.0,
                p99 as f64 / 1_000.0,
                max as f64 / 1_000.0,
                config.commit_granularity_millis(),
            );
        }
        println!(
            "  wal         peak {:.2} MiB, {} checkpoint(s)",
            o.peak_wal_bytes as f64 / (1024.0 * 1024.0),
            o.checkpoints,
        );
        if o.frames < frames {
            // Not an error, and worth saying out loud: the difference is audio
            // the device delivered that never reached the file.
            println!(
                "  !           {} frame(s) the device delivered were not written",
                frames - o.frames
            );
        }
    } else {
        println!(
            "  captured    {frames} frames, {read} bytes drained and discarded in {:.1} s",
            options.seconds
        );
    }
    println!(
        "  counters    {} overruns, {} underruns, {} dropped frames, {} stream errors",
        diagnostics.overruns,
        diagnostics.underruns,
        diagnostics.dropped_frames,
        diagnostics.stream_errors,
    );
    match &verdict {
        BitPerfect::Confirmed => println!("  verdict     {}", verdict.summary()),
        BitPerfect::Refuted { reasons } | BitPerfect::Unconfirmed { reasons } => {
            println!(
                "  verdict     {}",
                if matches!(verdict, BitPerfect::Refuted { .. }) {
                    "not bit-perfect"
                } else {
                    "bit-perfect not established"
                }
            );
            for reason in reasons {
                println!("              - {reason}");
            }
        }
    }
    if let (Some(path), Some(s)) = (&options.project, session) {
        println!("  persisted   capture {} in {}", s.id(), path.display());
    }

    Ok(())
}

/// Reads the ring for the requested duration and throws the bytes away.
///
/// Used only when there is no `--project` to write them into. Draining still
/// matters: a ring nobody reads fills in [`vcw_audio::buffers::MIN_MILLIS`] and
/// every callback after that is an overrun, so a verb that skipped this would
/// report a fault of its own making. Timing the loop off the wall clock rather
/// than off a frame count means a device delivering nothing still ends when it
/// was asked to.
fn drain(reader: &mut vcw_audio::buffers::RingReader, run_for: Duration) -> u64 {
    let mut scratch = vec![0u8; 64 * 1024];
    let deadline = Instant::now() + run_for;
    let mut total = 0u64;
    while Instant::now() < deadline {
        let n = reader.read(&mut scratch);
        if n == 0 {
            // Nothing ready. A tenth of the ring is long enough not to spin and
            // far short of filling it, whatever the rate.
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        total += n as u64;
    }
    // Whatever the device handed over between the last read and the deadline.
    loop {
        let n = reader.read(&mut scratch);
        if n == 0 {
            break;
        }
        total += n as u64;
    }
    total
}

/// Opens the project, creating it if this is the first capture into it.
fn open_or_create(path: &std::path::Path) -> Result<Project> {
    if path.exists() {
        Project::open(path).with_context(|| format!("opening {}", path.display()))
    } else {
        Project::create(path).with_context(|| format!("creating {}", path.display()))
    }
}
