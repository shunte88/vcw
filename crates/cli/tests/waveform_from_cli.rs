/*
 *  waveform_from_cli.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-09's exit criterion from the outside: the pyramid drawn by the shipped binary.
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

//! WP-09's exit criterion from the outside: the pyramid drawn by the shipped
//! binary.
//!
//! The library tests in `vcw-project` already prove the arithmetic - that every
//! level agrees, that a thrown-away pyramid rebuilds identically, that the read
//! cost follows the span rather than the recording. What they cannot prove is
//! that any of it is reachable, and §4.5 asks for the whole workflow to be
//! drivable with no UI compiled at all.
//!
//! So this records a side with `vcw session` and draws it with `vcw waveform`,
//! and checks the two things a caller depends on: that the answer is exactly as
//! wide as it asked for, whatever the zoom, and that the level the pyramid chose
//! is the coarsest one that still fills a column.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

/// Records a short side into `path`, from the simulated source.
fn record(path: &Path, seconds: f64) {
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
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Draws `path` and returns the parsed JSON.
fn draw(path: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["waveform", &path.display().to_string(), "--json"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<String>>();
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let out = Command::new(VCW)
        .args(&args)
        .output()
        .expect("run vcw waveform");
    assert!(
        out.status.success(),
        "drawing failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("the drawing is JSON")
}

/// Every channel's column count.
fn widths(drawing: &Value) -> Vec<usize> {
    drawing["channels"]
        .as_array()
        .expect("channels")
        .iter()
        .map(|c| c["columns"].as_array().expect("columns").len())
        .collect()
}

/// §37, from the outside: whatever the zoom, the answer is the width that was
/// asked for and the level is the coarsest one that still fills a column.
#[test]
fn the_drawing_is_as_wide_as_it_was_asked_for_at_every_zoom() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("draw.vcw");
    record(&path, 3.0);

    // 3 s of 48 kHz is about 144,000 frames in 12 blocks of 250 ms, and the
    // rungs are 1, 256 and 12,000 frames, so the width alone decides which one
    // is read: a column spanning a whole block reads the block rung, and one
    // spanning fewer than 256 frames leaves only the samples themselves.
    //
    // "About", because `record` runs a real capture in real time and nothing
    // here pins its length to the frame. So every width below sits with margin
    // inside a rung rather than on its boundary. It did not: 12 columns is the
    // exact count at which the block rung stops fitting, and a Windows runner
    // that produced 65,760 frames for this 3 s request read "summary256" and
    // failed a test about zoom for a reason that had nothing to do with zoom.
    //
    // That shortfall was a defect in `vcw session` rather than a slow machine -
    // `record` returned before the device was running, so the script's `sleep`
    // spent 1.6 s of itself waiting for WASAPI to open - and it is fixed. The
    // margin stays anyway: a test that only passes when a real-time capture
    // lands on an exact frame count is a test that will go red for a reason it
    // is not about.
    for (pixels, level) in [
        ("6", "block"),
        ("10", "block"),
        ("100", "summary256"),
        ("400", "summary256"),
        ("200000", "samples"),
    ] {
        let drawing = draw(&path, &["--pixels", pixels]);
        assert_eq!(
            drawing["level"], level,
            "at {pixels} px the pyramid chose {}",
            drawing["level"]
        );
        let wanted: usize = pixels.parse().unwrap();
        assert_eq!(
            widths(&drawing),
            vec![wanted, wanted],
            "at {pixels} px the drawing was the wrong width"
        );
    }
}

/// The two halves of §19 that a user can see: a picture, and a picture that is
/// still there after the summaries are recomputed from the audio.
#[test]
fn rebuilding_the_pyramid_draws_the_same_picture() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rebuild.vcw");
    record(&path, 2.0);

    let before = draw(&path, &["--pixels", "64"]);
    let after = draw(&path, &["--pixels", "64", "--rebuild"]);

    assert_eq!(before["channels"], after["channels"]);
    assert_eq!(before["level"], after["level"]);
    assert!(
        before["channels"][0]["peak"].as_f64().expect("peak") > 0.0,
        "the simulated source is not silent, so neither is its drawing"
    );
}

/// A span is a span whichever rung serves it, and the CLI is where that is
/// easiest to get wrong: seconds in, frames out.
#[test]
fn a_span_asked_for_in_seconds_comes_back_as_the_frames_it_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("span.vcw");
    record(&path, 3.0);

    let drawing = draw(&path, &["--start", "0.5", "--end", "1.5", "--pixels", "50"]);
    assert_eq!(drawing["start_frame"], 24_000);
    assert_eq!(drawing["end_frame"], 72_000);
    assert_eq!(drawing["frames_per_pixel"], 960.0);
    assert_eq!(drawing["channels"][0]["covered"], 48_000);
}

/// One channel on request, rather than every channel always.
#[test]
fn a_single_channel_can_be_asked_for_on_its_own() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("one.vcw");
    record(&path, 1.5);

    let both = draw(&path, &["--pixels", "20"]);
    let one = draw(&path, &["--pixels", "20", "--channel", "1"]);
    assert_eq!(widths(&both).len(), 2);
    assert_eq!(widths(&one).len(), 1);
    assert_eq!(one["channels"][0], both["channels"][1]);
}
