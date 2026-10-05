/*
 *  main.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  `vinyl-audio-test` - spike S1, the utility REQUIREMENTS §47 asks for.
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

//! `vinyl-audio-test` - spike S1, the utility REQUIREMENTS §47 asks for.
//!
//! Twelve behaviors, and which subcommand covers each:
//!
//! | § | behavior | where |
//! |---|---|---|
//! | 1 | list devices | `devices` |
//! | 2 | list supported formats | `devices --verbose`, `formats` |
//! | 3 | open the requested stream | `capture` |
//! | 4 | create a SQLite project | `capture` (via `capture-core::db`) |
//! | 5 | capture PCM into SQLite blocks | `capture` |
//! | 6 | live peak/RMS | `capture` (meter line) |
//! | 7 | capture diagnostics | `capture` report |
//! | 8 | play captured audio from SQLite | `play` |
//! | 9 | verify block checksums | `verify`, and automatically after `capture` |
//! | 10 | simulate interruption and recovery | `crash-test` |
//! | 11 | requested vs negotiated format | `capture` report, cross-checked against the kernel |
//! | 12 | overruns, underruns, dropped frames | `capture` and `play` reports |
//!
//! Storage, verification and crash recovery are S2's code (`capture-core`),
//! driven here by a live device instead of a synthetic source. That is
//! deliberate: it means S2's storage findings carry over to S1 without an
//! asterisk, and there is one implementation to port at WP-02/WP-04.

mod capture;
mod devices;
mod hwparams;
mod meter;
mod play;

use anyhow::{Result, bail};
use capture_core::config::Layout;
use capture_core::verify;
use clap::{Parser, Subcommand, ValueEnum};
use cpal::SampleFormat;

#[derive(Parser)]
#[command(
    name = "vinyl-audio-test",
    about = "Spike S1: end-to-end audio capture, storage, playback and verification",
    long_about = None
)]
struct Cli {
    /// Emit JSON instead of a human report.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum Fmt {
    I16,
    I24,
    I32,
    F32,
}

impl From<Fmt> for SampleFormat {
    fn from(f: Fmt) -> Self {
        match f {
            Fmt::I16 => SampleFormat::I16,
            Fmt::I24 => SampleFormat::I24,
            Fmt::I32 => SampleFormat::I32,
            Fmt::F32 => SampleFormat::F32,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// §47.1, §47.2 - list devices and what they will accept.
    Devices {
        /// Show every supported configuration, not just the defaults.
        #[arg(long, short)]
        verbose: bool,
        /// Only devices usable for input.
        #[arg(long)]
        input_only: bool,
    },
    /// §47.2 - supported formats for one device, in full.
    Formats {
        /// Device name or unique substring. Defaults to the default input.
        device: Option<String>,
    },
    /// §47.3–.7, .11, .12 - capture into a SQLite project and report on it.
    Capture {
        #[arg(long)]
        device: Option<String>,
        #[arg(long)]
        rate: Option<u32>,
        #[arg(long)]
        channels: Option<u16>,
        #[arg(long, value_enum)]
        format: Option<Fmt>,
        /// Ask the device for a specific callback size, in frames.
        #[arg(long)]
        buffer_frames: Option<u32>,
        #[arg(long, default_value = "./capture.vcw")]
        db: String,
        #[arg(long, default_value_t = 30)]
        duration: u64,
        /// Block duration. S2's measured recommendation is 250 ms.
        #[arg(long, default_value_t = 250)]
        block_ms: u32,
        #[arg(long, default_value_t = 1)]
        batch_blocks: usize,
        #[arg(long, default_value_t = 500)]
        ring_ms: u32,
        #[arg(long, value_enum, default_value_t = Layout::PerChannel)]
        layout: Layout,
        /// Meter print interval; 0 to silence the meter.
        #[arg(long, default_value_t = 1000)]
        meter_ms: u64,
    },
    /// §47.8, §47.12 - play a capture back out of SQLite.
    Play {
        #[arg(long, default_value = "./capture.vcw")]
        db: String,
        #[arg(long)]
        device: Option<String>,
        /// Stop after this many seconds rather than playing the whole capture.
        #[arg(long)]
        max_secs: Option<u64>,
    },
    /// §47.9 - integrity, checksums and sequencing over a stored capture.
    Verify {
        #[arg(default_value = "./capture.vcw")]
        db: String,
    },
    /// §47.10 - kill a live capture mid-flight and verify what survived.
    CrashTest {
        #[arg(long)]
        device: Option<String>,
        #[arg(long)]
        rate: Option<u32>,
        #[arg(long)]
        channels: Option<u16>,
        #[arg(long, value_enum)]
        format: Option<Fmt>,
        #[arg(long, default_value = "./crash.vcw")]
        db: String,
        /// Seconds of capture before the process is killed.
        #[arg(long, default_value_t = 7)]
        kill_after: u64,
        #[arg(long, default_value_t = 3)]
        cycles: u32,
        #[arg(long, default_value_t = 250)]
        block_ms: u32,
        #[arg(long, default_value_t = 1)]
        batch_blocks: usize,
        /// Ring capacity. Measured finding: varying this 100..1000 ms does not
        /// change recovery loss at all - the writer keeps the ring near-empty,
        /// so it is a throughput cushion, not a durability exposure.
        #[arg(long, default_value_t = 500)]
        ring_ms: u32,
        /// Allowance for audio the driver has buffered but not yet handed to a
        /// callback. This is real, unrecoverable exposure on SIGKILL and it is
        /// *not* covered by commit granularity. At 192 kHz on `hw:` here ALSA
        /// reports buffer=32768 frames = 170 ms; 250 ms leaves margin.
        #[arg(long, default_value_t = 250)]
        inflight_ms: u32,
        /// Internal: the child process entry point.
        #[arg(long, hide = true)]
        child: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Devices {
            verbose,
            input_only,
        } => cmd_devices(cli.json, verbose, input_only),
        Command::Formats { device } => cmd_formats(cli.json, device.as_deref()),
        Command::Capture {
            device,
            rate,
            channels,
            format,
            buffer_frames,
            db,
            duration,
            block_ms,
            batch_blocks,
            ring_ms,
            layout,
            meter_ms,
        } => {
            let report = capture::run(capture::Request {
                device,
                rate,
                channels,
                format: format.map(Into::into),
                buffer_frames,
                db_path: db,
                duration,
                block_ms,
                batch_blocks,
                ring_ms,
                layout,
                meter_interval_ms: meter_ms.max(1),
                quiet: cli.json || meter_ms == 0,
            })?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_capture(&report);
            }
            if report.verdict.starts_with("FAIL") {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Play {
            db,
            device,
            max_secs,
        } => {
            let report = play::run(&db, device.as_deref(), max_secs)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_play(&report);
            }
            if report.verdict.starts_with("FAIL") {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Verify { db } => {
            let report = verify::verify_live(&db)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_verify(&report);
            }
            let ok = report.integrity_ok && report.checksum_failures == 0;
            if !ok {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::CrashTest {
            device,
            rate,
            channels,
            format,
            db,
            kill_after,
            cycles,
            block_ms,
            batch_blocks,
            ring_ms,
            inflight_ms,
            child,
        } => cmd_crash_test(CrashArgs {
            json: cli.json,
            device,
            rate,
            channels,
            format: format.map(Into::into),
            db,
            kill_after,
            cycles,
            block_ms,
            batch_blocks,
            ring_ms,
            inflight_ms,
            child,
        }),
    }
}

fn cmd_devices(json: bool, verbose: bool, input_only: bool) -> Result<()> {
    let mut all = devices::enumerate()?;
    if input_only {
        all.retain(|d| !d.input_configs.is_empty());
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    let mut host = String::new();
    for d in &all {
        if d.host != host {
            host = d.host.clone();
            println!("\n=== host: {host} ===");
        }
        let mut flags = Vec::new();
        if d.is_default_input {
            flags.push("default input");
        }
        if d.is_default_output {
            flags.push("default output");
        }
        if d.direct_hardware {
            flags.push("DIRECT HARDWARE - bit-perfect capable");
        }
        if d.converting {
            flags.push("plug layer - silently converts, never bit-perfect");
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", flags.join(", "))
        };
        println!("\n{}{flags}", d.name);
        // The id is what `--device` should be given: names collide, ids do not.
        if let Some(id) = &d.id {
            println!("  id: {id}");
        }
        if let Some(c) = &d.default_input {
            println!(
                "  default in : {} ch  {} Hz  {}",
                c.channels, c.min_rate, c.sample_format
            );
        }
        if let Some(c) = &d.default_output {
            println!(
                "  default out: {} ch  {} Hz  {}",
                c.channels, c.min_rate, c.sample_format
            );
        }
        if !d.input_configs.is_empty() {
            println!("  input  configs: {}", d.input_configs.len());
        }
        if !d.output_configs.is_empty() {
            println!("  output configs: {}", d.output_configs.len());
        }
        if verbose {
            for c in &d.input_configs {
                println!(
                    "    in   {:>2} ch  {:>6}–{:<6} Hz  {:<4} ({} B)  buffer {:?}",
                    c.channels,
                    c.min_rate,
                    c.max_rate,
                    c.sample_format,
                    c.bytes_per_sample,
                    c.buffer_frames
                );
            }
        }
        for e in &d.errors {
            println!("  ! {e}");
        }
    }
    println!("\n{} devices", all.len());
    Ok(())
}

fn cmd_formats(json: bool, device: Option<&str>) -> Result<()> {
    let all = devices::enumerate()?;
    let hits: Vec<_> = match device {
        // Match the id first and exactly: `hw:CARD=0,DEV=0` should select that
        // PCM, not every device whose name happens to contain the substring.
        Some(q) => {
            let by_id: Vec<_> = all.iter().filter(|d| d.id.as_deref() == Some(q)).collect();
            if by_id.is_empty() {
                all.iter()
                    .filter(|d| {
                        d.name.contains(q) || d.id.as_deref().is_some_and(|i| i.contains(q))
                    })
                    .collect()
            } else {
                by_id
            }
        }
        None => all.iter().filter(|d| d.is_default_input).collect(),
    };
    if hits.is_empty() {
        bail!("no device matching {device:?}");
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }
    for d in hits {
        println!(
            "\n{} ({})  id: {}",
            d.name,
            d.host,
            d.id.as_deref().unwrap_or("<none>")
        );
        println!("  input:");
        for c in &d.input_configs {
            println!(
                "    {:>2} ch  {:>6}–{:<6} Hz  {:<4} ({} B/sample)  buffer {:?}",
                c.channels,
                c.min_rate,
                c.max_rate,
                c.sample_format,
                c.bytes_per_sample,
                c.buffer_frames
            );
        }
        println!("  output:");
        for c in &d.output_configs {
            println!(
                "    {:>2} ch  {:>6}–{:<6} Hz  {:<4} ({} B/sample)  buffer {:?}",
                c.channels,
                c.min_rate,
                c.max_rate,
                c.sample_format,
                c.bytes_per_sample,
                c.buffer_frames
            );
        }
    }
    Ok(())
}

struct CrashArgs {
    json: bool,
    device: Option<String>,
    rate: Option<u32>,
    channels: Option<u16>,
    format: Option<SampleFormat>,
    db: String,
    kill_after: u64,
    cycles: u32,
    block_ms: u32,
    batch_blocks: usize,
    ring_ms: u32,
    inflight_ms: u32,
    child: bool,
}

fn cmd_crash_test(a: CrashArgs) -> Result<()> {
    let CrashArgs {
        json,
        device,
        rate,
        channels,
        format,
        db,
        kill_after,
        cycles,
        block_ms,
        batch_blocks,
        ring_ms,
        inflight_ms,
        child,
    } = a;
    if child {
        // Capture until killed. The parent supplies the axe.
        let _ = capture::run(capture::Request {
            device,
            rate,
            channels,
            format,
            buffer_frames: None,
            db_path: db,
            duration: 86_400,
            block_ms,
            batch_blocks,
            ring_ms,
            layout: Layout::PerChannel,
            meter_interval_ms: 1000,
            quiet: true,
        })?;
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    let mut results = Vec::new();
    for cycle in 1..=cycles {
        for suffix in ["", "-wal", "-shm"] {
            std::fs::remove_file(format!("{db}{suffix}")).ok();
        }
        let mut proc = std::process::Command::new(&exe)
            .arg("crash-test")
            .arg("--child")
            .arg("--db")
            .arg(&db)
            .arg("--kill-after")
            .arg(kill_after.to_string())
            .arg("--block-ms")
            .arg(block_ms.to_string())
            .arg("--batch-blocks")
            .arg(batch_blocks.to_string())
            .arg("--ring-ms")
            .arg(ring_ms.to_string())
            .args(device.iter().flat_map(|d| ["--device", d.as_str()]))
            .args(
                rate.iter()
                    .flat_map(|r| ["--rate".to_string(), r.to_string()]),
            )
            .args(
                channels
                    .iter()
                    .flat_map(|c| ["--channels".to_string(), c.to_string()]),
            )
            .args(
                format
                    .iter()
                    .flat_map(|f| ["--format".to_string(), format!("{f:?}").to_lowercase()]),
            )
            .stdout(std::process::Stdio::null())
            .spawn()?;
        std::thread::sleep(std::time::Duration::from_secs(kill_after));
        // SIGKILL: no unwinding, no flush, no cooperation. The only interruption
        // model worth testing is the one the application cannot participate in.
        proc.kill()?;
        let _ = proc.wait();

        let report = verify::verify_live(&db)?;
        // Worst-case loss is one un-committed batch, by construction.
        // A SIGKILL loses the uncommitted batch *plus* whatever the driver has
        // buffered but never delivered. Budgeting only the batch (as this test
        // first did) understates the floor and fails correct runs. Recovery is
        // also quantised to a block boundary, so the effective loss rounds up.
        // Ring capacity is deliberately absent: it was measured not to matter.
        // See docs/spikes/S1-cpal-capture.md.
        let commit_budget = (block_ms as f64 / 1000.0) * batch_blocks as f64;
        let inflight_budget = inflight_ms as f64 / 1000.0;
        let block_secs = block_ms as f64 / 1000.0;
        let raw = commit_budget + inflight_budget;
        let budget = (raw / block_secs).ceil() * block_secs;
        let lost = (kill_after as f64 - report.duration_secs).max(0.0);
        results.push(serde_json::json!({
            "cycle": cycle,
            "killed_at_secs": kill_after,
            "recovered_secs": report.duration_secs,
            "lost_secs": lost,
            "loss_budget_secs": budget,
            "loss_budget_commit_secs": commit_budget,
            "loss_budget_inflight_secs": inflight_budget,
            "loss_budget_ring_secs_unused": ring_ms as f64 / 1000.0,
            "within_budget": lost <= budget + 1e-9,
            "report": report,
        }));
        if !json {
            println!(
                "cycle {cycle}: killed at {kill_after:.2}s, recovered {:.2}s \
                 (lost {lost:.2}s, budget {budget:.2}s) integrity={} checksums={} gaps={}",
                report.duration_secs,
                if report.integrity_ok { "ok" } else { "FAILED" },
                report.checksum_failures,
                report.sequence_gaps
            );
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    }
    let all_ok = results.iter().all(|r| {
        r["within_budget"].as_bool() == Some(true)
            && r["report"]["integrity_ok"].as_bool() == Some(true)
            && r["report"]["checksum_failures"].as_u64() == Some(0)
    });
    if !json {
        println!(
            "\n{}",
            if all_ok {
                "PASS: every kill recovered within commit granularity + driver buffer, integrity and checksums clean"
            } else {
                "FAIL: see cycles above"
            }
        );
    }
    if !all_ok {
        std::process::exit(1);
    }
    Ok(())
}

fn print_capture(r: &capture::Report) {
    println!("\n=== capture ===");
    println!("device            {}", r.device);
    println!(
        "requested         rate={:?} channels={:?} format={:?}",
        r.format.requested_rate, r.format.requested_channels, r.format.requested_format
    );
    println!(
        "negotiated        {} Hz  {} ch  {}  ({} B/sample)  buffer {}",
        r.format.negotiated_rate,
        r.format.negotiated_channels,
        r.format.negotiated_format,
        r.format.bytes_per_sample,
        r.format.buffer_size
    );
    if r.format.divergences.is_empty() {
        println!("                  request honored exactly");
    } else {
        for d in &r.format.divergences {
            println!("  ! diverged      {d}");
        }
    }
    match r.kernel_agrees {
        Some(true) => {
            println!("kernel hw_params  agrees - the device really is running this format")
        }
        Some(false) => {
            println!("kernel hw_params  DISAGREES - a conversion is happening below CPAL")
        }
        None => println!("kernel hw_params  not determinable here"),
    }
    for p in &r.kernel_hw_params {
        println!(
            "                  {} : {} {} Hz {} ch period={:?} buffer={:?}",
            p.path,
            p.format.as_deref().unwrap_or("?"),
            p.rate.unwrap_or(0),
            p.channels.unwrap_or(0),
            p.period_size,
            p.buffer_size
        );
    }
    println!("\nelapsed           {:.2} s", r.elapsed_secs);
    println!("callbacks         {}", r.callbacks);
    println!("frames captured   {}", r.frames_captured);
    println!(
        "frames dropped    {}  ({} overrun events)",
        r.frames_dropped, r.overrun_events
    );
    println!(
        "blocks committed  {}  ({} rows, {})",
        r.blocks_committed,
        r.rows_written,
        capture_core::metrics::human_bytes(r.bytes_written)
    );
    println!(
        "commit latency    p50 {:.1} ms  p99 {:.1} ms  max {:.1} ms",
        r.commit_latency_ms.p50_us as f64 / 1000.0,
        r.commit_latency_ms.p99_us as f64 / 1000.0,
        r.commit_latency_ms.max_us as f64 / 1000.0
    );
    println!(
        "peak WAL          {}",
        capture_core::metrics::human_bytes(r.peak_wal_bytes)
    );
    println!(
        "final meter       L {:.2} / {:.2} dBFS   R {:.2} / {:.2} dBFS{}",
        r.final_meter.peak_dbfs[0],
        r.final_meter.rms_dbfs[0],
        r.final_meter.peak_dbfs[1],
        r.final_meter.rms_dbfs[1],
        if r.clipped { "   *** CLIPPED ***" } else { "" }
    );
    for e in &r.stream_errors {
        println!("  ! stream error  {e}");
    }
    if let Some(v) = &r.verify {
        print_verify(v);
    }
    println!("\n{}", r.verdict);
}

fn print_play(r: &play::Report) {
    println!("\n=== playback ===");
    println!("device            {}", r.device);
    println!(
        "stored            {} Hz  {} ch  {} B/sample  {}  ({} blocks, {:.2} s)",
        r.stored_rate,
        r.stored_channels,
        r.stored_bytes_per_sample,
        r.stored_layout,
        r.blocks,
        r.duration_secs
    );
    println!("output format     {}", r.output_format);
    if let Some(n) = &r.conversion_note {
        println!("  ! {n}");
    }
    println!("frames played     {}", r.frames_played);
    println!(
        "short callbacks   {}  (1 expected: the end of the capture)",
        r.short_callbacks
    );
    for e in &r.device_errors {
        println!("  ! device error  {e}");
    }
    println!("elapsed           {:.2} s", r.elapsed_secs);
    println!("\n{}", r.verdict);
}

fn print_verify(r: &verify::VerifyReport) {
    println!("\n=== verify ===");
    println!(
        "integrity         {}",
        if r.integrity_ok {
            "ok".to_string()
        } else {
            r.integrity_detail.clone()
        }
    );
    println!(
        "blocks/rows       {} / {}   frames {}   {:.2} s",
        r.blocks, r.rows, r.frames, r.duration_secs
    );
    println!("checksum failures {}", r.checksum_failures);
    println!(
        "pattern check     {}",
        if r.pattern_checked {
            format!("{} failures", r.pattern_failures)
        } else {
            "n/a (live audio has no expected-value oracle)".into()
        }
    );
    println!("sequence gaps     {}", r.sequence_gaps);
    println!(
        "database          {}",
        capture_core::metrics::human_bytes(r.db_bytes)
    );
}
