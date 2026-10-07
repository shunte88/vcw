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

use crate::event::{EdgeName, ProvenanceName};

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
    /// Unix seconds at finalization, or `null` while it is still running.
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
    /// How long it is, in seconds.
    ///
    /// `end - start`, and carried rather than left to the caller on purpose: a
    /// track length is the number a person reads off the screen and the number
    /// `vcw list` prints, and two subtractions in two languages is how those
    /// two come to disagree about a rounding.
    pub seconds: f64,
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
    /// Normalized genres, in order (§32).
    pub genres: Vec<String>,
    /// Record label.
    pub label: String,
    /// Catalog number off the label, which is what identifies a pressing.
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
    /// Catalog number.
    pub catalog: String,
    /// Country.
    pub country: String,
    /// Format, as the provider describes the medium.
    pub format: String,
    /// How many tracks it lists, which is the first thing to compare against
    /// what was detected, or `None` where the provider did not say.
    ///
    /// Discogs is the reason this is optional. Its search endpoint returns no
    /// tracklist at all - only a release fetch has one - so a count of zero
    /// would be the panel saying "this pressing has no tracks" when what it
    /// means is "ask again and I will know".
    pub tracks: Option<u32>,
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
    /// The first frame covered, in seconds.
    pub start_seconds: f64,
    /// One past the last frame covered, in seconds.
    pub end_seconds: f64,
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
    /// Whether every file's channels are summed to one on the way out.
    ///
    /// The release's `is_mono`. A confirmation dialog has to say it: the fold
    /// is invisible once the files exist - they are simply mono - so the only
    /// place to notice it was not wanted is before the export runs.
    pub fold_to_mono: bool,
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
            seconds: record.end.saturating_sub(record.start) as f64 / hz,
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
    /// Builds the view from a search hit.
    ///
    /// Separate from [`Candidate::of`] because a provider's *search* answer and
    /// its *release* answer are different types with different completeness: a
    /// search hit knows how many tracks a listing claims without having fetched
    /// them, and `tracks` is therefore an `Option` there and a count here.
    /// Zero is the honest reading of "the listing did not say".
    #[must_use]
    pub fn found(provider: &str, candidate: &vcw_metadata::release::Candidate) -> Self {
        Self {
            provider: provider.to_owned(),
            id: candidate.id.clone(),
            album: candidate.album.clone(),
            artist: candidate.artist.clone(),
            year: candidate.year,
            label: candidate.label.clone(),
            catalog: candidate.catalog.clone(),
            country: candidate.country.clone(),
            format: candidate.format.clone(),
            tracks: candidate
                .tracks
                .map(|n| u32::try_from(n).unwrap_or(u32::MAX)),
        }
    }

    /// Builds the view from a provider's full release.
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
            tracks: Some(
                release
                    .media
                    .iter()
                    .map(|m| m.tracks.len())
                    .sum::<usize>()
                    .try_into()
                    .unwrap_or(u32::MAX),
            ),
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
    pub fn of(
        capture_id: i64,
        drawn: &vcw_signal::waveform::Waveform,
        rate: vcw_types::SampleRate,
    ) -> Self {
        let hz = f64::from(rate.hz()).max(1.0);
        Self {
            capture_id,
            start_frame: drawn.start,
            end_frame: drawn.end,
            start_seconds: drawn.start as f64 / hz,
            end_seconds: drawn.end as f64 / hz,
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
            fold_to_mono: plan.fold_to_mono,
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

/// One measurement behind a boundary.
///
/// The evidence a detector recorded, carried through unchanged: `level-db`,
/// `flatness`, `gap-frames`. A UI shows these when a person asks *why* a
/// boundary is where it is, and it cannot be asked to interpret them - the
/// names are kebab-case and the units are implied by the name, which is
/// [`vcw_types::observation::Evidence`]'s contract and not this crate's to
/// restate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Measurement {
    /// What was measured.
    pub name: String,
    /// The value, in whatever unit the name implies.
    pub value: f64,
}

/// A boundary, as the track editor needs it.
///
/// Distinct from [`Track`] on purpose, and the difference is the whole reason
/// this type exists. A track is a *pair* of boundaries that survived §24's
/// promotion policy; a boundary is a single observation, and most of the ones a
/// detector produces never become a track. `Policy::min_sources` turned 270
/// candidates into 6 on the real side, and the 264 that did not make it are
/// still rows - so an editor that only ever sees tracks cannot show a person
/// the boundary the detector nearly kept, which is exactly the one they want to
/// promote by hand.
///
/// [`promoted`](Self::promoted) is the flag that separates the two, and it is
/// computed here rather than inferred in the frontend: it means *some track
/// names this boundary as its start or its end*, which is a join and not a
/// property of the row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Boundary {
    /// Row id, for a command that moves, locks or deletes one.
    pub id: i64,
    /// The side it is on.
    pub side_id: i64,
    /// The side letter, so a flat list groups without a second lookup.
    pub side: String,
    /// The frame it sits at, in that side's capture timeline.
    pub at_frame: u64,
    /// The same position in seconds, divided by the capture's own rate.
    pub seconds: f64,
    /// Which way the audio crosses it.
    pub edge: EdgeName,
    /// How much to trust it, in `0.0..=1.0`.
    pub confidence: f32,
    /// What decided its position.
    pub provenance: ProvenanceName,
    /// Every provenance that reported it.
    pub sources: Vec<ProvenanceName>,
    /// How many distinct detectors agreed, which is what §24's policy
    /// thresholds on. Sent as its own field rather than left as
    /// `sources.length`, because a person who placed the boundary reports zero
    /// sources and is not less certain for it.
    pub agreement: u32,
    /// Whether analysis may move it (§24).
    pub locked: bool,
    /// Whether a track is bounded by it.
    ///
    /// `false` is the interesting case: a detected boundary that no track uses.
    pub promoted: bool,
    /// The measurements behind it.
    pub evidence: Vec<Measurement>,
}

/// A project in the library, as the browser lists it (§34).
///
/// One row per `.vcw` file found under the library root, and the counts are
/// read out of each file rather than cached anywhere: §2 leaves one copy of the
/// truth, and a browser that showed a stale track count would be a second.
///
/// A file that will not open still gets a row, with [`problem`](Self::problem)
/// set and the counts left at zero. That is deliberate rather than defensive -
/// a directory of records is somewhere a person keeps things, so a partial
/// download, a foreign `.vcw` from a newer schema and a file still being
/// written are all normal, and a browser that silently omitted them would be
/// hiding the one thing worth saying.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Absolute path to the `.vcw` file.
    pub path: String,
    /// The file stem, which is what a person named it.
    pub name: String,
    /// Release title, empty until something fills one in.
    pub album: String,
    /// Release artist.
    pub album_artist: String,
    /// Catalog number (§32), which is how a vinyl library is actually
    /// indexed.
    pub catalog: String,
    /// Release year.
    pub year: Option<u32>,
    /// How many sides the project has rows for.
    pub sides: u32,
    /// How many tracks.
    pub tracks: u32,
    /// How many captures.
    pub captures: u32,
    /// Total recorded audio, in seconds.
    pub seconds: f64,
    /// The size of the `.vcw` file on disk, which for a vinyl project is most
    /// of what a person wants to know before opening it.
    ///
    /// `file_bytes` rather than `bytes`, and the longer name is load-bearing:
    /// `no_pcm_crosses_the_boundary` bans a field called `bytes` outright,
    /// because in this contract that word has only ever meant sample data. The
    /// guard fired on the first draft of this type, which is the guard working
    /// - the honest fix is to say which bytes, not to rename around the check.
    pub file_bytes: u64,
    /// Last modification, in unix seconds.
    pub modified: i64,
    /// Whether the release has a front cover stored.
    ///
    /// A flag and not the image. A cover is a megabyte or two and a library is
    /// a hundred rows, so shipping the bytes with the listing would make
    /// opening the browser cost more than opening a project. The flag is what a
    /// table needs to decide between an image and a placeholder, and it is free
    /// here because the walk already has the file open: `length(bytes)` reads
    /// the row, not the blob.
    pub has_artwork: bool,
    /// A waveform small enough to be an icon: peak magnitude per column, in
    /// `0..=1`, over the whole of the first capture. Empty when there is none.
    ///
    /// The tile view needs a picture of every project, and until a release is
    /// assigned there is no cover to be one. The waveform is what the project
    /// already is - a side of a record has a shape, and two rips of different
    /// records never look alike - so it is the honest placeholder rather than a
    /// repeated sleeve glyph.
    ///
    /// This is the same reader the real waveform uses at a tiny width, not a
    /// second summary: `Shape::whole` over [`PREVIEW_COLUMNS`], served out of
    /// the `sampleblocks_levels` covering index, which is 14 ms for a
    /// 26-minute side and the reason this can ride on the listing at all.
    /// One array and not three, because at this size min and max are mirror
    /// images and rms is invisible.
    pub preview: Vec<f32>,
    /// Why the file could not be read, where it could not.
    pub problem: Option<String>,
}

/// How many columns a [`Project::preview`] carries.
///
/// A tile is a couple of hundred CSS pixels wide, so this is already more
/// detail than it can draw. It is a constant rather than a parameter because
/// the listing cannot know how wide the tile will be, and a preview that
/// changed length with the window would defeat the cache the browser keeps.
pub const PREVIEW_COLUMNS: u32 = 96;

impl Boundary {
    /// Builds the view from a row, its side letter and its capture's rate.
    ///
    /// `promoted` is passed in rather than looked up, because the caller is
    /// reading every track anyway and doing it per boundary would be a query
    /// per row.
    #[must_use]
    pub fn of(
        side: vcw_types::Side,
        record: &vcw_project::track::Boundary,
        rate: vcw_types::SampleRate,
        promoted: bool,
    ) -> Self {
        let hz = f64::from(rate.hz()).max(1.0);
        Self {
            id: record.id,
            side_id: record.side_id,
            side: side.letter().to_string(),
            at_frame: record.at_frame,
            seconds: record.at_frame as f64 / hz,
            edge: record.edge.into(),
            confidence: record.confidence,
            provenance: record.provenance.into(),
            sources: record.sources.iter().copied().map(Into::into).collect(),
            agreement: u32::try_from(record.agreement()).unwrap_or(u32::MAX),
            locked: record.locked,
            promoted,
            evidence: record
                .evidence
                .iter()
                .map(|e| Measurement {
                    name: e.name.clone(),
                    value: e.value,
                })
                .collect(),
        }
    }
}

/// What accepting a release did to the project (§26).
///
/// Returned by the command rather than published as an event, because a person
/// pressed a button and is looking at the answer. The four counts are the
/// interesting part: a tracklist that lines up exactly is the happy case and
/// needs no explanation, and one that does not is something a person has to
/// see before they trust the titles.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    /// The release title that was written.
    pub album: String,
    /// The release artist that was written.
    pub album_artist: String,
    /// How many tracks were retitled.
    pub named: u32,
    /// How many were left alone because a person had confirmed them.
    pub kept: u32,
    /// Provider positions that named no track in this project.
    pub unmatched: Vec<String>,
    /// Project tracks the tracklist did not cover, as §29 positions.
    pub unnamed: Vec<String>,
    /// Tracks the release's layout moved to another side, as their new
    /// positions, when the project's own layout was not the record's.
    pub relaid: Vec<String>,
    /// Bytes of cover art stored with the release, or zero if none arrived.
    pub artwork: u32,
}

impl Accepted {
    /// Builds the view from what [`vcw_core::identity::accept`] reported.
    #[must_use]
    pub fn of(release: &vcw_metadata::release::Release, applied: &vcw_core::Applied) -> Self {
        Self {
            album: release.album.clone(),
            album_artist: release.album_artist.clone(),
            named: u32::try_from(applied.tracks.len()).unwrap_or(u32::MAX),
            kept: u32::try_from(applied.confirmed.len()).unwrap_or(u32::MAX),
            unmatched: applied.unmatched.clone(),
            unnamed: applied.unnamed.clone(),
            relaid: applied.relaid.clone(),
            artwork: 0,
        }
    }

    /// Records that a cover of this size was stored with the release.
    ///
    /// Separate from [`Accepted::of`] because the cover is downloaded by the
    /// caller rather than by `accept`: it is a second network round trip and
    /// one that is allowed to fail without losing the release.
    #[must_use]
    pub fn with_artwork(mut self, bytes: usize) -> Self {
        self.artwork = u32::try_from(bytes).unwrap_or(u32::MAX);
        self
    }
}

/// One third-party component the binary links, as a UI shows it.
///
/// The TypeScript-facing mirror of [`vcw_export::notices::Notice`], which is
/// where the facts live: the list is derived from `vcw-export`'s cargo features
/// and a build without `mp3` has no LGPL component to declare. Mirrored rather
/// than exported directly because `vcw-export` has no business depending on
/// `serde` or `ts-rs` for one struct.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// The crate, or the library it vendors.
    pub component: String,
    /// The SPDX expression the crate declares.
    pub license: String,
    /// What VCW can do because it is linked.
    pub provides: String,
    /// Where the complete corresponding source is published.
    pub source: String,
    /// Whether the license grants the right to modify the component and relink
    /// it into VCW, which is the sentence LGPL-3.0 §4 requires be offered.
    pub copyleft: bool,
}

impl From<&vcw_export::notices::Notice> for Notice {
    fn from(notice: &vcw_export::notices::Notice) -> Self {
        Self {
            component: notice.component.to_owned(),
            license: notice.license.to_owned(),
            provides: notice.provides.to_owned(),
            source: notice.source.to_owned(),
            copyleft: notice.copyleft,
        }
    }
}

/// Which build of VCW this is, and what it links (WP-28).
///
/// Two things at once, and on purpose. The identity is the first thing anyone
/// asks for in a bug report, and the notices are a license obligation: shipping
/// `mp3lame-sys` compiles libmp3lame into the binary under LGPL-3.0, inside a
/// product whose own code is MIT, and someone who installs the package and
/// never opens the repository is otherwise told nothing about it.
///
/// **Nothing here is written out as prose.** Every field is read from the crate
/// that owns the fact - the manifest, the features, the schema - which is the
/// rule [`ExportPlan`]'s refusal advice arrived at the hard way. A dialog with a
/// license sentence typed into it is a dialog that is wrong about a build nobody
/// rebuilt it for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct About {
    /// The product's name.
    pub product: String,
    /// What the project is, in the sentence the repository leads with.
    ///
    /// The name above answers "what is this called"; a person who has just
    /// installed a package and opened the only dialog that explains itself is
    /// asking the other question.
    pub description: String,
    /// The release this is.
    pub version: String,
    /// `debug` or `release`. A timing complaint against a debug build is a
    /// different conversation, and this is the field that ends it early.
    pub profile: String,
    /// The day this was compiled, as `YYYY-MM-DD`.
    ///
    /// Stamped by `build.rs`, because a version answers "which release" and
    /// this answers the question people holding a build actually ask, which is
    /// "is this the one from Tuesday". `SOURCE_DATE_EPOCH` wins where it is
    /// set, so a reproducible build stays reproducible.
    pub built: String,
    /// Where the source is.
    pub repository: String,
    /// Whoever holds the copyright on VCW's own code.
    pub authors: Vec<String>,
    /// The SPDX expression VCW's own code is under.
    pub license: String,
    /// The operating system the binary was built for.
    pub os: String,
    /// The processor architecture.
    pub arch: String,
    /// The bundled SQLite, as the library reports itself at run time.
    pub sqlite: String,
    /// The project schema this build writes.
    pub schema_version: u32,
    /// The audio format version this build writes.
    pub format_version: u32,
    /// Every third-party component this build owes a notice for.
    pub notices: Vec<Notice>,
}

impl About {
    /// This build, described.
    ///
    /// A free function's worth of work with no inputs, because there is nothing
    /// to ask: every answer is a compile-time constant of the crate it is read
    /// from, or a string the bundled SQLite hands back.
    #[must_use]
    pub fn current() -> Self {
        Self {
            // Named, not read. `CARGO_PKG_DESCRIPTION` is *this* crate's
            // description - first light found "The typed command, event and
            // view-model surface between the Rust core and any UI" at the top
            // of the dialog - and the workspace's description, which is the
            // product's name, is inherited by no crate. `tests` below checks
            // this literal against the workspace manifest.
            product: "VCW - The Vinyl Capture Workstation".to_owned(),
            // Named for the same reason as `product`, and checked the same way:
            // it lives in `[workspace.metadata.vcw]` because no crate inherits
            // it and a manifest is not shipped beside a binary.
            description: "VCW the de facto tool for vinyl capture across platforms: a \
                          complete workstation for listeners preserving and cataloging \
                          their collections, with reusable services for people building \
                          their own workflows and interfaces"
                .to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            profile: if cfg!(debug_assertions) {
                "debug".to_owned()
            } else {
                "release".to_owned()
            },
            built: env!("VCW_BUILT").to_owned(),
            repository: env!("CARGO_PKG_REPOSITORY").to_owned(),
            authors: env!("CARGO_PKG_AUTHORS")
                .split(':')
                .filter(|who| !who.is_empty())
                .map(str::to_owned)
                .collect(),
            license: env!("CARGO_PKG_LICENSE").to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            sqlite: vcw_project::sqlite::runtime_version().to_owned(),
            schema_version: vcw_project::SCHEMA_VERSION,
            format_version: vcw_project::FORMAT_VERSION,
            notices: vcw_export::notices::notices()
                .iter()
                .map(Notice::from)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::About;

    /// The two fields of [`About`] that are literals, so the two that can
    /// drift. Everything else is a constant of the crate it is read from.
    #[test]
    fn the_product_is_what_the_workspace_calls_it() {
        assert_eq!(
            About::current().product,
            declared("workspace.package"),
            "the dialog's product name is not the one the workspace declares"
        );
    }

    /// The sentence under the name, which the repository's own description is.
    ///
    /// A separate key from `[workspace.package] description` rather than a
    /// replacement for it: that one is the product's name, which is what
    /// crates.io shows and what the dialog titles itself with.
    #[test]
    fn the_description_is_what_the_project_says_it_is() {
        assert_eq!(
            About::current().description,
            declared("workspace.metadata.vcw"),
            "the dialog's description is not the one the workspace declares"
        );
    }

    /// The `description` under one `[section]` of the workspace manifest.
    ///
    /// Section-aware because there are now two of them, and a search for the
    /// first `description = ` in the file would answer both questions with
    /// whichever key happens to be written higher up.
    fn declared(section: &str) -> String {
        let manifest = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"),
        )
        .expect("the workspace manifest is two directories above this crate");
        manifest
            .lines()
            .skip_while(|line| line.trim() != format!("[{section}]"))
            .skip(1)
            .take_while(|line| !line.trim_start().starts_with('['))
            .find_map(|line| line.trim().strip_prefix("description = "))
            .map(|value| value.trim_matches('"').to_owned())
            .unwrap_or_else(|| panic!("[{section}] declares a description"))
    }
}
