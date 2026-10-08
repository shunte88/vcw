/*
 *  import_from_cli.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The import verb through the shipped binary, and the round trip out to a tagged file.
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
//! The import verb through the shipped binary, and the round trip out to a
//! tagged file (§12, §33).
//!
//! `crates/import`'s own tests prove the grammar, the timeline and the landing.
//! This proves the *verb*, and then keeps going: an imported project is put
//! through `vcw tracks` and `vcw export` without any of them being told where
//! the audio came from. That is the whole claim of WP-20 - an Audacity project
//! becomes a capture, and the rest of the toolchain works on it unchanged - so
//! it is asserted by walking the chain rather than by reading the code.
//!
//! The CI half uses `rate-trap.aup3`, which is real Audacity bytes: 24 of the
//! user's 25 rips are float32 and this is one of them, which is why the export
//! here is WAV and why the FLAC refusal is worth a test of its own. Its audio is
//! 256 frames, so the whole chain runs in about a second.
//!
//! The corpus half is `#[ignore]`d and does the thing the plan actually asks
//! for: one AUP3 and its AUP4 conversion, each imported and exported, with the
//! two exports compared byte for byte. Two generations of one Audacity project
//! have to leave VCW as the same audio, and nothing smaller than the real files
//! can say so.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

/// Where the real projects are, for the `#[ignore]`d half. Required when those
/// run; see `crates/import/tests/corpus.rs` for why it is not optional.
const CORPUS_ENV: &str = "VCW_AUP_CORPUS";
/// Where a landing may put several hundred megabytes. `/tmp` is tmpfs here.
const SCRATCH_ENV: &str = "VCW_SCRATCH";

/// Runs the binary and returns stdout, failing loudly with both streams.
fn vcw(args: &[&str]) -> String {
    let out = Command::new(VCW).args(args).output().expect("run vcw");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "vcw {} failed:\n{stdout}{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// Runs the binary expecting a refusal, and returns what it said.
fn refused(args: &[&str]) -> String {
    let out = Command::new(VCW).args(args).output().expect("run vcw");
    assert!(
        !out.status.success(),
        "vcw {} was supposed to fail",
        args.join(" ")
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A committed fixture, which is real Audacity bytes with the audio shrunk.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../import/tests/fixtures")
        .join(name)
}

/// A directory to work in, on real storage if the operator said where.
fn scratch() -> tempfile::TempDir {
    match std::env::var(SCRATCH_ENV) {
        Ok(dir) => tempfile::tempdir_in(dir).expect("a temporary directory in VCW_SCRATCH"),
        Err(_) => tempfile::tempdir().expect("a temporary directory"),
    }
}

/// The data chunk of a WAV file, found by walking the chunks.
///
/// Offset 44 is a trap: the exported header is `WAVE_FORMAT_EXTENSIBLE`, so the
/// `fmt ` chunk is 40 bytes and there is an `id3 ` chunk after the audio.
fn data_chunk(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("read the export");
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

/// One ID3v2 text frame out of a WAV's `id3 ` chunk.
///
/// Read by hand rather than with `lofty` on purpose: the export wrote those
/// bytes with `lofty`, and a reader that shares the writer's idea of the format
/// cannot catch the writer being wrong about it. Frame sizes are synchsafe, and
/// a text frame's payload is an encoding byte followed by the string.
fn id3_text(path: &Path, frame: &[u8; 4]) -> Option<String> {
    let bytes = std::fs::read(path).expect("read the export");
    let mut at = 12;
    let tag = loop {
        assert!(at + 8 <= bytes.len(), "no id3 chunk in {}", path.display());
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if bytes[at..at + 4].eq_ignore_ascii_case(b"id3 ") {
            break &bytes[at + 8..at + 8 + size];
        }
        at += 8 + size + size % 2;
    };
    assert_eq!(&tag[0..3], b"ID3", "the chunk does not hold an ID3 tag");

    let mut at = 10;
    while at + 10 <= tag.len() {
        let id = &tag[at..at + 4];
        if id == [0, 0, 0, 0] {
            return None; // padding
        }
        let size = tag[at + 4..at + 8]
            .iter()
            .fold(0_usize, |acc, byte| (acc << 7) | usize::from(byte & 0x7F));
        let payload = &tag[at + 10..at + 10 + size];
        if id == frame {
            let text = payload.get(1..).unwrap_or_default();
            return Some(
                String::from_utf8_lossy(text)
                    .trim_end_matches('\0')
                    .to_owned(),
            );
        }
        at += 10 + size;
    }
    None
}

/// Every regular file under a directory, recursively. Empty if it is not there.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// Imports a project and returns the JSON report and the project's path.
fn import(source: &Path, destination: &Path) -> (serde_json::Value, PathBuf) {
    let printed = vcw(&[
        "import",
        &source.display().to_string(),
        "--output",
        &destination.display().to_string(),
        "--json",
    ]);
    let report: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);
    (report, destination.to_path_buf())
}

#[test]
fn a_dry_run_reads_a_real_project_and_writes_nothing() {
    // The rate trap, end to end through the verb. `project/@rate` says 192000
    // and the audio is at 48000; a dry run that reported the wrong one would be
    // a dry run that lied about what it was going to do.
    let dir = scratch();
    let destination = dir.path().join("rate-trap.vcw");
    let printed = vcw(&[
        "import",
        &fixture("rate-trap.aup3").display().to_string(),
        "--output",
        &destination.display().to_string(),
        "--dry-run",
        "--json",
    ]);
    let report: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);

    assert_eq!(report["dry_run"], true);
    assert_eq!(report["format"], "aup3");
    assert_eq!(report["rate"], 48_000);
    assert_eq!(report["editor_rate_preference"], 192_000.0);
    assert_eq!(report["channels"], 2);
    assert_eq!(report["storage_format"], "Float32");
    assert_eq!(report["frames"], 256);
    assert_eq!(report["orphan_blocks"], 0);
    assert!(
        !destination.exists(),
        "a dry run wrote {}",
        destination.display()
    );
}

#[test]
fn an_imported_project_round_trips_to_a_tagged_export() {
    let dir = scratch();
    let destination = dir.path().join("rate-trap.vcw");
    let (report, project) = import(&fixture("rate-trap.aup3"), &destination);
    let path = project.display().to_string();

    // What landed. The three labels all sit over audio the shrinker removed,
    // which is also the shape of a project somebody deleted a clip from, so
    // they are reported rather than silently dropped.
    assert_eq!(report["capture_id"], 1);
    assert_eq!(report["side"], "A");
    assert_eq!(report["rate"], 48_000);
    assert_eq!(report["frames"], 256);
    assert_eq!(report["tracks"], 0);
    assert_eq!(report["tags"], 2);
    let skipped = report["labels_skipped"].as_array().expect("labels_skipped");
    assert_eq!(skipped.len(), 3);
    assert!(
        skipped[0]["why"]
            .as_str()
            .expect("a reason")
            .contains("end of the audio"),
        "got {}",
        report["labels_skipped"]
    );

    // The tags came across as a release, which is what makes the export tagged.
    let printed = vcw(&["release", &path, "--json", "show"]);
    let release: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);
    assert_eq!(release["album"], "Stardonas (Transverse)");
    // An import states neither intent, so both read false before anyone is asked.
    assert_eq!(release["mono"], false);
    assert_eq!(release["riaa_eq"], false);

    // And they are settable, separately: a stereo pressing that still wants the
    // curve is the ordinary case, so one flag must not carry the other.
    vcw(&["release", &path, "set", "--riaa", "true"]);
    let printed = vcw(&["release", &path, "--json", "show"]);
    let release: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);
    assert_eq!(release["mono"], false);
    assert_eq!(release["riaa_eq"], true);

    // A track, added by the same verb a recorded capture uses. 240 frames of
    // the 256 that landed, so the export has to resolve a span and not just
    // copy the capture.
    vcw(&[
        "tracks", &path, "add", "--side", "A", "--start", "0", "--end", "0.005",
    ]);
    vcw(&["tracks", &path, "set", "1", "--title", "Transverse"]);

    let out = dir.path().join("out");
    let printed = vcw(&[
        "export",
        &path,
        "--into",
        &out.display().to_string(),
        "--format",
        "wav",
        "--json",
    ]);
    let export: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);
    assert_eq!(export["container"], "wav");
    assert_eq!(export["report"]["files"], 1);
    assert_eq!(export["frames"], 240);

    let file = Path::new(export["items"][0]["path"].as_str().expect("a path"));
    assert!(file.is_file(), "{} was not written", file.display());
    assert_eq!(
        data_chunk(file).len(),
        240 * 2 * 4,
        "240 stereo float32 frames"
    );

    // And the tags, read out of the bytes rather than out of the writer.
    assert_eq!(id3_text(file, b"TIT2").as_deref(), Some("Transverse"));
    assert_eq!(
        id3_text(file, b"TALB").as_deref(),
        Some("Stardonas (Transverse)"),
        "the album came from an Audacity ALBUM tag and survived to the file"
    );
    // `number/total`, which is how ID3v2 spells a track number. The total is
    // the count on this track's own disc, so a one-track import is `1/1` and a
    // double album's second record is `1/8` upwards rather than `9/17`.
    assert_eq!(id3_text(file, b"TRCK").as_deref(), Some("1/1"));
}

#[test]
fn a_float32_import_cannot_be_flac_and_is_told_so_before_anything_is_written() {
    // 24 of the user's 25 rips are float32, so this is the ordinary case and not
    // an edge one. The refusal has to arrive before the first file, because a
    // half-written album is worse than a rejected command - and that is the
    // property this test is really about, which is why it still asks for the
    // refusal now that `--narrow refuse` has to be said out loud.
    let dir = scratch();
    let destination = dir.path().join("rate-trap.vcw");
    let (_, project) = import(&fixture("rate-trap.aup3"), &destination);
    let path = project.display().to_string();
    vcw(&[
        "tracks", &path, "add", "--side", "A", "--start", "0", "--end", "0.005",
    ]);

    let out = dir.path().join("out");
    let said = refused(&[
        "export",
        &path,
        "--into",
        &out.display().to_string(),
        "--format",
        "flac",
        "--narrow",
        "refuse",
    ]);
    assert!(
        said.contains("FLAC") && said.contains("WAV"),
        "the refusal should name the format that will work: {said}"
    );
    // Nothing at all, not even the directory. This used to allow an empty
    // `Album/` to be left behind, because the refusal came from the first
    // `Writer::create` and the plan had already made the directories by then.
    // The container is now vetted in `splitter::plan`, so a format the capture
    // cannot go into is refused before the filesystem is touched.
    assert!(
        files_under(&out).is_empty(),
        "a file was written under {} before the refusal: {:?}",
        out.display(),
        files_under(&out)
    );
    assert!(
        !out.exists(),
        "{} was created for an export that was refused",
        out.display()
    );
}

#[test]
fn a_destination_that_already_exists_is_refused() {
    // An import creates a file, so unlike `vcw recover` it defaults to doing the
    // work. That only stays safe if it will not walk over something.
    let dir = scratch();
    let destination = dir.path().join("taken.vcw");
    std::fs::write(&destination, b"not a project").expect("write");

    let said = refused(&[
        "import",
        &fixture("rate-trap.aup3").display().to_string(),
        "--output",
        &destination.display().to_string(),
    ]);
    assert!(
        said.contains("exists"),
        "the refusal should say why: {said}"
    );
    assert_eq!(
        std::fs::read(&destination).expect("read"),
        b"not a project",
        "the file that was already there was touched"
    );
}

#[test]
fn a_file_that_is_not_an_audacity_project_is_refused_by_the_verb() {
    let dir = scratch();
    let source = dir.path().join("sleeve.jpg");
    std::fs::write(&source, b"\xff\xd8\xff\xe0not audacity").expect("write");

    let said = refused(&["import", &source.display().to_string()]);
    assert!(
        said.to_lowercase().contains("audacity") || said.to_lowercase().contains("sqlite"),
        "the refusal should say what it was looking for: {said}"
    );
}

#[test]
#[ignore = "reads the real corpus; set VCW_AUP_CORPUS and run with --ignored"]
fn both_generations_of_one_project_export_as_the_same_audio() {
    // WP-20's last exit criterion: one AUP3 and one AUP4, each round-tripped
    // into a tagged export. `simples_test` is the project that exists in both
    // generations, and the conversion is meant to be lossless on the audio
    // layer, so the two exports have to be identical byte for byte.
    //
    // Everything after `vcw import` is the ordinary toolchain. Nothing in
    // `tracks` or `export` is told that the audio came out of Audacity.
    let corpus = PathBuf::from(std::env::var(CORPUS_ENV).unwrap_or_else(|_| {
        panic!("{CORPUS_ENV} is not set; this test reads the real Audacity corpus")
    }));
    let dir = scratch();

    let mut exported = Vec::new();
    for generation in ["aup3", "aup4"] {
        let source = corpus.join(format!("simples_test.{generation}"));
        assert!(source.is_file(), "{} is missing", source.display());
        let project = dir.path().join(format!("{generation}.vcw"));
        let (report, project) = import(&source, &project);
        let path = project.display().to_string();

        assert_eq!(report["format"], generation);
        assert_eq!(
            report["rate"], 192_000,
            "wavetrack/@rate, not project/@rate"
        );
        assert_eq!(report["storage_format"], "Int24Padded");
        assert_eq!(report["channels"], 2);

        // The labels became tracks, so the export has something to name. A
        // project with none would export nothing and pass by doing nothing.
        let printed = vcw(&["tracks", &path, "--json", "list"]);
        let listed: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);
        let tracks = listed["sides"][0]["tracks"].as_array().expect("tracks");
        assert!(!tracks.is_empty(), "no tracks landed from the labels");

        vcw(&[
            "release",
            &path,
            "set",
            "--album",
            "Simples",
            "--artist",
            "The Simples",
            "--year",
            "2026",
        ]);

        let out = dir.path().join(format!("out-{generation}"));
        let printed = vcw(&[
            "export",
            &path,
            "--into",
            &out.display().to_string(),
            "--format",
            "wav",
            "--json",
        ]);
        let export: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);
        assert_eq!(export["report"]["files"], tracks.len());

        let mut files: Vec<PathBuf> = export["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|item| PathBuf::from(item["path"].as_str().expect("a path")))
            .collect();
        files.sort();

        // Tagged, in the bytes, from the file that was actually written.
        assert_eq!(id3_text(&files[0], b"TALB").as_deref(), Some("Simples"));
        assert_eq!(
            id3_text(&files[0], b"TPE1")
                .or_else(|| id3_text(&files[0], b"TPE2"))
                .as_deref(),
            Some("The Simples")
        );

        exported.push(files);
        // The landed project is several hundred megabytes and the audio is the
        // user's; it goes now rather than at the end of the run.
        std::fs::remove_file(&project).expect("remove the landed project");
    }

    let (three, four) = (&exported[0], &exported[1]);
    assert_eq!(three.len(), four.len(), "the same number of files");
    for (three, four) in three.iter().zip(four) {
        assert_eq!(three.file_name(), four.file_name(), "the same name");
        // Streamed rather than compared as two `Vec<u8>`: a side of this record
        // is 419 MB of audio a generation, and a test that needs a gigabyte of
        // resident memory to make its point is a test that fails on the Pi.
        compare_data_chunks(three, four);
    }
}

/// Fails with a frame number if two WAV files' audio differs.
///
/// A frame number rather than a byte offset because that is what a listener
/// would hear, and because the two numbers differ by the frame size, which is
/// exactly the sort of arithmetic a failing test should not leave to the reader.
fn compare_data_chunks(left: &Path, right: &Path) {
    let (mut left_file, left_span) = data_chunk_reader(left);
    let (mut right_file, right_span) = data_chunk_reader(right);
    assert_eq!(
        left_span,
        right_span,
        "{:?} came out a different length from the two generations",
        left.file_name()
    );

    let mut a = vec![0_u8; 1 << 20];
    let mut b = vec![0_u8; 1 << 20];
    let mut at = 0_usize;
    while at < left_span {
        let want = (left_span - at).min(a.len());
        left_file.read_exact(&mut a[..want]).expect("read");
        right_file.read_exact(&mut b[..want]).expect("read");
        if a[..want] != b[..want] {
            let offset = a[..want]
                .iter()
                .zip(&b[..want])
                .position(|(a, b)| a != b)
                .expect("a difference");
            panic!(
                "{:?} differs between the AUP3 and the AUP4 at byte {}; the \
                 conversion is supposed to be lossless on the audio layer",
                left.file_name(),
                at + offset
            );
        }
        at += want;
    }
}

/// A file positioned at the start of its `data` chunk, and that chunk's length.
fn data_chunk_reader(path: &Path) -> (std::io::BufReader<std::fs::File>, usize) {
    let mut file = std::io::BufReader::new(std::fs::File::open(path).expect("open the export"));
    let mut header = [0_u8; 12];
    file.read_exact(&mut header).expect("read the RIFF header");
    assert_eq!(&header[0..4], b"RIFF");
    loop {
        let mut chunk = [0_u8; 8];
        file.read_exact(&mut chunk).expect("read a chunk header");
        let size = u32::from_le_bytes(chunk[4..8].try_into().unwrap()) as usize;
        if &chunk[0..4] == b"data" {
            return (file, size);
        }
        std::io::copy(
            &mut file.by_ref().take((size + size % 2) as u64),
            &mut std::io::sink(),
        )
        .expect("skip a chunk");
    }
}
