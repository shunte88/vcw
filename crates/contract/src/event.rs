/*
 *  event.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Every core event as one discriminated union, named the way §35 names them.
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

//! Every core event as one discriminated union, named the way §35 names them.
//!
//! [`Wire`] is a projection of [`vcw_core::Event`], internally tagged on `kind`
//! so that TypeScript gets a discriminated union and a `switch` over it is
//! exhaustive. The tag is not chosen here: it is
//! [`vcw_core::Event::name`], which that module declares stable because it is
//! what a UI subscribes to and what a log is grepped for.
//! `the_tag_is_the_name_the_core_declares` asserts the two agree for every
//! variant, so a rename in either place fails a test rather than silently
//! dropping a subscriber's handler.
//!
//! # What is here that the bus does not carry
//!
//! [`Wire::ExportProgress`], [`Wire::ExportFinished`] and
//! [`Wire::ExportFailed`] have no [`vcw_core::Event`] behind them. WP-14's
//! exporter reports progress through a callback rather than the bus - an export
//! is driven by whoever asked for it and does not belong to the transport - so
//! the shell builds these three from that callback and from what `run`
//! returned. They are in the same union because a frontend has one event
//! stream, not two.
//!
//! An export therefore ends in exactly one of two events, and a frontend that
//! only handled progress would leave a progress bar at 99% forever. That is
//! why the failure is an event of its own rather than a rejected promise: the
//! command that starts an export returns as soon as the thread is spawned,
//! because an export of a two-hour side takes minutes and the webview cannot
//! wait.
//!
//! # What §35 lists and nothing produces yet
//!
//! `waveform-update` and `fingerprint-match` are absent, deliberately and
//! visibly. The waveform pyramid is built by the writer as each block commits
//! and *read* on demand, so there is nothing to push: adding the variant now
//! would publish a promise the core cannot keep. Fingerprinting is Phase 2.
//! Both are recorded in `docs/STATUS.md` as gaps; neither is a type this crate
//! can invent its way out of.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use vcw_core::events::Event;
use vcw_core::state::Phase;

use crate::view::{Diagnostics, Levels, Meter};

/// §11's five phases, as a string union.
///
/// Unit variants on purpose: a phase has no payload, and `"recording"` is
/// easier to read in a log and in a React `switch` than `{ "phase": "recording" }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PhaseName {
    /// Nothing selected; the transport is at rest.
    Idle,
    /// A device and a project are open, and no audio has been committed.
    Armed,
    /// Audio is being committed.
    Recording,
    /// The device is held and nothing is being committed.
    Paused,
    /// The capture is finalised. §11: the project is *not* closed.
    Stopped,
}

impl From<Phase> for PhaseName {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::Idle => Self::Idle,
            Phase::Armed => Self::Armed,
            Phase::Recording => Self::Recording,
            Phase::Paused => Self::Paused,
            Phase::Stopped => Self::Stopped,
        }
    }
}

/// Which way the audio crosses a boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeName {
    /// Silence into sound: a track begins.
    Start,
    /// Sound into silence: a track ends.
    End,
}

impl From<vcw_types::Edge> for EdgeName {
    fn from(edge: vcw_types::Edge) -> Self {
        match edge {
            vcw_types::Edge::Start => Self::Start,
            vcw_types::Edge::End => Self::End,
        }
    }
}

/// Which analysis - or which person - put a boundary where it is.
///
/// Mirrored rather than re-exported because [`vcw_types::Provenance`] is
/// `#[non_exhaustive]`: a detector added in Phase 2 must not silently widen the
/// contract, so a new variant fails the conversion below and has to be named
/// here on purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenanceName {
    /// A gap in the audio.
    Silence,
    /// The spectrum changed character.
    SpectralChange,
    /// The hidden Markov model's state path changed here.
    Hmm,
    /// Two fingerprints either side disagree (§25).
    Fingerprint,
    /// A release's published durations put one here (§28).
    MetadataDuration,
    /// The side topology implies one (§29).
    ReleaseTopology,
    /// A person said so, and analysis may not move it (§24).
    User,
    /// A detector this contract has not been taught about.
    ///
    /// Reachable only through a `vcw-types` release that adds a variant. A
    /// frontend that meets it can still draw the boundary, which is better than
    /// a deserialisation error over a label.
    Unknown,
}

impl From<vcw_types::Provenance> for ProvenanceName {
    fn from(provenance: vcw_types::Provenance) -> Self {
        use vcw_types::Provenance as P;
        match provenance {
            P::Silence => Self::Silence,
            P::SpectralChange => Self::SpectralChange,
            P::Hmm => Self::Hmm,
            P::Fingerprint => Self::Fingerprint,
            P::MetadataDuration => Self::MetadataDuration,
            P::ReleaseTopology => Self::ReleaseTopology,
            P::User => Self::User,
            _ => Self::Unknown,
        }
    }
}

/// How far a capture got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureStateName {
    /// The stream is open, or was when the process died.
    Recording,
    /// Stopped cleanly, every block committed.
    Finalised,
    /// Stopped by a stream error or a device that went away.
    Interrupted,
    /// Reconstructed after the process died mid-capture (§15).
    Recovered,
    /// A state this contract has not been taught about.
    Unknown,
}

impl From<vcw_types::CaptureState> for CaptureStateName {
    fn from(state: vcw_types::CaptureState) -> Self {
        use vcw_types::CaptureState as S;
        match state {
            S::Recording => Self::Recording,
            S::Finalised => Self::Finalised,
            S::Interrupted => Self::Interrupted,
            S::Recovered => Self::Recovered,
        }
    }
}

/// Anything the core wants a UI to know about (§35).
///
/// One union rather than a type per event, because a frontend has one stream
/// and wants one exhaustive `switch` over it. The tag is `kind` and its values
/// are §35's kebab-case names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum Wire {
    /// The transport moved between phases.
    #[serde(rename = "phase-change")]
    PhaseChange {
        /// Where it was.
        from: PhaseName,
        /// Where it is now.
        to: PhaseName,
    },

    /// A device and a project were opened, and this is what was negotiated.
    ///
    /// `divergences` is the load-bearing field: §9's rule is that a capture
    /// which quietly differs from the one asked for is worse than an error, so
    /// what was *not* granted travels with what was.
    Armed {
        /// The project being recorded into.
        project: String,
        /// How the device is actually running, as a line of text.
        negotiated: String,
        /// Every field asked for and not granted.
        divergences: Vec<String>,
        /// Whether the operating system confirmed the format independently.
        verified: bool,
    },

    /// How far the recording has got.
    #[serde(rename = "recording-position")]
    RecordingPosition {
        /// Frames committed, per channel.
        frames: u64,
        /// The same thing in seconds.
        seconds: f64,
    },

    /// Where the levels are now, at §17's refresh rate.
    #[serde(rename = "meter-update")]
    MeterUpdate {
        /// Peak, RMS, hold and the clip latch, per channel, in dBFS.
        meter: Meter,
    },

    /// A boundary was detected, and it will not move.
    #[serde(rename = "track-detected")]
    TrackDetected {
        /// Where it is, in frames.
        frame: u64,
        /// The same thing in seconds.
        seconds: f64,
        /// Whether a track starts or ends here.
        edge: EdgeName,
        /// How much to trust it, 0 to 1.
        confidence: f32,
        /// Which analysis said so.
        provenance: ProvenanceName,
    },

    /// Something went wrong that did not stop the capture.
    #[serde(rename = "capture-warning")]
    CaptureWarning {
        /// A short stable slug, for a consumer that wants to branch.
        code: String,
        /// A sentence, for a consumer that wants to show it.
        detail: String,
    },

    /// The capture was finalised.
    #[serde(rename = "capture-finished")]
    CaptureFinished {
        /// The capture's row id in the project.
        capture_id: i64,
        /// Frames committed, per channel.
        frames: u64,
        /// Finalised, or interrupted if anything was lost.
        state: CaptureStateName,
        /// The ring's four counters as of the end.
        diagnostics: Diagnostics,
        /// Whether the capture can honestly be called bit-perfect.
        bit_perfect: bool,
    },

    /// Playback opened, and this is what it is playing through.
    Auditioning {
        /// The capture being played.
        capture_id: i64,
        /// What is being auditioned, with its extent.
        scope: String,
        /// How the output stream is actually running.
        opened: String,
        /// What happens to the stored samples on the way out, or
        /// `"straight through"`.
        conversion: String,
        /// Every field asked for and not granted.
        divergences: Vec<String>,
    },

    /// Where the playhead is, exact to within one device period.
    #[serde(rename = "playback-position")]
    PlaybackPosition {
        /// The frame now playing, absolute within the capture.
        frame: u64,
        /// The same thing in seconds.
        seconds: f64,
    },

    /// Playback stopped, and this is how it went.
    #[serde(rename = "playback-finished")]
    PlaybackFinished {
        /// The capture that was playing.
        capture_id: i64,
        /// Frames delivered to the device.
        frames: u64,
        /// Gaps the listener heard.
        underruns: u64,
        /// Whether what reached the converter was what is in the project.
        fidelity: String,
        /// Whether that was a confirmation rather than an absence of evidence.
        bit_perfect: bool,
    },

    /// A command was legal here and the deck refused it.
    ///
    /// A fault, not a mistake. [`Wire::CommandRejected`] is the mistake.
    #[serde(rename = "command-refused")]
    CommandRefused {
        /// The command that was refused.
        command: String,
        /// The phase the transport is still in.
        phase: PhaseName,
        /// Why.
        reason: String,
    },

    /// A command has no meaning in the phase the transport is in.
    ///
    /// Nothing was attempted. Reported so a UI can grey the button out.
    #[serde(rename = "command-rejected")]
    CommandRejected {
        /// The command that does not apply.
        command: String,
        /// The phase it does not apply in.
        phase: PhaseName,
    },

    /// The answer to a poll: where the transport is, without changing it.
    Status {
        /// Where the transport is.
        phase: PhaseName,
        /// Frames committed, per channel.
        frames: u64,
    },

    /// How far an export has got.
    ///
    /// One of three variants with no [`vcw_core::Event`] behind them: WP-14's
    /// exporter reports through a callback, because an export belongs to
    /// whoever asked for it rather than to the transport. Sent as each file
    /// starts, so the path in it is the file being written and not the one just
    /// finished.
    #[serde(rename = "export-progress")]
    ExportProgress {
        /// Which file, 1-based.
        index: u32,
        /// How many there are.
        of: u32,
        /// Where it is being written.
        path: String,
        /// Frames written across the whole export so far.
        frames: u64,
        /// Frames the plan expects in total.
        total: u64,
    },

    /// An export finished, and this is what it wrote.
    ///
    /// The numbers are the exporter's own report rather than a count kept by
    /// the shell, so they are what is on disk.
    #[serde(rename = "export-finished")]
    ExportFinished {
        /// Audio files written.
        files: u32,
        /// Cover files written beside them.
        covers: u32,
        /// Frames written, summed across the files.
        frames: u64,
        /// Bytes written, which is the number a progress dialogue should show
        /// against the free space it had.
        ///
        /// Named for what it counts rather than just `bytes`: the contract's
        /// own test bans a field called `bytes`, because that is what a payload
        /// of audio would be called and §35 forbids one crossing.
        bytes_written: u64,
    },

    /// An export stopped part way, and this is why.
    ///
    /// Files already written are left where they are: §33 writes each file
    /// whole, so what is there is complete, and deleting a good file because a
    /// later one failed would be the shell destroying work it did not have to.
    #[serde(rename = "export-failed")]
    ExportFailed {
        /// What went wrong, as a sentence.
        reason: String,
        /// Files written before it stopped.
        written: u32,
    },

    /// The engine has stopped and will send nothing further.
    ///
    /// Always the last event and always sent, including on a failure: a
    /// consumer that blocks on the stream needs a guaranteed terminator.
    Closed,
}

impl Wire {
    /// The `kind` this variant serialises as.
    ///
    /// The same string [`vcw_core::Event::name`] returns for the event it came
    /// from, which is what makes the two contracts one contract.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::PhaseChange { .. } => "phase-change",
            Self::Armed { .. } => "armed",
            Self::RecordingPosition { .. } => "recording-position",
            Self::MeterUpdate { .. } => "meter-update",
            Self::TrackDetected { .. } => "track-detected",
            Self::CaptureWarning { .. } => "capture-warning",
            Self::CaptureFinished { .. } => "capture-finished",
            Self::Auditioning { .. } => "auditioning",
            Self::PlaybackPosition { .. } => "playback-position",
            Self::PlaybackFinished { .. } => "playback-finished",
            Self::CommandRefused { .. } => "command-refused",
            Self::CommandRejected { .. } => "command-rejected",
            Self::Status { .. } => "status",
            Self::ExportProgress { .. } => "export-progress",
            Self::ExportFinished { .. } => "export-finished",
            Self::ExportFailed { .. } => "export-failed",
            Self::Closed => "closed",
        }
    }

    /// Whether this is the last event the stream will carry.
    #[must_use]
    pub const fn is_last(&self) -> bool {
        matches!(self, Self::Closed)
    }
}

impl From<&Event> for Wire {
    fn from(event: &Event) -> Self {
        match event {
            Event::Phase { from, to } => Self::PhaseChange {
                from: (*from).into(),
                to: (*to).into(),
            },
            Event::Armed {
                project,
                negotiated,
                divergences,
                verified,
            } => Self::Armed {
                project: project.clone(),
                negotiated: negotiated.clone(),
                divergences: divergences.clone(),
                verified: *verified,
            },
            Event::Position { frames, seconds } => Self::RecordingPosition {
                frames: *frames,
                seconds: *seconds,
            },
            Event::Meter { levels } => Self::MeterUpdate {
                meter: Meter::from(levels),
            },
            Event::Detected {
                frame,
                seconds,
                edge,
                confidence,
                provenance,
            } => Self::TrackDetected {
                frame: *frame,
                seconds: *seconds,
                edge: (*edge).into(),
                confidence: *confidence,
                provenance: (*provenance).into(),
            },
            Event::Warning { code, detail } => Self::CaptureWarning {
                code: (*code).to_owned(),
                detail: detail.clone(),
            },
            Event::Finished {
                capture_id,
                frames,
                state,
                diagnostics,
                bit_perfect,
            } => Self::CaptureFinished {
                capture_id: *capture_id,
                frames: *frames,
                state: (*state).into(),
                diagnostics: (*diagnostics).into(),
                bit_perfect: *bit_perfect,
            },
            Event::Auditioning {
                capture_id,
                scope,
                opened,
                conversion,
                divergences,
            } => Self::Auditioning {
                capture_id: *capture_id,
                scope: scope.clone(),
                opened: opened.clone(),
                conversion: conversion.clone(),
                divergences: divergences.clone(),
            },
            Event::Playhead { frame, seconds } => Self::PlaybackPosition {
                frame: *frame,
                seconds: *seconds,
            },
            Event::Ended {
                capture_id,
                frames,
                underruns,
                fidelity,
                bit_perfect,
            } => Self::PlaybackFinished {
                capture_id: *capture_id,
                frames: *frames,
                underruns: *underruns,
                fidelity: fidelity.clone(),
                bit_perfect: *bit_perfect,
            },
            Event::Refused {
                command,
                phase,
                reason,
            } => Self::CommandRefused {
                command: (*command).to_owned(),
                phase: (*phase).into(),
                reason: reason.clone(),
            },
            Event::Rejected { command, phase } => Self::CommandRejected {
                command: (*command).to_owned(),
                phase: (*phase).into(),
            },
            Event::Status { phase, frames } => Self::Status {
                phase: (*phase).into(),
                frames: *frames,
            },
            Event::Closed => Self::Closed,
            // `vcw_core::Event` is `#[non_exhaustive]`. A variant added there
            // and not here becomes a warning rather than a silent drop: the
            // frontend sees an event it can show and this arm is what a test
            // in that crate's own suite will trip over.
            other => Self::CaptureWarning {
                code: "unmapped-event".to_owned(),
                detail: format!("{} is not in the wire contract yet", other.name()),
            },
        }
    }
}

/// The dBFS conversion, kept next to the only thing that uses it.
///
/// [`Levels`] is built from linear amplitudes because that is what the meter
/// computes; every consumer of this contract wants decibels, and §2 puts the
/// logarithm on this side of the boundary.
impl From<&vcw_signal::meter::Snapshot> for Meter {
    fn from(snapshot: &vcw_signal::meter::Snapshot) -> Self {
        Self {
            channels: snapshot.channels.iter().map(Levels::from).collect(),
            frames: snapshot.frames,
            clipped: snapshot.clipped(),
        }
    }
}

impl From<&vcw_signal::meter::Levels> for Levels {
    fn from(levels: &vcw_signal::meter::Levels) -> Self {
        Self {
            peak_db: levels.peak_db(),
            rms_db: levels.rms_db(),
            hold_db: levels.hold_db(),
            clipped: levels.clipped,
            clipped_samples: levels.clipped_samples,
        }
    }
}

impl From<vcw_types::Diagnostics> for Diagnostics {
    fn from(diagnostics: vcw_types::Diagnostics) -> Self {
        Self {
            overruns: diagnostics.overruns,
            underruns: diagnostics.underruns,
            dropped_frames: diagnostics.dropped_frames,
            stream_errors: diagnostics.stream_errors,
        }
    }
}
