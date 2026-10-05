/*
 *  waveform.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `vcw waveform` verb: draw a capture at the terminal (§19, §4.5).
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

//! The `vcw waveform` verb: draw a capture at the terminal (§19, §4.5).
//!
//! §4.5 asks for the whole workflow to be drivable with no UI, and a waveform is
//! the first thing in VCW that is genuinely *visual*. So this draws one, in
//! characters, at whatever width the caller asks for. It is not a toy: it is the
//! only way to look at what the pyramid returns without a React app, and it is
//! what the §37 measurements in `docs/STATUS.md` were taken with.
//!
//! The picture is the columns, nothing more. Every level of the ladder produces
//! the same shape of answer, so the same renderer draws a whole side at one
//! block a column and forty milliseconds at one sample a column, and the only
//! thing that changes is the line reporting which level it came from.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use vcw_project::waveform::{self, Rebuild, Shape};
use vcw_project::{Project, session};
use vcw_signal::waveform::{Column, Request, Waveform};

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to draw from.
    pub(crate) project: PathBuf,
    /// Capture to draw. `None` takes the only one, or the most recent.
    pub(crate) capture: Option<i64>,
    /// Channel to draw. `None` draws all of them.
    pub(crate) channel: Option<u16>,
    /// Where to start, in seconds.
    pub(crate) start: Option<f64>,
    /// Where to end, in seconds. `None` runs to the end of the capture.
    pub(crate) end: Option<f64>,
    /// Columns to draw.
    pub(crate) pixels: u32,
    /// Rows to draw each channel over.
    pub(crate) rows: u32,
    /// Recompute the pyramid from the stored audio before drawing.
    pub(crate) rebuild: bool,
    /// Machine-readable output: the columns as JSON.
    pub(crate) json: bool,
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }
    let mut project = Project::open(&args.project)
        .with_context(|| format!("opening {}", args.project.display()))?;

    let capture_id = match args.capture {
        Some(id) => id,
        None => {
            let all = session::all(project.conn())?;
            match all.last() {
                Some(record) => record.id,
                None => bail!("{} has no captures", args.project.display()),
            }
        }
    };

    if args.rebuild {
        let done = waveform::rebuild(&mut project, capture_id, Rebuild::All)?;
        if !args.json {
            println!(
                "  rebuilt    {} of {} block(s) from the stored audio",
                done.rewritten, done.examined
            );
        }
    }

    let shape = Shape::of(project.conn(), capture_id)?;
    let record = session::load(project.conn(), capture_id)?
        .ok_or_else(|| anyhow::anyhow!("capture {capture_id} is not in this project"))?;
    let rate = f64::from(record.info.rate.hz()).max(1.0);

    let frames = |seconds: f64| (seconds.max(0.0) * rate) as u64;
    let start = args.start.map_or(0, frames).min(shape.frames);
    let end = args.end.map_or(shape.frames, frames).min(shape.frames);
    let request = Request::new(start, end, args.pixels);

    let channels: Vec<u16> = match args.channel {
        Some(channel) => vec![channel],
        None => (0..shape.channels).collect(),
    };

    let began = Instant::now();
    let drawn: Vec<Waveform> = channels
        .iter()
        .map(|&channel| waveform::read(project.conn(), capture_id, channel, &request))
        .collect::<vcw_project::Result<_>>()?;
    let took = began.elapsed();
    project.close()?;

    if args.json {
        let value = serde_json::json!({
            "capture_id": capture_id,
            "rate": record.info.rate.hz(),
            "start_frame": start,
            "end_frame": end,
            "pixels": args.pixels,
            "level": drawn.first().map(|w| w.level.as_str()),
            "frames_per_pixel": request.frames_per_pixel(),
            "read_micros": took.as_micros() as u64,
            "channels": drawn.iter().zip(&channels).map(|(wave, &channel)| {
                serde_json::json!({
                    "channel": channel,
                    "covered": wave.covered,
                    "peak": wave.peak(),
                    "columns": wave.columns.iter().map(|c| serde_json::json!({
                        "min": c.min, "max": c.max, "rms": c.rms, "frames": c.frames,
                    })).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
        });
        println!("{value}");
        return Ok(());
    }

    println!(
        "  capture    {capture_id}, {} Hz, {} ch, {:.3} s",
        record.info.rate.hz(),
        shape.channels,
        record.duration_secs()
    );
    println!(
        "  span       {:.3} s to {:.3} s, {} column(s) of {:.1} frame(s)",
        start as f64 / rate,
        end as f64 / rate,
        args.pixels,
        request.frames_per_pixel()
    );
    if let Some(wave) = drawn.first() {
        println!(
            "  level      {} ({} frame(s) a triplet), read in {:.3} ms",
            wave.level.as_str(),
            shape.levels.stride(wave.level),
            took.as_secs_f64() * 1000.0
        );
    }
    for (wave, channel) in drawn.iter().zip(&channels) {
        println!("  channel {channel}  peak {:.4}", wave.peak());
        for line in plot(&wave.columns, args.rows) {
            println!("  {line}");
        }
    }
    Ok(())
}

/// Draws columns as characters, centered on zero.
///
/// Each row is one horizontal slice of the amplitude range. A column is drawn
/// where its min-to-max span reaches that slice, with a denser character where
/// the RMS reaches it too - so the outline is the extremes and the shading is
/// the energy, which is the same thing a drawn waveform shows and for the same
/// reason.
fn plot(columns: &[Column], rows: u32) -> Vec<String> {
    let rows = rows.max(1);
    let half = f64::from(rows) / 2.0;
    let mut out = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        // Row 0 is the top, which is +1.0.
        let hi = 1.0 - f64::from(row) / half;
        let lo = 1.0 - f64::from(row + 1) / half;
        let line: String = columns
            .iter()
            .map(|column| {
                if column.is_empty() {
                    return ' ';
                }
                let (min, max) = (f64::from(column.min), f64::from(column.max));
                if max < lo || min > hi {
                    return ' ';
                }
                let rms = f64::from(column.rms);
                if -rms <= hi && rms >= lo { '#' } else { '|' }
            })
            .collect();
        out.push(line);
    }
    out
}
