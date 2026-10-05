/*
 *  release.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The vcw release verb: the record the project is of (§28, §32).
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

//! The `vcw release` verb: the record the project is of (§28, §32).
//!
//! One release per project, because a project is one record. Identification fills
//! most of these fields in and this is the operator's own hand - the same rows
//! either way, which is the point of `vcw-project`'s release module not depending
//! on `vcw-metadata`: a release found on Discogs and one typed in at a terminal
//! are the same record.
//!
//! `set` is also what the new-project prompt writes. Artist, title and catalog
//! number are the three fields worth asking for up front, because they are what a
//! search needs and what automation cannot guess; `--mono` and `--riaa` are the
//! other two, and they are asked for a stronger reason - no search and no analysis
//! can answer them at all. A mono pressing transferred with a stereo cartridge
//! gives two channels that are nearly identical and never exactly so, and the
//! equalization a record was cut with leaves no trace in the audio it was applied
//! to. Both are stated or they are nothing.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vcw_project::release::{self, Artwork, ROLES};
use vcw_project::{Project, disc, side};
use vcw_types::vinyl::Numbering;

/// What `vcw release` was asked to do.
#[derive(Debug, Clone)]
pub(crate) enum Task {
    /// Print what the project knows.
    Show,
    /// Change one or more fields.
    Set(Box<Change>),
    /// Store a cover image.
    Artwork {
        /// The image file.
        file: PathBuf,
        /// Which image it is.
        role: String,
    },
    /// Print the artwork index.
    Covers,
}

/// The fields `vcw release set` was given, each `None` for "leave it".
#[derive(Debug, Clone, Default)]
pub(crate) struct Change {
    /// The album title.
    pub(crate) album: Option<String>,
    /// The album artist.
    pub(crate) artist: Option<String>,
    /// The year of this pressing.
    pub(crate) year: Option<u32>,
    /// Genres, separated by semicolons.
    pub(crate) genres: Option<String>,
    /// The label.
    pub(crate) label: Option<String>,
    /// The catalog number.
    pub(crate) catalog: Option<String>,
    /// The country of pressing.
    pub(crate) country: Option<String>,
    /// The barcode.
    pub(crate) barcode: Option<String>,
    /// The composer.
    pub(crate) composer: Option<String>,
    /// Free text.
    pub(crate) comments: Option<String>,
    /// How many discs the record is.
    pub(crate) discs: Option<u32>,
    /// The numbering scheme.
    pub(crate) numbering: Option<String>,
    /// Whether to mark the metadata accepted.
    pub(crate) confirm: bool,
    /// Whether this is a mono pressing.
    pub(crate) mono: Option<bool>,
    /// Whether the RIAA curve is to be applied on playback and export.
    pub(crate) riaa: Option<bool>,
}

/// What `vcw release` was asked to do, and to which project.
#[derive(Debug, Clone)]
pub(crate) struct Args {
    /// The project.
    pub(crate) project: PathBuf,
    /// The task.
    pub(crate) task: Task,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Runs one `vcw release` subcommand.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }
    let mut project = Project::open(&args.project)
        .with_context(|| format!("opening {}", args.project.display()))?;

    match &args.task {
        Task::Show => show(&mut project, args.json)?,
        Task::Set(change) => {
            let mut record = release::ensure(&mut project)?;
            apply(&mut record, change)?;
            release::store(&mut project, &record)?;
            show(&mut project, args.json)?;
        }
        Task::Artwork { file, role } => {
            if !ROLES.contains(&role.as_str()) {
                bail!(
                    "{role:?} is not an artwork role - one of {}",
                    ROLES.join(", ")
                );
            }
            let bytes =
                std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
            let mime = mime_of(file, &bytes)
                .ok_or_else(|| anyhow::anyhow!("{} is not a PNG, JPEG or WebP", file.display()))?;
            let id = release::put_artwork(&mut project, role, mime, &bytes, None)?;
            println!(
                "artwork {id} stored as {role}: {mime}, {} bytes",
                bytes.len()
            );
        }
        Task::Covers => covers(&project, args.json)?,
    }
    project.close()?;
    Ok(())
}

fn show(project: &mut Project, json: bool) -> Result<()> {
    let record = release::ensure(project)?;
    let sides = side::list(project.conn())?;
    let recorded = disc::recorded(project.conn())?;
    let missing = disc::missing(project.conn())?;

    if json {
        println!(
            "{{\"album\":{},\"artist\":{},\"year\":{},\"genres\":{},\"label\":{},\
             \"catalog\":{},\"country\":{},\"barcode\":{},\"composer\":{},\
             \"discs\":{},\"discs_recorded\":{},\"numbering\":\"{}\",\
             \"sides\":{},\"confirmed\":{},\"mono\":{},\"riaa_eq\":{},\"empty\":{}}}",
            quote(&record.album),
            quote(&record.album_artist),
            record
                .year
                .map_or_else(|| "null".to_string(), |y| y.to_string()),
            quote(&record.genre_list()),
            quote(&record.label),
            quote(&record.catalog),
            quote(&record.country),
            record
                .barcode
                .as_deref()
                .map_or_else(|| "null".to_string(), quote),
            quote(&record.composer),
            record.discs,
            recorded,
            numbering_name(record.numbering),
            sides.len(),
            record.confirmed,
            record.is_mono,
            record.riaa_eq,
            record.is_empty()
        );
        return Ok(());
    }

    if record.is_empty() {
        println!("nothing known yet - `vcw release set --artist ... --album ...` is the start");
    }
    field("album", &record.album);
    field("artist", &record.album_artist);
    if let Some(year) = record.year {
        field("year", &year.to_string());
    }
    field("genres", &record.genre_list());
    field("label", &record.label);
    field("catalog", &record.catalog);
    field("country", &record.country);
    if let Some(barcode) = &record.barcode {
        field("barcode", barcode);
    }
    field("composer", &record.composer);
    field("comments", &record.comments);
    if let Some(id) = &record.musicbrainz_id {
        field("musicbrainz", id);
    }
    if let Some(id) = &record.discogs_id {
        field("discogs", id);
    }
    println!(
        "  {:<12} {} claimed, {recorded} with audio, {} side(s) present",
        "discs",
        record.discs,
        sides.len()
    );
    field("numbering", numbering_name(record.numbering));
    field("mono", if record.is_mono { "yes" } else { "no" });
    field("riaa eq", if record.riaa_eq { "yes" } else { "no" });
    field("confirmed", if record.confirmed { "yes" } else { "no" });
    if !missing.is_empty() {
        let letters: String = missing.iter().map(|s| s.letter()).collect();
        field("still to do", &letters);
    }
    Ok(())
}

fn covers(project: &Project, json: bool) -> Result<()> {
    let index = release::artwork_index(project.conn())?;
    if json {
        let mut entries: Vec<String> = Vec::with_capacity(index.len());
        for art in &index {
            entries.push(format!(
                "{{\"id\":{},\"role\":{},\"mime\":{},\"width\":{},\"height\":{},\"bytes\":{}}}",
                art.id,
                quote(&art.role),
                quote(&art.mime),
                art.width
                    .map_or_else(|| "null".to_string(), |w| w.to_string()),
                art.height
                    .map_or_else(|| "null".to_string(), |h| h.to_string()),
                size_of(project, art)?
            ));
        }
        println!("{{\"artwork\":[{}]}}", entries.join(","));
        return Ok(());
    }
    if index.is_empty() {
        println!("no artwork - `vcw release artwork <file>` stores a cover");
        return Ok(());
    }
    for art in &index {
        println!(
            "  {:<8} {:<12} {} bytes{}",
            art.role,
            art.mime,
            size_of(project, art)?,
            match (art.width, art.height) {
                (Some(w), Some(h)) => format!(", {w}x{h}"),
                _ => String::new(),
            }
        );
    }
    Ok(())
}

/// The stored size of one image.
///
/// Asked for separately because [`release::artwork_index`] deliberately leaves the
/// bytes behind: a four-disc set's covers are megabytes, and listing what is there
/// should not load them.
fn size_of(project: &Project, art: &Artwork) -> Result<u64> {
    Ok(release::artwork_bytes(project.conn(), &art.role)?.unwrap_or(0))
}

/// Applies the fields that were given.
fn apply(record: &mut release::Record, change: &Change) -> Result<()> {
    if let Some(album) = &change.album {
        record.album = album.clone();
    }
    if let Some(artist) = &change.artist {
        record.album_artist = artist.clone();
    }
    if let Some(year) = change.year {
        record.year = Some(year);
    }
    if let Some(genres) = &change.genres {
        record.genres = genres
            .split(';')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_owned)
            .collect();
    }
    if let Some(label) = &change.label {
        record.label = label.clone();
    }
    if let Some(catalog) = &change.catalog {
        record.catalog = catalog.clone();
    }
    if let Some(country) = &change.country {
        record.country = country.clone();
    }
    if let Some(barcode) = &change.barcode {
        record.barcode = (!barcode.is_empty()).then(|| barcode.clone());
    }
    if let Some(composer) = &change.composer {
        record.composer = composer.clone();
    }
    if let Some(comments) = &change.comments {
        record.comments = comments.clone();
    }
    if let Some(discs) = change.discs {
        if discs == 0 {
            bail!("a record has at least one disc");
        }
        record.discs = discs;
    }
    if let Some(mono) = change.mono {
        record.is_mono = mono;
    }
    if let Some(riaa) = change.riaa {
        record.riaa_eq = riaa;
    }
    if let Some(numbering) = &change.numbering {
        record.numbering = match numbering.as_str() {
            "alpha" => Numbering::Alpha,
            "numeric" => Numbering::Numeric,
            other => bail!("{other:?} is not a numbering scheme - alpha or numeric"),
        };
    }
    if change.confirm {
        record.confirmed = true;
    }
    Ok(())
}

fn field(name: &str, value: &str) {
    if !value.is_empty() {
        println!("  {name:<12} {value}");
    }
}

const fn numbering_name(numbering: Numbering) -> &'static str {
    match numbering {
        Numbering::Alpha => "alpha",
        Numbering::Numeric => "numeric",
    }
}

/// The image type, from the file's own first bytes.
///
/// Read from the content rather than the extension because the extension is the
/// operator's guess and the magic number is the file's own answer - and because a
/// tag writer that declares the wrong MIME type produces a file players will not
/// show a cover for (§33).
fn mime_of(path: &std::path::Path, bytes: &[u8]) -> Option<&'static str> {
    let _ = path;
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// A JSON string, escaped enough for the fields a release holds.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
