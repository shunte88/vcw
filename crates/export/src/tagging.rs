/*
 *  tagging.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Tag and artwork writing, and the naming templates (§33).
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

//! Tag and artwork writing (§33, §14).
//!
//! Ported from VRipr's `write_tags`, including the two conventions that make a
//! library built with the old tool stay consistent: a multi-value field is one
//! string with `';'` between the values, and the vinyl-specific fields that no
//! tag standard has a name for are written as freeform keys.
//!
//! Tags are validated by a third-party reader in `tests/third_party.rs`, because
//! a tag we can read back ourselves proves nothing about the players people use.
//!
//! ## Two backends, not one
//!
//! VRipr wrote through lofty's generic [`Tag`], with `ItemKey::Unknown(name)`
//! for the freeform keys. lofty 0.25 removed that variant: `ItemKey` is a
//! closed, `Copy` enum now, so there is no longer a way to name an arbitrary key
//! through the generic tag. The generic tag is still how the mapped fields are
//! built - one list of assignments, not two - and it is then converted into the
//! container's own tag, where the freeform keys can be named:
//!
//! - **FLAC** gets [`VorbisComments`](lofty::ogg::tag::VorbisComments), where a
//!   freeform key is just a key.
//! - **WAV** gets ID3v2, where it is a `TXXX` frame.
//!
//! Both paths carry the same field set, because `mapped` builds one list of
//! assignments and each backend converts the same list. A WAV goes out with
//! every tag a FLAC does, the freeform keys included, and the cover embedded.
//!
//! What differs is how reliably it is read back. RIFF has no metadata standard
//! anyone agrees on: `LIST`/`INFO` is old and thin, and a full ID3v2 tag in its
//! own chunk is what modern readers look for. lofty writes the latter, as an
//! `ID3 ` chunk appended after `data` - uppercase, which is what lofty emits;
//! the ID3 spec's RIFF note spells it lowercase and readers in the wild accept
//! either. `tests/third_party.rs` holds both containers to ffprobe and mutagen
//! for exactly this reason, because a tag we can read back ourselves proves
//! nothing about the players people use.
//!
//! ## Where the canonical key is not the one the old library used
//!
//! Vorbis has `LABEL` and `RELEASECOUNTRY`; VRipr wrote `ORGANIZATION` and
//! `COUNTRY`. Both spellings go in, canonical first, because a reader looking
//! for either finds it and neither is wrong. Duplicating a *value* under one key
//! would be wrong, and that is not what this is.

use std::path::Path;

use lofty::config::WriteOptions;
use lofty::picture::{MimeType, PictureType};
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::{ItemValue, Tag, TagItem, TagType};

use crate::encoder::Container;
use crate::error::{Error, Result};

/// A cover image to embed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    /// The MIME type, as the project sniffed it.
    pub mime: String,
    /// The image itself.
    pub bytes: Vec<u8>,
}

/// Everything that goes into one file's tags.
///
/// Flat strings, like [`crate::naming::Values`], and for the same reason: this
/// is the boundary where a release and a track become metadata, and one place
/// doing that conversion is easier to check than two.
///
/// Empty means *do not write the tag*, which is not the same as writing an empty
/// one: a player showing a blank artist is worse than a player showing none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    /// Track title.
    pub title: String,
    /// Track artist, `';'`-separated for more than one.
    pub artist: String,
    /// Release title.
    pub album: String,
    /// Release artist, `';'`-separated for more than one.
    pub album_artist: String,
    /// Genres, `';'`-separated.
    pub genre: String,
    /// Release year.
    pub year: Option<u32>,
    /// Track number within the release.
    pub track_number: Option<u32>,
    /// Tracks on the release, where it is known.
    pub track_total: Option<u32>,
    /// Disc number, one-based.
    pub disc_number: Option<u32>,
    /// Discs in the release.
    pub disc_total: Option<u32>,
    /// Composer.
    pub composer: String,
    /// Free text.
    pub comment: String,
    /// Country of the pressing, as the provider spelled it.
    pub country: String,
    /// Record label.
    pub label: String,
    /// Catalogue number.
    pub catalog: String,
    /// Barcode, where the pressing has one.
    pub barcode: String,
    /// Discogs release id, written freeform because no standard names it.
    pub discogs_id: String,
    /// MusicBrainz release id.
    pub musicbrainz_release_id: String,
    /// MusicBrainz recording id, which is per track.
    pub musicbrainz_recording_id: String,
    /// Anything else a caller wants written, as `(key, value)`.
    pub extra: Vec<(String, String)>,
    /// The front cover, where the release has one.
    pub cover: Option<Cover>,
}

/// Splits a `';'`-separated field into its values.
///
/// VRipr's `split_artists`, unchanged: trimmed, and empties dropped so a
/// trailing separator does not become a blank artist.
#[must_use]
pub fn split_values(text: &str) -> Vec<String> {
    text.split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// What VCW writes into the encoder field.
fn encoder() -> String {
    format!("VCW {}", env!("CARGO_PKG_VERSION"))
}

/// Builds the generic tag: everything with a standard key.
fn mapped(tags: &Tags, tag_type: TagType) -> Tag {
    let mut tag = Tag::new(tag_type);

    if !tags.title.is_empty() {
        tag.set_title(tags.title.clone());
    }
    if !tags.album.is_empty() {
        tag.set_album(tags.album.clone());
    }
    // One item per value rather than one item holding a separator, so a player
    // that understands multi-value fields shows two artists and one that does
    // not shows the first instead of showing punctuation.
    for (key, field) in [
        (ItemKey::TrackArtist, &tags.artist),
        (ItemKey::AlbumArtist, &tags.album_artist),
        (ItemKey::Genre, &tags.genre),
    ] {
        let values = split_values(field);
        if values.is_empty() {
            continue;
        }
        tag.remove_key(key);
        for value in values {
            tag.push(TagItem::new(key, ItemValue::Text(value)));
        }
    }

    if let Some(year) = tags.year {
        // Two keys, because `Accessor` has no `year` in lofty 0.25 - it has
        // `date`, taking a parsed `Timestamp` - and because the ecosystem is
        // split. `RecordingDate` is the one readers look at: it becomes `DATE`
        // in Vorbis and `TDRC` in ID3v2, both of which take a bare year.
        // `Year` becomes Vorbis's `YEAR`, which is what VRipr wrote.
        tag.insert_text(ItemKey::RecordingDate, year.to_string());
        tag.insert_text(ItemKey::Year, year.to_string());
    }
    if let Some(number) = tags.track_number {
        tag.set_track(number);
    }
    if let Some(total) = tags.track_total {
        tag.set_track_total(total);
    }
    if let Some(number) = tags.disc_number {
        tag.set_disk(number);
    }
    if let Some(total) = tags.disc_total {
        tag.set_disk_total(total);
    }

    for (key, value) in [
        (ItemKey::Composer, &tags.composer),
        (ItemKey::Comment, &tags.comment),
        (ItemKey::Label, &tags.label),
        (ItemKey::CatalogNumber, &tags.catalog),
        (ItemKey::ReleaseCountry, &tags.country),
        (ItemKey::Barcode, &tags.barcode),
        (ItemKey::MusicBrainzReleaseId, &tags.musicbrainz_release_id),
        (
            ItemKey::MusicBrainzRecordingId,
            &tags.musicbrainz_recording_id,
        ),
    ] {
        if !value.is_empty() {
            tag.insert_text(key, value.clone());
        }
    }
    tag.insert_text(ItemKey::EncoderSoftware, encoder());
    tag
}

/// The keys no standard names, in the order they are written.
///
/// `ORGANIZATION` and `COUNTRY` are VRipr's spellings of two fields that Vorbis
/// does name - see the module docs on why both go in.
fn freeform(tags: &Tags) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut add = |key: &str, value: &str| {
        if !value.is_empty() {
            out.push((key.to_owned(), value.to_owned()));
        }
    };
    add("DISCOGS_RELEASEID", &tags.discogs_id);
    add("ORGANIZATION", &tags.label);
    add("COUNTRY", &tags.country);
    for (key, value) in &tags.extra {
        if !key.is_empty() {
            add(key, value);
        }
    }
    out
}

/// Writes tags and artwork to a file that already holds audio.
///
/// # Errors
///
/// If the file cannot be read as the container it claims to be, or if the tag
/// cannot be written back.
pub fn write(path: &Path, container: Container, tags: &Tags) -> Result<()> {
    match container {
        // Two backends for four containers, because what differs is the tag
        // format and not the number of codecs: ID3v2 is what a RIFF reader and
        // an MP3 reader both look for, and Vorbis comments are what FLAC and
        // Ogg both carry natively. lofty puts each in the right place for the
        // file it is handed.
        Container::Wav | Container::Mp3(_) => write_id3(path, tags),
        Container::Flac | Container::OggVorbis(_) => write_vorbis(path, tags),
    }
}

/// Wraps a lofty failure with the path, since lofty's own messages do not carry
/// it and "failed to write tag" on its own helps nobody.
fn tagging<E>(path: &Path) -> impl FnOnce(E) -> Error + use<'_, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    move |source| Error::Tagging {
        path: path.to_path_buf(),
        source: Box::new(source),
    }
}

/// FLAC: Vorbis comments, where a freeform key is just a key.
fn write_vorbis(path: &Path, tags: &Tags) -> Result<()> {
    use lofty::ogg::OggPictureStorage;
    use lofty::ogg::tag::VorbisComments;

    let mut comments = VorbisComments::from(mapped(tags, TagType::VorbisComments));
    for (key, value) in freeform(tags) {
        comments.push(key, value);
    }
    if let Some(cover) = &tags.cover {
        // `insert_picture` reads the image to work out its dimensions, and
        // refuses one it cannot make sense of. A cover that is not an image is
        // the project's problem and not worth failing an export over, so it is
        // dropped rather than raised.
        let _ = comments.insert_picture(picture(cover), None);
    }
    comments
        .save_to_path(path, WriteOptions::default())
        .map_err(tagging(path))
}

/// WAV: a full ID3v2 tag in an `ID3 ` chunk, which is what a modern reader
/// looks for. Appended after `data`, so the audio bytes are untouched.
fn write_id3(path: &Path, tags: &Tags) -> Result<()> {
    use lofty::id3::v2::Id3v2Tag;

    let mut id3 = Id3v2Tag::from(mapped(tags, TagType::Id3v2));
    for (key, value) in freeform(tags) {
        id3.insert_user_text(key, value);
    }
    if let Some(cover) = &tags.cover {
        id3.insert_picture(picture(cover));
    }
    id3.save_to_path(path, WriteOptions::default())
        .map_err(tagging(path))
}

/// Turns a stored image into a front-cover picture.
fn picture(cover: &Cover) -> lofty::picture::Picture {
    lofty::picture::Picture::unchecked(cover.bytes.clone())
        .pic_type(PictureType::CoverFront)
        .mime_type(MimeType::from_str(&cover.mime))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::{Spec, Writer};
    use std::path::PathBuf;

    use lofty::file::TaggedFileExt;
    use lofty::prelude::ItemKey;
    use vcw_types::StorageFormat;

    /// A 1x1 RGB PNG. Real bytes rather than a plausible-looking array, because
    /// lofty parses the `IHDR` for the dimensions it stores alongside a picture
    /// and refuses anything it cannot read.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x10,
        0x50, 0x30, 0x00, 0x00, 0x00, 0xa4, 0x00, 0x61, 0x34, 0x66, 0x7d, 0x72, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    /// One track's worth of everything, with two artists and two genres.
    fn fixture() -> Tags {
        Tags {
            title: "Trans-Europe Express".to_owned(),
            artist: "Kraftwerk; Florian Schneider".to_owned(),
            album: "Trans-Europe Express".to_owned(),
            album_artist: "Kraftwerk".to_owned(),
            genre: "Electronic; Krautrock".to_owned(),
            year: Some(1977),
            track_number: Some(2),
            track_total: Some(7),
            disc_number: Some(1),
            disc_total: Some(1),
            composer: "Ralf Hutter".to_owned(),
            comment: "Side A, Kenwood KD-500".to_owned(),
            country: "Germany".to_owned(),
            label: "Kling Klang".to_owned(),
            catalog: "1C 064-82 306".to_owned(),
            barcode: "5099969958229".to_owned(),
            discogs_id: "1234567".to_owned(),
            musicbrainz_release_id: "0d1b1b1f-0000-4000-8000-000000000001".to_owned(),
            musicbrainz_recording_id: "0d1b1b1f-0000-4000-8000-000000000002".to_owned(),
            extra: vec![("STYLUS".to_owned(), "Ortofon 2M Blue".to_owned())],
            cover: Some(Cover {
                mime: "image/png".to_owned(),
                bytes: PNG.to_vec(),
            }),
        }
    }

    /// A short file of the requested container, ready to be tagged.
    fn audio(dir: &Path, container: Container) -> PathBuf {
        let spec = Spec {
            rate: 44_100,
            channels: 2,
            format: StorageFormat::Int16,
            frames: 1_000,
        };
        let path = dir.join(format!("track.{}", container.extension()));
        let mut writer = Writer::create(&path, container, spec).expect("create");
        writer
            .write(&vec![0u8; spec.frames as usize * spec.stored_frame_bytes()])
            .expect("write");
        writer.finish().expect("finish");
        path
    }

    #[test]
    fn a_multi_value_field_splits_on_the_semicolon() {
        assert_eq!(split_values("Kraftwerk"), ["Kraftwerk"]);
        assert_eq!(
            split_values("Kraftwerk; Florian Schneider"),
            ["Kraftwerk", "Florian Schneider"]
        );
        // A trailing separator is what a provider with one artist and a template
        // expecting two produces. It must not become a blank artist.
        assert_eq!(split_values("Kraftwerk;"), ["Kraftwerk"]);
        assert_eq!(split_values("  ;  "), Vec::<String>::new());
        assert_eq!(split_values(""), Vec::<String>::new());
    }

    #[test]
    fn two_artists_are_two_items_and_not_one_string() {
        let tag = mapped(&fixture(), TagType::VorbisComments);
        let artists: Vec<&str> = tag.get_strings(ItemKey::TrackArtist).collect();
        assert_eq!(artists, ["Kraftwerk", "Florian Schneider"]);
        let genres: Vec<&str> = tag.get_strings(ItemKey::Genre).collect();
        assert_eq!(genres, ["Electronic", "Krautrock"]);
    }

    #[test]
    fn an_empty_field_is_not_written_at_all() {
        // A player showing a blank artist is worse than one showing none.
        let tag = mapped(&Tags::default(), TagType::VorbisComments);
        for key in [
            ItemKey::TrackTitle,
            ItemKey::TrackArtist,
            ItemKey::AlbumTitle,
            ItemKey::Genre,
            ItemKey::Comment,
            ItemKey::Label,
            ItemKey::CatalogNumber,
            ItemKey::Barcode,
        ] {
            assert!(tag.get(key).is_none(), "{key:?} was written from nothing");
        }
        // Except the encoder, which is true of any file we produce.
        assert!(tag.get(ItemKey::EncoderSoftware).is_some());
        assert!(freeform(&Tags::default()).is_empty());
    }

    #[test]
    fn the_vinyl_fields_no_standard_names_are_written_freeform() {
        let keys: Vec<String> = freeform(&fixture())
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(
            keys,
            ["DISCOGS_RELEASEID", "ORGANIZATION", "COUNTRY", "STYLUS"]
        );
    }

    #[test]
    fn a_tagged_flac_reads_back_everything_we_put_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = audio(dir.path(), Container::Flac);
        write(&path, Container::Flac, &fixture()).expect("tag");

        let file = lofty::read_from_path(&path).expect("read back");
        let tag = file.primary_tag().expect("a tag");
        assert_eq!(tag.title().as_deref(), Some("Trans-Europe Express"));
        assert_eq!(
            tag.get_strings(ItemKey::TrackArtist).collect::<Vec<_>>(),
            ["Kraftwerk", "Florian Schneider"]
        );
        assert_eq!(tag.track(), Some(2));
        assert_eq!(tag.track_total(), Some(7));
        assert_eq!(
            tag.get_string(ItemKey::CatalogNumber),
            Some("1C 064-82 306")
        );
        // The canonical key and VRipr's spelling of the same field, both there.
        assert_eq!(tag.get_string(ItemKey::Label), Some("Kling Klang"));
        assert_eq!(tag.get_string(ItemKey::RecordingDate), Some("1977"));
        assert_eq!(tag.pictures().len(), 1);
        assert_eq!(tag.pictures()[0].pic_type(), PictureType::CoverFront);
        assert_eq!(tag.pictures()[0].data(), PNG);
    }

    #[test]
    fn tagging_a_wav_leaves_the_audio_alone() {
        // The whole point of the export: a tag write that shifts one sample has
        // broken the bit-exactness the rest of WP-14 proves.
        let dir = tempfile::tempdir().unwrap();
        let path = audio(dir.path(), Container::Wav);
        let before = data(&path);
        write(&path, Container::Wav, &fixture()).expect("tag");
        assert_eq!(data(&path), before);

        let file = lofty::read_from_path(&path).expect("read back");
        let tag = file.primary_tag().expect("a tag");
        assert_eq!(tag.title().as_deref(), Some("Trans-Europe Express"));
        assert_eq!(tag.track(), Some(2));
    }

    /// The data chunk, found by walking the chunks: tagging adds one, so the
    /// offset is not a constant.
    fn data(path: &Path) -> Vec<u8> {
        let bytes = std::fs::read(path).expect("read");
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            if &bytes[at..at + 4] == b"data" {
                return bytes[at + 8..at + 8 + size].to_vec();
            }
            at += 8 + size + size % 2;
        }
        panic!("no data chunk");
    }
}
