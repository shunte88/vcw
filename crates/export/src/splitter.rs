/*
 *  splitter.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Cutting committed blocks into per-track streams using the edit
 *  instructions.
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

//! Cutting committed blocks into per-track streams using the edit instructions.
//!
//! §33: "Export operates from immutable source blocks plus project edit
//! instructions." Nothing here writes to the project, and nothing here consults
//! anything but the blocks and the rows above them, which is what makes an
//! export reproducible and a re-export idempotent.
//!
//! ## Plan, then run
//!
//! [`plan`] resolves every track to a path, a span and a set of tags and writes
//! nothing. [`run`] takes that plan and produces the files. Two calls rather than
//! one because everything that can be known before the first byte is written
//! should be: a naming template with a typo in it, two tracks that want the same
//! file, a file already on disk. An export is minutes of work over gigabytes of
//! audio, and finding the collision at track nine is finding it too late.
//!
//! It also gives the CLI its `--dry-run` for free, and the UI a list to show
//! before the operator commits to it.
//!
//! ## Sample-accurate, and why that costs nothing
//!
//! A track boundary lands wherever the operator or the detector put it, which is
//! almost never on a block boundary. [`vcw_project::pcm::Reader`] already deals
//! with that - it reassembles the per-channel blobs and hands out interleaved
//! frames from any frame in the capture - so the splitter does no arithmetic on
//! blocks at all. That is deliberate: the one correct way to get samples out of a
//! project is the one playback uses, and a second path that had its own opinion
//! about block edges is a second path that can be wrong on its own.
//!
//! ## What a track's audio is
//!
//! The span between its two boundaries, clamped to what was committed, and
//! nothing else. No fade, no lead-in, no gap trimming: the boundaries are the
//! edit instruction, and an exporter that quietly added 200 ms of run-in would
//! make the bit-exactness the rest of WP-14 proves untestable.

use std::collections::HashMap;
use std::path::PathBuf;

// The connection type through `vcw-project`'s re-export rather than through a
// direct `rusqlite` dependency: ADR-0003 puts the database behind that crate,
// and an exporter that named the driver itself would be a second door to it.
use vcw_project::{Connection, pcm, release, side, track};
use vcw_types::Span;
use vcw_types::vinyl::{Numbering, Side};

use crate::encoder::{Container, Spec, Writer};
use crate::error::{Error, Result};
use crate::naming::{self, Values};
use crate::tagging::{self, Cover, Tags};

/// What to do with the release's front cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Artwork {
    /// Leave it out.
    None,
    /// Inside each file's tags.
    Embed,
    /// One image file beside the exported tracks, the way VRipr did it.
    Folder,
    /// Both, which is the default.
    ///
    /// VRipr wrote only the folder image, and players that read embedded art
    /// showed nothing. Writing both costs one copy of a JPEG per album and means
    /// the export looks right in a file manager *and* on a phone.
    #[default]
    Both,
}

impl Artwork {
    /// Whether a cover goes in the tags.
    #[must_use]
    pub const fn embeds(self) -> bool {
        matches!(self, Self::Embed | Self::Both)
    }

    /// Whether a cover goes beside the files.
    #[must_use]
    pub const fn beside(self) -> bool {
        matches!(self, Self::Folder | Self::Both)
    }
}

/// What an export was asked for.
#[derive(Debug, Clone)]
pub struct Request {
    /// The container to write.
    pub container: Container,
    /// The naming template, in [`naming`]'s token language.
    pub template: String,
    /// The directory everything goes under.
    pub into: PathBuf,
    /// The sides to export, or empty for every side that has tracks.
    pub sides: Vec<Side>,
    /// What to do with the cover.
    pub artwork: Artwork,
    /// Whether a file already there may be replaced.
    pub overwrite: bool,
}

impl Request {
    /// A request to export every side into a directory.
    #[must_use]
    pub fn new(into: impl Into<PathBuf>, container: Container) -> Self {
        Self {
            container,
            template: naming::DEFAULT_TEMPLATE.to_owned(),
            into: into.into(),
            sides: Vec::new(),
            artwork: Artwork::default(),
            overwrite: false,
        }
    }

    /// Whether a side was asked for.
    fn wants(&self, side: Side) -> bool {
        self.sides.is_empty() || self.sides.contains(&side)
    }
}

/// One file to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The track row behind it.
    pub track_id: i64,
    /// The side it is on.
    pub side: Side,
    /// Its number within the side.
    pub number: u32,
    /// The capture its audio comes from.
    pub capture_id: i64,
    /// The frames to cut, already clamped to what was committed.
    pub span: Span,
    /// Where it goes.
    pub path: PathBuf,
    /// What goes in its tags.
    pub tags: Tags,
}

impl Item {
    /// How many frames of audio this file holds.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.span.frames()
    }
}

/// Everything an export will do, before it does any of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The files, in side and track order.
    pub items: Vec<Item>,
    /// Cover images to write beside the tracks, one per directory.
    pub covers: Vec<PathBuf>,
    /// The container every item is written in.
    pub container: Container,
    /// The cover, where there is one and it is wanted.
    pub cover: Option<Cover>,
}

impl Plan {
    /// Frames of audio across every item, which is what a progress bar is over.
    #[must_use]
    pub fn frames(&self) -> u64 {
        self.items.iter().map(Item::frames).sum()
    }
}

/// How far an export has got.
///
/// Passed to the callback [`run`] takes, which is what §35's `export-progress`
/// event is built on. Frames rather than bytes because a FLAC file's size is not
/// known until it is written, and frames are what the plan counted.
#[derive(Debug, Clone, Copy)]
pub struct Progress<'a> {
    /// The item being written.
    pub item: &'a Item,
    /// Its index in the plan.
    pub index: usize,
    /// How many items there are.
    pub of: usize,
    /// Frames written of this item.
    pub frames: u64,
    /// Frames written across the whole plan.
    pub total: u64,
}

/// What an export produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Report {
    /// Audio files written.
    pub files: usize,
    /// Cover images written beside them.
    pub covers: usize,
    /// Frames of audio written.
    pub frames: u64,
    /// Bytes on disk, audio files only.
    pub bytes: u64,
}

/// Resolves a project into a list of files, writing nothing.
///
/// # Errors
///
/// If the template has an unknown token, if two tracks want the same file, if a
/// file is already there and `overwrite` is not set, if a requested side has no
/// tracks or no capture, or if the project cannot be read.
pub fn plan(conn: &Connection, request: &Request) -> Result<Plan> {
    // Before anything else, because a typo in a template is the one failure that
    // is certain to affect every file and costs nothing to find.
    let unknown = naming::validate(&request.template);
    if !unknown.is_empty() {
        return Err(Error::UnknownTokens {
            tokens: unknown.iter().map(ToString::to_string).collect(),
        });
    }

    let release = release::load(conn)?.unwrap_or_default();
    let positions = position_map(conn, release.numbering)?;
    let listing = track::listing(conn)?;
    if listing.is_empty() {
        return Err(Error::Nothing);
    }

    let cover = cover(conn, request)?;
    let mut items = Vec::new();
    // Path to the first track that claimed it, so a collision can name both.
    let mut claimed: HashMap<PathBuf, u32> = HashMap::new();
    let mut covers: Vec<PathBuf> = Vec::new();
    let mut captures: HashMap<i64, Option<i64>> = HashMap::new();

    for (side, record) in listing {
        if !request.wants(side) {
            continue;
        }
        let capture = match captures.entry(record.side_id) {
            std::collections::hash_map::Entry::Occupied(seen) => *seen.get(),
            std::collections::hash_map::Entry::Vacant(slot) => {
                *slot.insert(side::by_id(conn, record.side_id)?.and_then(|row| row.capture))
            }
        };
        let Some(capture_id) = capture else {
            return Err(Error::NoAudio {
                side: side.letter(),
            });
        };

        let position = positions.get(&record.id).cloned().unwrap_or_default();
        let values = values(&release, &record, side, &position);
        let relative = naming::path_for(&request.template, &values);
        // The extension is the container's and not the template's, so a naming
        // template works whichever format it is exported in. Appended rather
        // than set with `with_extension`, which would read `Symphony No. 5` as
        // a file called `Symphony No` with an extension of ` 5` and replace it.
        let mut path = request.into.join(&relative).into_os_string();
        path.push(".");
        path.push(request.container.extension());
        let path = PathBuf::from(path);

        if let Some(first) = claimed.insert(path.clone(), record.number) {
            return Err(Error::NameCollision {
                first,
                second: record.number,
                path,
            });
        }
        if !request.overwrite && path.exists() {
            return Err(Error::Exists { path });
        }

        // The span is clamped here as well as in `Reader::open`, so the plan's
        // frame count is the one that will actually be written: a detector can
        // put the last boundary past the end of a capture, and a progress bar
        // that never reaches its total is a progress bar nobody trusts.
        let layout = pcm::Layout::of(conn, capture_id)?;
        let span = Span::new(record.start, record.end).clamp_to(layout.frames);

        if let (Some(cover), Some(directory)) = (cover.as_ref(), path.parent())
            && request.artwork.beside()
        {
            let beside = directory.join(format!("folder.{}", extension(&cover.mime)));
            if !covers.contains(&beside) {
                covers.push(beside);
            }
        }

        items.push(Item {
            track_id: record.id,
            side,
            number: record.number,
            capture_id,
            span,
            path,
            tags: tags(&release, &record, side, &position, request, cover.as_ref()),
        });
    }

    if items.is_empty() {
        return Err(Error::NothingToExport {
            side: request.sides.first().map_or('?', |side| side.letter()),
        });
    }

    Ok(Plan {
        items,
        covers,
        container: request.container,
        cover,
    })
}

/// Writes a plan's files.
///
/// `on` is called at least once per item and at most once per read, which is
/// what feeds §35's `export-progress`.
///
/// # Errors
///
/// If a directory cannot be made, if a capture cannot be read, if a container
/// refuses the audio, or if a file cannot be written or tagged.
pub fn run(conn: &Connection, plan: &Plan, on: &mut dyn FnMut(Progress<'_>)) -> Result<Report> {
    let mut report = Report::default();
    let of = plan.items.len();

    for (index, item) in plan.items.iter().enumerate() {
        if let Some(directory) = item.path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        report.bytes += cut(conn, item, plan.container, &mut |frames| {
            on(Progress {
                item,
                index,
                of,
                frames,
                total: report.frames + frames,
            });
        })?;
        report.frames += item.frames();
        report.files += 1;

        tagging::write(&item.path, plan.container, &item.tags)?;
    }

    tracing::info!(
        files = report.files,
        frames = report.frames,
        bytes = report.bytes,
        container = ?plan.container,
        "wrote an export"
    );

    if let Some(cover) = &plan.cover {
        for beside in &plan.covers {
            if let Some(directory) = beside.parent() {
                std::fs::create_dir_all(directory)?;
            }
            std::fs::write(beside, &cover.bytes)?;
            report.covers += 1;
        }
    }

    Ok(report)
}

/// Plans an export and runs it.
///
/// # Errors
///
/// Anything [`plan`] or [`run`] can raise.
pub fn export(
    conn: &Connection,
    request: &Request,
    on: &mut dyn FnMut(Progress<'_>),
) -> Result<Report> {
    let plan = plan(conn, request)?;
    run(conn, &plan, on)
}

/// How much audio to move per read.
///
/// 64 KiB rounded down to a whole frame: big enough that the per-read overhead
/// is noise against a block decode, small enough that memory is flat whatever
/// the length of the track. It is deliberately not a block size - the reader
/// deals in frames and the writer deals in whatever it is handed.
const CHUNK_BYTES: usize = 64 * 1024;

/// Cuts one track out of a capture and returns the file's size.
fn cut(
    conn: &Connection,
    item: &Item,
    container: Container,
    on: &mut dyn FnMut(u64),
) -> Result<u64> {
    let mut reader = pcm::Reader::open(conn, item.capture_id, item.span)?;
    let layout = *reader.layout();
    let frame_bytes = layout.frame_bytes().max(1);
    let spec = Spec {
        rate: layout.rate.hz(),
        channels: layout.channels,
        format: layout.format,
        frames: reader.span().frames(),
    };

    let mut writer = Writer::create(&item.path, container, spec)?;
    let mut buffer = vec![0u8; (CHUNK_BYTES / frame_bytes).max(1) * frame_bytes];
    let mut frames = 0u64;
    loop {
        let read = reader.fill(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write(&buffer[..read])?;
        frames += (read / frame_bytes) as u64;
        on(frames);
    }
    writer.finish()
}

/// Track id to rendered position, so the naming template gets its `A1`.
fn position_map(conn: &Connection, numbering: Numbering) -> Result<HashMap<i64, String>> {
    Ok(track::positions(conn, numbering)?
        .into_iter()
        .map(|(record, position)| (record.id, position))
        .collect())
}

/// The front cover, if the project has one and the request wants it anywhere.
fn cover(conn: &Connection, request: &Request) -> Result<Option<Cover>> {
    if matches!(request.artwork, Artwork::None) {
        return Ok(None);
    }
    Ok(
        release::artwork(conn, release::Artwork::FRONT)?.map(|art| Cover {
            mime: art.mime,
            bytes: art.bytes,
        }),
    )
}

/// A file extension for an image, from what the project sniffed.
///
/// `jpg` for anything unrecognised, because that is what a cover downloaded from
/// a provider almost always is and a folder image with the wrong extension is
/// still shown by every file manager that looks at the bytes.
fn extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "jpg",
    }
}

/// A track and its release, as naming-template values.
fn values(release: &release::Record, record: &track::Record, side: Side, position: &str) -> Values {
    Values {
        title: record.title.clone(),
        artist: record.artist_or(&release.album_artist).to_owned(),
        album: release.album.clone(),
        album_artist: release.album_artist.clone(),
        genre: release.genre_list(),
        year: release
            .year
            .map(|year| year.to_string())
            .unwrap_or_default(),
        tracknum: record.number.to_string(),
        composer: record.composer_or(&release.composer).to_owned(),
        country: release.country.clone(),
        catalog: release.catalog.clone(),
        label: release.label.clone(),
        discogs_id: release.discogs_id.clone().unwrap_or_default(),
        side: side.letter().to_string(),
        position: position.to_owned(),
        disc: side.disc().to_string(),
    }
}

/// A track and its release, as tags.
fn tags(
    release: &release::Record,
    record: &track::Record,
    side: Side,
    position: &str,
    request: &Request,
    cover: Option<&Cover>,
) -> Tags {
    Tags {
        title: record.title.clone(),
        artist: record.artist_or(&release.album_artist).to_owned(),
        album: release.album.clone(),
        album_artist: release.album_artist.clone(),
        genre: release.genres.join(";"),
        year: release.year,
        track_number: Some(record.number),
        track_total: None,
        disc_number: Some(side.disc()),
        disc_total: Some(release.discs.max(1)),
        composer: record.composer_or(&release.composer).to_owned(),
        comment: record.comments.clone().unwrap_or_default(),
        country: release.country.clone(),
        label: release.label.clone(),
        catalog: release.catalog.clone(),
        barcode: release.barcode.clone().unwrap_or_default(),
        discogs_id: release.discogs_id.clone().unwrap_or_default(),
        musicbrainz_release_id: release.musicbrainz_id.clone().unwrap_or_default(),
        musicbrainz_recording_id: record.musicbrainz_id.clone().unwrap_or_default(),
        // The position on the record, which no tag standard names and which is
        // the one piece of provenance a vinyl rip has and a CD rip does not.
        extra: vec![("VINYL_POSITION".to_owned(), position.to_owned())],
        cover: if request.artwork.embeds() {
            cover.cloned()
        } else {
            None
        },
    }
}
