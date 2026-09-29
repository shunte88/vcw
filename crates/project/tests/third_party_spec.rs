/*
 *  third_party_spec.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-18's exit criterion: a third-party tool reads a project from the spec alone.
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
//! WP-18's exit criterion: a third-party tool reads a project from the spec alone.
//!
//! §49 says the format shall be usable by third-party tools without the GUI, and
//! WP-18's exit criterion is that a third-party tool can read a project using the
//! specification alone. That is a claim about `docs/SCHEMA.md`, not about this
//! crate, so it cannot be tested from inside the crate's own API.
//!
//! `tools/vcw-read.py` is the third party. It is Python, it uses nothing but the
//! standard library, and it was written against the document rather than against
//! the code - which is why writing it changed the document: the endianness of a
//! sample, the layout of `Int24Padded`, which CRC-32 `checksum` is, whether a
//! track span is half-open and the required `meta` keys were all things the
//! specification expected a reader to already know. They are in it now.
//!
//! This test drives that reader over a project the *product* wrote and checks
//! the audio it gets out is the audio the product's own reader returns, frame for
//! frame. It is a comparison between two implementations of one document, which
//! is the only form the criterion can honestly take: comparing the document to
//! the code would only prove the generator works.

use std::path::{Path, PathBuf};
use std::process::Command;

use vcw_project::persistence::{Config, Writer};
use vcw_project::{Project, pcm, release, side, track};
use vcw_types::vinyl::Side;
use vcw_types::{CaptureInfo, CaptureMode, CaptureState, SampleRate, StorageFormat};

/// The tool under test, found relative to the crate rather than to the cwd.
fn reader() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/vcw-read.py");
    assert!(
        path.is_file(),
        "{} is missing; it is the subject of this test, not an optional extra",
        path.display()
    );
    path
}

/// A Python 3 interpreter, however this platform spells it.
///
/// Required, not optional. A test that turns itself off when the interpreter is
/// absent would report that the specification is readable on a machine where
/// nothing had read it, and this project has been bitten by exactly that.
fn python() -> Command {
    for (program, args) in [
        ("python3", &[][..]),
        ("python", &[][..]),
        ("py", &["-3"][..]),
    ] {
        let mut probe = Command::new(program);
        probe.args(args).arg("--version");
        if probe
            .output()
            .is_ok_and(|out| out.status.success() && out.stdout.starts_with(b"Python 3"))
        {
            let mut found = Command::new(program);
            found.args(args);
            return found;
        }
        // `py --version` prints to stdout on 3.x but a 2.x interpreter prints to
        // stderr, so a failed match above is "not Python 3" rather than "absent".
    }
    panic!(
        "no Python 3 interpreter found (tried python3, python, py -3). \
         WP-18's exit criterion is that a third-party tool can read a project \
         from the spec alone, and this is the third-party tool."
    );
}

/// Runs the reader and returns its stdout, failing with both streams.
fn read(args: &[&str]) -> String {
    let out = python()
        .arg(reader())
        .args(args)
        .output()
        .expect("run the reader");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "vcw-read.py {} failed:\n{stdout}{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// Runs the reader expecting a non-zero exit, and returns both its streams.
///
/// Both, because a refusal and a report are different things: the reader prints
/// what it found on stdout and says why it is giving up on stderr, and a test
/// that only reads one of them cannot tell "refused for the right reason" from
/// "refused at all".
fn refused(args: &[&str]) -> String {
    let out = python()
        .arg(reader())
        .args(args)
        .output()
        .expect("run the reader");
    assert!(
        !out.status.success(),
        "vcw-read.py {} was supposed to refuse, and printed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout)
    );
    let mut said = String::from_utf8_lossy(&out.stdout).into_owned();
    said.push_str(&String::from_utf8_lossy(&out.stderr));
    said
}

/// A ramp, as interleaved stereo frames of the given format.
///
/// A ramp rather than silence or noise: silence cannot tell a correct channel
/// layout from a swapped one, and noise makes a failure impossible to read. Each
/// channel gets a different slope so an interleaving mistake shows up as the
/// wrong slope rather than as nothing.
fn ramp(format: StorageFormat, frames: u32, channels: u16) -> Vec<u8> {
    let width = format.bytes_per_sample();
    let mut out = Vec::with_capacity(frames as usize * channels as usize * width);
    for frame in 0..frames {
        for channel in 0..channels {
            let value = i32::try_from(frame).unwrap() * (i32::from(channel) + 1);
            let bytes = value.to_le_bytes();
            match format {
                StorageFormat::Int16 => out.extend_from_slice(&bytes[..2]),
                StorageFormat::Int24Packed => out.extend_from_slice(&bytes[..3]),
                _ => out.extend_from_slice(&bytes),
            }
        }
    }
    out
}

/// Everything the test needs to know about the project it wrote.
struct Written {
    _dir: tempfile::TempDir,
    path: PathBuf,
    capture_id: i64,
    frames: u64,
    audio: Vec<u8>,
}

/// Writes a project through the product's own capture writer, then lays a record
/// over it: a side, two titled tracks and a release.
///
/// The writer rather than hand-built INSERTs on purpose. The claim under test is
/// that a third party can read *what VCW writes*, so anything synthesised here
/// would be testing the spec against a second guess at the format.
fn written(format: StorageFormat, frames: u32) -> Written {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("spec.vcw");
    let project = Project::create(&path).expect("create");

    let info = CaptureInfo::unverified(SampleRate(48_000), 2, format, CaptureMode::Shared);
    let audio = ramp(format, frames, 2);
    let mut writer = Writer::begin(project, &info, Config::default()).expect("begin");
    writer.push(&audio).expect("push");
    writer.flush().expect("flush");
    let (_, mut project, session) = writer
        .finish_with_project(CaptureState::Finalised)
        .expect("finish");
    let capture_id = session.id();

    side::attach(&mut project, Side::A, capture_id).expect("attach the capture to side A");
    let half = u64::from(frames) / 2;
    for (title, (start, end)) in [
        ("Europe Endless", (0, half)),
        ("Hall of Mirrors", (half, u64::from(frames))),
    ] {
        let track_id = track::add_track(&mut project, Side::A, start, end).expect("add a track");
        track::update(&mut project, track_id, &track::Update::title(title)).expect("title it");
    }

    let mut release = release::ensure(&mut project).expect("a release row");
    release.album = "Trans-Europe Express".to_owned();
    release.album_artist = "Kraftwerk".to_owned();
    release.year = Some(1977);
    release.genres = vec!["Electronic".to_owned(), "Krautrock".to_owned()];
    release::store(&mut project, &release).expect("store the release");
    project.close().expect("close");

    Written {
        _dir: dir,
        path,
        capture_id,
        frames: u64::from(frames),
        audio,
    }
}

/// The data chunk of a WAV file, found by walking the chunks.
fn data_chunk(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("read the extraction");
    assert_eq!(&bytes[0..4], b"RIFF");
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"data" {
            return bytes[at + 8..at + 8 + size].to_vec();
        }
        at += 8 + size + size % 2;
    }
    panic!("no data chunk in {}", path.display());
}

/// The whole of a capture, read through the path playback uses.
fn product_audio(path: &Path, capture_id: i64) -> Vec<u8> {
    let project = Project::open_read_only(path).expect("open read-only");
    let layout = pcm::Layout::of(project.conn(), capture_id).expect("layout");
    let mut source = pcm::Reader::open(project.conn(), capture_id, layout.span()).expect("reader");
    let mut audio = Vec::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let filled = source.fill(&mut buffer).expect("fill");
        if filled == 0 {
            break;
        }
        audio.extend_from_slice(&buffer[..filled]);
    }
    audio
}

#[test]
fn a_third_party_reader_agrees_about_what_the_project_holds() {
    let project = written(StorageFormat::Int24Packed, 12_000);
    let path = project.path.display().to_string();
    let printed = read(&[&path, "identify", "--json"]);
    let said: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);

    assert_eq!(said["schema_version"], vcw_project::SCHEMA_VERSION);
    assert_eq!(said["format_version"], vcw_project::FORMAT_VERSION);
    assert_eq!(said["captures"][0]["sample_rate"], 48_000);
    assert_eq!(said["captures"][0]["channels"], 2);
    assert_eq!(said["captures"][0]["storage_format"], "Int24Packed");
    assert_eq!(said["captures"][0]["bytes_per_sample"], 3);
    assert_eq!(said["captures"][0]["frames"], project.frames);
    assert_eq!(said["captures"][0]["state"], "finalised");
    assert_eq!(said["captures"][0]["interrupted"], false);

    // The record over the audio, which is the half a third party cannot guess.
    assert_eq!(said["release"]["album"], "Trans-Europe Express");
    assert_eq!(said["release"]["album_artist"], "Kraftwerk");
    assert_eq!(said["release"]["year"], 1977);
    assert_eq!(said["release"]["genres"][1], "Krautrock");
    assert_eq!(said["sides"][0]["side"], "A");
    assert_eq!(said["sides"][0]["disc"], 1);
    assert_eq!(said["sides"][0]["capture_id"], project.capture_id);

    let tracks = said["sides"][0]["tracks"].as_array().expect("tracks");
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0]["title"], "Europe Endless");
    assert_eq!(tracks[0]["start_frame"], 0);
    assert_eq!(tracks[0]["end_frame"], project.frames / 2);
    assert_eq!(tracks[1]["start_frame"], project.frames / 2);
    assert_eq!(tracks[1]["end_frame"], project.frames);
}

#[test]
fn a_third_party_reader_extracts_the_same_bytes_the_product_does() {
    // Every stored format, because the widths are where a specification earns
    // its keep: 3 is the awkward one, 4 is ambiguous between three meanings, and
    // a reader that guessed would still pass on 16-bit.
    for format in StorageFormat::ALL {
        let project = written(format, 6_000);
        let path = project.path.display().to_string();
        let out = project.path.with_extension("whole.wav");

        read(&[
            &path,
            "extract",
            "--capture",
            &project.capture_id.to_string(),
            "--out",
            &out.display().to_string(),
        ]);

        let third_party = data_chunk(&out);
        let product = product_audio(&project.path, project.capture_id);
        assert_eq!(
            third_party.len(),
            product.len(),
            "{format:?}: the third-party reader got {} bytes, the product {}",
            third_party.len(),
            product.len()
        );
        assert!(
            third_party == product,
            "{format:?}: the third-party reader and the product disagree about the \
             audio. docs/SCHEMA.md is what stands between them, so one of the two \
             is reading it wrong."
        );
        // And both agree with what was pushed in, which is what makes this a
        // round trip rather than two readers sharing a misreading.
        assert!(
            third_party == project.audio,
            "{format:?}: neither reader returned the bytes the writer was given"
        );
    }
}

#[test]
fn a_third_party_reader_resolves_a_track_span_the_same_way() {
    // The half-open span, which the specification did not state until this test
    // needed it. A reader that treated `end` as inclusive would be one frame long
    // on every track, and the error would look like rounding.
    //
    // Track 1 is what proves it, not track 2. Track 2 ends where the capture
    // ends, so an extra frame gets clamped away by the end of the audio and an
    // inclusive reader looks correct; the first of two adjacent tracks has room
    // to be wrong. That hole was in this test until the mutation check found it.
    let project = written(StorageFormat::Float32, 8_000);
    let path = project.path.display().to_string();
    let width = StorageFormat::Float32.bytes_per_sample() * 2;
    let half = (project.frames / 2) as usize;

    let first = project.path.with_extension("a1.wav");
    read(&[
        &path,
        "extract",
        "--side",
        "A",
        "--track",
        "1",
        "--out",
        &first.display().to_string(),
    ]);
    let extracted = data_chunk(&first);
    assert_eq!(
        extracted.len(),
        half * width,
        "track 1 is [0, {half}), so exactly half the capture and not a frame more"
    );
    assert_eq!(extracted, project.audio[..half * width]);

    let second = project.path.with_extension("a2.wav");
    read(&[
        &path,
        "extract",
        "--side",
        "A",
        "--track",
        "2",
        "--out",
        &second.display().to_string(),
    ]);
    let extracted = data_chunk(&second);
    assert_eq!(
        extracted.len(),
        half * width,
        "and track 2 is the other half"
    );
    assert_eq!(
        extracted,
        project.audio[half * width..],
        "track 2 starts where track 1 stopped: one boundary, no gap and no overlap"
    );
}

#[test]
fn a_third_party_reader_can_check_the_project_it_was_given() {
    let project = written(StorageFormat::Int16, 4_000);
    let path = project.path.display().to_string();
    let printed = read(&[&path, "verify"]);
    assert!(printed.contains("0 problem(s)"), "got {printed}");

    // And it has to notice a damaged block, or the check is decoration. One byte
    // in one blob, which is the failure the checksum column exists for: SQLite's
    // own integrity check cannot see it, because a corrupted blob is still a
    // valid blob.
    {
        let conn = rusqlite::Connection::open(&project.path).expect("open to damage");
        conn.execute(
            "UPDATE sampleblocks SET samples = zeroblob(length(samples)) \
             WHERE blockid = (SELECT MIN(blockid) FROM sampleblocks)",
            [],
        )
        .expect("damage one block");
    }
    let said = refused(&[&path, "verify"]);
    assert!(
        said.contains("checksum") && said.contains("1 problem(s)"),
        "the reader did not report the damaged block: {said}"
    );
}

#[test]
fn a_third_party_reader_refuses_what_it_should_not_read() {
    let dir = tempfile::tempdir().expect("a temporary directory");

    // Not a database at all.
    let sleeve = dir.path().join("sleeve.jpg");
    std::fs::write(&sleeve, b"\xff\xd8\xff\xe0 not sqlite").expect("write");
    let said = refused(&[&sleeve.display().to_string(), "identify"]);
    assert!(said.contains("not a SQLite database"), "got {said}");

    // An Audacity project, which is the near miss that matters: same idea, same
    // `sampleblocks` table, different `application_id`. The specification tells a
    // reader to dispatch on that word, and this is the file that proves it did.
    let audacity = dir.path().join("looks-familiar.aup3");
    {
        let conn = rusqlite::Connection::open(&audacity).expect("create");
        conn.pragma_update(None, "application_id", 0x4155_4459_i64)
            .expect("Audacity's application_id");
        conn.pragma_update(None, "user_version", 0x0307_0000_i64)
            .expect("3.7.0.0");
        conn.execute_batch("CREATE TABLE project (doc BLOB)")
            .expect("something on disk");
    }
    let said = refused(&[&audacity.display().to_string(), "identify"]);
    assert!(
        said.contains("Audacity"),
        "the refusal should name what the file actually is: {said}"
    );
}
