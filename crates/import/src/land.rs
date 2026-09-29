/*
 *  land.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Landing an Audacity project as a VCW capture.
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
//! Landing an Audacity project as a VCW capture (§12).
//!
//! The step that makes import worth doing. Everything before it reads someone
//! else's file; this writes ours, and it writes it through the same
//! [`vcw_project::persistence::Writer`] a real capture uses. That is the whole
//! design: an imported capture is not a special kind of capture, so playback,
//! export, the waveform pyramids, boundaries, recovery and validation all work
//! on it with no import-shaped branch anywhere in them. WP-20's purpose was to
//! exercise the toolchain, and it can only do that if the toolchain cannot tell.
//!
//! # What is carried across, and what is not
//!
//! **Audio**, frame for frame, byte for byte, assembled by [`crate::timeline`]
//! with the gaps between clips written as silence so that every label still
//! names the audio it named. No sample is converted (D4).
//!
//! **Labels become tracks.** An Audacity label is a titled `t`..`t1` span, which
//! is exactly what a VCW track is: two boundaries and a title. They land as
//! [`vcw_types::Provenance::User`] boundaries, locked, because a person placed
//! them by hand in Audacity and §24 says analysis may not move what a person
//! placed. Re-running detection over an imported project therefore adds
//! boundaries beside them rather than dragging them about.
//!
//! **Tags become release metadata**, where they mean the same thing. Audacity
//! has six standard tag names and allows any other, so the ones that map are
//! mapped and every tag is *also* written verbatim into `meta` under
//! `import.tag.*` - a lossy mapping plus a lossless record, rather than a lossy
//! mapping alone.
//!
//! **Not carried:** gain, pan, mute, solo and envelopes. All five are editor
//! state in Audacity, not properties of the recording, and a `.vcw` has nowhere
//! to put them because §4.1 keeps the capture untouched and applies everything
//! at export. An all-unity envelope is absent by definition ([`crate::model`]);
//! a real one would silently change the audio if we applied it and silently
//! change the user's intent if we dropped it, so a project carrying one is
//! reported rather than half-honoured.
//!
//! # Which side
//!
//! Side A unless told otherwise, and one side per import. An Audacity project
//! has nothing in it that says which face of which disc it holds - a rip of a
//! whole LP into one file is the common case in the corpus, and two faces
//! sharing one capture is a thing VCW already allows. So the side is the
//! caller's to state, defaulting to the first, and nothing is guessed from
//! label positions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use vcw_project::persistence::{Config, Writer};
use vcw_project::{Project as Destination, meta, release, side, track};
use vcw_types::vinyl::Side;
use vcw_types::{CaptureInfo, CaptureMode, CaptureState, SampleRate, StorageFormat};

use crate::error::Result;
use crate::model::{Label, Project};
use crate::sniff::Version;
use crate::timeline::Timeline;

/// `meta` key prefix under which every Audacity tag is recorded verbatim.
pub const TAG_PREFIX: &str = "import.tag.";

/// `meta` key holding the file the audio was imported from.
pub const SOURCE_KEY: &str = "import.source";

/// `meta` key holding the Audacity project version the source was.
pub const VERSION_KEY: &str = "import.audacity_format";

/// `meta` key holding the Audacity build that last wrote the source.
pub const WRITER_KEY: &str = "import.audacity_version";

/// How to land a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Which side the capture is. Nothing in an Audacity project says.
    pub side: Side,
    /// Writer tuning. The default is D3, the same as a live capture, so that an
    /// imported project's blocks are the size everything else expects.
    pub config: Config,
    /// Whether to turn labels into tracks.
    pub labels: bool,
    /// Whether to copy the tags onto the release.
    pub tags: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            side: Side::A,
            config: Config::default(),
            labels: true,
            tags: true,
        }
    }
}

/// What landing a project produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landed {
    /// The project written.
    pub path: PathBuf,
    /// Which Audacity format the source was.
    pub version: Version,
    /// The capture the audio landed as.
    pub capture_id: i64,
    /// Which side it was attached to.
    pub side: Side,
    /// The rate, from `wavetrack/@rate`.
    pub rate: SampleRate,
    /// Channels, one per wave track.
    pub channels: u16,
    /// How the samples are stored, which is how Audacity stored them.
    pub storage_format: StorageFormat,
    /// Frames written per channel.
    pub frames: u64,
    /// Blocks written.
    pub blocks: u64,
    /// Clips the timeline was assembled from, across all channels.
    pub clips: usize,
    /// Tracks written from labels.
    pub tracks: usize,
    /// Labels that could not become a track, with the reason.
    pub labels_skipped: Vec<(String, &'static str)>,
    /// Tags recorded.
    pub tags: usize,
    /// Wave tracks carrying an envelope that was not carried across.
    pub envelopes_dropped: usize,
}

impl Landed {
    /// The capture's duration in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.rate.hz() == 0 {
            return 0.0;
        }
        self.frames as f64 / f64::from(self.rate.hz())
    }
}

/// Where a landing reads from.
///
/// A struct rather than four arguments because there are two callers with two
/// different reasons to have the pieces already: [`land`] opens the file itself,
/// and a caller that has surveyed a project - the CLI, printing what it found
/// before writing anything - should not have to open it twice.
#[derive(Debug)]
pub struct Source<'a> {
    /// A connection to the Audacity project, which is only ever read.
    pub conn: &'a Connection,
    /// The document, already modelled.
    pub document: &'a Project,
    /// The file the connection is to, recorded as provenance.
    pub path: &'a Path,
    /// Which generation it is.
    pub version: Version,
}

/// Reads an Audacity project and writes it as a new `.vcw`.
///
/// The source is opened read-only and never written to - not even to checkpoint
/// its log. `destination` must not exist.
///
/// # Errors
///
/// Anything [`crate::sniff`], [`crate::read`], [`crate::model`],
/// [`crate::audit()`] or [`crate::timeline`] can refuse, plus
/// [`crate::Error::Project`] if the destination cannot be written.
pub fn land(source: &Path, destination: &Path, options: &Options) -> Result<Landed> {
    tracing::info!(
        source = %source.display(),
        destination = %destination.display(),
        "importing an Audacity project"
    );
    let (read_only, sniffed) = crate::sniff::open(source)?;
    let survey = crate::read::survey(&read_only, source)?;
    let document = Project::from_events(&survey.document.events)?;
    land_from(
        &Source {
            conn: &read_only,
            document: &document,
            path: source,
            version: sniffed.version,
        },
        destination,
        options,
    )
}

/// Writes a `.vcw` from a project that is already open and modelled.
///
/// # Errors
///
/// Anything [`crate::audit()`] or [`crate::timeline`] can refuse, plus
/// [`crate::Error::Project`] if the destination cannot be written.
pub fn land_from(source: &Source<'_>, destination: &Path, options: &Options) -> Result<Landed> {
    let document = source.document;
    // Before a byte is written: every block reference resolves and every
    // declared length agrees with the audio table.
    crate::audit::audit(source.conn, document)?;
    let mut timeline = Timeline::plan(source.conn, document)?;

    let info = CaptureInfo {
        rate: timeline.rate(),
        channels: timeline.channels(),
        storage_format: timeline.storage_format(),
        // No stream was opened. See `CaptureMode::Imported` for why this is a
        // variant rather than a plausible-looking default.
        capture_mode: CaptureMode::Imported,
        host_api: None,
        device_id: None,
        // Deliberately not the Audacity version: this column names the device
        // that made the recording, which the source file does not know either.
        device_name: None,
        os_verified: false,
        os_report: None,
    };

    let project = Destination::create(destination)?;
    let mut writer = Writer::begin(project, &info, options.config)?;
    let chunk = writer.block_frames();
    while let Some(bytes) = timeline.next_chunk(chunk)? {
        writer.push(&bytes)?;
    }
    // Finalised, not Recovered or Interrupted: the import either read the whole
    // timeline or returned an error above. There is no partial case to report.
    let (outcome, mut project, _session) = writer.finish_with_project(CaptureState::Finalised)?;

    side::attach(&mut project, options.side, outcome.capture_id)?;
    provenance(&mut project, source, document)?;

    let rate = timeline.rate();
    let mut landed = Landed {
        path: destination.to_path_buf(),
        version: source.version,
        capture_id: outcome.capture_id,
        side: options.side,
        rate,
        channels: timeline.channels(),
        storage_format: timeline.storage_format(),
        frames: outcome.frames,
        blocks: outcome.blocks,
        clips: document.tracks.iter().map(|t| t.clips.len()).sum(),
        tracks: 0,
        labels_skipped: Vec::new(),
        tags: 0,
        envelopes_dropped: document
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .filter(|clip| !clip.envelope.is_empty())
            .count(),
    };

    if options.labels {
        let (written, skipped) =
            adopt_labels(&mut project, options.side, document, rate, landed.frames)?;
        landed.tracks = written;
        landed.labels_skipped = skipped;
    }
    if options.tags {
        landed.tags = adopt_tags(&mut project, &document.tags)?;
    }
    project.close()?;
    tracing::info!(
        destination = %landed.path.display(),
        version = ?landed.version,
        rate = landed.rate.hz(),
        channels = landed.channels,
        storage = ?landed.storage_format,
        frames = landed.frames,
        blocks = landed.blocks,
        clips = landed.clips,
        tracks = landed.tracks,
        // Counted, not named. A skipped label is a track title, and a log is a
        // file people paste into bug reports.
        labels_skipped = landed.labels_skipped.len(),
        tags = landed.tags,
        "an Audacity project landed as a capture"
    );
    Ok(landed)
}

/// Records where the audio came from, in `meta`.
///
/// In the project rather than in a log, because provenance that lives outside
/// the file it describes is provenance that gets separated from it.
fn provenance(project: &mut Destination, source: &Source<'_>, document: &Project) -> Result<()> {
    let conn = project.conn();
    meta::set(conn, SOURCE_KEY, &source.path.display().to_string())?;
    meta::set(conn, VERSION_KEY, source.version.as_str())?;
    if let Some(written_by) = &document.audacity_version {
        meta::set(conn, WRITER_KEY, written_by)?;
    }
    for (name, value) in &document.tags {
        meta::set(conn, &format!("{TAG_PREFIX}{name}"), value)?;
    }
    Ok(())
}

/// Turns each label into a track, and reports the ones that could not be.
///
/// A label is skipped rather than fixed up in three cases, all of which a person
/// can produce in Audacity by accident. A **point label**, where `t1 == t`,
/// marks a position rather than a span. One whose span **rounds to no frames**
/// is not a track either. And one that starts **at or past the end of the
/// audio** describes audio that is not in the project: deleting a clip in
/// Audacity does not delete the labels over it, so this is the ordinary shape of
/// a project someone has edited, not a corruption. A label that merely *runs
/// past* the end is kept and clipped to it, because that is a track whose tail
/// was trimmed rather than a track that is not there.
///
/// Each is reported rather than logged: `vcw import` prints them, because a
/// silently dropped track title is exactly the kind of loss an import is
/// supposed to be trusted not to inflict.
fn adopt_labels(
    project: &mut Destination,
    side: Side,
    document: &Project,
    rate: SampleRate,
    frames: u64,
) -> Result<(usize, Vec<(String, &'static str)>)> {
    let mut written = 0;
    let mut skipped = Vec::new();
    let mut labels: Vec<&Label> = document
        .label_tracks
        .iter()
        .flat_map(|list| &list.labels)
        .collect();
    labels.sort_by(|a, b| a.t.total_cmp(&b.t));
    for label in labels {
        let start = frames_at(rate, label.t);
        let end = frames_at(rate, label.t1).min(frames);
        if start >= frames {
            skipped.push((
                label.title.clone(),
                "it begins at or after the end of the audio",
            ));
            continue;
        }
        if end <= start {
            skipped.push((
                label.title.clone(),
                if label.t1 <= label.t {
                    "a point label, which marks a position rather than a track"
                } else {
                    "shorter than one frame"
                },
            ));
            continue;
        }
        let id = track::add_track(project, side, start, end)?;
        if !label.title.is_empty() {
            track::update(project, id, &track::Update::title(label.title.clone()))?;
        }
        written += 1;
    }
    Ok((written, skipped))
}

/// Copies the tags VCW has a field for onto the release.
///
/// Audacity's six standard names are `TITLE`, `ARTIST`, `ALBUM`,
/// `TRACKNUMBER`, `YEAR` and `GENRE`; a user may add any other. Only the ones
/// that mean the same thing in both models are mapped, and every tag is in
/// `meta` regardless, so nothing here is the only copy of anything.
///
/// `TITLE` maps to the album only when there is no `ALBUM`, which is what a
/// whole-side rip tagged by hand usually looks like. `TRACKNUMBER` is not
/// mapped: on a side rip it names one track among many and the release has no
/// field it belongs in.
fn adopt_tags(project: &mut Destination, tags: &BTreeMap<String, String>) -> Result<usize> {
    if tags.is_empty() {
        return Ok(0);
    }
    let mut record = release::ensure(project)?;
    let get = |name: &str| {
        tags.get(name)
            .map(|value| value.trim())
            .filter(|v| !v.is_empty())
    };
    if let Some(album) = get("ALBUM").or_else(|| get("TITLE")) {
        record.album = album.to_owned();
    }
    if let Some(artist) = get("ARTIST") {
        record.album_artist = artist.to_owned();
    }
    if let Some(year) = get("YEAR").and_then(|text| text.parse::<u32>().ok()) {
        record.year = Some(year);
    }
    if let Some(genre) = get("GENRE") {
        // Normalised on the way in, because the column says normalised (§32) and
        // an unnormalised value there would be a trap for the tagger.
        record.genres = vcw_metadata::Genres::builtin().normalise(genre);
    }
    if let Some(comments) = get("COMMENTS") {
        record.comments = comments.to_owned();
    }
    release::store(project, &record)?;
    Ok(tags.len())
}

/// Seconds to frames, rounded to the nearest and never negative.
///
/// Rounded rather than truncated for the same reason [`crate::model`] rounds
/// trims: at 192 kHz a truncation loses a sample, and a label that shares a
/// frame with its neighbour is the difference between two adjacent tracks and
/// two overlapping ones.
fn frames_at(rate: SampleRate, seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds * f64::from(rate.hz())).round() as u64
}
