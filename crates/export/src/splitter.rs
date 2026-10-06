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
use vcw_types::vinyl::{Numbering, Side};
use vcw_types::{Span, StorageFormat};

use crate::encoder::{Container, Dither, Narrowing, Spec, Writer, narrowed};
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
    /// What a `Float32` capture becomes for a container that cannot take one.
    ///
    /// Defaults to [`crate::encoder::Width::Refuse`], which is the behavior VCW
    /// had before this field existed: a float capture and a FLAC request is a
    /// refusal naming the containers that would have taken it.
    pub narrowing: Narrowing,
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
            narrowing: Narrowing::default(),
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
///
/// `PartialEq` and not `Eq`, since [`Narrowing::headroom_db`] is a float. Two
/// plans comparing equal is a test's question, and no map is keyed on one.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The files, in side and track order.
    pub items: Vec<Item>,
    /// Cover images to write beside the tracks, one per directory.
    pub covers: Vec<PathBuf>,
    /// The container every item is written in.
    pub container: Container,
    /// Whether every item's channels are summed to one on the way out.
    ///
    /// The release's `is_mono`, taken once at plan time: it is a property of the
    /// pressing, not of a track, and an export that folded some files and not
    /// others would be an album nobody could play.
    pub fold_to_mono: bool,
    /// The cover, where there is one and it is wanted.
    pub cover: Option<Cover>,
    /// The narrowing the request asked for, carried so that [`run`] writes the
    /// files [`plan`] vetted and not whatever the default was.
    pub narrowing: Narrowing,
    /// The format float samples will actually land in, where any will.
    ///
    /// Not the same question as [`narrowing`](Self::narrowing), which is what
    /// was *asked* for: a request may ask for 24 bits and change nothing,
    /// because the capture is already integers or because the container takes
    /// the float as it is. A dry run has to be able to tell those apart - a
    /// line predicting a narrowing that will not happen is worse than no line -
    /// so this is set only when a file really will be rounded.
    pub narrowed_to: Option<StorageFormat>,
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
    ///
    /// Measured after tagging, so it includes the tags and any embedded cover -
    /// which is what the files actually occupy. The cover images written
    /// *beside* the files are not in here; [`Report::covers`] counts those.
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
    let numbers = numbers(conn, release.numbering)?;
    let listing = track::listing(conn)?;
    if listing.is_empty() {
        return Err(Error::Nothing);
    }

    let cover = cover(conn, request)?;
    let mut items = Vec::new();
    // Path to the first track that claimed it, so a collision can name both.
    let mut claimed: HashMap<PathBuf, String> = HashMap::new();
    let mut covers: Vec<PathBuf> = Vec::new();
    let mut narrowed_to = None;
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

        let numbers = numbers.get(&record.id).cloned().unwrap_or_default();
        let values = values(&release, &record, side, &numbers);
        let relative = naming::path_for(&request.template, &values);
        // The extension is the container's and not the template's, so a naming
        // template works whichever format it is exported in. Appended rather
        // than set with `with_extension`, which would read `Symphony No. 5` as
        // a file called `Symphony No` with an extension of ` 5` and replace it.
        let mut path = request.into.join(&relative).into_os_string();
        path.push(".");
        path.push(request.container.extension());
        let path = PathBuf::from(path);

        // The positions, not the numbers within the sides: `tracks 2 and 2`
        // named neither track and was the first thing a real project said.
        if let Some(first) = claimed.insert(path.clone(), numbers.alpha.clone()) {
            return Err(Error::NameCollision {
                first,
                second: numbers.alpha,
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

        // The container is asked here, where the refusal costs nothing, rather
        // than left to the first `Writer::create`. The spec is the one `cut`
        // will build from the same layout and the same clamped span, so a plan
        // that resolves is a plan that can be written: a dry run that printed
        // `3 file(s) in FLAC` for a float32 capture and then died on file one
        // was a plan nobody could trust.
        //
        // Vetted *after* any narrowing, because narrowing is what decides
        // whether a float capture has a FLAC path at all: `narrowed` is a no-op
        // unless the container refuses the capture as it stands, so a request
        // that asked for nothing is vetted exactly as it always was.
        let stored = Spec {
            rate: layout.rate.hz(),
            channels: if release.is_mono { 1 } else { layout.channels },
            format: layout.format,
            frames: span.frames(),
        };
        let out = narrowed(request.container, request.narrowing, stored);
        if out.format != stored.format {
            narrowed_to = Some(out.format);
        }
        Writer::vet(request.container, &out)?;

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
            tags: tags(&release, &record, side, &numbers, request, cover.as_ref()),
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
        fold_to_mono: release.is_mono,
        cover,
        narrowing: request.narrowing,
        narrowed_to,
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
        cut(
            conn,
            item,
            plan.container,
            plan.fold_to_mono,
            plan.narrowing,
            &mut |frames| {
                on(Progress {
                    item,
                    index,
                    of,
                    frames,
                    total: report.frames + frames,
                });
            },
        )?;
        report.frames += item.frames();
        report.files += 1;

        tagging::write(&item.path, plan.container, &item.tags)?;

        // Counted after the tagger and from the filesystem, not from what the
        // writer said it wrote. Tags are not free and an embedded cover is not
        // close to free: a 4 MB sleeve scan across a ten-track side is 40 MB
        // that `Writer::finish` has no way of knowing about, because it
        // returned before the tagger opened the file. Taking the writer's
        // number made every export under-report its own size, which is the one
        // number a person checks against the free space they just used up.
        report.bytes += std::fs::metadata(&item.path)?.len();
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
    fold_to_mono: bool,
    narrowing: Narrowing,
    on: &mut dyn FnMut(u64),
) -> Result<u64> {
    let mut reader = pcm::Reader::open(conn, item.capture_id, item.span)?;
    let layout = *reader.layout();
    let frame_bytes = layout.frame_bytes().max(1);
    // One channel out whatever went in, and the same frame count: a fold
    // changes the width of a frame and not how many there are.
    let folding = fold_to_mono && layout.channels > 1;
    let stored = Spec {
        rate: layout.rate.hz(),
        channels: if folding { 1 } else { layout.channels },
        format: layout.format,
        frames: reader.span().frames(),
    };
    // The same question `plan` asked, asked again from the same layout: a file
    // written here is the file that was vetted there.
    let spec = narrowed(container, narrowing, stored);
    let narrowing = (spec.format != stored.format).then_some(narrowing);

    let mut writer = Writer::create(&item.path, container, spec)?;
    let mut buffer = vec![0u8; (CHUNK_BYTES / frame_bytes).max(1) * frame_bytes];
    let mut folded = Vec::new();
    let mut rounded = Vec::new();
    // Per file and from a constant, which is what keeps a dithered export
    // reproducible - see `Narrowing`. A counter would do; a xorshift state has
    // to be odd and non-zero, and this one is the 64-bit constant the algorithm
    // is usually published with.
    let mut noise = 0x2545_F491_4F6C_DD1Du64;
    let mut frames = 0u64;
    loop {
        let read = reader.fill(&mut buffer)?;
        if read == 0 {
            break;
        }
        let mut chunk: &[u8] = &buffer[..read];
        if folding {
            fold(layout.format, layout.channels, chunk, &mut folded);
            chunk = &folded;
        }
        if let Some(narrowing) = narrowing {
            narrow(spec.format, narrowing, chunk, &mut rounded, &mut noise);
            chunk = &rounded;
        }
        writer.write(chunk)?;
        frames += (read / frame_bytes) as u64;
        on(frames);
    }
    writer.finish()
}

/// Rounds `Float32` samples to integers at the width the request asked for.
///
/// The arithmetic is in `f64` throughout rather than in the `f32` it came from,
/// because a 32-bit target multiplies by 2^31 and an `f32` cannot hold the
/// result to the nearest integer - rounding in `f32` would quantize twice and
/// lose the bottom bits of every sample on the way to a wider container than
/// the one it started in.
///
/// Clamping is not optional and is not a setting. Nothing clips in floating
/// point, so a float capture may hold samples past +/-1.0 and a cast that
/// wrapped them would turn a loud passage into a full-scale inversion.
/// [`Narrowing::headroom_db`] is the knob for avoiding the clamp; this is what
/// happens when nobody turned it.
fn narrow(
    to: StorageFormat,
    narrowing: Narrowing,
    src: &[u8],
    dst: &mut Vec<u8>,
    noise: &mut u64,
) -> usize {
    let full = narrowing.to.full_scale().unwrap_or(1 << 31);
    let scale = narrowing.gain() * full as f64;
    let (ceiling, floor) = (full - 1, -full);
    dst.clear();
    let (samples, _) = src.as_chunks::<4>();
    for bytes in samples {
        let mut value = f64::from(f32::from_le_bytes(*bytes)) * scale;
        if narrowing.dither == Dither::Tpdf {
            value += tpdf(noise);
        }
        // `as i64` saturates on an out-of-range float and maps NaN to 0, so the
        // clamp is about the signal and not about the cast.
        store(to, (value.round() as i64).clamp(floor, ceiling), dst);
    }
    dst.len()
}

/// One triangular dither sample, in the +/-1 LSB the name promises.
///
/// Two independent uniforms over +/-1/2 LSB, summed. That is the whole of TPDF:
/// the sum of two rectangular distributions is a triangular one, and a
/// triangular dither of exactly this width is what makes the quantization error
/// independent of the signal instead of a function of it.
///
/// xorshift64 and not a dependency: the requirement on this generator is that
/// it be white enough to dither with and the same every run, and sixty years of
/// audio were dithered with less.
fn tpdf(state: &mut u64) -> f64 {
    let mut next = || {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        // 53 bits, which is every bit an f64 mantissa holds, mapped to -1/2..1/2.
        (*state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    next() + next()
}

/// What a track is called, in the three forms an export needs.
///
/// `track::Record::number` is none of them. It is the number *within its side*
/// (§29), so on any two-sided record side B's track 2 and side A's track 2 are
/// both `2` - and using it for the naming template made every side-B track
/// collide with a side-A track the moment two of them were untitled, while
/// using it for the tag put two tracks numbered 2 on one album. Both were real:
/// the first export of a real two-sided project refused with `tracks 2 and 2
/// both export to .../02 -.flac`.
#[derive(Debug, Clone)]
struct Numbers {
    /// The release's chosen form, `A1` or `6`: the template's `{tracknum}`.
    ///
    /// VRipr's `{tracknum}` was this, and §29 keeps VRipr's numbering, so the
    /// default template writes `A1 - The Rainbow` exactly as VRipr did.
    rendered: String,
    /// The alpha position, whatever the numbering: `{position}`, and the
    /// `VINYL_POSITION` tag, which is provenance and not presentation.
    alpha: String,
    /// One-based within its disc, counting across that disc's sides.
    ///
    /// What a tag's track number means: unique within a disc, restarting on the
    /// next one, which is why this is not the release-wide sequence.
    within_disc: u32,
    /// Tracks on this track's disc, which is the track number's denominator.
    ///
    /// Counted over the whole project rather than over the sides that were
    /// asked for: `6` of a record is `6 of 8` whether or not side C was in the
    /// same export, and a file whose tags depended on what else was exported
    /// beside it would be a file that could not be re-exported.
    on_disc: u32,
}

impl Default for Numbers {
    /// What an unnumbered track gets, which nothing in a loaded project is.
    fn default() -> Self {
        Self {
            rendered: String::new(),
            alpha: String::new(),
            within_disc: 0,
            on_disc: 0,
        }
    }
}

/// Track id to the three numbers, in listing order.
///
/// The release-wide `sequence` is the same rule `track::positions` applies, and
/// for `Numbering::Alpha` - the default - it is not used at all.
fn numbers(conn: &Connection, numbering: Numbering) -> Result<HashMap<i64, Numbers>> {
    let mut map = HashMap::new();
    let mut sequence = 0;
    let mut per_disc: HashMap<u32, u32> = HashMap::new();
    let mut discs: Vec<(i64, u32)> = Vec::new();
    for (side, record) in track::listing(conn)? {
        sequence += 1;
        let within_disc = per_disc
            .entry(side.disc())
            .and_modify(|n| *n += 1)
            .or_insert(1);
        let position = record.position(side);
        discs.push((record.id, side.disc()));
        map.insert(
            record.id,
            Numbers {
                rendered: numbering.render(position, sequence),
                alpha: position.alpha(),
                within_disc: *within_disc,
                on_disc: 0,
            },
        );
    }

    // A second pass, because a disc's total is only known once its last side has
    // been counted and the first track on it was numbered long before that.
    for (id, disc) in discs {
        if let (Some(numbers), Some(&total)) = (map.get_mut(&id), per_disc.get(&disc)) {
            numbers.on_disc = total;
        }
    }
    Ok(map)
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
/// `jpg` for anything unrecognized, because that is what a cover downloaded from
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
fn values(
    release: &release::Record,
    record: &track::Record,
    side: Side,
    numbers: &Numbers,
) -> Values {
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
        tracknum: numbers.rendered.clone(),
        composer: record.composer_or(&release.composer).to_owned(),
        country: release.country.clone(),
        catalog: release.catalog.clone(),
        label: release.label.clone(),
        discogs_id: release.discogs_id.clone().unwrap_or_default(),
        side: side.letter().to_string(),
        position: numbers.alpha.clone(),
        disc: side.disc().to_string(),
    }
}

/// A track and its release, as tags.
fn tags(
    release: &release::Record,
    record: &track::Record,
    side: Side,
    numbers: &Numbers,
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
        track_number: Some(numbers.within_disc),
        // `None` rather than `Some(0)` for a track no numbering reached, which
        // is nothing in a loaded project: `Tags` treats an absent field as *do
        // not write it*, and `6 of 0` is worse than `6`.
        track_total: (numbers.on_disc > 0).then_some(numbers.on_disc),
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
        extra: vec![("VINYL_POSITION".to_owned(), numbers.alpha.clone())],
        cover: if request.artwork.embeds() {
            cover.cloned()
        } else {
            None
        },
    }
}

/// Sums a capture's channels into one, at `1/channels` gain.
///
/// The operator stated the pressing is mono at setup ([`release::Record::is_mono`])
/// and §33 makes that an export-time decision: the capture stays stereo, because
/// the two channels a stereo cartridge gets out of a mono groove are near
/// identical and never exactly so, and discarding one on the way in cannot be
/// undone.
///
/// The gain is why this cannot clip. Summing two full-scale channels needs a bit
/// that is not there; attenuating each by `1/n` first and then summing is the
/// same arithmetic as averaging, and an average of values in a range is in that
/// range. For two channels `1/n` is -6.02 dB, which is the number the request
/// was written in.
///
/// Integer formats are summed as `i64` and divided once, so the only rounding is
/// the final truncation - and that truncates toward zero, so it adds no DC. The
/// float path stays in `f32` and is not clamped: a float project may legitimately
/// hold samples past full scale, and quietly limiting them here would be an edit.
///
/// `src` must be a whole number of frames. Returns the bytes written to `dst`.
fn fold(format: StorageFormat, channels: u16, src: &[u8], dst: &mut Vec<u8>) -> usize {
    let width = format.bytes_per_sample();
    let n = channels as usize;
    dst.clear();
    if n <= 1 {
        dst.extend_from_slice(src);
        return src.len();
    }
    for frame in src.chunks_exact(width * n) {
        match format {
            StorageFormat::Float32 => {
                let (samples, _) = frame.as_chunks::<4>();
                let sum: f32 = samples.iter().copied().map(f32::from_le_bytes).sum();
                dst.extend_from_slice(&(sum / n as f32).to_le_bytes());
            }
            _ => {
                let sum: i64 = frame.chunks_exact(width).map(|s| sample(format, s)).sum();
                store(format, sum / n as i64, dst);
            }
        }
    }
    dst.len()
}

/// One stored integer sample, at its own scale rather than left-justified.
///
/// Its own scale is what lets the sum be divided and stored back without a
/// second shift, which is what keeps the fold exact.
fn sample(format: StorageFormat, bytes: &[u8]) -> i64 {
    match format {
        StorageFormat::Int16 => i64::from(i16::from_le_bytes([bytes[0], bytes[1]])),
        // Sign-extend 24 bits by landing them in the top of a word and shifting
        // back down - the same arithmetic `StorageFormat::decode_sample` does.
        StorageFormat::Int24Packed => {
            i64::from(i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8)
        }
        StorageFormat::Int24Padded | StorageFormat::Int32 => {
            i64::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        // Handled by the caller, which never reaches here.
        StorageFormat::Float32 => 0,
    }
}

/// Appends one integer sample in the stored format.
fn store(format: StorageFormat, value: i64, dst: &mut Vec<u8>) {
    let narrowed = value as i32;
    match format {
        StorageFormat::Int16 => dst.extend_from_slice(&(narrowed as i16).to_le_bytes()),
        StorageFormat::Int24Packed => dst.extend_from_slice(&narrowed.to_le_bytes()[0..3]),
        StorageFormat::Int24Padded | StorageFormat::Int32 => {
            dst.extend_from_slice(&narrowed.to_le_bytes());
        }
        StorageFormat::Float32 => unreachable!("the float path does not go through store"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::Width;

    /// One frame, as stored, for a format and a list of channel values.
    fn frame(format: StorageFormat, values: &[i64]) -> Vec<u8> {
        let mut out = Vec::new();
        for &v in values {
            store(format, v, &mut out);
        }
        out
    }

    /// `src` as interleaved little-endian `f32` bytes.
    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// The integer samples `narrow` produced, at `to`'s width.
    fn narrowed_samples(to: StorageFormat, narrowing: Narrowing, values: &[f32]) -> Vec<i64> {
        let mut out = Vec::new();
        let mut noise = 0x2545_F491_4F6C_DD1Du64;
        narrow(to, narrowing, &floats(values), &mut out, &mut noise);
        let width = to.bytes_per_sample();
        out.chunks_exact(width)
            .map(|bytes| sample(to, bytes))
            .collect()
    }

    /// Narrowing with no dither and no headroom, which is pure rounding.
    fn plain(to: Width) -> Narrowing {
        Narrowing {
            to,
            dither: Dither::None,
            headroom_db: 0.0,
        }
    }

    #[test]
    fn rounding_a_float_lands_on_the_scale_the_width_defines() {
        // Full scale is the width's, and +1.0 is one step past the top of a
        // two's-complement range - which is the clamp doing its job and not an
        // off-by-one. -1.0 is exactly representable and must not clamp.
        assert_eq!(
            narrowed_samples(
                StorageFormat::Int24Packed,
                plain(Width::Bits24),
                &[0.0, 0.5, -0.5, 1.0, -1.0]
            ),
            vec![0, 4_194_304, -4_194_304, 8_388_607, -8_388_608]
        );
        assert_eq!(
            narrowed_samples(
                StorageFormat::Int32,
                plain(Width::Bits32),
                &[0.0, 0.5, -1.0]
            ),
            vec![0, 1_073_741_824, -2_147_483_648]
        );
    }

    #[test]
    fn a_float_past_full_scale_is_clamped_and_not_wrapped() {
        // The failure this exists for: a cast that wrapped would turn the
        // loudest moment of a record into a full-scale inversion, which is the
        // single worst thing an exporter can do quietly. Floats do not clip, so
        // these samples are ordinary in a `f32` capture.
        let got = narrowed_samples(
            StorageFormat::Int24Packed,
            plain(Width::Bits24),
            &[1.4, -1.4, f32::INFINITY, f32::NEG_INFINITY, f32::NAN],
        );
        assert_eq!(
            got,
            vec![8_388_607, -8_388_608, 8_388_607, -8_388_608, 0],
            "a float past the ceiling must clamp to it"
        );
    }

    #[test]
    fn headroom_is_what_keeps_a_hot_capture_off_the_ceiling() {
        let with_room = Narrowing {
            headroom_db: 6.0,
            ..plain(Width::Bits24)
        };
        // The same sample, once clamped and once not: 0.9 at -6 dB is 0.451 of
        // full scale, and nothing is lost.
        assert_eq!(
            narrowed_samples(StorageFormat::Int24Packed, plain(Width::Bits24), &[1.2]),
            vec![8_388_607],
            "without headroom this clamps"
        );
        let [quieter] = narrowed_samples(StorageFormat::Int24Packed, with_room, &[1.2])[..] else {
            panic!("one sample in, one sample out")
        };
        assert!(
            quieter < 8_388_607,
            "headroom did not bring the sample under the ceiling: {quieter}"
        );
    }

    #[test]
    fn dither_is_noise_and_is_the_same_noise_every_time() {
        // A ramp well inside full scale, so nothing here is about clamping.
        let values: Vec<f32> = (0..64).map(|n| f64::from(n) as f32 / 1000.0).collect();
        let dithered = Narrowing {
            dither: Dither::Tpdf,
            ..plain(Width::Bits24)
        };
        let clean = narrowed_samples(StorageFormat::Int24Packed, plain(Width::Bits24), &values);
        let noisy = narrowed_samples(StorageFormat::Int24Packed, dithered, &values);

        assert_ne!(clean, noisy, "the dither added nothing");
        // Within a step and a bit of where rounding alone put them: TPDF is
        // +/-1 LSB, so this is the whole claim about how loud the noise is.
        for (a, b) in clean.iter().zip(&noisy) {
            assert!((a - b).abs() <= 2, "dither moved a sample by {}", a - b);
        }
        // §33 says an export is reproducible from the blocks and the edit
        // instructions. A random dither would quietly make that false, so the
        // generator is seeded per file from a constant and this is the test
        // that says so.
        assert_eq!(
            noisy,
            narrowed_samples(StorageFormat::Int24Packed, dithered, &values),
            "the same export gave two different files"
        );
    }

    #[test]
    fn the_fold_is_the_average_in_every_integer_format() {
        // Deliberately asymmetric: a fold that dropped a channel, or summed
        // without the gain, gives a different answer for every one of these.
        let cases: &[(StorageFormat, i64, i64, i64)] = &[
            (StorageFormat::Int16, 1000, -200, 400),
            (StorageFormat::Int16, 32767, 32767, 32767),
            (StorageFormat::Int16, -32768, -32768, -32768),
            (StorageFormat::Int24Packed, 8_388_607, 8_388_607, 8_388_607),
            (StorageFormat::Int24Packed, -8_388_608, 0, -4_194_304),
            (StorageFormat::Int24Padded, 123_456, -123_456, 0),
            (
                StorageFormat::Int32,
                i32::MAX as i64,
                i32::MAX as i64,
                i32::MAX as i64,
            ),
            (
                StorageFormat::Int32,
                i32::MIN as i64,
                i32::MIN as i64,
                i32::MIN as i64,
            ),
        ];
        let mut dst = Vec::new();
        for &(format, left, right, want) in cases {
            let src = frame(format, &[left, right]);
            let wrote = fold(format, 2, &src, &mut dst);
            assert_eq!(wrote, format.bytes_per_sample(), "{format:?}");
            assert_eq!(
                sample(format, &dst),
                want,
                "{format:?}: ({left} + {right}) / 2"
            );
        }
    }

    #[test]
    fn two_full_scale_channels_cannot_clip() {
        // The whole reason for the gain. Summing first would wrap or saturate.
        let mut dst = Vec::new();
        for format in [
            StorageFormat::Int16,
            StorageFormat::Int24Packed,
            StorageFormat::Int24Padded,
            StorageFormat::Int32,
        ] {
            let full = match format {
                StorageFormat::Int16 => 32767,
                StorageFormat::Int24Packed | StorageFormat::Int24Padded => 8_388_607,
                _ => i64::from(i32::MAX),
            };
            fold(format, 2, &frame(format, &[full, full]), &mut dst);
            assert_eq!(sample(format, &dst), full, "{format:?} clipped");
        }
    }

    #[test]
    fn the_float_fold_averages_and_does_not_clamp() {
        let mut src = Vec::new();
        src.extend_from_slice(&1.5f32.to_le_bytes());
        src.extend_from_slice(&0.5f32.to_le_bytes());
        let mut dst = Vec::new();
        assert_eq!(fold(StorageFormat::Float32, 2, &src, &mut dst), 4);
        let got = f32::from_le_bytes([dst[0], dst[1], dst[2], dst[3]]);
        // Past full scale on the way in and past it on the way out: clamping
        // here would be an edit, and §33 says export does not edit.
        assert!((got - 1.0).abs() < 1e-6, "{got}");
    }

    #[test]
    fn a_mono_capture_is_copied_rather_than_folded() {
        let src = frame(StorageFormat::Int16, &[1234, -4321]);
        let mut dst = Vec::new();
        assert_eq!(fold(StorageFormat::Int16, 1, &src, &mut dst), src.len());
        assert_eq!(dst, src);
    }

    #[test]
    fn four_channels_fold_at_a_quarter() {
        // §8 allows more than two, and `1/n` is the rule rather than `1/2`.
        let src = frame(StorageFormat::Int32, &[400, 300, 200, 100]);
        let mut dst = Vec::new();
        fold(StorageFormat::Int32, 4, &src, &mut dst);
        assert_eq!(sample(StorageFormat::Int32, &dst), 250);
    }
}
