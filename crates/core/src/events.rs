/*
 *  events.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The event surface the core reports through (§35).
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

//! The event surface the core reports through (§35).
//!
//! The other half of §35. Every consumer - the CLI, the Tauri shell, a log -
//! subscribes to the same stream, so none of them can hold a different idea of
//! what the transport is doing. A command's result is an event and not a return
//! value for exactly that reason: a returned value is known only to whoever
//! asked.
//!
//! # Not coalesced on the way out
//!
//! S3 measured this rather than guessed it: meter and waveform frames at 750 Hz
//! crossed the Tauri boundary with 12.5x headroom and zero loss, and the real
//! constraint turned out to be main-thread *rendering*, not the boundary. So
//! events are sent as they happen and coalescing is the consumer's business -
//! the webview reads the latest state once per paint. Coalescing here would
//! throw away ordering that costs nothing to keep.
//!
//! # High-frequency PCM never appears here
//!
//! §35 is explicit, and the rule shapes the enum: [`Event::Position`] carries a
//! frame count, not frames. Meter and waveform events (WP-08, WP-09) will carry
//! *summaries* computed Rust-side. Nothing in this module will ever carry
//! samples.
//!
//! # What is here and what is not
//!
//! §35 names `meter-update`, `waveform-update`, `recording-position`,
//! `track-detected`, `fingerprint-match`, `capture-warning` and
//! `export-progress`. WP-07 owns the two that describe the transport -
//! `recording-position` and `capture-warning` - plus the phase changes and
//! command outcomes that §35's examples imply but do not name. The rest arrive
//! with the work packages that generate them, and [`Event`] is
//! `#[non_exhaustive]` so they can - except `waveform-update`, which will not
//! arrive at all: the pyramid is read on demand in a millisecond and a push
//! cannot make the picture newer than the commit that made it drawable. The
//! measurement is in `vcw-contract`'s `event` module documentation.
//! `fingerprint-match` is Phase 2's.

use std::fmt;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use vcw_signal::meter::Snapshot;
use vcw_types::{CaptureState, Diagnostics, Edge, Provenance};

use crate::state::Phase;

/// Something the core is reporting (§35).
///
/// `PartialEq` but not `Eq`, because [`Event::Position`] carries a `f64`. That
/// is the right shape for the field - a consumer that would only divide frames
/// by the rate should not have to - and comparing two positions for exact
/// equality was never going to be meaningful anyway.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// The transport moved between phases.
    Phase {
        /// Where it was.
        from: Phase,
        /// Where it is now.
        to: Phase,
    },
    /// A device and a project were opened, and this is what was negotiated.
    ///
    /// Carries the divergences rather than only the result, because §9's whole
    /// point is that a capture which quietly differs from the one asked for is
    /// worse than an error.
    Armed {
        /// The project being recorded into.
        project: String,
        /// How the device is actually running, as a line of text.
        negotiated: String,
        /// Every field that was asked for and not granted.
        divergences: Vec<String>,
        /// Whether the operating system confirmed the format independently.
        verified: bool,
    },
    /// How far the recording has got. §35's `recording-position`.
    ///
    /// Frames, not samples, and never the samples themselves.
    Position {
        /// Frames committed, per channel.
        frames: u64,
        /// The same thing in seconds, for a consumer that would only divide.
        seconds: f64,
    },
    /// Where the levels are now. §35's `meter-update`.
    ///
    /// Published from the moment the transport is armed, not from the moment
    /// it records, because §50's "set the level" step happens before the
    /// needle goes down and there is nothing to set it against otherwise.
    ///
    /// Sent at §17's refresh rate, which is faster than anything else on this
    /// bus by an order of magnitude. A consumer that does not draw meters
    /// should ignore it by name rather than formatting it.
    Meter {
        /// Peak, RMS, peak-hold and the clip latch, per channel.
        levels: Snapshot,
    },
    /// A track boundary was detected. §35's `track-detected`.
    ///
    /// One event per boundary, not one per track, because that is what §24
    /// describes and what analysis actually produces: an edge with a case
    /// attached. Two of them make a track, and deciding which two is the
    /// resolver's job rather than this bus's.
    ///
    /// Published by the live pass while the record turns, so a boundary
    /// announced here is provisional in the sense §22 means - and it is only
    /// announced once it cannot move, so a UI can draw the marker and leave it
    /// there. The refine pass after the capture is where the same boundary may
    /// be superseded, which is visible in the provenance and the confidence
    /// rather than implied by the order events arrived in.
    Detected {
        /// Where the boundary is, in frames.
        frame: u64,
        /// The same thing in seconds.
        seconds: f64,
        /// Whether a track starts or ends here.
        edge: Edge,
        /// How much to trust it, in 0..=1.
        confidence: f32,
        /// Which analysis said so.
        provenance: Provenance,
    },
    /// Something went wrong that did not stop the capture. §35's
    /// `capture-warning`.
    Warning {
        /// A short stable slug, for a consumer that wants to branch.
        code: &'static str,
        /// A sentence for a consumer that wants to show it.
        detail: String,
    },
    /// The capture was finalised.
    Finished {
        /// The capture's row id in the project.
        capture_id: i64,
        /// Frames committed, per channel.
        frames: u64,
        /// How it was recorded: finalised, or interrupted if anything was lost.
        state: CaptureState,
        /// The ring's four counters as of the end.
        diagnostics: Diagnostics,
        /// Whether the capture can honestly be called bit-perfect.
        bit_perfect: bool,
    },
    /// Playback opened on a device, and this is what it is playing through.
    ///
    /// The playback analogue of [`Event::Armed`], and it carries the same
    /// awkward truth: §21 wants bit-perfect playback where supported, so a
    /// consumer needs to know whether the samples reaching the converter are
    /// the samples in the project or a conversion of them.
    Auditioning {
        /// The capture being played.
        capture_id: i64,
        /// What is being auditioned: the whole side, a region, a track or a
        /// boundary, with its extent in seconds.
        scope: String,
        /// How the output stream is actually running, as a line of text.
        opened: String,
        /// What has to happen to the stored samples on the way out, or
        /// `"straight through"` when the answer is nothing.
        conversion: String,
        /// Every field that was asked for and not granted.
        divergences: Vec<String>,
    },
    /// Where the playhead is. The playback counterpart of
    /// [`Event::Position`].
    ///
    /// The frame being fed to the converter, not an estimate from a buffer
    /// depth: the chunk the callback is reading carries the frame it starts
    /// at, so this is exact to within one period.
    Playhead {
        /// The frame now playing, absolute within the capture.
        frame: u64,
        /// The same thing in seconds.
        seconds: f64,
    },
    /// Playback would not open, so there is no audition and there never was.
    ///
    /// The playback counterpart of [`Event::Refused`], and a variant of its own
    /// rather than a [`Event::Warning`] because a refusal is *terminal*: no
    /// [`Event::Auditioning`] came before it and no [`Event::Ended`] will
    /// follow it, so a consumer that put a transport into a playing state when
    /// it asked has nothing else coming to take it out again.
    ///
    /// A device that cannot play the capture's rate is refused rather than
    /// resampled (§9, §21), and the open happens on the thread that will own
    /// the stream, so by the time it fails the command that asked has already
    /// returned successfully. This is the answer.
    Denied {
        /// The capture that was asked for.
        capture_id: i64,
        /// What was asked for, in frames: the project could not be read in the
        /// case that matters, so there is no rate to express it in seconds.
        scope: String,
        /// Why it was refused.
        reason: String,
    },
    /// Playback stopped, and this is how it went.
    ///
    /// An underrun here is a gap the listener heard. It cannot be repaired
    /// afterwards the way a capture's can be re-run, so it is reported
    /// plainly rather than folded into a health score.
    Ended {
        /// The capture that was playing.
        capture_id: i64,
        /// Frames delivered to the device.
        frames: u64,
        /// Gaps the listener heard.
        underruns: u64,
        /// Whether the audio reaching the converter can honestly be called
        /// the audio in the project.
        fidelity: String,
        /// Whether that answer was a confirmation rather than an absence of
        /// evidence.
        bit_perfect: bool,
    },
    /// A command was legal here and the deck refused it.
    ///
    /// The transport did not move. Kept apart from [`Event::Rejected`] because
    /// this is a fault and that is a mistake, and an operator needs to know
    /// which they are looking at.
    Refused {
        /// The command that was refused.
        command: &'static str,
        /// The phase the transport is still in.
        phase: Phase,
        /// Why.
        reason: String,
    },
    /// A command has no meaning in the phase the transport is in.
    ///
    /// Nothing was attempted, because there was nothing to attempt: §11's
    /// illegal transitions have no implementation to reach. Reported rather
    /// than ignored so a UI can grey the button out next time.
    Rejected {
        /// The command that does not apply.
        command: &'static str,
        /// The phase it does not apply in.
        phase: Phase,
    },
    /// The answer to [`Command::Poll`](crate::commands::Command::Poll).
    Status {
        /// Where the transport is.
        phase: Phase,
        /// Frames committed, per channel.
        frames: u64,
    },
    /// The engine has stopped and will send nothing further.
    ///
    /// Always the last event, and always sent - including when the engine is
    /// shutting down because something failed. A consumer that blocks on the
    /// stream needs a guaranteed terminator or it waits for ever.
    Closed,
}

impl Event {
    /// The kebab-case name §35 uses for the event.
    ///
    /// Stable, because it is what a UI subscribes to and what a log is grepped
    /// for. The variant names can be refactored; these cannot.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Phase { .. } => "phase-change",
            Self::Armed { .. } => "armed",
            Self::Position { .. } => "recording-position",
            Self::Meter { .. } => "meter-update",
            Self::Detected { .. } => "track-detected",
            Self::Warning { .. } => "capture-warning",
            Self::Finished { .. } => "capture-finished",
            Self::Refused { .. } => "command-refused",
            Self::Rejected { .. } => "command-rejected",
            Self::Auditioning { .. } => "auditioning",
            Self::Playhead { .. } => "playback-position",
            Self::Denied { .. } => "playback-refused",
            Self::Ended { .. } => "playback-finished",
            Self::Status { .. } => "status",
            Self::Closed => "closed",
        }
    }

    /// Whether this is the last event the stream will carry.
    #[must_use]
    pub const fn is_last(&self) -> bool {
        matches!(self, Self::Closed)
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Phase { from, to } => write!(f, "{from} -> {to}"),
            Self::Armed {
                project,
                negotiated,
                divergences,
                verified,
            } => {
                write!(f, "armed on {negotiated} into {project}")?;
                if *verified {
                    write!(f, ", os-confirmed")?;
                }
                for divergence in divergences {
                    write!(f, "; {divergence}")?;
                }
                Ok(())
            }
            Self::Position { frames, seconds } => write!(f, "{frames} frames, {seconds:.3} s"),
            Self::Meter { levels } => write!(f, "{levels}"),
            Self::Detected {
                seconds,
                edge,
                confidence,
                provenance,
                ..
            } => write!(
                f,
                "{} at {seconds:.3} s, confidence {confidence:.2}, from {}",
                edge.as_str(),
                provenance.as_str()
            ),
            Self::Warning { code, detail } => write!(f, "{code}: {detail}"),
            Self::Finished {
                capture_id,
                frames,
                state,
                diagnostics,
                bit_perfect,
            } => write!(
                f,
                "capture {capture_id} {}: {frames} frames, {} overrun(s), \
                 {} underrun(s), {} dropped, {} error(s), bit-perfect {}",
                state.as_str(),
                diagnostics.overruns,
                diagnostics.underruns,
                diagnostics.dropped_frames,
                diagnostics.stream_errors,
                if *bit_perfect { "yes" } else { "no" }
            ),
            Self::Auditioning {
                capture_id,
                scope,
                opened,
                conversion,
                divergences,
            } => {
                write!(
                    f,
                    "auditioning {scope} of capture {capture_id} on {opened}, {conversion}"
                )?;
                for divergence in divergences {
                    write!(f, "; {divergence}")?;
                }
                Ok(())
            }
            Self::Playhead { frame, seconds } => write!(f, "{frame} ({seconds:.3} s)"),
            Self::Denied {
                capture_id,
                scope,
                reason,
            } => write!(
                f,
                "refused to play {scope} of capture {capture_id}: {reason}"
            ),
            Self::Ended {
                capture_id,
                frames,
                underruns,
                fidelity,
                bit_perfect,
            } => write!(
                f,
                "capture {capture_id} played {frames} frames with {underruns} gap(s): \
                 {fidelity}, bit-perfect {}",
                if *bit_perfect { "yes" } else { "no" }
            ),
            Self::Refused {
                command,
                phase,
                reason,
            } => write!(f, "{command} refused while {phase}: {reason}"),
            Self::Rejected { command, phase } => {
                write!(f, "{command} does not apply while {phase}")
            }
            Self::Status { phase, frames } => write!(f, "{phase}, {frames} frames"),
            Self::Closed => f.write_str("closed"),
        }
    }
}

/// A fan-out event bus: every subscriber sees every event, in order.
///
/// Deliberately not a dependency. `std::sync::mpsc` gives one receiver per
/// channel, so the bus holds a sender per subscriber and pushes to each; a
/// subscriber that has been dropped is pruned on the next send. That is the
/// whole implementation, and it is enough because S3 measured the traffic this
/// has to carry and the boundary was not the constraint.
///
/// Sending is not allowed to fail the engine. A subscriber that has gone away
/// is normal - a webview reloaded, a CLI piped into `head` - and must never
/// take a recording down with it, which is §36's isolation applied to the one
/// place every worker touches.
#[derive(Clone, Default)]
pub struct Bus {
    subscribers: Arc<Mutex<Vec<Sender<Event>>>>,
}

impl fmt::Debug for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.subscribers.lock().map_or(0, |s| s.len());
        f.debug_struct("Bus").field("subscribers", &count).finish()
    }
}

impl Bus {
    /// A bus with no subscribers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a subscriber and hands back its end of the stream.
    #[must_use]
    pub fn subscribe(&self) -> Events {
        let (tx, rx) = channel();
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.push(tx);
        }
        Events { rx }
    }

    /// Sends to every subscriber, dropping the ones that have gone.
    ///
    /// Returns how many received it, which the tests use and nothing else
    /// needs: the engine does not care and must not.
    pub fn publish(&self, event: &Event) -> usize {
        let Ok(mut subscribers) = self.subscribers.lock() else {
            // A poisoned bus means a subscriber panicked while holding the
            // lock. Losing events is bad; taking the capture down with it is
            // worse, and this is the one place that trade-off is made.
            return 0;
        };
        subscribers.retain(|tx| tx.send(event.clone()).is_ok());
        subscribers.len()
    }

    /// How many subscribers are currently attached.
    #[must_use]
    pub fn subscribers(&self) -> usize {
        self.subscribers.lock().map_or(0, |s| s.len())
    }
}

/// One subscriber's view of the event stream.
pub struct Events {
    rx: Receiver<Event>,
}

impl fmt::Debug for Events {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Events")
    }
}

impl Events {
    /// Waits for the next event.
    ///
    /// `None` once the engine has gone and the stream is drained.
    #[must_use]
    pub fn next(&self) -> Option<Event> {
        self.rx.recv().ok()
    }

    /// Takes the next event if one is ready, without waiting.
    #[must_use]
    pub fn try_next(&self) -> Option<Event> {
        // Empty and Disconnected both give `None`: "nothing for you" and
        // "nothing ever again" look the same to a caller that is polling, and
        // the one that wants to tell them apart uses `collect_until_closed`,
        // where `Closed` is the terminator rather than the channel's state.
        self.rx.try_recv().ok()
    }

    /// Everything waiting right now.
    #[must_use]
    pub fn drain(&self) -> Vec<Event> {
        std::iter::from_fn(|| self.try_next()).collect()
    }

    /// Waits until [`Event::Closed`], collecting everything on the way.
    ///
    /// The honest way to read a finished session: the terminator is guaranteed,
    /// so this cannot hang on a healthy engine and cannot truncate on a busy
    /// one.
    #[must_use]
    pub fn collect_until_closed(&self) -> Vec<Event> {
        let mut seen = Vec::new();
        while let Some(event) = self.next() {
            let last = event.is_last();
            seen.push(event);
            if last {
                break;
            }
        }
        seen
    }
}

impl Iterator for Events {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        Self::next(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_subscriber_sees_every_event_in_order() {
        let bus = Bus::new();
        let first = bus.subscribe();
        let second = bus.subscribe();

        bus.publish(&Event::Phase {
            from: Phase::Idle,
            to: Phase::Armed,
        });
        bus.publish(&Event::Position {
            frames: 48_000,
            seconds: 1.0,
        });
        bus.publish(&Event::Closed);

        for events in [&first, &second] {
            let seen = events.collect_until_closed();
            assert_eq!(seen.len(), 3);
            assert_eq!(seen[0].name(), "phase-change");
            assert_eq!(seen[1].name(), "recording-position");
            assert!(seen[2].is_last());
        }
    }

    #[test]
    fn a_subscriber_that_goes_away_is_pruned_and_takes_nothing_with_it() {
        let bus = Bus::new();
        let keeper = bus.subscribe();
        {
            let _leaver = bus.subscribe();
            assert_eq!(bus.subscribers(), 2);
        }
        // The dropped receiver is only noticed on the next send, which is the
        // point: the engine never blocks to find out who is still listening.
        assert_eq!(bus.publish(&Event::Closed), 1);
        assert_eq!(bus.subscribers(), 1);
        assert!(keeper.next().is_some_and(|e| e.is_last()));
    }

    #[test]
    fn publishing_with_nobody_listening_is_not_an_error() {
        let bus = Bus::new();
        assert_eq!(bus.publish(&Event::Closed), 0);
    }

    #[test]
    fn the_names_are_the_ones_section_35_uses() {
        // These strings are the contract. Renaming a variant is refactoring;
        // renaming one of these breaks every subscriber that ever shipped.
        assert_eq!(
            Event::Position {
                frames: 0,
                seconds: 0.0
            }
            .name(),
            "recording-position"
        );
        assert_eq!(
            Event::Warning {
                code: "x",
                detail: String::new()
            }
            .name(),
            "capture-warning"
        );
    }

    #[test]
    fn an_event_says_something_useful_when_printed() {
        let armed = Event::Armed {
            project: "side-a.vcw".to_owned(),
            negotiated: "192000 Hz, 2 ch, S32".to_owned(),
            divergences: vec!["asked for 96000 Hz, got 192000 Hz".to_owned()],
            verified: true,
        };
        let line = armed.to_string();
        assert!(line.contains("side-a.vcw"), "{line}");
        assert!(line.contains("os-confirmed"), "{line}");
        assert!(line.contains("asked for 96000 Hz"), "{line}");
    }
}
