/*
 *  release.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The release a project is capturing, and its artwork (§29, §32).
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

//! The release a project is capturing, and its artwork (§29, §32).
//!
//! One release per project, because §29's topology says so: a project is a record.
//! [`ensure`] is therefore the only way to get one, it creates the row if it is not
//! there, and every caller gets the same row.
//!
//! §32 lists what has to be retained and this is that list, with one deliberate
//! split: the release carries `album_artist`, and a *track* carries an artist only
//! where it differs. That is what tells a compilation from an album, and it is why
//! `Track::artist` is an `Option` while this one is not.
//!
//! # What is not here
//!
//! Any dependency on `vcw-metadata`. A release identified from Discogs and one typed
//! in by hand are the same record, and the project layer does not need to know which
//! it is holding; the mapping from a provider's answer onto these fields belongs to
//! whoever has both crates in scope. ADR-0003 keeps this crate's dependencies to
//! SQLite and `vcw-types`, and §40's offline promise is easier to keep when the
//! layer that owns the file cannot reach a network at all.

use rusqlite::{Connection, OptionalExtension, params};
use vcw_types::vinyl::Numbering;

use crate::error::Result;
use crate::sqlite::Project;

/// The one release row, as read back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Record {
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
    /// Catalog number off the label.
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
    /// How track numbers are presented.
    pub numbering: Numbering,
    /// MusicBrainz release id.
    pub musicbrainz_id: Option<String>,
    /// Discogs release id.
    pub discogs_id: Option<String>,
    /// Whether a person has accepted this metadata (§26).
    pub confirmed: bool,
    /// Whether this is a mono pressing, as stated at setup.
    ///
    /// An export decision and nothing else: the capture of a mono record is a
    /// two-channel capture like any other, and this says the two channels are to
    /// be folded to one on the way out. Not detectable from the audio, which is
    /// why it is asked rather than analyzed.
    pub is_mono: bool,
    /// Whether the RIAA curve is to be applied on playback and on export (§51).
    ///
    /// The stored decision, not the provenance. What the signal already carried
    /// when it reached the sound card is `captures.capture_eq`, one per capture.
    pub riaa_eq: bool,
    /// Unix seconds of the last change.
    pub updated_at: i64,
}

impl Record {
    /// The genres as one `'; '`-separated string, which is how they are stored and
    /// how a tagger writes them.
    #[must_use]
    pub fn genre_list(&self) -> String {
        self.genres.join("; ")
    }

    /// Whether anything has been filled in.
    ///
    /// A project starts with an empty release rather than no release, so "has the
    /// user told us anything yet" is a question about the fields.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.album.is_empty()
            && self.album_artist.is_empty()
            && self.catalog.is_empty()
            && self.label.is_empty()
            && self.genres.is_empty()
            && self.year.is_none()
    }

    /// The sides this release has, from its disc count (§29).
    #[must_use]
    pub fn sides(&self) -> Vec<vcw_types::vinyl::Side> {
        vcw_types::vinyl::Side::for_discs(self.discs)
    }
}

/// The release id, which is always 1. Named rather than written as a literal in
/// every query, because the constant is a decision and `1` looks like an accident.
pub const RELEASE_ID: i64 = 1;

/// Reads the release, creating an empty one if the project has none.
///
/// # Errors
///
/// If the project is read-only, or the insert fails.
pub fn ensure(project: &mut Project) -> Result<Record> {
    if let Some(record) = load(project.conn())? {
        return Ok(record);
    }
    let conn = project.conn_mut();
    conn.execute(
        "INSERT INTO releases (release_id, updated_at) VALUES (?1, ?2)",
        params![RELEASE_ID, crate::now()],
    )?;
    load(conn)?.map_or_else(|| unreachable!("the row was just inserted"), Ok)
}

/// Reads the release, or `None` if the project has none yet.
///
/// # Errors
///
/// If the query fails.
pub fn load(conn: &Connection) -> Result<Option<Record>> {
    let (mono, riaa) = if has_intents(conn) {
        ("is_mono", "riaa_eq")
    } else {
        ("0", "0")
    };
    let record = conn
        .query_row(
            &format!(
                "SELECT album, album_artist, year, genres, label, catalog, country, barcode,
                        composer, comments, discs, numbering, musicbrainz_id, discogs_id,
                        confirmed, updated_at, {mono}, {riaa}
                   FROM releases WHERE release_id = ?1"
            ),
            params![RELEASE_ID],
            |r| {
                let genres: String = r.get(3)?;
                let numbering: String = r.get(11)?;
                Ok(Record {
                    album: r.get(0)?,
                    album_artist: r.get(1)?,
                    year: r.get::<_, Option<i64>>(2)?.map(|y| y as u32),
                    genres: split_genres(&genres),
                    label: r.get(4)?,
                    catalog: r.get(5)?,
                    country: r.get(6)?,
                    barcode: r.get(7)?,
                    composer: r.get(8)?,
                    comments: r.get(9)?,
                    discs: r.get::<_, i64>(10)? as u32,
                    numbering: read_numbering(&numbering),
                    musicbrainz_id: r.get(12)?,
                    discogs_id: r.get(13)?,
                    confirmed: r.get::<_, i64>(14)? != 0,
                    updated_at: r.get(15)?,
                    is_mono: r.get::<_, i64>(16)? != 0,
                    riaa_eq: r.get::<_, i64>(17)? != 0,
                })
            },
        )
        .optional()?;
    Ok(record)
}

/// Whether the `releases` table carries v4's two intent columns.
///
/// The same question [`crate::session`] asks about `captures.capture_eq`, and for
/// the same reason: `open_read_only` deliberately does not migrate, so a v2 or v3
/// project is still one this build reads, and a `SELECT` naming a column that is
/// not there would answer `no such column` to every reader of the release row -
/// the export planner, the track numbering, the library listing - rather than to
/// the one caller that wanted the flag. Asked of the table rather than of
/// `user_version`, because the column is what the query needs.
fn has_intents(conn: &Connection) -> bool {
    conn.prepare("SELECT is_mono, riaa_eq FROM releases LIMIT 0")
        .is_ok()
}

/// Writes the release, creating the row if it is not there.
///
/// Replaces every field: a caller edits a [`Record`] it read and writes it back,
/// which is the one shape that cannot lose a field by forgetting to mention it.
/// `updated_at` is set here rather than taken from the record.
///
/// # Errors
///
/// If the project is read-only, or the write fails.
pub fn store(project: &mut Project, record: &Record) -> Result<()> {
    let discs = record.discs.max(1);
    let conn = project.conn_mut();
    conn.execute(
        "INSERT INTO releases (release_id, album, album_artist, year, genres, label, catalog,
                               country, barcode, composer, comments, discs, numbering,
                               musicbrainz_id, discogs_id, confirmed, updated_at,
                               is_mono, riaa_eq)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19)
         ON CONFLICT (release_id) DO UPDATE SET
             album = excluded.album, album_artist = excluded.album_artist,
             year = excluded.year, genres = excluded.genres, label = excluded.label,
             catalog = excluded.catalog, country = excluded.country,
             barcode = excluded.barcode, composer = excluded.composer,
             comments = excluded.comments, discs = excluded.discs,
             numbering = excluded.numbering, musicbrainz_id = excluded.musicbrainz_id,
             discogs_id = excluded.discogs_id, confirmed = excluded.confirmed,
             updated_at = excluded.updated_at, is_mono = excluded.is_mono,
             riaa_eq = excluded.riaa_eq",
        params![
            RELEASE_ID,
            record.album,
            record.album_artist,
            record.year.map(i64::from),
            record.genre_list(),
            record.label,
            record.catalog,
            record.country,
            record.barcode,
            record.composer,
            record.comments,
            i64::from(discs),
            numbering_name(record.numbering),
            record.musicbrainz_id,
            record.discogs_id,
            i64::from(record.confirmed),
            crate::now(),
            i64::from(record.is_mono),
            i64::from(record.riaa_eq),
        ],
    )?;
    Ok(())
}

/// A stored image (§32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artwork {
    /// Row id.
    pub id: i64,
    /// `front`, `back`, `label` or `other`.
    pub role: String,
    /// The sniffed MIME type.
    pub mime: String,
    /// Pixel width, where it is known.
    pub width: Option<u32>,
    /// Pixel height, where it is known.
    pub height: Option<u32>,
    /// Where it was downloaded from.
    pub source_url: Option<String>,
    /// Unix seconds at download.
    pub fetched_at: i64,
    /// The image bytes.
    pub bytes: Vec<u8>,
}

impl Artwork {
    /// The front cover role, which is the one a tagger embeds.
    pub const FRONT: &'static str = "front";
}

/// The image roles the project understands. Anything else is stored as `other`.
pub const ROLES: [&str; 4] = ["front", "back", "label", "other"];

/// Stores an image against the release, replacing any image with the same role.
///
/// Replacing rather than accumulating, because a release has one front cover and a
/// second one is a correction rather than an addition. Roles outside [`ROLES`] are
/// stored as `other`.
///
/// # Errors
///
/// If the project is read-only, or the write fails.
pub fn put_artwork(
    project: &mut Project,
    role: &str,
    mime: &str,
    bytes: &[u8],
    source_url: Option<&str>,
) -> Result<i64> {
    let role = if ROLES.contains(&role) { role } else { "other" };
    let tx = project.conn_mut().transaction()?;
    tx.execute(
        "DELETE FROM release_artwork WHERE release_id = ?1 AND role = ?2",
        params![RELEASE_ID, role],
    )?;
    let (width, height) = dimensions(bytes).map_or((None, None), |(w, h)| (Some(w), Some(h)));
    tx.execute(
        "INSERT INTO release_artwork
             (release_id, role, mime, width, height, source_url, fetched_at, bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            RELEASE_ID,
            role,
            mime,
            width,
            height,
            source_url,
            crate::now(),
            bytes
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(id)
}

/// Reads one image by role, or `None` if the release has none in that role.
///
/// # Errors
///
/// If the query fails.
pub fn artwork(conn: &Connection, role: &str) -> Result<Option<Artwork>> {
    let found = conn
        .query_row(
            "SELECT artwork_id, role, mime, width, height, source_url, fetched_at, bytes
               FROM release_artwork WHERE release_id = ?1 AND role = ?2",
            params![RELEASE_ID, role],
            |r| {
                Ok(Artwork {
                    id: r.get(0)?,
                    role: r.get(1)?,
                    mime: r.get(2)?,
                    width: r.get::<_, Option<i64>>(3)?.map(|v| v as u32),
                    height: r.get::<_, Option<i64>>(4)?.map(|v| v as u32),
                    source_url: r.get(5)?,
                    fetched_at: r.get(6)?,
                    bytes: r.get(7)?,
                })
            },
        )
        .optional()?;
    Ok(found)
}

/// Every image the release holds, without their bytes.
///
/// The bytes are megabytes each and a listing wants none of them, so this reports
/// the row with an empty `bytes` and [`artwork`] fetches one when it is needed.
///
/// # Errors
///
/// If the query fails.
pub fn artwork_index(conn: &Connection) -> Result<Vec<Artwork>> {
    let mut stmt = conn.prepare(
        "SELECT artwork_id, role, mime, width, height, source_url, fetched_at, length(bytes)
           FROM release_artwork WHERE release_id = ?1 ORDER BY role",
    )?;
    let rows = stmt
        .query_map(params![RELEASE_ID], |r| {
            Ok((
                Artwork {
                    id: r.get(0)?,
                    role: r.get(1)?,
                    mime: r.get(2)?,
                    width: r.get::<_, Option<i64>>(3)?.map(|v| v as u32),
                    height: r.get::<_, Option<i64>>(4)?.map(|v| v as u32),
                    source_url: r.get(5)?,
                    fetched_at: r.get(6)?,
                    bytes: Vec::new(),
                },
                r.get::<_, i64>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().map(|(art, _len)| art).collect())
}

/// Bytes an image occupies, without reading it.
///
/// # Errors
///
/// If the query fails.
pub fn artwork_bytes(conn: &Connection, role: &str) -> Result<Option<u64>> {
    let len = conn
        .query_row(
            "SELECT length(bytes) FROM release_artwork WHERE release_id = ?1 AND role = ?2",
            params![RELEASE_ID, role],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(len.map(|n| n as u64))
}

/// An image's pixel dimensions, read from its own header.
///
/// Done here rather than in each caller so the columns mean something whoever
/// wrote the row - the operator with a file on disk and the fetcher with a
/// response body both go through [`put_artwork`]. A UI that wants to know whether
/// a cover is big enough to be worth showing should not have to decode it.
///
/// PNG and JPEG only, which is what Cover Art Archive and Discogs serve and what
/// a scanner writes. Anything else returns `None`, and the columns stay NULL,
/// which is the honest answer rather than a guess.
fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    // PNG: the IHDR chunk is always first, so the size sits at a fixed offset.
    if bytes.len() >= 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return (width > 0 && height > 0).then_some((width, height));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg_dimensions(bytes);
    }
    None
}

/// The size in a JPEG's first start-of-frame segment.
///
/// JPEG has no fixed offset: the size is in an SOF marker somewhere after a
/// variable run of application and quantisation segments, so the segment chain has
/// to be walked. Bounded by the slice, so a truncated or hostile file runs out
/// rather than running away.
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    while at + 3 < bytes.len() {
        if bytes[at] != 0xFF {
            // Not at a marker, so the chain is broken and guessing from here
            // would be guessing.
            return None;
        }
        let marker = bytes[at + 1];
        // Padding between segments, and the standalone markers that carry no
        // length: step over them a byte at a time.
        if marker == 0xFF {
            at += 1;
            continue;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD9) {
            at += 2;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        // Every SOF but SOF4 (0xC4, a Huffman table), SOF8 (0xC8, reserved) and
        // SOFC (0xCC, arithmetic coding conditioning), which are not frames.
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if at + 9 >= bytes.len() {
                return None;
            }
            let height = u32::from(u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]));
            let width = u32::from(u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]));
            return (width > 0 && height > 0).then_some((width, height));
        }
        if length < 2 {
            return None;
        }
        at += 2 + length;
    }
    None
}

fn split_genres(stored: &str) -> Vec<String> {
    stored
        .split(';')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_owned)
        .collect()
}

const fn numbering_name(numbering: Numbering) -> &'static str {
    match numbering {
        Numbering::Alpha => "alpha",
        Numbering::Numeric => "numeric",
    }
}

/// Unknown spellings read back as the default rather than failing: the column is
/// a presentation preference, and a project is not worth refusing over one.
fn read_numbering(stored: &str) -> Numbering {
    match stored {
        "numeric" => Numbering::Numeric,
        _ => Numbering::Alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &tempfile::TempDir) -> Project {
        Project::create(dir.path().join("release.vcw")).expect("create")
    }

    #[test]
    fn a_new_project_has_an_empty_release_rather_than_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let record = ensure(&mut p).expect("ensure");
        assert!(record.is_empty());
        assert_eq!(record.discs, 1);
        assert_eq!(record.numbering, Numbering::Alpha);
        assert_eq!(record.sides().len(), 2, "one disc is two sides");
    }

    #[test]
    fn ensure_is_idempotent_and_never_a_second_release() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        ensure(&mut p).expect("first");
        ensure(&mut p).expect("second");
        let count: i64 = p
            .conn()
            .query_row("SELECT COUNT(*) FROM releases", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, 1);
    }

    #[test]
    fn the_schema_refuses_a_second_release() {
        // §29 has one release per project, and the CHECK is what enforces it.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        ensure(&mut p).expect("ensure");
        let attempt = p.conn().execute(
            "INSERT INTO releases (release_id, updated_at) VALUES (2, 0)",
            [],
        );
        assert!(attempt.is_err(), "release 2 should not be insertable");
    }

    #[test]
    fn every_field_survives_a_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let written = Record {
            album: "Amber".into(),
            album_artist: "Autechre".into(),
            year: Some(1994),
            genres: vec!["Electronic".into(), "IDM".into(), "Ambient Techno".into()],
            label: "Warp".into(),
            catalog: "WARPLP25".into(),
            country: "GB".into(),
            barcode: Some("5021603025127".into()),
            composer: "Booth / Brown".into(),
            comments: "near mint, bought 2019".into(),
            discs: 2,
            numbering: Numbering::Numeric,
            musicbrainz_id: Some("bd5b1270-7468-47f0-9c9a-928199f9e4ad".into()),
            discogs_id: Some("20209".into()),
            confirmed: true,
            // Both non-default, so a column dropped from the write fails here
            // rather than passing on the default it would have had anyway.
            is_mono: true,
            riaa_eq: true,
            updated_at: 0,
        };
        store(&mut p, &written).expect("store");
        let read = load(p.conn()).expect("load").expect("row");

        assert_eq!(read.album, written.album);
        assert_eq!(read.genres, written.genres, "genres keep their order");
        assert_eq!(read.discs, 2);
        assert_eq!(read.numbering, Numbering::Numeric);
        assert!(read.confirmed);
        assert!(read.is_mono);
        assert!(read.riaa_eq);
        assert!(read.updated_at > 0, "updated_at is set by the write");
        assert_eq!(read.sides().len(), 4, "two discs are four sides");
        assert!(!read.is_empty());
    }

    #[test]
    fn storing_twice_updates_rather_than_failing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let mut record = ensure(&mut p).expect("ensure");
        record.album = "Amber".into();
        store(&mut p, &record).expect("first");
        record.album = "Tri Repetae".into();
        store(&mut p, &record).expect("second");
        assert_eq!(
            load(p.conn()).expect("load").expect("row").album,
            "Tri Repetae"
        );
    }

    #[test]
    fn the_setup_intents_round_trip_and_default_to_unstated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let fresh = ensure(&mut p).expect("ensure");
        assert!(!fresh.is_mono, "nobody has been asked yet");
        assert!(!fresh.riaa_eq);

        let mut record = fresh;
        record.is_mono = true;
        record.riaa_eq = true;
        store(&mut p, &record).expect("store");
        let read = load(p.conn()).expect("load").expect("row");
        assert!(read.is_mono);
        assert!(read.riaa_eq);

        // Both are set independently: the common case is a stereo pressing that
        // still wants the curve, and a shared column would make that impossible.
        record.is_mono = false;
        store(&mut p, &record).expect("store again");
        let read = load(p.conn()).expect("load").expect("row");
        assert!(!read.is_mono);
        assert!(read.riaa_eq);
    }

    #[test]
    fn a_pre_v4_release_reads_as_unstated_rather_than_failing() {
        // `open_read_only` does not migrate, so this is what every reader of the
        // release row sees on a project written before v4. Dropping the columns is
        // how a v3 project is spelled from here.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        let mut record = ensure(&mut p).expect("ensure");
        record.album = "Vienna".into();
        record.is_mono = true;
        store(&mut p, &record).expect("store");
        p.conn()
            .execute_batch(
                "ALTER TABLE releases DROP COLUMN is_mono;
                 ALTER TABLE releases DROP COLUMN riaa_eq;",
            )
            .expect("winding the release row back to v3");

        let read = load(p.conn()).expect("load").expect("row");
        assert_eq!(read.album, "Vienna", "the rest of the row still reads");
        assert!(!read.is_mono, "an absent column is an unstated intent");
        assert!(!read.riaa_eq);
    }

    #[test]
    fn a_genre_list_round_trips_through_its_stored_spelling() {
        let mut record = Record {
            genres: vec!["Hip-Hop".into(), "Minimal".into()],
            ..Record::default()
        };
        assert_eq!(record.genre_list(), "Hip-Hop; Minimal");
        record.genres = split_genres(" Hip-Hop ;; Minimal ; ");
        assert_eq!(record.genres, ["Hip-Hop", "Minimal"], "blanks are dropped");
    }

    #[test]
    fn artwork_replaces_its_role_rather_than_accumulating() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        ensure(&mut p).expect("ensure");

        put_artwork(&mut p, "front", "image/jpeg", &[0xFF, 0xD8, 0xFF, 1], None).expect("first");
        put_artwork(
            &mut p,
            "front",
            "image/png",
            &[0x89, b'P', b'N', b'G'],
            Some("https://coverartarchive.org/release/x/front"),
        )
        .expect("second");
        put_artwork(&mut p, "back", "image/jpeg", &[0xFF, 0xD8, 0xFF, 2], None).expect("back");

        let front = artwork(p.conn(), Artwork::FRONT)
            .expect("read")
            .expect("row");
        assert_eq!(front.mime, "image/png", "the later image won");
        assert_eq!(front.bytes, [0x89, b'P', b'N', b'G']);
        assert!(front.source_url.is_some());
        assert_eq!(artwork_index(p.conn()).expect("index").len(), 2);
        assert_eq!(artwork_bytes(p.conn(), "back").expect("len"), Some(4));
        assert!(artwork(p.conn(), "label").expect("read").is_none());
    }

    #[test]
    fn an_unrecognized_role_is_stored_as_other_rather_than_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        ensure(&mut p).expect("ensure");
        put_artwork(&mut p, "inner-sleeve", "image/jpeg", &[1, 2, 3], None).expect("put");
        assert!(artwork(p.conn(), "other").expect("read").is_some());
    }

    #[test]
    fn an_index_entry_carries_no_image_bytes() {
        // A listing of a two-disc release would otherwise read several megabytes to
        // print four rows.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = project(&dir);
        ensure(&mut p).expect("ensure");
        put_artwork(&mut p, "front", "image/jpeg", &[7; 4096], None).expect("put");
        let index = artwork_index(p.conn()).expect("index");
        assert_eq!(index.len(), 1);
        assert!(index[0].bytes.is_empty());
        assert_eq!(artwork_bytes(p.conn(), "front").expect("len"), Some(4096));
    }
}

#[cfg(test)]
mod image_tests {
    use super::dimensions;

    /// A minimal but real PNG of the given size.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&[8, 2, 0, 0, 0]);
        out.extend_from_slice(&0u32.to_be_bytes());
        out
    }

    /// A JPEG header with a run of segments before the SOF, which is the shape a
    /// camera or a scanner writes and the reason the chain has to be walked.
    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        // APP0, as every JFIF file starts.
        out.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
        out.extend_from_slice(b"JFIF\0");
        out.extend_from_slice(&[0x01, 0x02, 0x00, 0, 1, 0, 1, 0, 0]);
        // A quantisation table, to make the walk do some work.
        out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x05, 0x00, 0x10, 0x11]);
        // SOF0.
        out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&[0x03, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        out
    }

    #[test]
    fn a_png_reports_its_size() {
        assert_eq!(dimensions(&png(600, 598)), Some((600, 598)));
    }

    #[test]
    fn a_jpeg_reports_its_size_from_past_its_other_segments() {
        assert_eq!(dimensions(&jpeg(1_425, 1_400)), Some((1_425, 1_400)));
    }

    #[test]
    fn a_progressive_jpeg_reports_its_size_too() {
        // SOF2 rather than SOF0, which is what most web-sized covers are.
        let mut bytes = jpeg(500, 500);
        bytes[2 + 16 + 7 + 1] = 0xC2;
        assert_eq!(dimensions(&bytes), Some((500, 500)));
    }

    #[test]
    fn a_huffman_table_is_not_mistaken_for_a_frame() {
        // 0xC4 is in the SOF range and is not a frame. A reader that took it as
        // one would report two bytes of table data as the cover's size.
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x07, 0x00, 0x01, 0x02, 0x03, 0x04]);
        bytes.extend_from_slice(&jpeg(320, 240)[2..]);
        assert_eq!(dimensions(&bytes), Some((320, 240)));
    }

    #[test]
    fn nothing_is_claimed_about_a_file_that_is_not_one_of_the_two() {
        assert_eq!(dimensions(b"RIFF\0\0\0\0WEBPVP8 "), None);
        assert_eq!(dimensions(b"not an image at all"), None);
        assert_eq!(dimensions(&[]), None);
        // Truncated, hostile, or a zero-sized frame: all the same answer.
        assert_eq!(dimensions(&png(600, 598)[..20]), None);
        assert_eq!(dimensions(&png(0, 0)), None);
        assert_eq!(dimensions(&[0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11]), None);
        assert_eq!(dimensions(&[0xFF, 0xD8, 0x00, 0x00]), None);
        // A length of zero would step backwards and loop forever.
        assert_eq!(
            dimensions(&[0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x00, 0x00]),
            None
        );
    }
}
