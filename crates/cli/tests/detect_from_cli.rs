/*
 *  detect_from_cli.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Detection from the shipped binary: the live marker and the refine pass (22, 24, 4.5).
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

//! Detection from the shipped binary (§22, §24, §4.5).
//!
//! The library tests prove the detectors classify and the resolver resolves.
//! This proves the part §4.5 cares about: that both halves of §22 are reachable
//! with no UI compiled at all - the live marker out of `vcw session`, and the
//! post-capture pass out of `vcw detect`, evidence and all.
//!
//! The simulated source is uniform noise, so the only boundary a side of it can
//! have is the one at its start. That is enough for what is being tested here,
//! which is the plumbing rather than the classification: `tests/vripr_parity.rs`
//! in `vcw-signal` is where the classification is measured, against 595 real
//! sides' worth of material.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

/// Records a short side into `path` and returns the events it published.
fn record(path: &Path, seconds: f64) -> Vec<Value> {
    let out = Command::new(VCW)
        .args([
            "session",
            &path.display().to_string(),
            "--rate",
            "48000",
            "--channels",
            "2",
            "--json",
            "--script",
            &format!("arm,record,sleep {seconds},stop"),
        ])
        .output()
        .expect("run vcw session");
    assert!(
        out.status.success(),
        "recording failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Runs `vcw detect` and returns the parsed JSON.
fn detect(path: &Path, extra: &[&str]) -> Value {
    let mut args: Vec<String> = ["detect", &path.display().to_string(), "--json"]
        .iter()
        .map(|arg| (*arg).to_owned())
        .collect();
    args.extend(extra.iter().map(|arg| (*arg).to_owned()));
    let out = Command::new(VCW)
        .args(&args)
        .output()
        .expect("run vcw detect");
    assert!(
        out.status.success(),
        "detection failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("the answer is JSON")
}

/// §35's event, out of the binary, while the capture was still running.
#[test]
fn a_marker_reaches_the_command_line_while_the_side_is_recording() {
    // The live detector settles for 1.2 s before it will call anything, so a
    // 2.5 s side left it 1.3 s to publish a marker: ample on this machine, and
    // not ample on an emulated aarch64 runner, where it published none and
    // turned a tag round red on a tree with nothing wrong in it. The claim
    // here is that a marker reaches the command line, not that 2.5 s is long
    // enough for one - so the side gets longer until it does. A fast machine
    // still pays for one 2.5 s take; a slow one pays in seconds rather than in
    // a red leg, and a detector that never publishes still fails below.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut events = Vec::new();
    for attempt in 1..=3 {
        let project = dir.path().join(format!("live-{attempt}.vcw"));
        events = record(&project, 2.5 * f64::from(attempt));
        if events
            .iter()
            .any(|event| event["event"] == "track-detected")
        {
            break;
        }
    }

    let markers: Vec<&Value> = events
        .iter()
        .filter(|event| event["event"] == "track-detected")
        .collect();
    assert!(
        !markers.is_empty(),
        "no track-detected among {} event(s)",
        events.len()
    );
    let first = markers[0];
    assert_eq!(first["edge"], "start");
    assert_eq!(first["frame"], 0);
    assert_eq!(first["provenance"], "silence");
    assert!(
        first["confidence"].as_f64().expect("a number") > 0.0,
        "a marker with no confidence is not evidence of anything"
    );

    // Before the capture was declared finished, which is the whole point of
    // publishing it live.
    let finished = events
        .iter()
        .position(|event| event["event"] == "capture-finished")
        .expect("the capture must finish");
    let last = events
        .iter()
        .rposition(|event| event["event"] == "track-detected")
        .expect("a marker");
    assert!(last < finished, "a marker followed capture-finished");
}

/// §22's post-capture pass, and §24's evidence, from the outside.
#[test]
fn the_refine_pass_runs_from_the_command_line_and_shows_its_working() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path().join("refine.vcw");
    record(&project, 3.0);

    let found = detect(&project, &["--evidence"]);
    assert_eq!(found["rate"], 48_000);
    assert!(
        found["windows"].as_u64().expect("windows") > 20,
        "3 s at a 100 ms window is about thirty, got {}",
        found["windows"]
    );

    let detectors = found["detectors"].as_array().expect("detectors");
    let names: Vec<&str> = detectors
        .iter()
        .map(|detector| detector["provenance"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, vec!["silence", "spectral-change", "hmm"]);

    let boundaries = found["boundaries"].as_array().expect("boundaries");
    assert!(!boundaries.is_empty(), "a side with audio in it has edges");
    for boundary in boundaries {
        assert!(
            !boundary["sources"].as_array().expect("sources").is_empty(),
            "a boundary from nowhere: {boundary}"
        );
        assert!(
            !boundary["evidence"]
                .as_array()
                .expect("--evidence was asked for")
                .is_empty(),
            "no measurements behind {boundary}"
        );
        assert_eq!(boundary["locked"], false, "nothing here was confirmed");
    }

    // Uniform noise never falls quiet, so the side is one track from end to end.
    let tracks = found["implied_tracks"].as_array().expect("tracks");
    assert_eq!(tracks.len(), 1, "{tracks:?}");
    assert!(tracks[0]["start_secs"].as_f64().expect("a start") < 0.001);
}

/// §22's adaptive threshold, and the flags that reach it.
#[test]
fn the_settings_asked_for_are_the_settings_used() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path().join("flags.vcw");
    record(&project, 2.0);

    let found = detect(&project, &["--adaptive", "--min-sound", "0.5"]);
    assert_eq!(found["settings"]["adaptive"], true);
    assert!(
        (found["settings"]["min_sound_secs"]
            .as_f64()
            .expect("a number")
            - 0.5)
            .abs()
            < 1e-9
    );
    // Adaptive means the threshold was derived, so the floor it came from is
    // reported rather than left blank: §24's provenance applied to a setting.
    for detector in found["detectors"].as_array().expect("detectors") {
        assert!(
            !detector["floor_db"].is_null(),
            "{} derived a threshold from nothing: {detector}",
            detector["provenance"]
        );
    }

    // Evidence is off unless asked for: a JSON consumer that wants a boundary
    // list should not have to read thirty measurements to find it.
    let plain = detect(&project, &[]);
    for boundary in plain["boundaries"].as_array().expect("boundaries") {
        assert!(boundary["evidence"].is_null(), "{boundary}");
    }
}
