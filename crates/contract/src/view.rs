/*
 *  view.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §35's view models: what a UI is given to draw, with the units already resolved.
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

//! §35's view models: what a UI is given to draw, with the units already resolved.
//!
//! A view model is not a row. [`Track`] carries the seconds a track runs for,
//! not the two boundary ids it is made of, because §2 puts the arithmetic on
//! this side of the boundary and because the ids are an implementation detail of
//! the project schema - moving a boundary is a command, not something a frontend
//! computes. The same rule sends dBFS rather than amplitudes and a side letter
//! rather than a side index.
//!
//! Every type here is flat and owns its data. Nothing borrows from a
//! `Connection`, so a view can be built inside a lock and sent out of it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One channel's levels, in dBFS.
///
/// Decibels rather than the amplitudes the meter computes: `20 * log10(x)` is
/// signal processing, and §2 does not allow that in the frontend. The floor is
/// [`vcw_signal::meter::SILENCE_DB`], so a silent channel is a large negative
/// number rather than `-Infinity`, which does not survive JSON.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Levels {
    /// Highest magnitude since the previous snapshot.
    pub peak_db: f32,
    /// Root mean square over the meter's window.
    pub rms_db: f32,
    /// The hold needle: a recent maximum, falling.
    pub hold_db: f32,
    /// Whether this channel has clipped since the latch was cleared.
    pub clipped: bool,
    /// Samples at or beyond full scale since the latch was cleared.
    pub clipped_samples: u64,
}

/// Every channel at one instant: §17's Rust-generated snapshot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Meter {
    /// One entry per channel, in stream order.
    pub channels: Vec<Levels>,
    /// Frames metered since the meter was made or last reset.
    pub frames: u64,
    /// Whether any channel has clipped. Computed here so a clip indicator does
    /// not need a reduce in the view layer.
    pub clipped: bool,
}

/// The ring's four counters.
///
/// Sent in full rather than as a health score, because §38 keeps them apart on
/// purpose: an overrun is audio nobody collected and a stream error is the host
/// complaining, and a single number would hide which happened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    /// Times the ring filled before the writer drained it.
    pub overruns: u64,
    /// Times the device callback found no data ready.
    pub underruns: u64,
    /// Frames known to be lost. Non-zero means not bit-perfect.
    pub dropped_frames: u64,
    /// Stream errors reported by the host.
    pub stream_errors: u64,
}

/// A device, as a list needs it.
///
/// A projection of [`vcw_audio::devices::DeviceReport`], which carries every
/// advertised configuration family and is far more than a picker wants. What
/// survives is the identity, the label, what it can do, and anything that went
/// wrong interrogating it - `problems` being non-empty does not mean unusable,
/// and "busy" is the common one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// The identity to select on and to persist. Never the name: §7's rule is
    /// that names are neither unique nor stable.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// `alsa`, `wasapi`, `coreaudio`, as CPAL spells it.
    pub host: String,
    /// How the device is attached: USB, PCI, Bluetooth.
    pub interface: String,
    /// What sits between us and the converter, as a line of text.
    pub transport: String,
    /// Whether it can capture.
    pub can_capture: bool,
    /// Whether it can play back.
    pub can_play: bool,
    /// This host's default input.
    pub is_default_input: bool,
    /// This host's default output.
    pub is_default_output: bool,
    /// Which of §8's rates it advertises for capture, ascending.
    pub capture_rates: Vec<u32>,
    /// Which of §8's formats it advertises for capture.
    pub capture_formats: Vec<String>,
    /// The channel counts it advertises for capture, ascending.
    pub capture_channels: Vec<u16>,
    /// Whatever went wrong while interrogating it.
    pub problems: Vec<String>,
}

/// A capture, as a list of takes needs it.
///
/// A projection of [`vcw_project::session::Record`]: the row plus the two
/// things a person actually chooses on, which are how long it is and whether it
/// came off the device unaltered. §9's `osVerified` is separate from
/// `bitPerfect` on purpose - the first says an independent check was made, the
/// second says what it found, and a UI that showed only the second would be
/// claiming bit-perfection on the strength of the API's own report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    /// Row id, which is what every other command refers to it by.
    pub id: i64,
    /// Frames recorded, per channel.
    pub frames: u64,
    /// The same thing in seconds, at the rate it was recorded at.
    pub seconds: f64,
    /// Sample rate in Hz.
    pub rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Sample format, spelled as an arm request would spell it.
    pub format: String,
    /// How the device was opened: `shared`, `native` or `exclusive`.
    pub mode: String,
    /// `recording`, `finalised` or `interrupted`.
    pub state: String,
    /// The device it came off, where the row records one.
    pub device: Option<String>,
    /// The host API that opened it.
    pub host: Option<String>,
    /// Whether the operating system confirmed the format independently (§9).
    pub os_verified: bool,
    /// Unix seconds at the start.
    pub started_at: i64,
    /// Unix seconds at finalisation, or `null` while it is still running.
    pub finished_at: Option<i64>,
    /// Overruns, underruns, dropped frames and stream errors.
    pub diagnostics: Diagnostics,
}

/// A side of a record.
///
/// The letter is the identity here, not the index: §29 numbers sides A, B, C, D
/// and a frontend should never be doing `index / 2 + 1` to find the disc. Both
/// are sent, and the arithmetic stayed in Rust.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Side {
    /// Row id, for commands that name a side.
    pub id: i64,
    /// The side letter: `A`, `B`, `C`, `D`.
    pub letter: String,
    /// Which disc it is on, one-based.
    pub disc: u32,
    /// `"a"` or `"b"`: which face of that disc.
    pub face: String,
    /// The capture holding its audio, where one has been recorded.
    pub capture_id: Option<i64>,
    /// A title, where the label prints one for the side.
    pub title: Option<String>,
}

/// A track, as an editor needs it.
///
/// `start` and `end` are seconds, and `startFrame`/`endFrame` are beside them
/// because a waveform is drawn in frames and a duration is read in seconds. The
/// two boundary ids are here too, and they are the only reason: a command that
/// moves or locks a boundary has to name one, and a track is the only place a
/// UI can learn which two are its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// Row id.
    pub id: i64,
    /// The side it is on.
    pub side_id: i64,
    /// The side letter, so a flat list can be grouped without a second lookup.
    pub side: String,
    /// One-based number within the side.
    pub number: u32,
    /// §29's position as it is presented: `A1`, or `1` under continuous
    /// numbering. Resolved here because the rule is the release's, not the
    /// track's.
    pub position: String,
    /// Where it starts, in seconds.
    pub start: f64,
    /// Where it ends, in seconds.
    pub end: f64,
    /// The frame it starts at.
    pub start_frame: u64,
    /// The frame it ends at.
    pub end_frame: u64,
    /// The boundary it starts at, for a command that moves or locks one.
    pub start_boundary: i64,
    /// The boundary it ends at.
    pub end_boundary: i64,
    /// Title, empty until something fills it in.
    pub title: String,
    /// Track artist, or `null` to take the release's (§32).
    pub artist: Option<String>,
    /// Composer, or `null` to take the release's.
    pub composer: Option<String>,
    /// Free text a person added.
    pub comments: Option<String>,
    /// The recording it was identified as (§25/§28).
    pub musicbrainz_id: Option<String>,
    /// Whether a person has confirmed the metadata.
    pub confirmed: bool,
}

/// The release, as the metadata pane needs it.
///
/// Genres arrive as a list rather than the `'; '` string they are stored as,
/// because splitting a delimited string is the kind of processing §2 keeps out
/// of the frontend and because a chip list wants an array.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    /// Release title.
    pub album: String,
    /// The artist credited for the release as a whole.
    pub album_artist: String,
    /// Release year, where it is known.
    pub year: Option<u32>,
    /// Normalised genres, in order (§32).
    pub genres: Vec<String>,
    /// Record label.
    pub label: String,
    /// Catalogue number off the label, which is what identifies a pressing.
    pub catalog: String,
    /// Country of the pressing.
    pub country: String,
    /// Barcode, where the sleeve carries one.
    pub barcode: Option<String>,
    /// Composer.
    pub composer: String,
    /// Whatever a person wrote about this copy.
    pub comments: String,
    /// How many discs the release has.
    pub discs: u32,
    /// `"alpha"` or `"continuous"`: how track numbers are presented (§29).
    pub numbering: String,
    /// MusicBrainz release id.
    pub musicbrainz_id: Option<String>,
    /// Discogs release id.
    pub discogs_id: Option<String>,
    /// Whether a person has accepted this metadata (§26).
    pub confirmed: bool,
}

/// One candidate release from a provider (§28).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// `discogs` or `musicbrainz`.
    pub provider: String,
    /// The provider's own id, which is what `select_release` takes back.
    pub id: String,
    /// Release title.
    pub album: String,
    /// Credited artist.
    pub artist: String,
    /// Year, where the provider knows.
    pub year: Option<u32>,
    /// Label.
    pub label: String,
    /// Catalogue number.
    pub catalog: String,
    /// Country.
    pub country: String,
    /// Format, as the provider describes the medium.
    pub format: String,
    /// How many tracks it lists, which is the first thing to compare against
    /// what was detected.
    pub tracks: u32,
}

/// A window of the waveform, ready to draw (§19).
///
/// Three floats per column and never a sample: §35 forbids PCM on this wire,
/// and a column is already a reduction of thousands of frames. The columns are
/// evenly spaced across `[startFrame, endFrame)`, so a renderer needs no index
/// arithmetic beyond its own x axis.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Waveform {
    /// The capture this describes.
    pub capture_id: i64,
    /// First frame covered.
    pub start_frame: u64,
    /// One past the last frame covered.
    pub end_frame: u64,
    /// Minimum, per column, in -1..=1.
    pub min: Vec<f32>,
    /// Maximum, per column.
    pub max: Vec<f32>,
    /// RMS, per column.
    pub rms: Vec<f32>,
}

/// What an export is about to do, before it does any of it.
///
/// The answer to a dry run, and what a confirmation dialog should be built
/// from. Every path is resolved and every collision already refused, so a plan
/// that exists is a plan that will run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlan {
    /// `wav` or `flac`.
    pub container: String,
    /// One entry per file, in the order they will be written.
    pub files: Vec<ExportFile>,
    /// Artwork files to be written beside the tracks.
    pub covers: Vec<String>,
    /// Frames across the whole export, which is what a progress bar divides by.
    pub frames: u64,
}

/// One file an export will write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    /// Where it will be written.
    pub path: String,
    /// The track it comes from.
    pub track_id: i64,
    /// §29's position, for a list that wants to show A1 rather than a row id.
    pub position: String,
    /// The title it will be tagged with.
    pub title: String,
    /// How many frames it will contain.
    pub frames: u64,
}

impl From<&vcw_audio::devices::DeviceReport> for Device {
    fn from(report: &vcw_audio::devices::DeviceReport) -> Self {
        use vcw_audio::devices::Direction;
        let input = report.direction(Direction::Input);
        Self {
            id: report.key.to_string(),
            name: report.name.clone(),
            host: report.key.host().to_owned(),
            interface: report.interface.clone(),
            transport: report.transport.to_string(),
            can_capture: report.supports(Direction::Input),
            can_play: report.supports(Direction::Output),
            is_default_input: report.is_default_input,
            is_default_output: report.is_default_output,
            capture_rates: input.standard_rates().iter().map(|r| r.hz()).collect(),
            capture_formats: input
                .standard_formats()
                .iter()
                .map(|f| crate::command::format_name(*f).to_owned())
                .collect(),
            capture_channels: input.channel_counts(),
            problems: report.problems.clone(),
        }
    }
}

impl From<&vcw_project::side::Record> for Side {
    fn from(record: &vcw_project::side::Record) -> Self {
        Self {
            id: record.id,
            letter: record.letter().to_string(),
            disc: record.disc(),
            face: match record.face() {
                vcw_types::vinyl::Face::First => "first".to_owned(),
                vcw_types::vinyl::Face::Second => "second".to_owned(),
            },
            capture_id: record.capture,
            title: record.title.clone(),
        }
    }
}

impl From<&vcw_project::release::Record> for Release {
    fn from(record: &vcw_project::release::Record) -> Self {
        Self {
            album: record.album.clone(),
            album_artist: record.album_artist.clone(),
            year: record.year,
            genres: record.genres.clone(),
            label: record.label.clone(),
            catalog: record.catalog.clone(),
            country: record.country.clone(),
            barcode: record.barcode.clone(),
            composer: record.composer.clone(),
            comments: record.comments.clone(),
            discs: record.discs,
            numbering: match record.numbering {
                vcw_types::vinyl::Numbering::Alpha => "alpha".to_owned(),
                vcw_types::vinyl::Numbering::Numeric => "numeric".to_owned(),
            },
            musicbrainz_id: record.musicbrainz_id.clone(),
            discogs_id: record.discogs_id.clone(),
            confirmed: record.confirmed,
        }
    }
}

impl Track {
    /// Builds the view from the three things it takes.
    ///
    /// Not a `From`, because a track row on its own cannot answer two of these
    /// questions: the side letter is the side's and the position is the
    /// release's numbering rule applied to both. Assembling them is what
    /// [`crate::read::tracks`] is for.
    #[must_use]
    pub fn of(
        side: vcw_types::Side,
        record: &vcw_project::track::Record,
        position: String,
        rate: vcw_types::SampleRate,
    ) -> Self {
        let hz = f64::from(rate.hz()).max(1.0);
        Self {
            id: record.id,
            side_id: record.side_id,
            side: side.letter().to_string(),
            number: record.number,
            position,
            start: record.start as f64 / hz,
            end: record.end as f64 / hz,
            start_frame: record.start,
            end_frame: record.end,
            start_boundary: record.start_boundary,
            end_boundary: record.end_boundary,
            title: record.title.clone(),
            artist: record.artist.clone(),
            composer: record.composer.clone(),
            comments: record.comments.clone(),
            musicbrainz_id: record.musicbrainz_id.clone(),
            confirmed: record.confirmed,
        }
    }
}

impl Candidate {
    /// Builds the view from a provider's answer.
    ///
    /// The provider's name is an argument because a
    /// [`vcw_metadata::release::Release`] does not carry it: whoever asked
    /// knows who answered, and storing it on the record would let the two
    /// disagree.
    #[must_use]
    pub fn of(provider: &str, release: &vcw_metadata::release::Release) -> Self {
        let vinyl = release.media.iter().find(|m| m.is_vinyl());
        let medium = vinyl.or_else(|| release.media.first());
        Self {
            provider: provider.to_owned(),
            id: release.id.clone(),
            album: release.album.clone(),
            artist: release.album_artist.clone(),
            year: release.year,
            label: release.label.clone(),
            catalog: release.catalog.clone(),
            country: release.country.clone(),
            format: medium.map(|m| m.format.clone()).unwrap_or_default(),
            tracks: release
                .media
                .iter()
                .map(|m| m.tracks.len())
                .sum::<usize>()
                .try_into()
                .unwrap_or(u32::MAX),
        }
    }
}

impl Waveform {
    /// Builds the view from one channel's columns.
    ///
    /// The three arrays are transposed out of [`vcw_signal::waveform::Column`]
    /// on purpose: a renderer walks one of them at a time, and three arrays of
    /// n floats is both smaller on the wire and faster to draw than n objects
    /// of three floats. The frame count per column is dropped, because an empty
    /// column is already visible as `min == max == 0`.
    #[must_use]
    pub fn of(capture_id: i64, drawn: &vcw_signal::waveform::Waveform) -> Self {
        Self {
            capture_id,
            start_frame: drawn.start,
            end_frame: drawn.end,
            min: drawn.columns.iter().map(|c| c.min).collect(),
            max: drawn.columns.iter().map(|c| c.max).collect(),
            rms: drawn.columns.iter().map(|c| c.rms).collect(),
        }
    }
}

impl ExportPlan {
    /// Builds the view from a plan, with §29's positions filled in.
    ///
    /// The positions come from the plan's own tags rather than a second lookup:
    /// WP-14's splitter already resolved each one to name the file, and asking
    /// the project again could answer differently.
    #[must_use]
    pub fn of(plan: &vcw_export::splitter::Plan) -> Self {
        Self {
            container: plan.container.extension().to_owned(),
            files: plan
                .items
                .iter()
                .map(|item| ExportFile {
                    path: item.path.display().to_string(),
                    track_id: item.track_id,
                    position: item
                        .tags
                        .extra
                        .iter()
                        .find(|(key, _)| key == "VINYL_POSITION")
                        .map(|(_, value)| value.clone())
                        .unwrap_or_default(),
                    title: item.tags.title.clone(),
                    frames: item.frames(),
                })
                .collect(),
            covers: plan
                .covers
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            frames: plan.frames(),
        }
    }
}

impl From<&vcw_project::session::Record> for Capture {
    fn from(record: &vcw_project::session::Record) -> Self {
        let info = &record.info;
        let hz = f64::from(info.rate.hz()).max(1.0);
        Self {
            id: record.id,
            frames: record.frames,
            seconds: record.frames as f64 / hz,
            rate: info.rate.hz(),
            channels: info.channels,
            format: crate::command::format_name(info.storage_format.sample_format()).to_owned(),
            mode: info.capture_mode.as_str().to_owned(),
            state: record.state.as_str().to_owned(),
            device: info.device_name.clone().or_else(|| info.device_id.clone()),
            host: info.host_api.clone(),
            os_verified: info.os_verified,
            started_at: record.started_at,
            finished_at: record.finished_at,
            diagnostics: record.diagnostics.into(),
        }
    }
}
