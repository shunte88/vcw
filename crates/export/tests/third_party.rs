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
//! out to a tool somebody else wrote - `ffprobe`, `flac`, `metaflac`, `sox`,
//! `ogginfo`, `ffmpeg` and mutagen - and believes it over us.
//!
//! The lossy containers need this more than the lossless ones, not less. A FLAC
//! we got wrong fails our own digest check; an MP3 we got wrong is still a file
//! a player opens, and the ways it can be wrong - a swapped channel pair, a
//! duration taken from a bitrate guess because the VBR header never got patched,
//! a quality setting that was parsed and then ignored - all survive every test
//! that only asks whether bytes came out.
//!
//! A missing tool skips its test rather than failing it, because these have to
//! pass on a CI runner with nothing installed. That would make an empty run look
//! green, so [`at_least_one_verifier_is_installed`] fails when nothing at all is
//! available: the gate can be unverified or it can be green, not both.

use std::path::{Path, PathBuf};
use std::process::Command;

use vcw_export::encoder::{Compression, Container, Spec, Writer};
// Quality is only named where a lossy container is, and every one of those is
// behind a feature.
#[cfg(any(feature = "mp3", feature = "ogg"))]
use vcw_export::encoder::Quality;
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

/// The sound chunk of an AIFF file, walked rather than assumed.
///
/// [`data_chunk`]'s twin, and separate from it because almost nothing is
/// shared: IFF counts big-endian, the sound data starts eight bytes into its
/// chunk rather than at the chunk's own start, and the form type is checked at
/// a different offset.
fn ssnd_chunk(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("read");
    assert_eq!(&bytes[0..4], b"FORM");
    assert_eq!(&bytes[8..12], b"AIFF");
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_be_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if id == b"SSND" {
            // offset and blockSize, then the samples.
            return bytes[at + 16..at + 8 + size].to_vec();
        }
        at += 8 + size + size % 2;
    }
    panic!("no SSND chunk in {}", path.display());
}

#[test]
fn ffprobe_agrees_about_every_aiff_we_write() {
    let Some(ffprobe) = tool("ffprobe") else {
        eprintln!("skipped: no ffprobe");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    // The whole point of this one is the `be` on the end of every codec name.
    // Our own tests compare the bytes against our own swap, which proves the
    // wiring and not the byte order; this asks a reader that has never seen
    // our code which way round the samples are.
    let cases: &[(StorageFormat, u32, u16, &str)] = &[
        (StorageFormat::Int16, 44_100, 2, "pcm_s16be"),
        (StorageFormat::Int24Packed, 96_000, 2, "pcm_s24be"),
        (StorageFormat::Int24Padded, 96_000, 2, "pcm_s24be"),
        (StorageFormat::Int32, 192_000, 2, "pcm_s32be"),
        (StorageFormat::Int24Packed, 48_000, 4, "pcm_s24be"),
    ];

    for &(format, rate, channels, codec) in cases {
        let spec = Spec {
            rate,
            channels,
            format,
            frames: 4_000,
        };
        let path = dir
            .path()
            .join(format!("{format:?}-{rate}-{channels}.aiff"));
        write(&path, Container::Aiff, spec);

        let said = run(
            &ffprobe,
            &[
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,sample_rate,channels,duration_ts",
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
        // `numSampleFrames`, read back by somebody else. A size field patched
        // to the wrong one of the three gives a duration that is out by a
        // factor of the frame size, which nothing else here would notice.
        assert!(said.contains("duration_ts=4000"), "{what}: {said}");
    }
}

#[test]
fn an_aiff_holds_the_same_samples_a_wav_does() {
    // The byte order proved by a round trip rather than by a name: ffmpeg
    // decodes our AIFF back to little-endian PCM, and it has to come out as
    // the bytes that went in. A swap in the wrong direction - or a swap of the
    // wrong width on packed 24-bit - survives `pcm_s24be` and dies here.
    let Some(ffmpeg) = tool("ffmpeg") else {
        eprintln!("skipped: no ffmpeg");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec {
        rate: 44_100,
        channels: 2,
        format: StorageFormat::Int24Packed,
        frames: 10_000,
    };
    let aiff = dir.path().join("source.aiff");
    let stored = write(&aiff, Container::Aiff, spec);

    let back = dir.path().join("back.wav");
    run(
        &ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-i",
            aiff.to_str().unwrap(),
            "-c:a",
            "pcm_s24le",
            back.to_str().unwrap(),
        ],
    );
    assert_eq!(data_chunk(&back), stored, "the samples came back different");
}

#[test]
fn a_tagged_aiff_still_holds_the_bytes_we_wrote() {
    // [`a_tagged_wav_still_holds_the_bytes_we_wrote`] for the other chunked
    // container. lofty puts the `ID3 ` chunk in a different place in an IFF
    // file than in a RIFF one, and "a different place" is exactly the failure
    // this is looking for.
    let dir = tempfile::tempdir().unwrap();
    let spec = Spec {
        rate: 44_100,
        channels: 2,
        format: StorageFormat::Int24Packed,
        frames: 12_000,
    };
    let path = dir.path().join("tagged.aiff");
    let stored = write(&path, Container::Aiff, spec);

    let mut expected = stored.clone();
    for sample in expected.as_chunks_mut::<3>().0 {
        sample.reverse();
    }
    assert_eq!(ssnd_chunk(&path), expected);

    tagging::write(&path, Container::Aiff, &tags()).expect("tag");
    assert_eq!(ssnd_chunk(&path), expected, "tagging moved the audio");

    if let Some(ffprobe) = tool("ffprobe") {
        let said = run(
            &ffprobe,
            &[
                "-v",
                "error",
                "-show_entries",
                "format_tags=title",
                "-of",
                "default=nw=1",
                path.to_str().unwrap(),
            ],
        );
        assert!(
            said.contains("title="),
            "a tagged aiff ffprobe reads: {said}"
        );
    }
}

#[test]
fn at_least_one_verifier_is_installed() {
    let found: Vec<&str> = ["ffprobe", "ffmpeg", "flac", "metaflac", "sox", "ogginfo"]
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
        write(&path, Container::Flac(Compression::default()), spec);
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
        let stored = write(&path, Container::Flac(Compression::default()), spec);
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
    write(&ours, Container::Flac(Compression::default()), spec);

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
    write(&path, Container::Flac(Compression::default()), spec);

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

/// A short 44.1 kHz stereo spec - the one every container here accepts.
fn short() -> Spec {
    Spec {
        rate: 44_100,
        channels: 2,
        format: StorageFormat::Int16,
        frames: 12_000,
    }
}

/// Writes a short file and tags it, returning the path.
fn tagged(dir: &Path, container: Container) -> PathBuf {
    let spec = short();
    let path = dir.join(format!("tagged.{}", container.extension()));
    write(&path, container, spec);
    tagging::write(&path, container, &tags()).expect("tag");
    path
}

/// Every container this build can write, as the thing that can write them says.
///
/// Derived from `Container::ALL` and `Writer::vet` rather than written out,
/// which is the point: a container added to the enum and not to a list here
/// would ship untagged and unverified while every test in this file stayed
/// green. A build without the `mp3` feature drops MP3 from the list by the same
/// mechanism that refuses it at plan time.
fn tag_cases() -> Vec<Container> {
    Container::ALL
        .into_iter()
        .filter(|container| Writer::vet(*container, &short()).is_ok())
        .collect()
}

#[test]
fn this_build_writes_the_containers_it_is_supposed_to() {
    // The other end of `tag_cases`: a derived list cannot drift from the enum,
    // but it can quietly shrink if a feature stops being default. All three
    // lossless containers are unconditional, and a default build has all five.
    let cases = tag_cases();
    assert!(
        cases.contains(&Container::Flac(Compression::default())),
        "{cases:?}"
    );
    assert!(cases.contains(&Container::Wav), "{cases:?}");
    assert!(cases.contains(&Container::Aiff), "{cases:?}");
    if cfg!(all(feature = "mp3", feature = "ogg")) {
        assert_eq!(cases.len(), Container::ALL.len(), "{cases:?}");
    }
}

#[test]
fn metaflac_reads_the_tags_we_wrote() {
    let Some(metaflac) = tool("metaflac") else {
        eprintln!("skipped: no metaflac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = tagged(dir.path(), Container::Flac(Compression::default()));

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
    write(&path, Container::Flac(Compression::default()), spec);

    let before = run(&metaflac, &["--show-md5sum", path.to_str().unwrap()]);
    tagging::write(&path, Container::Flac(Compression::default()), &tags()).expect("tag");
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

    // Every container we write, and each must at least carry the fields a player
    // puts on screen. FLAC and Ogg carry the full set as Vorbis comments; WAV,
    // AIFF and MP3 carry ID3v2, which has frames for fewer of them, so only the
    // common fields are asserted across all five.
    for container in tag_cases() {
        let path = tagged(dir.path(), container);
        // Both, because where ffprobe files a comment depends on the container
        // and not on us: RIFF, ID3 and FLAC land on the format, and an Ogg
        // stream's comment header lands on the stream. Asking for one of the
        // two reports an empty set for the other, which reads as a tagging
        // failure for a file mutagen can see every field in.
        let said = probe(&ffprobe, &path, "format_tags:stream_tags");
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
    let flac = tagged(dir.path(), Container::Flac(Compression::default()));
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
    let path = tagged(dir.path(), Container::Flac(Compression::default()));
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

// ---------------------------------------------------------------------------
// The lossy containers (§33, D5, WP-25). Nothing below reads the audio with our
// own code: a lossy encoder's output is only meaningful to a decoder, and the
// decoders here are ffmpeg's and Xiph's.
// ---------------------------------------------------------------------------

/// A stereo tone: `left` Hz in the left channel, `right` Hz in the right.
///
/// Int16 at half scale. Two different frequencies on purpose - it is the only
/// thing in the file that tells the channels apart, and a planar fan-out is
/// exactly the kind of code that swaps them and still produces a file every
/// player will happily play.
fn tone(rate: u32, seconds: u32, left: f64, right: f64) -> (Spec, Vec<u8>) {
    let frames = u64::from(rate) * u64::from(seconds);
    let spec = Spec {
        rate,
        channels: 2,
        format: StorageFormat::Int16,
        frames,
    };
    let mut stored = Vec::with_capacity(frames as usize * 4);
    for frame in 0..frames {
        let at = frame as f64 / f64::from(rate);
        for hz in [left, right] {
            let sample = (std::f64::consts::TAU * hz * at).sin() * 16_000.0;
            stored.extend_from_slice(&(sample as i16).to_le_bytes());
        }
    }
    (spec, stored)
}

/// Writes a tone through a container and returns the file's size.
fn write_tone(path: &Path, container: Container, rate: u32, left: f64, right: f64) -> u64 {
    let (spec, stored) = tone(rate, 3, left, right);
    let mut writer = Writer::create(path, container, spec).expect("create");
    for piece in stored.chunks(997 * spec.stored_frame_bytes()) {
        writer.write(piece).expect("write");
    }
    let bytes = writer.finish().expect("finish");
    assert_eq!(
        bytes,
        std::fs::metadata(path).expect("stat").len(),
        "{container} reported a byte count that is not the size of the file it wrote"
    );
    bytes
}

/// What ffprobe says about a stream, as one string of `key=value` lines.
fn probe(ffprobe: &Path, path: &Path, entries: &str) -> String {
    run(
        ffprobe,
        &[
            "-v",
            "error",
            "-show_entries",
            entries,
            "-of",
            "default=nw=1",
            path.to_str().unwrap(),
        ],
    )
}

/// The rough pitch of one channel of a file, by way of ffmpeg and sox.
///
/// ffmpeg decodes and downmixes the one channel to a WAV - `pan` rather than
/// `-map_channel`, which silently produced an empty file - and `sox -n stat`
/// reports a "Rough frequency" by counting zero crossings. Rough is enough:
/// the question is 1 kHz against 3 kHz, not a cent of tuning.
fn pitch(ffmpeg: &Path, sox: &Path, path: &Path, channel: &str) -> f64 {
    let mono = path.with_extension(format!("{channel}.wav"));
    run(
        ffmpeg,
        &[
            "-v",
            "error",
            "-y",
            "-i",
            path.to_str().unwrap(),
            "-af",
            &format!("pan=mono|c0={channel}"),
            mono.to_str().unwrap(),
        ],
    );
    let said = run(sox, &[mono.to_str().unwrap(), "-n", "stat"]);
    let line = said
        .lines()
        .find(|line| line.contains("frequency"))
        .unwrap_or_else(|| panic!("sox said nothing about frequency:\n{said}"));
    line.rsplit(':')
        .next()
        .and_then(|number| number.trim().parse().ok())
        .unwrap_or_else(|| panic!("could not read a frequency out of {line:?}"))
}

#[test]
#[cfg(feature = "mp3")]
fn ffprobe_agrees_about_the_mp3_we_write() {
    let Some(ffprobe) = tool("ffprobe") else {
        eprintln!("skipped: no ffprobe");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    // The three rates a rip plausibly arrives at that MPEG also defines. The
    // five 192 kHz rips in the corpus are refused rather than resampled, which
    // is `lossy::mp3_limits`' job and tested there.
    for rate in [32_000, 44_100, 48_000] {
        let path = dir.path().join(format!("{rate}.mp3"));
        write_tone(&path, Container::Mp3(Quality::High), rate, 1_000.0, 3_000.0);
        let said = probe(
            &ffprobe,
            &path,
            "stream=codec_name,sample_rate,channels:format=duration",
        );
        assert!(said.contains("codec_name=mp3"), "{rate}: {said}");
        assert!(
            said.contains(&format!("sample_rate={rate}")),
            "{rate}: {said}"
        );
        assert!(said.contains("channels=2"), "{rate}: {said}");

        // The duration is the test of the VBR header. Without a patched Xing
        // frame a decoder has to guess the length from the file size and the
        // first frame's bitrate, and for variable-bitrate audio that guess is
        // wrong - which is the whole reason `finish` seeks back to byte zero.
        let duration: f64 = said
            .lines()
            .find_map(|line| line.strip_prefix("duration="))
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("{rate}: no duration in {said}"));
        assert!(
            (duration - 3.0).abs() < 0.1,
            "{rate} Hz: three seconds of audio came back as {duration} s"
        );
    }
}

#[test]
#[cfg(feature = "mp3")]
fn the_vbr_header_is_a_real_one_and_not_the_placeholder() {
    // What the seek-back in `finish` writes over. libmp3lame emits a blank
    // frame at the top of the first encode call and expects it to be replaced
    // once the whole file is known; if the replacement never happens the frame
    // stays zeroed and nothing complains, so the string is worth looking for
    // directly rather than only through a decoder's duration.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vbr.mp3");
    write_tone(
        &path,
        Container::Mp3(Quality::High),
        44_100,
        1_000.0,
        3_000.0,
    );
    let head = std::fs::read(&path).expect("read");
    let head = &head[..2_000.min(head.len())];
    let found = |needle: &[u8]| head.windows(needle.len()).any(|at| at == needle);
    assert!(found(b"Xing"), "no Xing frame in the first 2 kB");
    assert!(found(b"LAME"), "no LAME version string in the first 2 kB");
}

#[test]
#[cfg(feature = "ogg")]
fn ffprobe_agrees_about_the_ogg_we_write() {
    let Some(ffprobe) = tool("ffprobe") else {
        eprintln!("skipped: no ffprobe");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    // Including 192 kHz, which is the point of having Ogg at all: it is the only
    // container besides WAV that takes a 192 kHz capture, and the only one that
    // takes one and also makes it small.
    for rate in [44_100, 48_000, 192_000] {
        let path = dir.path().join(format!("{rate}.ogg"));
        write_tone(
            &path,
            Container::OggVorbis(Quality::High),
            rate,
            1_000.0,
            3_000.0,
        );
        let said = probe(
            &ffprobe,
            &path,
            "stream=codec_name,sample_rate,channels:format=duration",
        );
        assert!(said.contains("codec_name=vorbis"), "{rate}: {said}");
        assert!(
            said.contains(&format!("sample_rate={rate}")),
            "{rate}: {said}"
        );
        assert!(said.contains("channels=2"), "{rate}: {said}");
        let duration: f64 = said
            .lines()
            .find_map(|line| line.strip_prefix("duration="))
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("{rate}: no duration in {said}"));
        assert!(
            (duration - 3.0).abs() < 0.05,
            "{rate} Hz: three seconds of audio came back as {duration} s"
        );
    }
}

#[test]
#[cfg(feature = "ogg")]
fn ogginfo_finds_nothing_wrong_with_our_ogg() {
    // Xiph's own validator, and the one reader that checks the page structure
    // rather than just decoding what it can. A stream with a bad granule
    // position or a missing end-of-stream flag plays fine and fails here.
    let Some(ogginfo) = tool("ogginfo") else {
        eprintln!("skipped: no ogginfo");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clean.ogg");
    write_tone(
        &path,
        Container::OggVorbis(Quality::High),
        44_100,
        1_000.0,
        3_000.0,
    );

    let said = run(&ogginfo, &[path.to_str().unwrap()]);
    assert!(
        !said.to_lowercase().contains("warning"),
        "ogginfo complained:\n{said}"
    );
    assert!(said.contains("Channels: 2"), "{said}");
    assert!(said.contains("Rate: 44100"), "{said}");
}

#[test]
fn the_channels_survive_a_lossy_encoder_in_the_order_they_went_in() {
    let (Some(ffmpeg), Some(sox)) = (tool("ffmpeg"), tool("sox")) else {
        eprintln!("skipped: needs both ffmpeg and sox");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    // FLAC as the control. If the fan-out were swapping channels this would
    // still pass, which is exactly why it is here: it says the tone generator
    // and the two measuring tools agree about which channel is which before any
    // claim is made about the encoders.
    let cases = std::iter::once(Container::Flac(Compression::default())).chain(
        tag_cases()
            .into_iter()
            .filter(|container| container.is_lossy()),
    );

    for container in cases {
        let path = dir.path().join(format!("tone.{}", container.extension()));
        write_tone(&path, container, 44_100, 1_000.0, 3_000.0);

        let left = pitch(&ffmpeg, &sox, &path, "FL");
        let right = pitch(&ffmpeg, &sox, &path, "FR");
        assert!(
            (left - 1_000.0).abs() < 50.0,
            "{container}: the left channel came back at {left} Hz, not 1 kHz"
        );
        assert!(
            (right - 3_000.0).abs() < 150.0,
            "{container}: the right channel came back at {right} Hz, not 3 kHz"
        );
    }
}

#[test]
#[cfg(any(feature = "mp3", feature = "ogg"))]
fn a_lower_quality_really_does_write_a_smaller_file() {
    // The setting is three words that travel from a select in the panel through
    // a settings file, a JSON command, a CLI flag and two different encoder
    // builders. Every step of that could drop it and still produce a playable
    // file at the default, so the only honest check is that the three levels
    // differ in the direction they say they do.
    let dir = tempfile::tempdir().unwrap();

    for container in tag_cases()
        .into_iter()
        .filter(|container| container.is_lossy())
    {
        let sizes: Vec<(Quality, u64)> = Quality::ALL
            .into_iter()
            .map(|quality| {
                let container = container.with_quality(quality);
                let path = dir
                    .path()
                    .join(format!("{quality}.{}", container.extension()));
                (
                    quality,
                    write_tone(&path, container, 44_100, 1_000.0, 3_000.0),
                )
            })
            .collect();

        for pair in sizes.windows(2) {
            let [(better, bigger), (worse, smaller)] = [pair[0], pair[1]];
            assert!(
                bigger > smaller,
                "{container}: {better} wrote {bigger} bytes and {worse} wrote {smaller}, \
                 so the quality is being parsed and then ignored"
            );
        }
        // And all three are far smaller than the 529 KiB of PCM that went in,
        // which is the other half of the claim: a lossy container that came out
        // bigger than the audio would be a copy with extra steps.
        for (quality, bytes) in sizes {
            assert!(
                bytes < 300_000,
                "{container} at {quality} wrote {bytes} bytes for three seconds of a tone"
            );
        }
    }
}

#[test]
#[cfg(any(feature = "mp3", feature = "ogg"))]
fn mutagen_reads_the_lossy_tags_we_wrote() {
    // The two tag formats again, in the two files that are their native homes:
    // ID3v2 in an MP3 rather than bolted into a RIFF chunk, and Vorbis comments
    // in an Ogg stream rather than in a FLAC metadata block. lofty picks the
    // backend from the file it is handed, so this is the test that it picked
    // right - and that the cover survives being base64'd into a comment, which
    // is how Ogg carries one.
    let Some(python) = tool("python3") else {
        eprintln!("skipped: no python3");
        return;
    };
    if !run(&python, &["-c", "import mutagen"]).is_empty() {
        eprintln!("skipped: python3 has no mutagen");
        return;
    }
    let dir = tempfile::tempdir().unwrap();

    #[cfg(feature = "mp3")]
    {
        let path = tagged(dir.path(), Container::Mp3(Quality::High));
        let script = "
import sys, mutagen
mp3 = mutagen.File(sys.argv[1])
assert str(mp3.tags['TIT2']) == \"L'Enfant Sauvage\", mp3.tags['TIT2']
assert str(mp3.tags['TALB']) == 'Terra Incognita', mp3.tags['TALB']
assert 'Gojira' in str(mp3.tags['TPE1']), mp3.tags['TPE1']
assert str(mp3.tags['TRCK']) == '3/11', mp3.tags['TRCK']
pictures = mp3.tags.getall('APIC')
assert len(pictures) == 1, pictures
assert pictures[0].mime == 'image/png', pictures[0].mime
assert round(mp3.info.length, 1) == 0.3, mp3.info.length
print('ok')
";
        let said = run(&python, &["-c", script, path.to_str().unwrap()]);
        assert!(said.trim_end().ends_with("ok"), "mutagen on mp3:\n{said}");
    }

    #[cfg(feature = "ogg")]
    {
        let path = tagged(dir.path(), Container::OggVorbis(Quality::High));
        let script = "
import base64, sys, mutagen
from mutagen.flac import Picture
ogg = mutagen.File(sys.argv[1])
assert ogg.tags['title'] == [\"L'Enfant Sauvage\"], ogg.tags['title']
assert ogg.tags['artist'] == ['Gojira', 'Joe Duplantier'], ogg.tags['artist']
assert ogg.tags['genre'] == ['Metal', 'Progressive'], ogg.tags['genre']
assert ogg.tags['discogs_releaseid'] == ['3778213']
blocks = ogg.tags['metadata_block_picture']
assert len(blocks) == 1, blocks
picture = Picture(base64.b64decode(blocks[0]))
assert picture.mime == 'image/png', picture.mime
assert picture.type == 3, picture.type
print('ok')
";
        let said = run(&python, &["-c", script, path.to_str().unwrap()]);
        assert!(said.trim_end().ends_with("ok"), "mutagen on ogg:\n{said}");
    }
}
