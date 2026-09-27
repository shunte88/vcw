/*
 *  command.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §35's commands, as the payloads a UI sends and the core's own types they become.
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

//! §35's commands, as the payloads a UI sends and the core's own types they become.
//!
//! §35 divides the surface in two: commands change state, events expose it.
//! This module is the first half, and every type in it is a *request* - what a
//! frontend can ask for - with a conversion into whatever the core actually
//! takes. The conversions are fallible where the request can be nonsense
//! (`"format": "s7"`) and infallible where it cannot, which is the difference
//! between a validation error a UI can show and a panic it cannot.
//!
//! The nine commands §35 names by example are all here:
//! `start_recording` is [`Transport::Record`] after an [`Arm`],
//! `pause_recording` and `stop_recording` are [`Transport`] verbs, `play` and
//! `seek` are [`Audition`] and [`Seek`], `move_marker` is [`Marker`],
//! `search_metadata` is [`Search`], `select_release` is [`Selection`] and
//! `export` is [`Export`].
//!
//! # Why the transport verbs are one enum and arming is not
//!
//! Every transport verb but one is a bare word: there is nothing to say about
//! `pause` except that it was asked for. `arm` carries a device, a project and
//! §9's four optional pins, and putting it in the same enum would make eight
//! of the nine variants empty payloads with a ninth that is a form. They are
//! separate for the same reason [`vcw_core::Command::parse`] cannot build an
//! `Arm`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use vcw_core::commands::{Command, Setup};
use vcw_core::playback::Scope;
use vcw_types::{CaptureMode, SampleFormat, Span};

/// A request this contract could not turn into something the core accepts.
///
/// Always about a *value*, never about a missing field - serde has already
/// refused anything shaped wrongly by the time one of these can be built - so
/// the message names the field and what it would have accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {why}")]
pub struct Invalid {
    /// Which field.
    pub field: &'static str,
    /// What was wrong with it, and what would be right.
    pub why: String,
}

impl Invalid {
    fn new(field: &'static str, why: impl Into<String>) -> Self {
        Self {
            field,
            why: why.into(),
        }
    }
}

/// A command with no payload: every transport verb except arming.
///
/// Serialised as a bare string, so `invoke("transport", { verb: "pause" })`
/// rather than an object wrapping one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    /// Close the device and project without recording. `Armed -> Idle`.
    Disarm,
    /// Begin committing audio. `Armed -> Recording`.
    Record,
    /// Stop committing without giving up the device. `Recording -> Paused`.
    Pause,
    /// Begin committing again. `Paused -> Recording`.
    Resume,
    /// Finalise the capture. The project stays open, per §11.
    Stop,
    /// Release the finished capture and return to rest. `Stopped -> Idle`.
    Reset,
    /// Ask where the transport is without changing it.
    ///
    /// The one command that bends §35's definition, and it earns its place: a
    /// webview that has just reloaded - which S3's R8 mitigation plans for -
    /// has no other way to find out where the transport got to.
    Poll,
    /// Finish any capture in progress, then shut the engine down.
    Shutdown,
}

impl From<Transport> for Command {
    fn from(verb: Transport) -> Self {
        match verb {
            Transport::Disarm => Self::Disarm,
            Transport::Record => Self::Record,
            Transport::Pause => Self::Pause,
            Transport::Resume => Self::Resume,
            Transport::Stop => Self::Stop,
            Transport::Reset => Self::Reset,
            Transport::Poll => Self::Poll,
            Transport::Shutdown => Self::Shutdown,
        }
    }
}

/// What to open, and what to pin while opening it (§9).
///
/// The four optional fields are §9's rule in a form: what the operator pins is
/// a hard requirement and what they leave out is the device's choice. A pinned
/// rate the device cannot do is an error and never a quiet substitution, which
/// is why they are `Option` here rather than defaulted to something plausible.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Arm {
    /// Device id as [`crate::view::Device::id`] gave it, or `null` for the
    /// simulated source - which is how the transport is driven with nothing
    /// plugged in.
    pub device: Option<String>,
    /// Project to record into, created if it does not exist.
    pub project: String,
    /// Sample rate to pin, in Hz.
    pub rate: Option<u32>,
    /// Channel count to pin.
    pub channels: Option<u16>,
    /// Sample format to pin: `s16`, `s24`, `s32` or `f32`.
    pub format: Option<String>,
    /// `exclusive` or `shared`. Defaults to exclusive, which is what §9 wants.
    pub mode: Option<String>,
    /// Ring capacity in milliseconds. `null` takes §10's default.
    pub ring_millis: Option<u32>,
}

impl TryFrom<Arm> for Setup {
    type Error = Invalid;

    fn try_from(arm: Arm) -> Result<Self, Self::Error> {
        if arm.project.trim().is_empty() {
            return Err(Invalid::new("project", "a project path is required"));
        }
        let format = match arm.format.as_deref() {
            None => None,
            Some(text) => Some(parse_format(text)?),
        };
        let mode = match arm.mode.as_deref() {
            None | Some("exclusive") => CaptureMode::Exclusive,
            Some("shared") => CaptureMode::Shared,
            Some(other) => {
                return Err(Invalid::new(
                    "mode",
                    format!("{other:?} is not a capture mode; use exclusive or shared"),
                ));
            }
        };
        Ok(Self {
            device: arm.device,
            project: PathBuf::from(arm.project),
            rate: arm.rate,
            channels: arm.channels,
            format,
            mode,
            ring_millis: arm.ring_millis,
        })
    }
}

/// A playback verb (§21).
///
/// Separate from [`Transport`], which is the capture transport, because the two
/// vocabularies only look alike. `stop` on a capture finalises audio that
/// cannot be recorded again; `stop` on an audition closes a device. A frontend
/// that shared one enum between them would be one typo away from the worst
/// possible confusion.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "verb", rename_all = "kebab-case")]
pub enum Playback {
    /// Start, or resume from where a pause left the playhead.
    Play,
    /// Stop feeding the device and keep the playhead.
    Pause,
    /// Close the device and end the audition.
    Stop,
    /// Move the playhead, in seconds from the start of the audition.
    Seek(Seek),
    /// Jump forward by the audition's skip interval.
    SkipForward,
    /// Jump back by the same.
    SkipBack,
}

impl From<Playback> for vcw_core::playback::Verb {
    fn from(playback: Playback) -> Self {
        match playback {
            Playback::Play => Self::Play,
            Playback::Pause => Self::Pause,
            Playback::Stop => Self::Stop,
            Playback::Seek(seek) => Self::Seek(seek.to),
            Playback::SkipForward => Self::SkipForward,
            Playback::SkipBack => Self::SkipBack,
        }
    }
}

/// What part of a capture to draw, and how wide (§17, §20).
///
/// Frames rather than seconds, unlike [`Region`]: a zoom is arithmetic on the
/// numbers a frontend was already given - a capture's frame count, a track's
/// start and end - and routing it through seconds would round twice and put the
/// waveform a pixel away from the boundary drawn on top of it.
///
/// `pixels` is the width of the canvas, so one column is one pixel and the
/// summary level the reader picks follows from it. Asking for more columns than
/// pixels is how a waveform ends up decoding audio it then throws away.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Zoom {
    /// Which capture.
    pub capture_id: i64,
    /// Which channel, zero-based.
    pub channel: u16,
    /// The first frame to draw.
    pub start_frame: u64,
    /// One past the last frame, or `null` for the end of the capture.
    pub end_frame: Option<u64>,
    /// How many columns to return, which is the canvas width in pixels.
    pub pixels: u32,
}

/// What a refused command looks like to a frontend.
///
/// Every command this contract describes either succeeds or comes back as one
/// of these. Two fields, and both are needed: `code` is a stable slug to branch
/// on, `message` is a sentence to show. A UI that switched on the message would
/// break the day the wording improved, and one that showed the code would put
/// `not-armed` in front of a person.
///
/// The codes are the shell's to define, because what can fail depends on what
/// is wired. They are listed in `app/src-tauri/src/state.rs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    /// A stable slug: `not-armed`, `invalid-argument`, `not-wired`.
    pub code: String,
    /// One sentence, fit to show a person.
    pub message: String,
    /// The field at fault, where one command argument is to blame.
    pub field: Option<String>,
}

/// The canonical wire spelling of a sample format.
///
/// The inverse of [`parse_format`] for the one alias per variant that a frontend
/// should send back, so a device report and an arm request agree on the word.
#[must_use]
pub const fn format_name(format: SampleFormat) -> &'static str {
    match format {
        SampleFormat::S16 => "s16",
        SampleFormat::S24 => "s24",
        SampleFormat::S32 => "s32",
        SampleFormat::F32 => "f32",
    }
}

/// The format names this contract accepts, which are the CLI's.
///
/// Spelled out rather than derived from [`SampleFormat`]'s `Display`: the wire
/// names are part of the contract and the enum's are not, so a rename in
/// `vcw-types` must not silently change what a frontend has to send.
///
/// Public because [`format_name`] is: the two halves of one mapping should be
/// reachable from the same places, and a caller that has to name a format for a
/// person also has to read one back.
///
/// # Errors
///
/// [`Invalid`] naming the `format` field when the text is not one of the
/// aliases above.
pub fn parse_format(text: &str) -> Result<SampleFormat, Invalid> {
    match text.trim().to_ascii_lowercase().as_str() {
        "s16" | "i16" => Ok(SampleFormat::S16),
        "s24" | "i24" => Ok(SampleFormat::S24),
        "s32" | "i32" => Ok(SampleFormat::S32),
        "f32" | "float" | "float32" => Ok(SampleFormat::F32),
        other => Err(Invalid::new(
            "format",
            format!("{other:?} is not a sample format; use s16, s24, s32 or f32"),
        )),
    }
}

/// What to play (§21).
///
/// Seconds rather than frames, because this is what a person clicked on and a
/// click is at a time rather than at a sample. The conversion needs the rate,
/// which is why it is a method rather than a `From`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "scope",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Audition {
    /// The complete capture.
    Whole,
    /// A selected region.
    Region {
        /// Where to start, in seconds.
        start: f64,
        /// Where to stop, in seconds.
        end: f64,
    },
    /// One track's extent, which the core resolves from its boundaries.
    Track {
        /// The track's row id.
        track_id: i64,
    },
    /// A boundary, with §21's context either side of it.
    Boundary {
        /// The boundary's row id.
        boundary_id: i64,
    },
}

impl Audition {
    /// The core's scope, for the two variants that need no project lookup.
    ///
    /// Returns `None` for [`Audition::Track`] and [`Audition::Boundary`],
    /// which name rows rather than times: resolving those needs the project,
    /// and this crate does not open one.
    #[must_use]
    pub fn scope(self, rate: u32) -> Option<Scope> {
        let frames = |seconds: f64| -> u64 {
            if seconds <= 0.0 {
                0
            } else {
                (seconds * f64::from(rate)).round() as u64
            }
        };
        match self {
            Self::Whole => Some(Scope::Whole),
            Self::Region { start, end } => {
                Some(Scope::Region(Span::new(frames(start), frames(end))))
            }
            Self::Track { .. } | Self::Boundary { .. } => None,
        }
    }
}

/// Where to move the playhead to, in seconds from the start of the capture.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Seek {
    /// Seconds from the start of the capture.
    pub to: f64,
}

/// Move a boundary, and with it whichever tracks it bounds (§31).
///
/// §35 calls this `move_marker`. The name here is the project's, because a
/// marker is what it looks like and a boundary is what it is: moving one
/// changes two tracks' extents and nothing is copied, which is the whole
/// reason WP-13's `tracks` table has no frame columns.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    /// Which boundary.
    pub boundary_id: i64,
    /// Where to put it, in seconds.
    pub to: f64,
    /// Whether to override §24's lock.
    ///
    /// Defaults to false, and a locked boundary is refused rather than moved.
    /// Overriding claims the boundary as the operator's on the way past, since
    /// whoever overrides a lock is the new author of that position.
    #[serde(default)]
    pub force: bool,
}

/// A region of a capture, in seconds.
///
/// Used by anything that works over part of a side without playing it: a
/// waveform window, an analysis pass, a region export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    /// Where it starts, in seconds.
    pub start: f64,
    /// Where it ends, in seconds.
    pub end: f64,
}

impl Region {
    /// The span this covers at a given rate, rounded to the nearest frame.
    #[must_use]
    pub fn span(self, rate: u32) -> Span {
        let frames = |seconds: f64| -> u64 {
            if seconds <= 0.0 {
                0
            } else {
                (seconds * f64::from(rate)).round() as u64
            }
        };
        Span::new(frames(self.start), frames(self.end))
    }
}

/// What to ask a metadata provider for (§28).
///
/// Every field is optional and at least one must be filled in, which the
/// provider decides rather than this type: a catalogue number alone is a better
/// query than an artist alone, and which combinations work is knowledge that
/// belongs in `vcw-metadata`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Search {
    /// Artist.
    pub artist: Option<String>,
    /// Album title.
    pub album: Option<String>,
    /// Catalogue number, which is what identifies a pressing.
    pub catalog: Option<String>,
    /// Barcode.
    pub barcode: Option<String>,
    /// `discogs` or `musicbrainz`. `null` asks every configured provider.
    pub provider: Option<String>,
}

/// Which candidate the operator accepted (§26).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    /// `discogs` or `musicbrainz`, as [`crate::view::Candidate::provider`] gave it.
    pub provider: String,
    /// The provider's own id.
    pub id: String,
}

/// What to export, where, and how (§33).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Export {
    /// Directory to write into. Created if it does not exist.
    pub into: String,
    /// `wav` or `flac`.
    pub format: String,
    /// A naming template, or `null` for the default.
    pub template: Option<String>,
    /// Side letters to export, or empty for every side that has tracks.
    #[serde(default)]
    pub sides: Vec<String>,
    /// `none`, `embed`, `folder` or `both`.
    pub artwork: Option<String>,
    /// Whether to replace files that are already there.
    #[serde(default)]
    pub overwrite: bool,
}

impl Export {
    /// Turns the request into WP-14's own.
    ///
    /// The three words a person types - the container, the artwork policy, a
    /// side letter - are parsed here rather than in each caller, because the
    /// CLI verb and the shell command have to accept the same spellings. The
    /// messages are written to be shown: a UI puts them beside the field.
    ///
    /// # Errors
    ///
    /// [`Invalid`] naming the field, for a container VCW does not write, an
    /// artwork policy that is not one of the four, or something that is not a
    /// side letter.
    pub fn request(&self) -> Result<vcw_export::splitter::Request, Invalid> {
        let container = vcw_export::encoder::Container::from_extension(&self.format).ok_or_else(
            || Invalid {
                field: "format",
                why: format!(
                    "{:?} is not a container VCW writes yet - wav or flac. MP3 and Ogg are release 0.2",
                    self.format
                ),
            },
        )?;

        let artwork = match self
            .artwork
            .as_deref()
            .unwrap_or("both")
            .to_ascii_lowercase()
            .as_str()
        {
            "none" => vcw_export::splitter::Artwork::None,
            "embed" => vcw_export::splitter::Artwork::Embed,
            "folder" => vcw_export::splitter::Artwork::Folder,
            "both" => vcw_export::splitter::Artwork::Both,
            other => {
                return Err(Invalid {
                    field: "artwork",
                    why: format!(
                        "{other:?} is not an artwork policy - none, embed, folder or both"
                    ),
                });
            }
        };

        let mut sides = Vec::with_capacity(self.sides.len());
        for given in &self.sides {
            let mut letters = given.chars();
            let (Some(letter), None) = (letters.next(), letters.next()) else {
                return Err(Invalid {
                    field: "sides",
                    why: format!("{given:?} is not a side letter - A to Z, A being first"),
                });
            };
            sides.push(
                vcw_types::vinyl::Side::from_letter(letter).ok_or_else(|| Invalid {
                    field: "sides",
                    why: format!("{letter:?} is not a side letter - A to Z, A being first"),
                })?,
            );
        }

        Ok(vcw_export::splitter::Request {
            container,
            template: self
                .template
                .clone()
                .unwrap_or_else(|| vcw_export::naming::DEFAULT_TEMPLATE.to_owned()),
            into: PathBuf::from(&self.into),
            sides,
            artwork,
            overwrite: self.overwrite,
        })
    }
}

/// Everything a frontend can ask for, as one union.
///
/// The Tauri shell does not use this - it exposes one `#[tauri::command]` per
/// verb, which is the idiom and gives better errors - but a single tagged union
/// is what a log, a replay of a session and any other transport need, and
/// having it here means the set of commands is enumerated in exactly one place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "command", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Request {
    /// Open a device and a project. `Idle -> Armed`.
    Arm(Arm),
    /// A transport verb with no payload.
    Transport {
        /// Which verb.
        verb: Transport,
    },
    /// Play something (§21).
    Play(Audition),
    /// Move the playhead.
    Seek(Seek),
    /// Move a boundary (§31).
    MoveMarker(Marker),
    /// Ask a provider about this record (§28).
    SearchMetadata(Search),
    /// Accept a candidate (§26).
    SelectRelease(Selection),
    /// Turn the project into files (§33).
    Export(Export),
}
