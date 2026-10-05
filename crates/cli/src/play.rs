/*
 *  play.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `vcw play` verb: audition a capture, or render it without a device.
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

//! The `vcw play` verb: audition a capture, or render it without a device.
//!
//! §21's four targets and six operations, drivable from a terminal. The same
//! architectural test `vcw session` applies to recording applies here to
//! playback: if a side could only be auditioned from the UI, the transport
//! would have leaked into the shell.
//!
//! # Two modes, and the second one is the interesting one
//!
//! Without `--render` this opens an output device and plays. With `--render` it
//! writes the bytes the converter would have been handed to a file instead, and
//! needs no sound card at all - which is what makes the gapless-seek claim
//! something CI can check on a machine with no audio hardware. See
//! [`vcw_core::playback::render`].
//!
//! # How it reads a script
//!
//! `--script "play,sleep 2,seek 30,sleep 2,stop"`, one verb per comma, exactly
//! as `vcw session` reads a capture. `sleep` is the driver's own verb, and the
//! playhead is published while it waits rather than after it.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use vcw_core::playback::{self, Audition, Cue, Player, Scope, Verb};
use vcw_core::{Bus, Event};
use vcw_project::{Project, session};
use vcw_types::{SampleRate, Span};

use crate::capture::{Format, Mode};

/// How often the playhead is published while a script is waiting.
///
/// The same 100 ms the engine uses for `recording-position`: four a second is
/// smoother than an operator can read, and it is the resolution a UI would
/// redraw a playhead at anyway.
const TICK: Duration = Duration::from_millis(100);

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to play from.
    pub(crate) project: PathBuf,
    /// Capture to play. `None` is the most recent one.
    pub(crate) capture: Option<i64>,
    /// Where to start, in seconds.
    pub(crate) start: Option<f64>,
    /// Where to end, in seconds.
    pub(crate) end: Option<f64>,
    /// Audition the boundary at this many seconds, with context either side.
    pub(crate) boundary: Option<f64>,
    /// Label the region as a track. WP-13 will make this the only argument it
    /// needs.
    pub(crate) track: Option<u32>,
    /// Output device id. `None` is the system default.
    pub(crate) device: Option<String>,
    /// A stream format to insist on. `None` lets playback pick the one that
    /// converts least.
    pub(crate) format: Option<Format>,
    /// How to open the device.
    pub(crate) mode: Mode,
    /// A whole audition on one line, verbs separated by commas.
    pub(crate) script: Option<String>,
    /// Write the audio to this file instead of to a device.
    pub(crate) render: Option<PathBuf>,
    /// Machine-readable output.
    pub(crate) json: bool,
}

impl Args {
    /// Resolves the capture id and the scope against the project.
    fn audition(&self) -> Result<(Audition, SampleRate, u64)> {
        let project = Project::open_read_only(&self.project)?;
        let capture_id = match self.capture {
            Some(id) => id,
            None => latest(project.conn())?,
        };
        let layout = vcw_project::Layout::of(project.conn(), capture_id)?;
        let rate = layout.rate;

        let scope = match (self.boundary, self.track, self.start, self.end) {
            (Some(at), _, _, _) => Scope::boundary(rate, frames(rate, at)),
            (None, Some(number), start, end) => Scope::Track {
                number,
                span: region(rate, start, end, layout.frames),
            },
            (None, None, None, None) => Scope::Whole,
            (None, None, start, end) => Scope::Region(region(rate, start, end, layout.frames)),
        };

        // Track edges, so `skip` in a `--script` moves between tracks rather
        // than by a fixed ten seconds. Read from the same open as the layout,
        // and empty on a side nothing has been analyzed from - which is the
        // case the fixed step exists for.
        let marks = vcw_project::track::edges_of_capture(project.conn(), capture_id)?;

        let mut audition = Audition::new(&self.project, capture_id)
            .scope(scope)
            .marks(marks);
        audition.device = self.device.clone();
        audition.format = self.format.map(Into::into);
        audition.mode = self.mode.into();
        Ok((audition, rate, layout.frames))
    }
}

/// Seconds to frames.
fn frames(rate: SampleRate, seconds: f64) -> u64 {
    vcw_types::span::frames_at(rate, seconds)
}

/// A region from whichever ends were given.
fn region(rate: SampleRate, start: Option<f64>, end: Option<f64>, total: u64) -> Span {
    Span::new(
        start.map_or(0, |s| frames(rate, s)),
        end.map_or(total, |e| frames(rate, e)),
    )
}

/// The most recent capture in a project.
fn latest(conn: &vcw_project::Connection) -> Result<i64> {
    let records = session::all(conn)?;
    records
        .iter()
        .map(|record| record.id)
        .max()
        .ok_or_else(|| anyhow::anyhow!("this project has no captures"))
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    let (audition, rate, total) = args.audition()?;
    match &args.render {
        Some(path) => rendered(args, &audition, rate, path),
        None => played(args, &audition, rate, total),
    }
}

/// `--render`: the whole chain, written to a file, with no device.
fn rendered(args: &Args, audition: &Audition, rate: SampleRate, path: &PathBuf) -> Result<()> {
    let cues = cues(args, rate)?;
    let started = Instant::now();
    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let report = playback::render(audition, &cues, &mut file)?;
    file.flush()?;
    let elapsed = started.elapsed().as_secs_f64();

    let mut out = std::io::stdout().lock();
    if args.json {
        writeln!(
            out,
            "{}",
            serde_json::json!({
                "capture_id": report.capture_id,
                "path": path,
                "span": { "start": report.span.start, "end": report.span.end },
                "rate": report.rate.hz(),
                "channels": report.channels,
                "format": format!("{:?}", report.format),
                "conversion": report.conversion,
                "frames": report.frames,
                "bytes": report.bytes,
                "seconds": report.seconds(),
                "underruns": report.health.underruns,
                "stale_chunks": report.health.stale_chunks,
                "elapsed": elapsed,
                "applied": report.applied.iter().map(|move_| serde_json::json!({
                    "verb": move_.verb.as_str(),
                    "after_frames": move_.after,
                    "landed": move_.landed,
                })).collect::<Vec<_>>(),
            })
        )?;
    } else {
        writeln!(out, "rendered   capture {}", report.capture_id)?;
        writeln!(
            out,
            "audio      {} frames, {:.3} s, {} byte(s) of {:?}",
            report.frames,
            report.seconds(),
            report.bytes,
            report.format
        )?;
        writeln!(out, "samples    {}", report.conversion)?;
        for made in &report.applied {
            writeln!(
                out,
                "{:<10} after {} frame(s), landed on {}",
                made.verb.as_str(),
                made.after,
                made.landed
            )?;
        }
        writeln!(
            out,
            "gaps       {} underrun(s), {} chunk(s) discarded by a seek",
            report.health.underruns, report.health.stale_chunks
        )?;
        writeln!(out, "written    {} in {elapsed:.3} s", path.display())?;
    }
    Ok(())
}

/// The live path: an output device, a script, and the bus.
fn played(args: &Args, audition: &Audition, rate: SampleRate, total: u64) -> Result<()> {
    let bus = Bus::new();
    let events = bus.subscribe();
    let started = Instant::now();
    let json = args.json;
    let printer = std::thread::spawn(move || {
        while let Some(event) = events.next() {
            print_event(&event, started, json);
        }
    });

    // Opened before the script runs, so a device that cannot play this
    // capture's rate is refused before anything is echoed - see
    // `vcw_audio::Error::RateUnavailable`, and note that VCW does not resample.
    let player = Player::open(audition, &bus)?;
    let mut unknown: Vec<String> = Vec::new();

    let script = args.script.clone().unwrap_or_else(|| "play".to_string());
    let mut asked_to_stop = false;
    for line in script.split(',') {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        echo(line, started, json);
        let (verb, rest) = match line.split_once(char::is_whitespace) {
            Some((verb, rest)) => (verb.to_ascii_lowercase(), rest.trim()),
            None => (line.to_ascii_lowercase(), ""),
        };
        if verb == "sleep" || verb == "wait" {
            let seconds: f64 = rest
                .parse()
                .map_err(|_| anyhow::anyhow!("sleep wants a number of seconds, got {rest:?}"))?;
            wait(&player, Duration::from_secs_f64(seconds.max(0.0)));
            continue;
        }
        match Verb::parse(line) {
            Some(Verb::Stop) => {
                asked_to_stop = true;
                break;
            }
            Some(verb) => {
                player.apply(verb)?;
            }
            None => unknown.push(verb),
        }
    }

    // A script that says `play` and nothing else means "play it", and one that
    // says `stop` has already said when to finish. Bounded by
    // the span's own length plus a margin, so a device that stops calling back
    // ends the verb rather than hanging it.
    if !asked_to_stop && !player.has_ended() && player.is_playing() {
        let margin = Duration::from_secs_f64(2.0 + total as f64 / f64::from(rate.hz().max(1)));
        let until = Instant::now() + margin;
        while !player.has_ended() && Instant::now() < until {
            wait(&player, TICK);
        }
    }

    let played = player.stop();
    drop(bus);
    printer.join().ok();

    let mut out = std::io::stdout().lock();
    if !json {
        writeln!(out, "fidelity   {}", played.fidelity.summary())?;
    }
    out.flush()?;

    if !unknown.is_empty() {
        bail!("not a playback verb: {}", unknown.join(", "));
    }
    if !played.was_gapless() {
        bail!(
            "playback had {} gap(s) the listener heard",
            played.health.underruns
        );
    }
    Ok(())
}

/// Waits, publishing the playhead as it goes.
///
/// A plain `sleep` would leave the transcript silent for the length of the
/// wait, which is exactly the part of an audition worth watching.
fn wait(player: &Player, how_long: Duration) {
    let until = Instant::now() + how_long;
    loop {
        player.tick();
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(TICK));
    }
}

/// Turns `--script` into render cues, timed by output position.
///
/// A render has no clock, so `sleep 2` becomes "two seconds of audio from
/// here" - which is what it means to a listener, and repeatable besides.
fn cues(args: &Args, rate: SampleRate) -> Result<Vec<Cue>> {
    let Some(script) = &args.script else {
        return Ok(Vec::new());
    };
    let mut cues = Vec::new();
    let mut at = 0u64;
    for line in script.split(',') {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (verb, rest) = match line.split_once(char::is_whitespace) {
            Some((verb, rest)) => (verb.to_ascii_lowercase(), rest.trim()),
            None => (line.to_ascii_lowercase(), ""),
        };
        if verb == "sleep" || verb == "wait" {
            let seconds: f64 = rest
                .parse()
                .map_err(|_| anyhow::anyhow!("sleep wants a number of seconds, got {rest:?}"))?;
            at = at.saturating_add(frames(rate, seconds));
            continue;
        }
        match Verb::parse(line) {
            Some(verb) => cues.push(Cue {
                after_frames: at,
                verb,
            }),
            None => bail!("not a playback verb: {verb}"),
        }
    }
    Ok(cues)
}

/// Echoes a command into the transcript.
fn echo(line: &str, started: Instant, json: bool) {
    let mut out = std::io::stdout().lock();
    if json {
        let _ = writeln!(
            out,
            "{}",
            serde_json::json!({ "t": started.elapsed().as_secs_f64(), "command": line })
        );
    } else {
        let _ = writeln!(out, "[{:7.3}] >> {line}", started.elapsed().as_secs_f64());
    }
    let _ = out.flush();
}

/// Prints one event, in the shape `vcw session` prints them.
fn print_event(event: &Event, started: Instant, json: bool) {
    let t = started.elapsed().as_secs_f64();
    let mut out = std::io::stdout().lock();
    if json {
        let mut value = crate::session::detail(event);
        value["t"] = serde_json::json!(t);
        value["event"] = serde_json::json!(event.name());
        let _ = writeln!(out, "{value}");
    } else {
        let _ = writeln!(out, "[{t:7.3}] {:<19} {event}", event.name());
    }
    let _ = out.flush();
}
