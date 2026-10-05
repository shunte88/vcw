/*
 *  capture.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  REQUIREMENTS §47.3–.7, §47.11, §47.12 - open a stream, store it, measure
 *  it.
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

//! REQUIREMENTS §47.3–.7, §47.11, §47.12 - open a stream, store it, measure it.
//!
//! The whole point of this path is that nothing touches the samples. CPAL's
//! typed `build_input_stream::<T>` would convert; `build_input_stream_raw` hands
//! back the device's own bytes, and those bytes go into the ring and then into
//! SQLite unaltered. Verification (§47.9) therefore checksums exactly what the
//! converter produced, which is the only definition of bit-perfect worth having.
//!
//! The callback's contract, from §10: never allocate, never lock, never do I/O,
//! never block. If the ring is full it drops and counts - the writer falling
//! behind must degrade into a reported gap, not into a stalled audio thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig, SupportedStreamConfigRange};
use serde::Serialize;

use capture_core::config::{Layout, Params};
use capture_core::{verify, writer};

use crate::devices;
use crate::hwparams;
use crate::meter::{Meter, Reading};

#[derive(Debug, Clone)]
pub struct Request {
    pub device: Option<String>,
    pub rate: Option<u32>,
    pub channels: Option<u16>,
    pub format: Option<SampleFormat>,
    pub buffer_frames: Option<u32>,
    pub db_path: String,
    pub duration: u64,
    pub block_ms: u32,
    pub batch_blocks: usize,
    pub ring_ms: u32,
    pub layout: Layout,
    pub meter_interval_ms: u64,
    pub quiet: bool,
}

#[derive(Debug, Serialize)]
pub struct FormatReport {
    pub requested_rate: Option<u32>,
    pub requested_channels: Option<u16>,
    pub requested_format: Option<String>,
    pub negotiated_rate: u32,
    pub negotiated_channels: u16,
    pub negotiated_format: String,
    pub bytes_per_sample: usize,
    pub buffer_size: String,
    /// True only when every requested field came back unchanged. A `None`
    /// request cannot be honored or violated, so it does not count against this.
    pub honored: bool,
    pub divergences: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub device: String,
    pub format: FormatReport,
    /// What the kernel says it actually negotiated. Empty off Linux.
    pub kernel_hw_params: Vec<hwparams::HwParams>,
    pub kernel_agrees: Option<bool>,
    pub elapsed_secs: f64,
    pub frames_captured: u64,
    pub frames_dropped: u64,
    pub overrun_events: u64,
    pub stream_errors: Vec<String>,
    pub callbacks: u64,
    pub blocks_committed: u64,
    pub rows_written: u64,
    pub bytes_written: u64,
    pub commit_latency_ms: capture_core::metrics::LatencySummary,
    pub peak_wal_bytes: u64,
    pub final_meter: Reading,
    pub clipped: bool,
    pub verify: Option<verify::VerifyReport>,
    pub verdict: String,
}

/// Choose a stream config, preferring exactly what was asked for.
///
/// Where the caller left a field open we bias towards the *widest integer*
/// format the device offers rather than CPAL's default, because the default is
/// frequently f32 - which on a 24-bit converter means the stack has already
/// converted before we ever see a sample.
fn choose(
    supported: Vec<SupportedStreamConfigRange>,
    req: &Request,
) -> Result<(StreamConfig, SampleFormat)> {
    if supported.is_empty() {
        bail!("device reports no supported input configurations");
    }
    let rank = |f: SampleFormat| match f {
        SampleFormat::I32 => 0,
        SampleFormat::I24 => 1,
        SampleFormat::I16 => 2,
        SampleFormat::F32 => 3,
        SampleFormat::F64 => 4,
        _ => 9,
    };

    let mut best: Option<(SupportedStreamConfigRange, u32)> = None;
    for range in supported {
        if let Some(ch) = req.channels
            && range.channels() != ch
        {
            continue;
        }
        if let Some(f) = req.format
            && range.sample_format() != f
        {
            continue;
        }
        let rate = match req.rate {
            Some(r) => {
                if r < range.min_sample_rate() || r > range.max_sample_rate() {
                    continue;
                }
                r
            }
            None => range.max_sample_rate(),
        };
        let score = rank(range.sample_format());
        if best
            .as_ref()
            .is_none_or(|(b, _)| score < rank(b.sample_format()))
        {
            best = Some((range, rate));
        }
    }

    let Some((range, rate)) = best else {
        bail!(
            "no supported configuration matches rate={:?} channels={:?} format={:?}",
            req.rate,
            req.channels,
            req.format
        );
    };

    let buffer_size = match req.buffer_frames {
        Some(n) => cpal::BufferSize::Fixed(n),
        None => cpal::BufferSize::Default,
    };
    let format = range.sample_format();
    let config = StreamConfig {
        channels: range.channels(),
        sample_rate: rate,
        buffer_size,
    };
    Ok((config, format))
}

pub fn run(req: Request) -> Result<Report> {
    let (device, device_name) = match &req.device {
        Some(q) => devices::find(q, true)?,
        None => devices::default_input()?,
    };

    let supported: Vec<_> = device
        .supported_input_configs()
        .context("querying supported input configs")?
        .collect();
    let (config, format) = choose(supported, &req)?;

    let mut divergences = Vec::new();
    if let Some(r) = req.rate
        && r != config.sample_rate
    {
        divergences.push(format!("rate {r} -> {}", config.sample_rate));
    }
    if let Some(c) = req.channels
        && c != config.channels
    {
        divergences.push(format!("channels {c} -> {}", config.channels));
    }
    if let Some(f) = req.format
        && f != format
    {
        divergences.push(format!("format {f:?} -> {format:?}"));
    }

    let bytes_per_sample = format.sample_size();
    let mut params = Params::default_for(config.sample_rate, config.channels, bytes_per_sample);
    params.block_ms = req.block_ms;
    params.batch_blocks = req.batch_blocks;
    params.ring_ms = req.ring_ms;
    params.layout = req.layout;
    params.duration = req.duration;

    if std::path::Path::new(&req.db_path).exists() {
        std::fs::remove_file(&req.db_path).ok();
        for suffix in ["-wal", "-shm"] {
            std::fs::remove_file(format!("{}{suffix}", req.db_path)).ok();
        }
    }

    let (producer, consumer) = rtrb::RingBuffer::<u8>::new(params.ring_bytes());
    let stop = Arc::new(AtomicBool::new(false));
    let blocks_committed = Arc::new(AtomicU64::new(0));

    let writer_handle = {
        let p = params.clone();
        let db = req.db_path.clone();
        let stop = Arc::clone(&stop);
        let blocks = Arc::clone(&blocks_committed);
        std::thread::Builder::new()
            .name("writer".into())
            .spawn(move || writer::run(p, db, consumer, stop, blocks))?
    };

    // Shared with the real-time callback. Everything here is lock-free by
    // construction; see the module note.
    let meter = Arc::new(Meter::default());
    let frames_captured = Arc::new(AtomicU64::new(0));
    let frames_dropped = Arc::new(AtomicU64::new(0));
    let overruns = Arc::new(AtomicU64::new(0));
    let callbacks = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    let stream = {
        let meter = Arc::clone(&meter);
        let frames_captured = Arc::clone(&frames_captured);
        let frames_dropped = Arc::clone(&frames_dropped);
        let overruns = Arc::clone(&overruns);
        let callbacks = Arc::clone(&callbacks);
        let errors_cb = Arc::clone(&errors);
        let frame_bytes = params.frame_bytes();
        let mut ring = producer;

        device
            .build_input_stream_raw(
                config,
                format,
                move |data, _info| {
                    let bytes = data.bytes();
                    callbacks.fetch_add(1, Ordering::Relaxed);
                    meter.observe(bytes, format, config.channels);
                    let frames = (bytes.len() / frame_bytes) as u64;
                    // Drop whole callbacks, never partial ones: a half-written
                    // buffer would corrupt frame alignment for everything after it.
                    if ring.slots() >= bytes.len()
                        && let Ok(chunk) = ring.write_chunk_uninit(bytes.len())
                    {
                        chunk.fill_from_iter(bytes.iter().copied());
                        frames_captured.fetch_add(frames, Ordering::Relaxed);
                    } else {
                        frames_dropped.fetch_add(frames, Ordering::Relaxed);
                        overruns.fetch_add(1, Ordering::Relaxed);
                    }
                },
                move |err| {
                    // Not the RT thread; a lock here is fine and the alternative
                    // is discarding the one message that explains a failure.
                    if let Ok(mut v) = errors_cb.lock() {
                        v.push(err.to_string());
                    }
                },
                None,
            )
            .with_context(|| format!("building input stream on {device_name:?}"))?
    };

    let started = Instant::now();
    stream.play().context("starting the input stream")?;

    // Give ALSA a moment to publish hw_params before reading them.
    std::thread::sleep(Duration::from_millis(200));
    let kernel_hw_params = hwparams::open_capture_streams();
    let kernel_agrees = kernel_agreement(&kernel_hw_params, &config, format);

    let deadline = started + Duration::from_secs(req.duration);
    let mut last_meter = Instant::now();
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        if !req.quiet && last_meter.elapsed() >= Duration::from_millis(req.meter_interval_ms) {
            last_meter = Instant::now();
            let r = meter.take();
            eprintln!(
                "  {:5.1}s  L {:7.2} dBFS peak / {:7.2} rms   R {:7.2} / {:7.2}   \
                 blocks {:<6} dropped {}{}",
                started.elapsed().as_secs_f64(),
                r.peak_dbfs[0],
                r.rms_dbfs[0],
                r.peak_dbfs[1],
                r.rms_dbfs[1],
                blocks_committed.load(Ordering::Relaxed),
                frames_dropped.load(Ordering::Relaxed),
                if r.clipped() { "  *** CLIP ***" } else { "" },
            );
        }
    }

    let elapsed = started.elapsed().as_secs_f64();
    drop(stream);
    stop.store(true, Ordering::Relaxed);
    let mut outcome = writer_handle
        .join()
        .map_err(|_| anyhow::anyhow!("writer thread panicked"))??;

    let final_meter = meter.take();
    let report_verify = verify::verify_live(&req.db_path).ok();
    let dropped = frames_dropped.load(Ordering::Relaxed);
    let stream_errors = errors.lock().map(|v| v.clone()).unwrap_or_default();

    let honored = divergences.is_empty();
    let verdict = verdict(
        dropped,
        &stream_errors,
        report_verify.as_ref(),
        kernel_agrees,
        honored,
    );

    Ok(Report {
        device: device_name,
        format: FormatReport {
            requested_rate: req.rate,
            requested_channels: req.channels,
            requested_format: req.format.map(|f| format!("{f:?}")),
            negotiated_rate: config.sample_rate,
            negotiated_channels: config.channels,
            negotiated_format: format!("{format:?}"),
            bytes_per_sample,
            buffer_size: format!("{:?}", config.buffer_size),
            honored,
            divergences,
        },
        kernel_hw_params,
        kernel_agrees,
        elapsed_secs: elapsed,
        frames_captured: frames_captured.load(Ordering::Relaxed),
        frames_dropped: dropped,
        overrun_events: overruns.load(Ordering::Relaxed),
        stream_errors,
        callbacks: callbacks.load(Ordering::Relaxed),
        blocks_committed: outcome.blocks,
        rows_written: outcome.rows,
        bytes_written: outcome.bytes,
        commit_latency_ms: outcome.commit_latency.summary(),
        peak_wal_bytes: outcome.peak_wal_bytes,
        final_meter,
        clipped: final_meter.clipped(),
        verify: report_verify,
        verdict,
    })
}

fn kernel_agreement(
    params: &[hwparams::HwParams],
    config: &StreamConfig,
    format: SampleFormat,
) -> Option<bool> {
    // Exactly one open capture stream is the unambiguous case. With none we
    // learned nothing; with several we cannot say which is ours.
    let [only] = params else { return None };
    let rate_ok = only.rate == Some(config.sample_rate);
    let ch_ok = only.channels == Some(config.channels);
    let fmt_ok = only
        .format
        .as_deref()
        .and_then(|f| hwparams::alsa_format_matches(f, format))?;
    Some(rate_ok && ch_ok && fmt_ok)
}

fn verdict(
    dropped: u64,
    errors: &[String],
    v: Option<&verify::VerifyReport>,
    kernel_agrees: Option<bool>,
    honored: bool,
) -> String {
    let mut faults = Vec::new();
    if dropped > 0 {
        faults.push(format!("{dropped} frames dropped"));
    }
    if !errors.is_empty() {
        faults.push(format!("{} stream errors", errors.len()));
    }
    if let Some(v) = v {
        if !v.integrity_ok {
            faults.push("database integrity check failed".into());
        }
        if v.checksum_failures > 0 {
            faults.push(format!("{} checksum failures", v.checksum_failures));
        }
        if v.sequence_gaps > 0 {
            faults.push(format!("{} sequence gaps", v.sequence_gaps));
        }
    } else {
        faults.push("verification did not run".into());
    }
    if !faults.is_empty() {
        return format!("FAIL: {}", faults.join("; "));
    }
    // A clean capture that silently got a different format than asked for is
    // not a pass in this tool's terms - that is the §8 failure mode.
    match (honored, kernel_agrees) {
        (false, _) => "PASS (capture clean, but the requested format was not honored)".into(),
        (true, Some(false)) => "PASS (capture clean, but the kernel negotiated a different format \
             - NOT bit-perfect)"
            .into(),
        (true, Some(true)) => "PASS (bit-perfect: kernel confirms the negotiated format)".into(),
        (true, None) => {
            "PASS (capture clean; kernel-level format unverified on this platform)".into()
        }
    }
}
