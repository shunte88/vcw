/*
 *  play_from_cli.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-10's exit criterion: gapless seek and honest fidelity, through the shipped binary.
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

//! WP-10's exit criterion: gapless seek and honest fidelity, through the
//! shipped binary.
//!
//! The subject is the `vcw` process, not the library, for the reason
//! `session_from_cli.rs` gives: §2 and §4.5 are about whether the application
//! can be driven with no UI present, and a test calling the Rust API would not
//! tell the difference.
//!
//! Every test here runs with no sound card. `vcw play --render` puts the bytes
//! the converter would have been handed into a file, which is what makes
//! "gapless" a comparison rather than an opinion - a gap is invisible from
//! outside a device, and a device that was fed silence reports the same success
//! as one that was fed music. The one thing rendering cannot measure is the
//! wall-clock delay before a seek reaches the converter on real hardware;
//! `vcw-core/tests/playback_live.rs` measures that and states the number.

use std::path::{Path, PathBuf};
use std::process::Command;

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

/// Frame width of the simulated capture: two channels of 32-bit integer.
const FRAME: usize = 8;

/// A project holding one capture of about `seconds` seconds, from the
/// simulated source, so this runs on a machine with nothing plugged in.
fn recorded(dir: &Path, seconds: f64) -> PathBuf {
    let path = dir.join("side.vcw");
    let out = Command::new(VCW)
        .args([
            "session",
            &path.display().to_string(),
            "--rate",
            "48000",
            "--channels",
            "2",
            "--script",
            &format!("arm,record,sleep {seconds},stop"),
        ])
        .output()
        .expect("run vcw session");
    assert!(
        out.status.success(),
        "recording failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    path
}

/// Renders through the binary and returns the audio and the JSON report.
fn render(project: &Path, out: &Path, extra: &[&str]) -> (Vec<u8>, serde_json::Value) {
    let mut args = vec![
        "play".to_string(),
        project.display().to_string(),
        "--render".to_string(),
        out.display().to_string(),
        "--json".to_string(),
    ];
    args.extend(extra.iter().map(ToString::to_string));
    let result = Command::new(VCW)
        .args(&args)
        .output()
        .expect("run vcw play");
    let stdout = String::from_utf8_lossy(&result.stdout).into_owned();
    // Both streams, because `vcw play` writes its report to stdout and says
    // why it could not play to stderr: a Windows runner with no usable device
    // failed this on 3df2507 and the whole message was "play failed:".
    assert!(
        result.status.success(),
        "play failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_str(stdout.trim()).expect(&stdout);
    (std::fs::read(out).expect("read the render"), report)
}

/// The exit criterion. A seek joins without a gap and without repeating.
#[test]
fn a_seek_is_gapless_through_the_shipped_binary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = recorded(dir.path(), 6.0);

    let (whole, _) = render(&project, &dir.path().join("whole.raw"), &[]);
    let (seeked, report) = render(
        &project,
        &dir.path().join("seek.raw"),
        &["--script", "play,sleep 2,seek 4,sleep 5"],
    );

    // Where the seek actually happened, as the binary reports it: a transport
    // moves between callbacks, so it lands at the first period boundary at or
    // after the frame the script named.
    let applied = report["applied"].as_array().expect("applied");
    let seek = applied
        .iter()
        .find(|made| made["verb"] == "seek")
        .expect("the seek");
    let after = seek["after_frames"].as_u64().expect("after") as usize;
    let landed = seek["landed"].as_u64().expect("landed") as usize;
    assert_eq!(
        landed,
        4 * 48_000,
        "the seek did not land where it was told"
    );

    let mut expected = whole[..after * FRAME].to_vec();
    expected.extend_from_slice(&whole[landed * FRAME..]);
    assert_eq!(
        seeked.len(),
        expected.len(),
        "the join lost or repeated audio"
    );
    assert_eq!(seeked, expected, "the audio either side of the join moved");

    // No silence was played in place of the audio that was discarded. The
    // callback drops the invalidated chunks and recycles them in the same pass,
    // so the feeder has buffers to refill before the next callback asks.
    assert_eq!(report["underruns"], 0, "the seek left a gap");
    assert!(
        report["stale_chunks"].as_u64().expect("stale") > 0,
        "nothing was discarded, so nothing was invalidated"
    );
}

/// §21's four targets, each playing its own extent and nothing else.
#[test]
fn the_four_audition_targets_play_what_they_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = recorded(dir.path(), 8.0);
    let (whole, report) = render(&project, &dir.path().join("whole.raw"), &[]);
    let frames = report["frames"].as_u64().expect("frames") as usize;
    assert_eq!(whole.len(), frames * FRAME);

    let slice = |from: f64, to: f64| {
        let from = (from * 48_000.0) as usize * FRAME;
        let to = ((to * 48_000.0) as usize * FRAME).min(whole.len());
        whole[from..to].to_vec()
    };

    let (region, _) = render(
        &project,
        &dir.path().join("region.raw"),
        &["--start", "1.5", "--end", "3.5"],
    );
    assert_eq!(
        region,
        slice(1.5, 3.5),
        "the region is not part of the side"
    );

    let (track, _) = render(
        &project,
        &dir.path().join("track.raw"),
        &["--track", "2", "--start", "4", "--end", "6"],
    );
    assert_eq!(track, slice(4.0, 6.0), "the track is not part of the side");

    let (boundary, _) = render(
        &project,
        &dir.path().join("boundary.raw"),
        &["--boundary", "4"],
    );
    assert_eq!(
        boundary,
        slice(1.0, 7.0),
        "the boundary audition is not the seam plus its context"
    );
}

/// A narrower stream format is allowed, and is never quiet about itself.
#[test]
fn a_conversion_is_reported_rather_than_hidden() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = recorded(dir.path(), 1.0);

    let (straight, report) = render(&project, &dir.path().join("straight.raw"), &[]);
    assert!(
        report["conversion"]
            .as_str()
            .expect("conversion")
            .contains("straight through"),
        "{report}"
    );

    let (narrow, report) = render(
        &project,
        &dir.path().join("narrow.raw"),
        &["--format", "s16"],
    );
    assert_eq!(report["format"], "S16");
    assert_eq!(narrow.len(), straight.len() / 2);
    assert_ne!(narrow, straight, "16 bits out of 32 changed nothing?");
}

/// A capture that is not there, and a region that is not in it, are refused.
#[test]
fn what_cannot_be_played_is_refused_rather_than_faked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = recorded(dir.path(), 1.0);
    let out = dir.path().join("nothing.raw");

    for extra in [
        vec!["--capture", "9999"],
        vec!["--start", "60", "--end", "70"],
    ] {
        let mut args = vec![
            "play".to_string(),
            project.display().to_string(),
            "--render".to_string(),
            out.display().to_string(),
        ];
        args.extend(extra.iter().map(ToString::to_string));
        let result = Command::new(VCW)
            .args(&args)
            .output()
            .expect("run vcw play");
        assert!(
            !result.status.success(),
            "{extra:?} should have been refused"
        );
    }
}
