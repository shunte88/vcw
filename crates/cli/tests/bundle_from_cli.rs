/*
 *  bundle_from_cli.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The diagnostic bundle, through the shipped binary: what it must say and what it must never carry (§42).
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
//! The diagnostic bundle, through the shipped binary: what it must say and what
//! it must never carry (§42).
//!
//! Half of §42 is a list of things to report, and that half is easy to test and
//! easy to get right. The other half is "without including recorded audio", and
//! a negative is the part worth a test, because nothing about a passing feature
//! test would notice the day somebody adds a helpful `samples` field.
//!
//! So the audio in these projects is not a ramp. It is the ASCII string
//! `VCW-AUDIO-MUST-NOT-APPEAR` repeated, which is perfectly legal PCM and also
//! a thing that can be searched for. If it ever turns up in a bundle - raw, hex
//! or base64 - the test says so by name.
//!
//! The same trick covers §39: the credential variables are set to values that
//! read as what they are, and the bundle has to report that they are configured
//! without repeating them.
//!
//! Through the binary rather than by calling `bundle::run`, for the usual
//! reason: a bundle is something a person produces from a command line while
//! something is wrong, and the argument parsing is part of what has to work.

use std::path::{Path, PathBuf};
use std::process::Command;

use vcw_project::persistence::{Config, Writer};
use vcw_project::{Project, meta, release, side, track};
use vcw_types::vinyl::Side;
use vcw_types::{CaptureEq, CaptureInfo, CaptureMode, CaptureState, SampleRate, StorageFormat};

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

/// Audio that can be searched for. Legal PCM, and unmistakable in a text file.
const MARKER: &[u8] = b"VCW-AUDIO-MUST-NOT-APPEAR";

/// The second channel's marker, so a leak can be attributed to a channel.
const CHANNEL_TWO_MARKER: &[u8] = b"VCW-CHANNEL-TWO-MUST-NOT-APPEAR";

/// A credential value that says what it is if it ever leaks.
const FAKE_TOKEN: &str = "leaked-discogs-token-0123456789abcdef";

/// A contact that must not be forwarded either: §42 does not need an address.
const FAKE_CONTACT: &str = "somebody@example.invalid";

/// Runs `vcw bundle` and parses what came out.
fn bundle(args: &[&str]) -> serde_json::Value {
    let out = Command::new(VCW)
        .arg("bundle")
        .args(args)
        // Absent unless a test sets them, so one test's credentials cannot make
        // another test's assertion pass or fail.
        .env_remove("VCW_DISCOGS_TOKEN")
        .env_remove("VCW_ACOUSTID_KEY")
        .env_remove("VCW_CONTACT")
        .output()
        .expect("run vcw bundle");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "vcw bundle {} failed:\n{stdout}{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{error}, in:\n{stdout}"))
}

/// A project with a capture in it, plus a side, two titled tracks and a release.
///
/// The titles and the album are here to be *looked for* later: they are the
/// record's metadata, and a bundle is not the place for them.
fn project_with_audio(dir: &Path, state: CaptureState) -> PathBuf {
    let path = dir.join("side-a.vcw");
    let project = Project::create(&path).expect("create");
    let info = CaptureInfo::unverified(
        SampleRate(48_000),
        2,
        StorageFormat::Int16,
        CaptureMode::Exclusive,
    )
    .with_eq(CaptureEq::Flat);

    // The marker has to survive being *stored*, not just being pushed. Blocks
    // are per channel and never interleaved, so a marker written straight into
    // an interleaved buffer is shredded by de-interleaving and would never
    // appear in a blob - which is how the first draft of this fixture managed to
    // make `a_bundle_carries_no_audio` pass even with a sample blob deliberately
    // embedded in the document. So the marker is laid into each channel's own
    // stream and the frames are interleaved from those, which puts the literal
    // bytes in `sampleblocks.samples` where a leak would pick them up.
    //
    // Big enough to matter, too: a few hundred bytes a channel would slip under
    // the length guard below, so this is a few blocks' worth.
    let frames = 24_000;
    let mut channel: Vec<Vec<u8>> = Vec::new();
    for marker in [MARKER, CHANNEL_TWO_MARKER] {
        let mut stream = Vec::new();
        while stream.len() < frames * 2 {
            stream.extend_from_slice(marker);
        }
        stream.truncate(frames * 2);
        channel.push(stream);
    }
    let frame_bytes = 4;
    let mut audio = Vec::with_capacity(frames * frame_bytes);
    for frame in 0..frames {
        for stream in &channel {
            audio.extend_from_slice(&stream[frame * 2..frame * 2 + 2]);
        }
    }

    let mut writer = Writer::begin(project, &info, Config::default()).expect("begin");
    writer.push(&audio).expect("push");
    writer.flush().expect("flush");
    let (_, mut project, _) = writer.finish_with_project(state).expect("finish");

    let frames = (audio.len() / frame_bytes) as u64;
    side::attach(&mut project, Side::A, 1).expect("attach");
    for (title, span) in [
        ("A Title Nobody Needs To Debug", (0, frames / 2)),
        ("Nor This One", (frames / 2, frames)),
    ] {
        let id = track::add_track(&mut project, Side::A, span.0, span.1).expect("track");
        track::update(&mut project, id, &track::Update::title(title)).expect("title");
    }
    let mut record = release::ensure(&mut project).expect("release");
    record.album = "An Album Nobody Needs To Debug".to_owned();
    record.album_artist = "A Private Listening Habit".to_owned();
    release::store(&mut project, &record).expect("store");
    // What an Audacity import parks in `meta`, which is the same kind of thing.
    meta::set(
        project.conn(),
        "import.tag.ALBUM",
        "An Album Nobody Needs To Debug",
    )
    .expect("a tag from an import");
    project.close().expect("close");
    path
}

/// Every string value anywhere in the document, with the path that reached it.
fn strings(value: &serde_json::Value, at: String, into: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::String(text) => into.push((at, text.clone())),
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                strings(item, format!("{at}[{index}]"), into);
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, field) in fields {
                strings(field, format!("{at}.{key}"), into);
            }
        }
        _ => {}
    }
}

#[test]
fn a_bundle_reports_what_section_42_asks_for() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = project_with_audio(dir.path(), CaptureState::Finalised);
    let said = bundle(&["--no-devices", &path.display().to_string()]);

    // Application version, and which build: a timing complaint against a debug
    // build is a different conversation.
    assert_eq!(said["vcw"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(said["vcw"]["sqlite"].is_string());
    assert!(matches!(
        said["vcw"]["profile"].as_str(),
        Some("debug" | "release")
    ));

    // The OS.
    assert_eq!(said["host"]["os"], std::env::consts::OS);
    assert_eq!(said["host"]["arch"], std::env::consts::ARCH);

    // The backend. Named even when the survey was skipped, because a machine
    // with no host API at all is a thing worth being able to see.
    assert!(
        said["audio"]["hosts"]
            .as_array()
            .is_some_and(|h| !h.is_empty()),
        "no host API reported"
    );
    assert_eq!(said["audio"]["surveyed"], false);

    // Project and database integrity.
    assert_eq!(said["project"]["file"], "side-a.vcw");
    assert_eq!(said["project"]["integrity"]["integrity_check"], "ok");
    assert_eq!(said["project"]["integrity"]["foreign_key_violations"], 0);
    assert_eq!(said["project"]["validate"]["clean"], true);
    assert_eq!(said["project"]["identity"]["application_id"], "0x56435700");
    assert_eq!(said["project"]["identity"]["journal_mode"], "wal");
    assert_eq!(said["project"]["counts"]["captures"], 1);
    assert_eq!(said["project"]["counts"]["tracks"], 2);
    assert_eq!(said["project"]["meta"]["required_keys_present"], 6);

    // The capture, its configuration and its counters: §42's "capture errors".
    let capture = &said["project"]["captures"][0];
    assert_eq!(capture["sample_rate"], 48_000);
    assert_eq!(capture["channels"], 2);
    assert_eq!(capture["storage_format"], "Int16");
    assert_eq!(capture["capture_mode"], "exclusive");
    // §42's bundle is what somebody reading a support report has. A rip that
    // sounds wrong because it was equalized twice is diagnosable from this line
    // and from nothing else in the file.
    assert_eq!(capture["capture_eq"], "flat");
    assert_eq!(capture["state"], "finalised");
    assert_eq!(capture["clean"], true);
    for counter in ["overruns", "underruns", "dropped_frames", "stream_errors"] {
        assert!(
            capture["diagnostics"][counter].is_u64(),
            "{counter} missing from the bundle"
        );
    }
}

#[test]
fn a_bundle_carries_no_audio() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = project_with_audio(dir.path(), CaptureState::Finalised);

    // With checksums, which is the run that reads every sample byte. If any code
    // path is going to hold audio in a hand and then put it in the document,
    // this is the one.
    let out = dir.path().join("bundle.json");
    let produced = Command::new(VCW)
        .args([
            "bundle",
            &path.display().to_string(),
            "--no-devices",
            "--checksums",
            "--out",
            &out.display().to_string(),
        ])
        .output()
        .expect("run vcw bundle");
    assert!(produced.status.success());
    let text = std::fs::read_to_string(&out).expect("the bundle");
    let said: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(said["project"]["validate"]["checksums_verified"], true);

    for marker in [MARKER, CHANNEL_TWO_MARKER] {
        let raw = String::from_utf8_lossy(marker).into_owned();
        assert!(
            !text.contains(&raw),
            "the bundle contains recorded audio (§42): {raw}"
        );
        // Encoded too. A hex or base64 blob is still recorded audio, and is what
        // a well-meaning "include the first block for context" would produce.
        let hex: String = marker.iter().map(|byte| format!("{byte:02x}")).collect();
        assert!(
            !text.to_lowercase().contains(&hex),
            "the bundle contains hex-encoded audio"
        );
    }

    // And nothing that looks like a blob went in under another name. A bundle is
    // facts about a project; the longest honest string in one is a device name,
    // an `os_report` or SQLite's integrity verdict.
    let mut found = Vec::new();
    strings(&said, String::new(), &mut found);
    for (at, text) in &found {
        assert!(
            text.len() < 4096,
            "{at} is {} characters long, which is not a fact about a project",
            text.len()
        );
    }

    // The record's own metadata is not in here either. It cannot help anybody
    // debug a dropout, and a bundle is something a person sends to a stranger.
    for private in [
        "A Title Nobody Needs To Debug",
        "Nor This One",
        "An Album Nobody Needs To Debug",
        "A Private Listening Habit",
    ] {
        assert!(!text.contains(private), "the bundle repeats {private:?}");
    }
    // Nor the path to it, which carries a home directory.
    assert!(
        !text.contains(&dir.path().display().to_string()),
        "the bundle carries the project's path"
    );
}

#[test]
fn a_bundle_carries_no_credentials() {
    let out = Command::new(VCW)
        .args(["bundle", "--no-devices"])
        .env("VCW_DISCOGS_TOKEN", FAKE_TOKEN)
        .env("VCW_ACOUSTID_KEY", FAKE_TOKEN)
        .env("VCW_CONTACT", FAKE_CONTACT)
        .output()
        .expect("run vcw bundle");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let said: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");

    assert!(
        !text.contains(FAKE_TOKEN),
        "the bundle leaked a token (§39)"
    );
    assert!(
        !text.contains(FAKE_CONTACT),
        "the bundle leaked the contact address"
    );

    // Configured, and how long: "my token is not working" is very often a
    // truncated paste, and a character count settles that without the token.
    assert_eq!(said["credentials"]["VCW_DISCOGS_TOKEN"]["configured"], true);
    assert_eq!(
        said["credentials"]["VCW_DISCOGS_TOKEN"]["characters"],
        FAKE_TOKEN.chars().count()
    );
    assert_eq!(said["credentials"]["VCW_ACOUSTID_KEY"]["configured"], true);
    // The contact gets no length. It is an email address, and a length is one
    // more thing about a person than a dropout report needs.
    assert_eq!(said["credentials"]["VCW_CONTACT"]["configured"], true);
    assert!(said["credentials"]["VCW_CONTACT"]["characters"].is_null());
}

#[test]
fn a_bundle_of_a_project_that_will_not_open_is_still_a_bundle() {
    // The project most worth a bundle is the one that will not open, so this is
    // the case the verb exists for rather than an edge of it.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("broken.vcw");
    std::fs::write(&path, b"this was never a database").expect("write");

    let said = bundle(&["--no-devices", &path.display().to_string()]);
    assert_eq!(said["project"]["exists"], true);
    assert!(
        said["project"]["open_error"].is_string(),
        "a project that will not open must say why"
    );
    // And the rest of the document survived, which is the point.
    assert_eq!(said["host"]["os"], std::env::consts::OS);
    assert!(said["vcw"]["version"].is_string());

    // A project that is not there at all, likewise.
    let missing = dir.path().join("never-existed.vcw");
    let said = bundle(&["--no-devices", &missing.display().to_string()]);
    assert_eq!(said["project"]["exists"], false);
    assert!(said["project"]["bytes"].is_null());
    assert!(said["project"]["open_error"].is_string());
}

#[test]
fn a_bundle_reports_a_damaged_block_when_it_is_asked_to_check() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = project_with_audio(dir.path(), CaptureState::Finalised);

    {
        let conn = rusqlite::Connection::open(&path).expect("open to damage");
        conn.execute(
            "UPDATE sampleblocks SET samples = zeroblob(length(samples))
               WHERE blockid = (SELECT MIN(blockid) FROM sampleblocks)",
            [],
        )
        .expect("damage one block");
    }

    // SQLite cannot see it: a corrupted blob is still a valid blob, which is the
    // whole reason the `checksum` column exists.
    let said = bundle(&["--no-devices", &path.display().to_string()]);
    assert_eq!(said["project"]["integrity"]["integrity_check"], "ok");
    assert_eq!(said["project"]["validate"]["clean"], true);
    assert_eq!(said["project"]["validate"]["checksums_verified"], false);

    // Asked to check, it finds it, and still exits zero: a diagnosis is not a
    // failure of the tool that made it.
    let said = bundle(&["--no-devices", "--checksums", &path.display().to_string()]);
    assert_eq!(said["project"]["validate"]["clean"], false);
    let findings = said["project"]["validate"]["findings"]
        .as_array()
        .expect("findings");
    assert!(
        findings.iter().any(|finding| finding["code"]
            .as_str()
            .is_some_and(|code| code.contains("checksum"))),
        "expected a checksum finding, got {findings:?}"
    );
}

#[test]
fn a_bundle_names_a_capture_that_did_not_finish() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = project_with_audio(dir.path(), CaptureState::Interrupted);
    let said = bundle(&["--no-devices", &path.display().to_string()]);
    let capture = &said["project"]["captures"][0];
    assert_eq!(capture["state"], "interrupted");
    assert!(
        capture["frames"].as_u64().is_some_and(|frames| frames > 0),
        "an interrupted capture still has the frames it committed"
    );
}

#[test]
fn a_bundle_with_no_project_is_a_report_about_the_machine() {
    // Somebody whose trouble is that no device appears has no project to name,
    // and is exactly the person who most needs to send a bundle.
    let said = bundle(&["--no-devices"]);
    assert!(said["project"].is_null());
    assert!(said["host"]["os"].is_string());
    assert!(said["audio"]["hosts"].is_array());
    assert_eq!(said["bundle_version"], 1);
}

#[test]
fn a_device_survey_is_summarized_unless_the_whole_thing_is_asked_for() {
    // The full snapshot of a development machine is 7.7 MB of JSON, most of it
    // ALSA plugin nodes advertising 1 to 64 channels in five formats. A bundle
    // nobody can send is a bundle nobody sends, so the default is a summary.
    let summary = bundle(&[]);
    assert_eq!(summary["audio"]["detail"], "summary");
    assert!(summary["audio"]["count"].as_u64().is_some());

    let rendered = serde_json::to_string(&summary).expect("render");
    assert!(
        rendered.len() < 1_000_000,
        "a summarized bundle is {} bytes, which is too big to send",
        rendered.len()
    );

    // Every summarized device still carries what diagnoses a capture: identity,
    // transport, the backend's own default, and anything that went wrong while
    // asking.
    if let Some(devices) = summary["audio"]["devices"].as_array() {
        for device in devices {
            assert!(device["host"].is_string(), "{device:?}");
            assert!(device["id"].is_string(), "{device:?}");
            assert!(device["capability_digest"].is_string(), "{device:?}");
            assert!(device["problems"].is_array(), "{device:?}");
            for direction in ["input", "output"] {
                assert!(device[direction]["supported"].is_boolean(), "{device:?}");
            }
        }
    }
}
