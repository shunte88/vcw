/*
 *  export_from_cli.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The export verb through the shipped binary (§33, §4.5).
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

//! The export verb through the shipped binary (§33, §4.5).
//!
//! `crates/export`'s own tests prove the splitter and the encoders against the
//! project layer. This proves the *verb*: that the whole of §50's last step is
//! reachable from a command line, on a machine with nothing plugged in, and that
//! what comes out is what the other verbs say is in there.
//!
//! The cross-check is the interesting part. `vcw play --render` already renders a
//! track's audio, and it was written for WP-10's gapless proof - so exporting the
//! same track and comparing the WAV's data chunk against that render is two
//! independent verbs agreeing about the same frames. They share `pcm::Reader`
//! underneath, which is the point: there is meant to be exactly one way to get
//! samples out of a project, and if export had grown its own the two would
//! disagree here.

use std::path::{Path, PathBuf};
use std::process::Command;

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

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

/// A project with one side of simulated audio, two titled tracks, a release and
/// a cover, built entirely through the binary.
///
/// `format` is pinned because it decides what can be exported: FLAC cannot carry
/// 32-bit samples while `flacenc` stops at 24 bits, which is a real limit and
/// not a test convenience.
fn side(dir: &Path, format: &str, seconds: f64) -> PathBuf {
    let project = dir.join("side.vcw");
    let path = project.display().to_string();
    vcw(&[
        "session",
        &path,
        "--rate",
        "48000",
        "--channels",
        "2",
        "--format",
        format,
        "--script",
        &format!("arm,record,sleep {seconds},stop"),
    ]);
    vcw(&["tracks", &path, "attach", "--side", "A"]);
    let half = seconds / 2.0;
    vcw(&[
        "tracks",
        &path,
        "add",
        "--side",
        "A",
        "--start",
        "0",
        "--end",
        &half.to_string(),
    ]);
    vcw(&[
        "tracks",
        &path,
        "add",
        "--side",
        "A",
        "--start",
        &half.to_string(),
        "--end",
        &seconds.to_string(),
    ]);
    vcw(&["tracks", &path, "set", "1", "--title", "Europe Endless"]);
    vcw(&["tracks", &path, "set", "2", "--title", "Hall of Mirrors"]);
    vcw(&[
        "release",
        &path,
        "set",
        "--album",
        "Trans-Europe Express",
        "--artist",
        "Kraftwerk",
        "--year",
        "1977",
        "--genres",
        "Electronic;Krautrock",
        "--label",
        "Kling Klang",
        "--catalog",
        "1C 064-82 306",
        "--country",
        "Germany",
    ]);
    let cover = dir.join("cover.png");
    std::fs::write(&cover, PNG).expect("write the cover");
    vcw(&[
        "release",
        &path,
        "artwork",
        &cover.display().to_string(),
        "--role",
        "front",
    ]);
    project
}

/// A 1x1 RGB PNG, so the artwork path has real bytes to move.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x10, 0x50, 0x30, 0x00,
    0x00, 0x00, 0xa4, 0x00, 0x61, 0x34, 0x66, 0x7d, 0x72, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// The data chunk of a WAV file, found by walking the chunks.
fn data_chunk(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("read");
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

/// Finds a tool on `PATH`, or `None`.
fn tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[test]
fn the_whole_export_runs_from_the_command_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = side(dir.path(), "s24", 4.0);
    let out = dir.path().join("out");

    let printed = vcw(&[
        "export",
        &project.display().to_string(),
        "--into",
        &out.display().to_string(),
        "--format",
        "flac",
        "--json",
    ]);
    let report: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);

    assert_eq!(report["container"], "flac");
    assert_eq!(report["report"]["files"], 2);
    assert_eq!(report["report"]["covers"], 1);
    assert_eq!(report["frames"], report["report"]["frames"]);

    let items = report["items"].as_array().expect("items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["title"], "Europe Endless");
    assert_eq!(items[0]["side"], "A");
    assert_eq!(items[1]["number"], 2);
    for item in items {
        let path = Path::new(item["path"].as_str().expect("a path"));
        assert!(path.is_file(), "{} was not written", path.display());
    }

    // The folder image, beside the tracks, as VRipr wrote it.
    let covers = report["covers"].as_array().expect("covers");
    assert_eq!(covers.len(), 1);
    let cover = Path::new(covers[0].as_str().expect("a path"));
    assert_eq!(std::fs::read(cover).expect("cover"), PNG);

    // And the tags, read by somebody else's software.
    if let Some(metaflac) = tool("metaflac") {
        let first = items[0]["path"].as_str().expect("a path");
        let said = Command::new(&metaflac)
            .args(["--export-tags-to=-", first])
            .output()
            .expect("metaflac");
        let tags = String::from_utf8_lossy(&said.stdout);
        for expected in [
            "TITLE=Europe Endless",
            "ALBUM=Trans-Europe Express",
            "ARTIST=Kraftwerk",
            "DATE=1977",
            "CATALOGNUMBER=1C 064-82 306",
            "VINYL_POSITION=A1",
        ] {
            assert!(
                tags.lines().any(|line| line == expected),
                "metaflac did not report {expected:?}:\n{tags}"
            );
        }
    }
}

#[test]
fn the_exported_wav_is_what_the_play_verb_renders() {
    // Two verbs, one claim. `--format s32` on purpose: a stored frame and a WAV
    // frame are the same bytes at 32 bits, so this is an identity and not a
    // transform.
    //
    // The span is given in seconds because `vcw play --track` is still only a
    // label - the comment in `play.rs` says WP-13 would make it resolve a span
    // and it does not yet - so the render is asked for as a region and the
    // JSON's own span is asserted to be the one the export used. Without that
    // check this test could compare the right bytes to the wrong frames and
    // pass.
    let dir = tempfile::tempdir().expect("tempdir");
    let project = side(dir.path(), "s32", 4.0);
    let out = dir.path().join("out");
    let printed = vcw(&[
        "export",
        &project.display().to_string(),
        "--into",
        &out.display().to_string(),
        "--format",
        "wav",
        "--json",
    ]);
    let report: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);

    for item in report["items"].as_array().expect("items") {
        let track = item["track_id"].as_i64().expect("a track id");
        let start = item["start_frame"].as_u64().expect("a start");
        let end = item["end_frame"].as_u64().expect("an end");
        let rendered = dir.path().join(format!("track-{track}.raw"));
        let said = vcw(&[
            "play",
            &project.display().to_string(),
            "--render",
            &rendered.display().to_string(),
            "--start",
            &(start as f64 / 48_000.0).to_string(),
            "--end",
            &(end as f64 / 48_000.0).to_string(),
            "--json",
        ]);
        let played: serde_json::Value = serde_json::from_str(said.trim()).expect(&said);
        assert_eq!(
            (
                played["span"]["start"].as_u64(),
                played["span"]["end"].as_u64()
            ),
            (Some(start), Some(end)),
            "the render is not over the frames the export cut"
        );

        let exported = data_chunk(Path::new(item["path"].as_str().expect("a path")));
        let played = std::fs::read(&rendered).expect("the render");
        assert_eq!(
            exported.len(),
            played.len(),
            "track {track}: {} exported bytes against {} played",
            exported.len(),
            played.len()
        );
        // Compared by hand rather than by `assert_eq!` on the vectors, which
        // would print a megabyte of samples on a one-byte difference.
        if let Some(at) = exported
            .iter()
            .zip(&played)
            .position(|(left, right)| left != right)
        {
            panic!(
                "track {track} differs at byte {at}: exported {:?}, played {:?}",
                &exported[at..(at + 8).min(exported.len())],
                &played[at..(at + 8).min(played.len())]
            );
        }
    }
}

#[test]
fn a_dry_run_resolves_everything_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = side(dir.path(), "s24", 2.0);
    let out = dir.path().join("out");

    let printed = vcw(&[
        "export",
        &project.display().to_string(),
        "--into",
        &out.display().to_string(),
        "--template",
        "{position} {title}",
        "--dry-run",
        "--json",
    ]);
    let report: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);
    assert!(report["report"].is_null(), "a dry run reported work done");
    let items = report["items"].as_array().expect("items");
    assert_eq!(items.len(), 2);
    assert!(
        items[0]["path"]
            .as_str()
            .expect("a path")
            .ends_with("A1 Europe Endless.flac")
    );
    assert!(!out.exists(), "a dry run created {}", out.display());
}

#[test]
fn a_dry_run_prints_the_paths_the_run_will_write() {
    // `--dry-run` exists to settle an argument with a naming template, and it
    // used to print four counts and no names: the only way to see what a
    // template had done was to run a real export and look at the directory,
    // which is the thing the flag is for avoiding. `--json` had every path all
    // along, which is no help to a person at a terminal.
    let dir = tempfile::tempdir().expect("tempdir");
    let project = side(dir.path(), "s24", 2.0);
    let out = dir.path().join("out");
    let project = project.display().to_string();
    let out_arg = out.display().to_string();
    let common = [
        "export",
        project.as_str(),
        "--into",
        out_arg.as_str(),
        "--template",
        "{position} {title}",
    ];

    let mut dry = common.to_vec();
    dry.push("--dry-run");
    let predicted = vcw(&dry);
    assert!(!out.exists(), "a dry run created {}", out.display());
    assert!(
        predicted.contains("A1 Europe Endless.flac"),
        "the dry run named no files:\n{predicted}"
    );

    // The point of the lines, not just their presence: a dry run is a
    // prediction, so it has to be the *same text* the run prints, line for
    // line. Printing absolute paths in one and relative in the other would make
    // the two impossible to compare by eye, which is how they would be read.
    let actually = vcw(&common);
    assert_eq!(
        numbered(&predicted),
        numbered(&actually),
        "the dry run did not predict the run\n--- dry\n{predicted}\n--- run\n{actually}"
    );
    assert_eq!(numbered(&predicted).len(), 2, "{predicted}");
}

/// The `  1/2    some/path` lines of an export's output, in order.
fn numbered(printed: &str) -> Vec<&str> {
    printed
        .lines()
        .filter(|line| {
            line.split_whitespace().next().is_some_and(|first| {
                first.split_once('/').is_some_and(|(n, of)| {
                    !n.is_empty() && n.chars().chain(of.chars()).all(|c| c.is_ascii_digit())
                })
            })
        })
        .collect()
}

#[test]
fn one_side_can_be_exported_on_its_own() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = side(dir.path(), "s24", 2.0);
    let out = dir.path().join("out");

    // Side B has no tracks in this project, so asking for it is a refusal that
    // names the side rather than an empty success.
    let said = refused(&[
        "export",
        &project.display().to_string(),
        "--into",
        &out.display().to_string(),
        "--side",
        "B",
    ]);
    assert!(said.contains("side B"), "{said}");
    assert!(!out.exists());

    let printed = vcw(&[
        "export",
        &project.display().to_string(),
        "--into",
        &out.display().to_string(),
        "--side",
        "A",
        "--json",
    ]);
    let report: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);
    assert_eq!(report["report"]["files"], 2);
}

#[test]
fn what_flac_cannot_carry_is_refused_with_the_reason() {
    // The one requirement gap WP-14 leaves open, asserted so it cannot be
    // forgotten: §8 allows a 32-bit capture and §33 requires FLAC, and
    // `flacenc` 0.5.1 stops at 24 bits. Refused loudly, with the remedy in the
    // message, rather than narrowed behind the operator's back - the corpus at
    // /data2/source_rips shows real 32-bit rips using the whole low byte, so
    // narrowing is not lossless and is not ours to decide.
    let dir = tempfile::tempdir().expect("tempdir");
    let project = side(dir.path(), "s32", 1.0);
    let said = refused(&[
        "export",
        &project.display().to_string(),
        "--into",
        &dir.path().join("out").display().to_string(),
        "--format",
        "flac",
    ]);
    assert!(said.contains("cannot be written as FLAC"), "{said}");
    // Both containers that will take a 32-bit capture, because a refusal that
    // names only one of them sends a person who wanted a small file to WAV.
    assert!(said.contains("as WAV"), "{said}");
    assert!(said.contains("Ogg Vorbis"), "{said}");
}

#[test]
fn a_second_export_over_the_first_needs_permission() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = side(dir.path(), "s24", 2.0);
    let out = dir.path().join("out").display().to_string();
    let path = project.display().to_string();

    vcw(&["export", &path, "--into", &out, "--json"]);
    let said = refused(&["export", &path, "--into", &out]);
    assert!(said.contains("already exists"), "{said}");
    vcw(&["export", &path, "--into", &out, "--overwrite", "--json"]);
}

#[test]
fn a_lossy_export_runs_from_the_command_line_and_says_what_it_wrote() {
    // The two new containers end to end: §33 names MP3 and OGG as initial
    // export formats, and the CLI is where §50's one-command run reaches them.
    // The quality is in the report because it is not recoverable from the file
    // afterwards - a VBR stream does not record which `-V` made it - so a log
    // of the run is the only place the setting survives.
    let dir = tempfile::tempdir().expect("tempdir");
    let project = side(dir.path(), "s16", 4.0);

    for (format, quality, expected) in [("mp3", Some("compact"), "compact"), ("ogg", None, "high")]
    {
        let out = dir.path().join(format!("out-{format}"));
        let mut args = vec![
            "export".to_owned(),
            project.display().to_string(),
            "--into".to_owned(),
            out.display().to_string(),
            "--format".to_owned(),
            format.to_owned(),
            "--json".to_owned(),
        ];
        if let Some(quality) = quality {
            args.push("--quality".to_owned());
            args.push(quality.to_owned());
        }
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let printed = vcw(&borrowed);
        let report: serde_json::Value = serde_json::from_str(printed.trim()).expect(&printed);

        assert_eq!(report["container"], format);
        assert_eq!(report["quality"], expected, "{format}: {printed}");
        assert_eq!(report["lossless"], false);
        assert_eq!(report["report"]["files"], 2);

        let items = report["items"].as_array().expect("items");
        let mut written = 0u64;
        for item in items {
            let path = Path::new(item["path"].as_str().expect("a path"));
            assert_eq!(
                path.extension().and_then(|ext| ext.to_str()),
                Some(format),
                "{} is not a .{format}",
                path.display()
            );
            written += std::fs::metadata(path).expect("stat").len();
        }
        assert_eq!(
            report["report"]["bytes"].as_u64(),
            Some(written),
            "{format}: the reported byte count is not what is on disk"
        );

        // And a reader we did not write agrees about what the container is.
        if let Some(ffprobe) = tool("ffprobe") {
            let said = Command::new(&ffprobe)
                .args([
                    "-v",
                    "error",
                    // The audio stream only. An MP3 with an embedded cover has
                    // two streams and the second one is a PNG, which is itself
                    // worth knowing: the artwork really did go in.
                    "-select_streams",
                    "a:0",
                    "-show_entries",
                    "stream=codec_name",
                    "-of",
                    "default=nw=1:nk=1",
                    items[0]["path"].as_str().expect("a path"),
                ])
                .output()
                .expect("ffprobe");
            let codec = String::from_utf8_lossy(&said.stdout).trim().to_owned();
            assert_eq!(
                codec,
                if format == "mp3" { "mp3" } else { "vorbis" },
                "ffprobe read a {codec} out of a .{format}"
            );
        }
    }
}

#[test]
fn a_quality_that_is_not_one_is_refused_before_anything_is_written() {
    // A typo in a flag that only some containers read. It is still a refusal -
    // taking "lovely" to mean the default would write two hours of someone's
    // record at a setting they did not choose - and it names the three words
    // that work, because a flag with three legal values should not need the
    // manual.
    let dir = tempfile::tempdir().expect("tempdir");
    let project = side(dir.path(), "s16", 1.0);
    let out = dir.path().join("out");

    let said = refused(&[
        "export",
        &project.display().to_string(),
        "--into",
        &out.display().to_string(),
        "--format",
        "mp3",
        "--quality",
        "lovely",
    ]);
    assert!(said.contains("transparent"), "{said}");
    assert!(!out.exists(), "a refused export created {}", out.display());

    // The same word against a lossless container is not an error the other way
    // round either: it is still not a quality, and a refusal that depended on
    // the format would be a flag people trip over.
    let said = refused(&[
        "export",
        &project.display().to_string(),
        "--into",
        &out.display().to_string(),
        "--format",
        "flac",
        "--quality",
        "lovely",
    ]);
    assert!(said.contains("transparent"), "{said}");
}
