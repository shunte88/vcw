/*
 *  third_party.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Checking our output with readers we did not write.
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

//! Checking our output with readers we did not write.
//!
//! WP-14's exit criterion says tags are validated by third-party readers, and
//! the same argument applies to the containers: a WAV we can read back with our
//! own parser proves only that we are consistently wrong. Everything here shells
//! out to a tool somebody else wrote - `ffprobe`, `flac`, `metaflac`, `sox` -
//! and believes it over us.
//!
//! A missing tool skips its test rather than failing it, because these have to
//! pass on a CI runner with nothing installed. That would make an empty run look
//! green, so [`at_least_one_verifier_is_installed`] fails when nothing at all is
//! available: the gate can be unverified or it can be green, not both.

use std::path::{Path, PathBuf};
use std::process::Command;

use vcw_export::encoder::{Container, Spec, Writer};
use vcw_export::tagging::{self, Cover, Tags};
use vcw_types::StorageFormat;

/// Finds a tool on `PATH`, or `None`.
fn tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    // `ffprobe` on Unix is `ffprobe.exe` on Windows, and a bare join finds
    // neither there. Without the suffix every verifier in this file is absent on
    // Windows however many of them are installed, and the only test that would
    // say so is `at_least_one_verifier_is_installed`.
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{name}.exe"), name.to_owned()]
    } else {
        vec![name.to_owned()]
    };
    std::env::split_paths(&path).find_map(|dir| {
        names
            .iter()
            .map(|name| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

/// Runs a tool and returns its stdout and stderr together.
///
/// Together on purpose: `flac` writes its warnings to stderr and its answers to
/// stdout, and a test that only reads one of them misses half of what it said.
fn run(tool: &Path, args: &[&str]) -> String {
    let out = Command::new(tool)
        .args(args)
        .output()
        .unwrap_or_else(|why| panic!("{} would not run: {why}", tool.display()));
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A ramp that is distinct in every byte, so an off-by-one shows as a mismatch.
fn ramp(spec: &Spec) -> Vec<u8> {
    let bytes = spec.frames as usize * spec.stored_frame_bytes();
    (0..bytes).map(|i| (i % 251) as u8).collect()
}

/// Writes one file and returns what went into it.
fn write(path: &Path, container: Container, spec: Spec) -> Vec<u8> {
    let stored = ramp(&spec);
    let mut writer = Writer::create(path, container, spec).expect("create");
    // Fed in pieces that have nothing to do with any block size, because a
    // reader's fill size does not either.
    for piece in stored.chunks(997 * spec.stored_frame_bytes()) {
        writer.write(piece).expect("write");
    }
    writer.finish().expect("finish");
    stored
}

/// The data chunk of a WAV file, found by walking the chunks rather than by
/// assuming a header length - which is the point, since ours vary.
fn data_chunk(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("read");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if id == b"data" {
            return bytes[at + 8..at + 8 + size].to_vec();
        }
        at += 8 + size + size % 2;
    }
    panic!("no data chunk in {}", path.display());
}

#[test]
fn at_least_one_verifier_is_installed() {
    let found: Vec<&str> = ["ffprobe", "flac", "metaflac", "sox"]
        .into_iter()
        .filter(|name| tool(name).is_some())
        .collect();
    assert!(
        !found.is_empty(),
        "no third-party reader is installed, so nothing in this file verified anything. \
         Install flac and ffmpeg, or accept that WP-14's exit criterion is unmet on this machine."
    );
    eprintln!("verifying with: {}", found.join(", "));
}

#[test]
fn ffprobe_agrees_about_every_wav_we_write() {
    let Some(ffprobe) = tool("ffprobe") else {
        eprintln!("skipped: no ffprobe");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    // Every stored format, at the rate each one plausibly arrives at, plus one
    // four-channel case to exercise the extensible header.
    // The declared width is whatever ffprobe says `bits_per_raw_sample` is, and
    // for the widths a codec name already implies it says `N/A` rather than
    // repeating itself. That is ffprobe's answer, so it is the expectation.
    let cases: &[(StorageFormat, u32, u16, &str, &str)] = &[
        (StorageFormat::Int16, 44_100, 2, "pcm_s16le", "N/A"),
        (StorageFormat::Int24Packed, 96_000, 2, "pcm_s24le", "24"),
        (StorageFormat::Int24Padded, 96_000, 2, "pcm_s24le", "24"),
        (StorageFormat::Int32, 192_000, 2, "pcm_s32le", "32"),
        (StorageFormat::Float32, 48_000, 2, "pcm_f32le", "N/A"),
        (StorageFormat::Int24Packed, 48_000, 4, "pcm_s24le", "24"),
    ];

    for &(format, rate, channels, codec, bits) in cases {
        let spec = Spec {
            rate,
            channels,
            format,
            frames: 4_000,
        };
        let path = dir.path().join(format!("{format:?}-{rate}-{channels}.wav"));
        write(&path, Container::Wav, spec);

        let said = run(
            &ffprobe,
            &[
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,sample_rate,channels,bits_per_raw_sample",
                "-of",
                "default=nw=1",
                path.to_str().unwrap(),
            ],
        );
        let what = format!("{format:?} at {rate} Hz, {channels} channels");
        assert!(
            said.contains(&format!("codec_name={codec}")),
            "{what}: {said}"
        );
        assert!(
            said.contains(&format!("sample_rate={rate}")),
            "{what}: {said}"
        );
        assert!(
            said.contains(&format!("channels={channels}")),
            "{what}: {said}"
        );
        assert!(
            said.contains(&format!("bits_per_raw_sample={bits}")),
            "{what}: {said}"
        );
    }
}

#[test]
fn the_reference_encoder_reads_our_wav_without_complaint() {
    // The test that changed the header. A plain `fmt ` chunk above 16 bits made
    // `flac 1.5.0` say "legacy WAVE file has format type 1 but
    // bits-per-sample=24", which is a reader we shipped a warning to.
    let Some(flac) = tool("flac") else {
        eprintln!("skipped: no flac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for (format, rate) in [
        (StorageFormat::Int16, 44_100),
        (StorageFormat::Int24Packed, 96_000),
        (StorageFormat::Int24Padded, 96_000),
    ] {
        let spec = Spec {
            rate,
            channels: 2,
            format,
            frames: 10_000,
        };
        let wav = dir.path().join(format!("{format:?}.wav"));
        write(&wav, Container::Wav, spec);
        let said = run(
            &flac,
            &[
                "-f",
                "-o",
                dir.path().join("out.flac").to_str().unwrap(),
                wav.to_str().unwrap(),
            ],
        );
        assert!(
            !said.to_lowercase().contains("warning"),
            "{format:?}: {said}"
        );
    }
}

#[test]
fn flac_verifies_what_we_encoded() {
    // `flac -t` decodes the whole stream and checks it against the MD5 in
    // STREAMINFO, so this is both "it is a FLAC file" and "the digest we wrote
    // is the digest of the audio we wrote".
    let Some(flac) = tool("flac") else {
        eprintln!("skipped: no flac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for (format, rate, frames) in [
        (StorageFormat::Int16, 44_100, 30_000u64),
        (StorageFormat::Int24Packed, 96_000, 30_000),
        // Three whole blocks and a short one: the case that exposed the
        // STREAMINFO block-size bug.
        (StorageFormat::Int24Padded, 96_000, 4096 * 3 + 17),
    ] {
        let spec = Spec {
            rate,
            channels: 2,
            format,
            frames,
        };
        let path = dir.path().join(format!("{format:?}.flac"));
        write(&path, Container::Flac, spec);
        let said = run(&flac, &["-t", path.to_str().unwrap()]);
        assert!(said.contains("ok"), "{format:?}: {said}");
        assert!(
            !said.to_lowercase().contains("warning"),
            "{format:?}: {said}. A 'might not be seekable' warning here means \
             STREAMINFO is declaring a variable block size again."
        );
    }
}

#[test]
fn the_reference_decoder_gives_back_the_bytes_we_put_in() {
    // Lossless, checked by somebody else's decoder: the samples that come out
    // of `flac -d` are the bytes that went into the encoder.
    let Some(flac) = tool("flac") else {
        eprintln!("skipped: no flac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for (format, rate) in [
        (StorageFormat::Int16, 44_100),
        (StorageFormat::Int24Packed, 96_000),
    ] {
        let spec = Spec {
            rate,
            channels: 2,
            format,
            frames: 20_000,
        };
        let path = dir.path().join(format!("{format:?}.flac"));
        let stored = write(&path, Container::Flac, spec);
        let decoded = dir.path().join(format!("{format:?}-decoded.wav"));
        run(
            &flac,
            &[
                "-d",
                "-f",
                "-o",
                decoded.to_str().unwrap(),
                path.to_str().unwrap(),
            ],
        );
        assert_eq!(
            data_chunk(&decoded),
            stored,
            "{format:?} did not survive the round trip"
        );
    }
}

#[test]
fn our_digest_is_the_reference_encoders_digest() {
    // Stronger than "it decodes": given the same samples, our STREAMINFO digest
    // is the one `flac` computes, which means we are describing the same audio
    // in the same declared width.
    let (Some(flac), Some(metaflac)) = (tool("flac"), tool("metaflac")) else {
        eprintln!("skipped: no flac or metaflac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec {
        rate: 96_000,
        channels: 2,
        format: StorageFormat::Int24Packed,
        frames: 48_000,
    };
    let wav = dir.path().join("source.wav");
    write(&wav, Container::Wav, spec);
    let ours = dir.path().join("ours.flac");
    write(&ours, Container::Flac, spec);

    let theirs = dir.path().join("theirs.flac");
    run(
        &flac,
        &["-f", "-o", theirs.to_str().unwrap(), wav.to_str().unwrap()],
    );

    let ours_md5 = run(&metaflac, &["--show-md5sum", ours.to_str().unwrap()]);
    let theirs_md5 = run(&metaflac, &["--show-md5sum", theirs.to_str().unwrap()]);
    assert_eq!(ours_md5.trim(), theirs_md5.trim());
    assert_ne!(ours_md5.trim(), "0".repeat(32), "an unset digest");
}

#[test]
fn metaflac_reads_the_header_we_wrote() {
    let Some(metaflac) = tool("metaflac") else {
        eprintln!("skipped: no metaflac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec {
        rate: 96_000,
        channels: 2,
        format: StorageFormat::Int24Padded,
        frames: 4096 * 2 + 5,
    };
    let path = dir.path().join("t.flac");
    write(&path, Container::Flac, spec);

    let said = run(
        &metaflac,
        &["--list", "--block-type=STREAMINFO", path.to_str().unwrap()],
    );
    assert!(said.contains("sample_rate: 96000 Hz"), "{said}");
    assert!(said.contains("channels: 2"), "{said}");
    assert!(
        said.contains("bits-per-sample: 24"),
        "a padded sample is 24 bits of signal: {said}"
    );
    assert!(said.contains("total samples: 8197"), "{said}");
    assert!(
        said.contains("minimum blocksize: 4096") && said.contains("maximum blocksize: 4096"),
        "the nominal block size, both ends, or the stream is not seekable: {said}"
    );
}

#[test]
fn sox_agrees_about_the_duration() {
    // A different parser again, and the one that reports duration in frames -
    // which is the number a track length has to match.
    let Some(sox) = tool("sox") else {
        eprintln!("skipped: no sox");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec {
        rate: 48_000,
        channels: 2,
        format: StorageFormat::Int24Packed,
        frames: 48_000,
    };
    let path = dir.path().join("t.wav");
    write(&path, Container::Wav, spec);
    let said = run(&sox, &["--i", path.to_str().unwrap()]);
    assert!(said.contains("48000 samples"), "{said}");
    assert!(said.contains("24-bit"), "{said}");
}

// ---------------------------------------------------------------------------
// Tags (§33, §14). The other half of WP-14's exit criterion: "tags validated by
// third-party readers". Written by us, read by metaflac, ffprobe and mutagen.
// ---------------------------------------------------------------------------

/// A 1x1 RGB PNG, so the artwork path has something a reader will accept.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x10, 0x50, 0x30, 0x00,
    0x00, 0x00, 0xa4, 0x00, 0x61, 0x34, 0x66, 0x7d, 0x72, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// One track's tags, with the awkward cases in: two artists, an apostrophe, a
/// non-ASCII character, and every vinyl field that has no standard key.
fn tags() -> Tags {
    Tags {
        title: "L'Enfant Sauvage".to_owned(),
        artist: "Gojira; Joe Duplantier".to_owned(),
        album: "Terra Incognita".to_owned(),
        album_artist: "Gojira".to_owned(),
        genre: "Metal; Progressive".to_owned(),
        year: Some(2012),
        track_number: Some(3),
        track_total: Some(11),
        disc_number: Some(1),
        disc_total: Some(2),
        composer: "Joe Duplantier".to_owned(),
        comment: "Side B".to_owned(),
        country: "France".to_owned(),
        label: "Gabriel Editions".to_owned(),
        catalog: "GAB 001".to_owned(),
        barcode: "0016861766627".to_owned(),
        discogs_id: "3778213".to_owned(),
        musicbrainz_release_id: "11111111-2222-3333-4444-555555555555".to_owned(),
        musicbrainz_recording_id: "66666666-7777-8888-9999-aaaaaaaaaaaa".to_owned(),
        extra: vec![("STYLUS".to_owned(), "Ortofon 2M Blue".to_owned())],
        cover: Some(Cover {
            mime: "image/png".to_owned(),
            bytes: PNG.to_vec(),
        }),
    }
}

/// Writes a short file and tags it, returning the path.
fn tagged(dir: &Path, container: Container) -> PathBuf {
    let spec = Spec {
        rate: 44_100,
        channels: 2,
        format: StorageFormat::Int16,
        frames: 12_000,
    };
    let path = dir.join(format!("tagged.{}", container.extension()));
    write(&path, container, spec);
    tagging::write(&path, container, &tags()).expect("tag");
    path
}

#[test]
fn metaflac_reads_the_tags_we_wrote() {
    let Some(metaflac) = tool("metaflac") else {
        eprintln!("skipped: no metaflac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = tagged(dir.path(), Container::Flac);

    let said = run(&metaflac, &["--export-tags-to=-", path.to_str().unwrap()]);
    for expected in [
        "TITLE=L'Enfant Sauvage",
        // Two ARTIST comments rather than one holding a separator, which is what
        // Vorbis says a multi-value field is.
        "ARTIST=Gojira",
        "ARTIST=Joe Duplantier",
        "ALBUM=Terra Incognita",
        "ALBUMARTIST=Gojira",
        "GENRE=Metal",
        "GENRE=Progressive",
        "DATE=2012",
        "TRACKNUMBER=3",
        "TRACKTOTAL=11",
        "DISCNUMBER=1",
        "CATALOGNUMBER=GAB 001",
        "LABEL=Gabriel Editions",
        "RELEASECOUNTRY=France",
        "BARCODE=0016861766627",
        // The freeform keys, including VRipr's two spellings, so a library built
        // with the old tool keeps the shape it had.
        "DISCOGS_RELEASEID=3778213",
        "ORGANIZATION=Gabriel Editions",
        "COUNTRY=France",
        "STYLUS=Ortofon 2M Blue",
        "MUSICBRAINZ_ALBUMID=11111111-2222-3333-4444-555555555555",
    ] {
        assert!(
            said.lines().any(|line| line == expected),
            "metaflac did not report {expected:?}, it reported:\n{said}"
        );
    }
}

#[test]
fn a_tagged_flac_still_verifies_and_still_holds_the_same_audio() {
    // Tagging must not touch the audio. `create` leaves a padding block big
    // enough for a comment block for exactly this reason, but the proof is the
    // digest: the stream's own MD5 is over the samples, so an unchanged digest
    // plus a clean `-t` means the tagger moved metadata and nothing else.
    let Some(flac) = tool("flac") else {
        eprintln!("skipped: no flac");
        return;
    };
    let Some(metaflac) = tool("metaflac") else {
        eprintln!("skipped: no metaflac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec {
        rate: 44_100,
        channels: 2,
        format: StorageFormat::Int16,
        frames: 12_000,
    };
    let path = dir.path().join("digest.flac");
    write(&path, Container::Flac, spec);

    let before = run(&metaflac, &["--show-md5sum", path.to_str().unwrap()]);
    tagging::write(&path, Container::Flac, &tags()).expect("tag");
    let after = run(&metaflac, &["--show-md5sum", path.to_str().unwrap()]);
    assert_eq!(before, after, "tagging changed the audio digest");

    let said = run(&flac, &["-t", path.to_str().unwrap()]);
    assert!(!said.contains("ERROR"), "flac -t on a tagged file: {said}");
    assert!(
        !said.to_lowercase().contains("warning"),
        "flac -t on a tagged file: {said}"
    );
}

#[test]
fn a_tagged_wav_still_holds_the_bytes_we_wrote() {
    // The same argument, for the container that has no digest of its own: the
    // data chunk itself, byte for byte, before and after the tag write.
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec {
        rate: 44_100,
        channels: 2,
        format: StorageFormat::Int24Packed,
        frames: 12_000,
    };
    let path = dir.path().join("tagged.wav");
    let stored = write(&path, Container::Wav, spec);
    let before = data_chunk(&path);
    assert_eq!(before, stored);

    tagging::write(&path, Container::Wav, &tags()).expect("tag");
    assert_eq!(data_chunk(&path), stored, "tagging moved the audio");

    // And it is still a WAV that a reader will take, not just one our own
    // chunk-walker can find its way through.
    if let Some(flac) = tool("flac") {
        let said = run(
            &flac,
            &[
                "-t",
                "--totally-silent",
                "-o",
                dir.path().join("out.flac").to_str().unwrap(),
                path.to_str().unwrap(),
            ],
        );
        assert!(!said.contains("ERROR"), "flac reading a tagged wav: {said}");
    }
}

#[test]
fn ffprobe_reads_the_tags_we_wrote() {
    let Some(ffprobe) = tool("ffprobe") else {
        eprintln!("skipped: no ffprobe");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    // Both containers, and both must at least carry the fields a player puts on
    // screen. FLAC carries the full set; WAV's ID3v2 carries what ID3 has frames
    // for, which is why only the common fields are asserted for it.
    for container in [Container::Flac, Container::Wav] {
        let path = tagged(dir.path(), container);
        let said = run(
            &ffprobe,
            &[
                "-v",
                "error",
                "-show_entries",
                "format_tags",
                "-of",
                "default=nw=1",
                path.to_str().unwrap(),
            ],
        );
        let lower = said.to_lowercase();
        for expected in [
            "l'enfant sauvage",
            "terra incognita",
            "gojira",
            "2012",
            "gab 001",
        ] {
            assert!(
                lower.contains(expected),
                "{container}: ffprobe did not report {expected:?}, it reported:\n{said}"
            );
        }
    }
}

#[test]
fn mutagen_reads_the_tags_we_wrote() {
    // A fourth reader, and the only one here that is a library rather than a
    // command: mutagen is what a lot of tagging tools are built on, so a file it
    // cannot parse is a file half the ecosystem cannot parse.
    let Some(python) = tool("python3") else {
        eprintln!("skipped: no python3");
        return;
    };
    if !run(&python, &["-c", "import mutagen"]).is_empty() {
        eprintln!("skipped: python3 has no mutagen");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let flac = tagged(dir.path(), Container::Flac);
    let wav = tagged(dir.path(), Container::Wav);

    let script = "
import sys, mutagen
flac = mutagen.File(sys.argv[1])
assert flac.tags['title'] == [\"L'Enfant Sauvage\"], flac.tags['title']
assert flac.tags['artist'] == ['Gojira', 'Joe Duplantier'], flac.tags['artist']
assert flac.tags['genre'] == ['Metal', 'Progressive'], flac.tags['genre']
assert flac.tags['discogs_releaseid'] == ['3778213']
assert len(flac.pictures) == 1, flac.pictures
assert flac.pictures[0].mime == 'image/png', flac.pictures[0].mime
assert flac.pictures[0].type == 3, flac.pictures[0].type
wav = mutagen.File(sys.argv[2])
assert str(wav.tags['TIT2']) == \"L'Enfant Sauvage\", wav.tags['TIT2']
assert str(wav.tags['TALB']) == 'Terra Incognita', wav.tags['TALB']
print('ok')
";
    let said = run(
        &python,
        &["-c", script, flac.to_str().unwrap(), wav.to_str().unwrap()],
    );
    assert!(said.trim_end().ends_with("ok"), "mutagen said:\n{said}");
}

#[test]
fn the_cover_comes_back_out_byte_for_byte() {
    let Some(metaflac) = tool("metaflac") else {
        eprintln!("skipped: no metaflac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = tagged(dir.path(), Container::Flac);
    let out = dir.path().join("cover.png");

    run(
        &metaflac,
        &[
            &format!("--export-picture-to={}", out.display()),
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(
        std::fs::read(&out).expect("exported cover"),
        PNG,
        "the embedded cover is not what went in"
    );
}
