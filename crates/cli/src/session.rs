/*
 *  session.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `vcw session` verb: drive a whole capture from the command line.
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

//! The `vcw session` verb: drive a whole capture from the command line.
//!
//! WP-07's exit criterion has two halves. The first is that invalid transitions
//! are unrepresentable, which [`vcw_core::state`] proves at compile time. The
//! second is that a *full capture session* can be driven from the CLI, and this
//! is it: arm, record, pause, resume, stop, reset, with every event §35 defines
//! printed as it happens and nothing but the core underneath.
//!
//! That is the architectural test §2 asks for. If a capture could only be made
//! from the UI, the transport would have leaked into the shell; because it can
//! be made from here, with no window, no webview and no frontend compiled at
//! all, it has not.
//!
//! # How it reads commands
//!
//! One verb per line, from `--script` or from stdin. Two of them are the
//! driver's rather than the core's:
//!
//! - `sleep <seconds>` - wait, which is how a script records for a while.
//! - `#` - a comment, so a script can explain itself.
//!
//! Everything else is a [`Command`], except `arm`, which needs a [`Setup`] and
//! so is assembled here from the verb's own arguments.
//!
//! # Why there is no prompt
//!
//! Events arrive on their own thread, whenever the engine has something to say,
//! and a prompt written to the same terminal would be scribbled over by the
//! next `recording-position`. So the session echoes each command into the
//! transcript instead - `>> record` - and the whole run reads back in order
//! afterwards, which is what a log is for.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use vcw_core::{Command, Engine, Event, Setup};

use crate::capture::{Eq, Format, Mode};

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to record into. Created if it does not exist.
    pub(crate) project: PathBuf,
    /// Device id. `None` is the simulated source, which is how the transport
    /// is driven on a machine with nothing plugged in.
    pub(crate) device: Option<String>,
    /// Sample rate to pin.
    pub(crate) rate: Option<u32>,
    /// Channel count to pin.
    pub(crate) channels: Option<u16>,
    /// Sample format to pin.
    pub(crate) format: Option<Format>,
    /// How to open the device.
    pub(crate) mode: Mode,
    /// Ring capacity in milliseconds.
    pub(crate) ring_millis: Option<u32>,
    /// What equalisation the hardware upstream already applied (§51).
    pub(crate) capture_eq: Eq,
    /// A whole session on one line, verbs separated by commas.
    pub(crate) script: Option<String>,
    /// Machine-readable output: one JSON object per event, one per line.
    pub(crate) json: bool,
    /// Print `meter-update` too. Off by default: at 50 Hz it is the only
    /// event on the bus that a person cannot read, and it would bury the
    /// transport transcript this verb exists to show.
    pub(crate) meters: bool,
}

impl Args {
    /// Builds the setup `arm` will use.
    fn setup(&self) -> Setup {
        let mut setup = match &self.device {
            Some(device) => Setup::device(device.clone(), &self.project),
            None => Setup::simulated(&self.project),
        };
        setup.rate = self.rate;
        setup.channels = self.channels;
        setup.format = self.format.map(Into::into);
        setup.mode = self.mode.into();
        setup.ring_millis = self.ring_millis;
        setup.eq = self.capture_eq.into();
        setup
    }
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    let setup = args.setup();
    let engine = Engine::start()?;
    let events = engine.events();
    let started = Instant::now();

    // The printer owns the stream and nothing else. It ends when the engine
    // says `closed`, which the engine guarantees to send, so this thread can
    // be joined rather than detached or killed.
    let json = args.json;
    let meters = args.meters;
    let printer = thread::spawn(move || {
        while let Some(event) = events.next() {
            let last = event.is_last();
            if meters || !matches!(event, Event::Meter { .. }) {
                print_event(&event, started, json);
            }
            if last {
                break;
            }
        }
    });

    // A verb this driver did not understand is a mistake worth failing over,
    // but not before the engine has been shut down properly: an unreadable
    // script is no reason to lose the side that is already recording.
    let mut unknown: Vec<String> = Vec::new();

    let outcome = match &args.script {
        Some(script) => {
            for line in script.split(',') {
                feed(&engine, &setup, line, started, json, &mut unknown)?;
            }
            Ok(())
        }
        None => {
            let stdin = std::io::stdin();
            let mut result = Ok(());
            for line in stdin.lock().lines() {
                match line {
                    Ok(line) => feed(&engine, &setup, &line, started, json, &mut unknown)?,
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
            }
            result
        }
    };

    // End of input is the end of the session. Shutdown finalises whatever is
    // still recording rather than abandoning it, so a script that forgets to
    // stop still keeps its audio.
    engine.shutdown()?;
    printer.join().ok();

    outcome?;
    if !unknown.is_empty() {
        bail!("not a session verb: {}", unknown.join(", "));
    }
    Ok(())
}

/// Turns one line of input into whatever it means.
fn feed(
    engine: &Engine,
    setup: &Setup,
    line: &str,
    started: Instant,
    json: bool,
    unknown: &mut Vec<String>,
) -> Result<()> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(());
    }

    let (verb, rest) = match line.split_once(char::is_whitespace) {
        Some((verb, rest)) => (verb, rest.trim()),
        None => (line, ""),
    };
    let verb = verb.to_ascii_lowercase();

    echo(line, started, json);

    match verb.as_str() {
        // The driver's own two verbs. `sleep` is what makes a script a
        // *session* rather than a list: recording for eight seconds is a wait,
        // not a command.
        "sleep" | "wait" => {
            let seconds: f64 = rest
                .parse()
                .map_err(|_| anyhow::anyhow!("sleep wants a number of seconds, got {rest:?}"))?;
            if seconds > 0.0 {
                thread::sleep(Duration::from_secs_f64(seconds));
            }
        }
        // `arm` is the one command that carries a description of a device, and
        // this verb's arguments are where that description comes from.
        "arm" => engine.send(Command::Arm(Box::new(setup.clone())))?,
        _ => match Command::parse(&verb) {
            Some(command) => engine.send(command)?,
            None => unknown.push(verb),
        },
    }
    Ok(())
}

/// Echoes a command into the transcript, so the run reads back in order.
fn echo(line: &str, started: Instant, json: bool) {
    let mut out = std::io::stdout().lock();
    if json {
        let _ = writeln!(
            out,
            "{}",
            serde_json::json!({
                "t": started.elapsed().as_secs_f64(),
                "command": line,
            })
        );
    } else {
        let _ = writeln!(out, "[{:7.3}] >> {line}", started.elapsed().as_secs_f64());
    }
    let _ = out.flush();
}

/// Prints one event.
fn print_event(event: &Event, started: Instant, json: bool) {
    let t = started.elapsed().as_secs_f64();
    let mut out = std::io::stdout().lock();
    if json {
        let mut value = detail(event);
        value["t"] = serde_json::json!(t);
        value["event"] = serde_json::json!(event.name());
        let _ = writeln!(out, "{value}");
    } else {
        let _ = writeln!(out, "[{t:7.3}] {:<19} {event}", event.name());
    }
    let _ = out.flush();
}

/// One event's fields, for a consumer that would rather not parse prose.
///
/// Shared with `vcw play`, which prints the same bus.
pub(crate) fn detail(event: &Event) -> serde_json::Value {
    match event {
        Event::Phase { from, to } => serde_json::json!({
            "from": from.as_str(), "to": to.as_str(),
        }),
        Event::Armed {
            project,
            negotiated,
            divergences,
            verified,
        } => serde_json::json!({
            "project": project,
            "negotiated": negotiated,
            "divergences": divergences,
            "verified": verified,
        }),
        Event::Position { frames, seconds } => serde_json::json!({
            "frames": frames, "seconds": seconds,
        }),
        Event::Meter { levels } => serde_json::json!({
            "frames": levels.frames,
            "channels": levels
                .channels
                .iter()
                .map(|c| serde_json::json!({
                    "peak": c.peak,
                    "peak_db": c.peak_db(),
                    "rms": c.rms,
                    "rms_db": c.rms_db(),
                    "hold_db": c.hold_db(),
                    "clipped": c.clipped,
                    "clipped_samples": c.clipped_samples,
                }))
                .collect::<Vec<_>>(),
        }),
        Event::Detected {
            frame,
            seconds,
            edge,
            confidence,
            provenance,
        } => serde_json::json!({
            "frame": frame,
            "seconds": seconds,
            "edge": edge.as_str(),
            "confidence": confidence,
            "provenance": provenance.as_str(),
        }),
        Event::Warning { code, detail } => serde_json::json!({
            "code": code, "detail": detail,
        }),
        Event::Finished {
            capture_id,
            frames,
            state,
            diagnostics,
            bit_perfect,
        } => serde_json::json!({
            "capture_id": capture_id,
            "frames": frames,
            "state": state.as_str(),
            "diagnostics": {
                "overruns": diagnostics.overruns,
                "underruns": diagnostics.underruns,
                "dropped_frames": diagnostics.dropped_frames,
                "stream_errors": diagnostics.stream_errors,
            },
            "bit_perfect": bit_perfect,
        }),
        Event::Refused {
            command,
            phase,
            reason,
        } => serde_json::json!({
            "command": command, "phase": phase.as_str(), "reason": reason,
        }),
        Event::Rejected { command, phase } => serde_json::json!({
            "command": command, "phase": phase.as_str(),
        }),
        Event::Status { phase, frames } => serde_json::json!({
            "phase": phase.as_str(), "frames": frames,
        }),
        Event::Auditioning {
            capture_id,
            scope,
            opened,
            conversion,
            divergences,
        } => serde_json::json!({
            "capture_id": capture_id,
            "scope": scope,
            "opened": opened,
            "conversion": conversion,
            "divergences": divergences,
        }),
        Event::Playhead { frame, seconds } => serde_json::json!({
            "frame": frame, "seconds": seconds,
        }),
        Event::Denied {
            capture_id,
            scope,
            reason,
        } => serde_json::json!({
            "capture_id": capture_id, "scope": scope, "reason": reason,
        }),
        Event::Ended {
            capture_id,
            frames,
            underruns,
            fidelity,
            bit_perfect,
        } => serde_json::json!({
            "capture_id": capture_id,
            "frames": frames,
            "underruns": underruns,
            "fidelity": fidelity,
            "bit_perfect": bit_perfect,
        }),
        Event::Closed => serde_json::json!({}),
        // `Event` is `#[non_exhaustive]`, and from outside the crate that
        // makes this arm compulsory rather than optional. A variant a later
        // work package adds still gets its name, its time and its prose - the
        // fields are what it loses, which is the right way round for a
        // consumer that has not been taught about it yet.
        other => serde_json::json!({ "detail": other.to_string() }),
    }
}
