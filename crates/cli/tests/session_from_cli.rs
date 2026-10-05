/*
 *  session_from_cli.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-07's exit criterion, second half: a full capture session driven from the CLI.
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

//! WP-07's exit criterion, second half: a full capture session driven from the CLI.
//!
//! The first half - that an invalid transition cannot be written down - is
//! proven at compile time in `vcw_core::state`. This is the other half, and it
//! has to be a real process to mean anything: the point of §4.5 and §2 is that
//! a capture can be made with no UI in the picture, and a test that called the
//! engine's Rust API directly would not distinguish "drivable headlessly" from
//! "drivable from a Rust program that happens to have no window".
//!
//! So the subject is the shipped `vcw` binary, given a script, and the evidence
//! is what lands in the project afterwards: a finalised capture, of about the
//! length the script asked for, that validates with every checksum recomputed.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use vcw_project::{Options, Project, recovery, session, validate};
use vcw_types::{CaptureEq, CaptureState};

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

/// Every test here is a real-time capture in a child process, and the harness
/// would otherwise start all eight at once. On a four-core hosted runner that
/// is what the timings measure: a Windows runner took 2.4 s to arm and dropped
/// 66,720 frames, so a session that asked for 0.6 s of audio ran for 17.3 s and
/// finished Interrupted. One at a time, and the numbers are about VCW again.
fn alone() -> std::sync::MutexGuard<'static, ()> {
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GATE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Runs a session from `--script` and returns whether it succeeded, and the
/// transcript.
fn script(project: &Path, verbs: &str) -> (bool, String) {
    let out = Command::new(VCW)
        .args(["session", &project.display().to_string(), "--script", verbs])
        .output()
        .expect("run vcw session");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// Every event name in the transcript, in order.
fn events(transcript: &str) -> Vec<String> {
    transcript
        .lines()
        .filter_map(|line| line.split_once("] "))
        .map(|(_, rest)| rest.split_whitespace().next().unwrap_or("").to_string())
        .filter(|name| !name.starts_with(">>"))
        .collect()
}

/// The one capture in a project, as the project itself records it.
fn only_capture(path: &Path) -> session::Record {
    let project = Project::open(path).expect("reopen");
    let all = session::all(project.conn()).expect("all");
    assert_eq!(all.len(), 1, "expected exactly one capture, got {all:?}");
    assert!(
        recovery::survey(project.conn()).expect("survey").is_empty(),
        "a session that stopped cleanly must not look unfinished"
    );
    let report = validate(
        &project,
        Options {
            verify_checksums: true,
        },
    )
    .expect("validate");
    assert!(report.is_clean(), "{:?}", report.findings);
    project.close().expect("close");
    all.into_iter().next().expect("the capture")
}

/// The exit criterion. A whole side, from the command line, with no UI.
#[test]
fn a_full_capture_session_runs_from_the_command_line() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("side-a.vcw");

    let (ok, transcript) = script(
        &path,
        "arm,record,sleep 0.8,pause,sleep 0.4,resume,sleep 0.8,stop,reset",
    );
    assert!(ok, "the session failed:\n{transcript}");

    // §11's diagram, walked, and §35's events, in the order they happened.
    let names = events(&transcript);
    let phases: Vec<&str> = transcript
        .lines()
        .filter(|l| l.contains("phase-change"))
        .filter_map(|l| l.split_once("-> "))
        .map(|(_, to)| to.trim())
        .collect();
    assert_eq!(
        phases,
        [
            "armed",
            "recording",
            "paused",
            "recording",
            "stopped",
            "idle"
        ],
        "{transcript}"
    );
    assert_eq!(
        names.iter().filter(|n| *n == "capture-finished").count(),
        1,
        "{transcript}"
    );
    assert_eq!(
        names.last().map(String::as_str),
        Some("closed"),
        "{transcript}"
    );
    assert!(
        names.iter().any(|n| n == "recording-position"),
        "no position reports: {transcript}"
    );

    // And the project agrees with the transcript. Two runs of 0.8 s with 0.4 s
    // of pause between them: about 1.6 s of audio, and about 2.0 s if the pause
    // had leaked into the recording.
    let record = only_capture(&path);
    assert_eq!(record.state, CaptureState::Finalised);
    assert!(record.finished_at.is_some());
    let seconds = record.duration_secs();
    assert!(
        (1.2..1.9).contains(&seconds),
        "expected about 1.6 s of audio, got {seconds:.3} s - a pause that was \
         recorded would read about 2.0 s:\n{transcript}"
    );
    assert!(
        transcript.contains(&format!("capture {}", record.id)),
        "the transcript never named the capture it made:\n{transcript}"
    );
}

/// The same thing from stdin, which is how an operator drives it.
#[test]
fn a_session_can_be_typed_one_verb_at_a_time() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("typed.vcw");

    let mut child = Command::new(VCW)
        .args(["session", &path.display().to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        // Comments and blank lines are the driver's, not the core's, and a
        // script that explains itself is worth having.
        stdin
            .write_all(b"# side one\narm\n\nrecord\nsleep 0.5\nstop\n")
            .expect("write");
    }
    // No `quit`: end of input ends the session, and it must finalise rather
    // than abandon what it has.
    let out = child.wait_with_output().expect("wait");
    let transcript = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{transcript}");

    let record = only_capture(&path);
    assert_eq!(record.state, CaptureState::Finalised);
    assert!(record.frames > 0, "nothing was recorded:\n{transcript}");
}

/// End of input finalises a capture nobody stopped.
#[test]
fn a_script_that_forgets_to_stop_still_keeps_its_audio() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("forgetful.vcw");

    let (ok, transcript) = script(&path, "arm,record,sleep 0.6");
    assert!(ok, "{transcript}");
    let record = only_capture(&path);
    assert_eq!(
        record.state,
        CaptureState::Finalised,
        "a shutdown must finalise the side, not abandon it:\n{transcript}"
    );
    assert!(record.frames > 0, "{transcript}");
}

/// A verb the driver does not know is a failure - after the audio is safe.
#[test]
fn an_unknown_verb_fails_the_run_without_costing_the_capture() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("typo.vcw");

    let (ok, transcript) = script(&path, "arm,record,sleep 0.5,eject,stop");
    assert!(!ok, "a typo must not pass silently:\n{transcript}");
    let record = only_capture(&path);
    assert_eq!(
        record.state,
        CaptureState::Finalised,
        "the capture was lost to a typo:\n{transcript}"
    );
    assert!(record.frames > 0, "{transcript}");
}

/// A command that does not apply is reported and changes nothing.
#[test]
fn a_command_out_of_turn_is_rejected_rather_than_obeyed() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out-of-turn.vcw");

    let (ok, transcript) = script(&path, "record,pause,stop,arm,record,sleep 0.4,stop");
    assert!(ok, "{transcript}");
    assert_eq!(
        transcript
            .lines()
            .filter(|l| l.contains("command-rejected"))
            .count(),
        3,
        "record, pause and stop are all illegal while idle:\n{transcript}"
    );
    // And the three rejections left nothing behind: one capture, from the arm.
    let record = only_capture(&path);
    assert!(record.frames > 0, "{transcript}");
}

/// Arming and thinking better of it leaves the project as it was found.
#[test]
fn an_abandoned_arm_leaves_no_capture_in_the_project() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("second-thoughts.vcw");

    let (ok, transcript) = script(&path, "arm,poll,disarm,poll");
    assert!(ok, "{transcript}");

    let project = Project::open(&path).expect("reopen");
    assert!(
        session::all(project.conn()).expect("all").is_empty(),
        "setting a level and walking away left a capture row:\n{transcript}"
    );
    assert!(
        recovery::survey(project.conn()).expect("survey").is_empty(),
        "{transcript}"
    );
    project.close().expect("close");
}

/// The meters are opt-in, and when asked for they are JSON a UI could use.
#[test]
fn the_meters_are_quiet_unless_asked_for_and_measured_when_they_are() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("levels.vcw");

    // Without the flag the transcript is the transport, and nothing else. At
    // 50 Hz a meter line for every tick would be the whole of it.
    let (ok, quiet) = script(&path, "arm,record,sleep 0.4,stop,reset");
    assert!(ok, "{quiet}");
    assert!(
        !quiet.contains("meter-update"),
        "meters leaked into the default transcript:\n{quiet}"
    );

    // With it, and in JSON, every reading carries a level per channel.
    let out = Command::new(VCW)
        .args([
            "session",
            &dir.path().join("levels-json.vcw").display().to_string(),
            "--script",
            "arm,record,sleep 0.4,stop,reset",
            "--meters",
            "--json",
        ])
        .output()
        .expect("run vcw session");
    assert!(out.status.success());
    let transcript = String::from_utf8_lossy(&out.stdout);

    let readings: Vec<serde_json::Value> = transcript
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value["event"] == "meter-update")
        .collect();
    assert!(
        readings.len() > 10,
        "400 ms at 50 Hz is about twenty readings, got {}",
        readings.len()
    );

    let last = readings.last().expect("a reading");
    let channels = last["channels"].as_array().expect("channels");
    assert_eq!(channels.len(), 2);
    for channel in channels {
        let rms = channel["rms_db"].as_f64().expect("rms_db");
        // The deterministic source is uniform over full scale, so its RMS is
        // 1/sqrt(3). See `vcw-core`'s `tests/metering_live.rs`.
        assert!(
            (rms + 4.771).abs() < 0.5,
            "the CLI reported {rms:.3} dBFS for a uniform source"
        );
        assert!(channel["peak_db"].as_f64().expect("peak_db") > -0.1);
    }
}

/// What the operator says about their preamp reaches the capture row.
///
/// §51 records the equalisation the signal already carried, and the only thing
/// that knows it is the person who wired the turntable up. It cannot be measured
/// from the audio afterwards, so the flag is the whole mechanism - and a flag
/// that is accepted and dropped would be worse than no flag, because the project
/// would then claim 'unknown' about a capture somebody described.
#[test]
fn the_operator_can_state_the_equalisation_and_it_is_what_the_project_keeps() {
    let _alone = alone();
    for (flag, expected) in [
        ("flat", CaptureEq::Flat),
        ("riaa", CaptureEq::Riaa),
        ("unknown", CaptureEq::Unknown),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(format!("{flag}.vcw"));
        let out = Command::new(VCW)
            .args([
                "session",
                &path.display().to_string(),
                "--capture-eq",
                flag,
                "--script",
                "arm,record,sleep 0.3,stop",
            ])
            .output()
            .expect("run vcw session");
        let transcript = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{flag}: {transcript}");

        let record = only_capture(&path);
        assert_eq!(record.info.eq, expected, "{flag}");
    }

    // Unstated is Unknown. Not an assumption - the absence of a statement.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("silent.vcw");
    let (ok, transcript) = script(&path, "arm,record,sleep 0.3,stop");
    assert!(ok, "{transcript}");
    assert_eq!(only_capture(&path).info.eq, CaptureEq::Unknown);

    // A curve nobody has heard of fails the run rather than becoming Unknown.
    let path = dir.path().join("typo.vcw");
    let out = Command::new(VCW)
        .args([
            "session",
            &path.display().to_string(),
            "--capture-eq",
            "columbia",
            "--script",
            "arm,record,sleep 0.3,stop",
        ])
        .output()
        .expect("run vcw session");
    assert!(
        !out.status.success(),
        "a curve VCW cannot apply must not be recorded as if it could"
    );
    assert!(
        !path.exists(),
        "a refused flag must not leave a project behind"
    );
}
