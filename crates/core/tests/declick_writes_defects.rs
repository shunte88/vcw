/*
 *  declick_writes_defects.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-30 step 4's exit criterion: the detector writes rows, re-runs, and respects a lock.
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

//! WP-30 §8 step 4: *the detector, writing defect rows, re-runnable, respecting
//! `locked`*.
//!
//! Three claims, and only the first is about the detector. The other two are what
//! makes a detector safe to point at a side twice:
//!
//! 1. A run over a capture with known damage in it writes a row per span, with the
//!    provenance, the kind and the repair the caller asked for.
//! 2. A second run with the same settings leaves the same list. A second run with
//!    stricter settings *shortens* it - which is the half that an insert-only
//!    detector gets wrong, because raising a threshold could then only ever add.
//! 3. A row a person has ruled on survives both, with their verdict and their
//!    credit intact, whatever the detector thinks of that span this time.
//!
//! Nothing here mocks the project layer, for the reason `from_a_project.rs` gives:
//! the claim is about rows in a real file, and every layer between the samples and
//! the row is part of what is being tested.

use std::path::Path;

use vcw_core::declick;
use vcw_project::persistence::{Config as WriteConfig, Writer};
use vcw_project::{Project, defect, side};
use vcw_signal::declick::Config;
use vcw_types::vinyl::Side;
use vcw_types::{
    CaptureEq, CaptureInfo, CaptureMode, CaptureState, Provenance, Repair, SampleRate,
    StorageFormat,
};

const RATE: u32 = 44_100;
const FRAMES: usize = 44_100 * 2;
/// Where the clicks go, on the left wall only.
const CLICKS: [usize; 5] = [11_111, 20_000, 41_337, 60_000, 77_777];

/// Two seconds of tonal material with a dither floor and five clicks in it.
///
/// The same shape as the detector's own fixture, and deliberately not silence:
/// a residual of zero has no standard deviation for the threshold to be taken
/// from, so a detector tested on silence is not being tested.
fn material() -> Vec<u8> {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut bytes = Vec::with_capacity(FRAMES * 4);
    for frame in 0..FRAMES {
        let t = frame as f64 / f64::from(RATE);
        let tone = 0.30 * (std::f64::consts::TAU * 220.0 * t).sin()
            + 0.15 * (std::f64::consts::TAU * 441.0 * t).sin()
            + 0.07 * (std::f64::consts::TAU * 1_337.0 * t).sin();
        let mut left = tone;
        if CLICKS.iter().any(|at| frame.abs_diff(*at) < 2) {
            left += if frame % 2 == 0 { 0.55 } else { -0.55 };
        }
        for value in [left, tone * 0.9] {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let dither = ((seed >> 40) as f64 / 16_777_216.0 - 0.5) * 0.004;
            let sample = ((value + dither).clamp(-1.0, 1.0) * 32_000.0) as i16;
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    bytes
}

/// A project holding one side of that material.
fn recorded(dir: &Path) -> (std::path::PathBuf, i64) {
    let info = CaptureInfo {
        rate: SampleRate(RATE),
        channels: 2,
        storage_format: StorageFormat::Int16,
        capture_mode: CaptureMode::Exclusive,
        host_api: Some("ALSA".into()),
        device_id: Some("hw:CARD=0,DEV=0".into()),
        device_name: Some("Cirrus Analog".into()),
        os_verified: false,
        os_report: None,
        eq: CaptureEq::Unknown,
    };
    let path = dir.join("declick.vcw");
    let project = Project::create(&path).expect("create");
    let mut writer = Writer::begin(project, &info, WriteConfig::default()).expect("begin");
    writer.push(&material()).expect("push");
    let (outcome, mut project, _) = writer
        .finish_with_project(CaptureState::Finalised)
        .expect("finish");
    side::ensure(&mut project, Side::A).expect("side");
    side::attach(&mut project, Side::A, outcome.capture_id).expect("attach");
    project.close().expect("close");
    (path, outcome.capture_id)
}

/// The whole of step 4, in the order the claims depend on each other.
#[test]
fn the_detector_writes_rows_reruns_and_respects_a_lock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (path, capture_id) = recorded(dir.path());
    let cfg = Config::default();
    let intent = declick::Intent::default();

    // 1. A run writes a row per span.
    let first = declick::scan_project(&path, capture_id, &cfg, &intent).expect("first pass");
    assert!(first.recorded);
    assert!(
        first.spans.len() >= CLICKS.len(),
        "five clicks, {} spans",
        first.spans.len()
    );
    assert_eq!(first.removed, 0, "nothing to replace on a first pass");
    assert_eq!(first.written, first.spans.len());

    let project = Project::open_read_only(&path).expect("open");
    let rows = defect::of_capture(project.conn(), capture_id).expect("read");
    project.close().expect("close");
    assert_eq!(rows.len(), first.spans.len());
    for row in &rows {
        assert_eq!(row.provenance, Provenance::Declick);
        assert_eq!(row.kind, "click");
        assert_eq!(row.repair, Repair::Interpolate);
        assert!(!row.locked, "a detector's row is not locked");
        assert!(row.end_frame > row.start_frame);
        assert!(row.touches(0) || row.touches(1));
        assert!(
            row.measurement_count() == 2,
            "§24 wants the measurements carried: {:?}",
            row.evidence
        );
    }
    // Every injected click is covered by a row on the damaged wall.
    for click in CLICKS {
        assert!(
            rows.iter()
                .any(|r| r.touches(0) && (r.start_frame..r.end_frame).contains(&(click as u64))),
            "no row covers the click at {click}"
        );
    }

    // 2. The same settings again give the same list, not two copies of it.
    let again = declick::scan_project(&path, capture_id, &cfg, &intent).expect("second pass");
    assert_eq!(again.spans, first.spans, "the detector is deterministic");
    assert_eq!(again.removed, first.written, "the stale rows went first");
    let project = Project::open_read_only(&path).expect("open");
    let after = defect::of_capture(project.conn(), capture_id).expect("read");
    project.close().expect("close");
    assert_eq!(
        after.len(),
        rows.len(),
        "a re-run duplicated rows instead of replacing them"
    );

    // 3. A person rules on two spans in the two different ways that are possible.
    //    They have to be two, because they exercise different clauses: a veto
    //    also takes the provenance over, so a delete scoped to the detector's own
    //    provenance would spare it whether it checked the lock or not. A span
    //    merely *pinned* is still the detector's, and the lock is the only thing
    //    standing between it and the next pass.
    let vetoed = after[1].clone();
    let pinned = after[2].clone();
    let mut project = Project::open(&path).expect("open");
    defect::set_repair(&mut project, vetoed.id, Repair::None).expect("veto");
    defect::set_lock(&mut project, pinned.id, true).expect("pin");
    project.close().expect("close");

    // 4. A stricter pass finds fewer spans. The unlocked ones go; the vetoed one
    //    stays, with the person's verdict and the person's credit on it.
    let strict = Config {
        threshold: 40.0,
        ..Config::default()
    };
    let third = declick::scan_project(&path, capture_id, &strict, &intent).expect("third pass");
    assert!(
        third.spans.len() < first.spans.len(),
        "a threshold of 40 sigma found {} spans where 4 found {}",
        third.spans.len(),
        first.spans.len()
    );
    let project = Project::open_read_only(&path).expect("open");
    let finally = defect::of_capture(project.conn(), capture_id).expect("read");
    project.close().expect("close");

    let kept = finally
        .iter()
        .find(|row| row.id == vetoed.id)
        .expect("the vetoed row survived a stricter pass");
    assert_eq!(kept.repair, Repair::None, "the veto held");
    assert!(kept.locked, "and it is still pinned");
    assert_eq!(
        kept.provenance,
        Provenance::User,
        "§24 reserves the credit as well as the position"
    );
    assert_eq!(kept.start_frame, vetoed.start_frame);
    assert_eq!(kept.end_frame, vetoed.end_frame);

    let held = finally
        .iter()
        .find(|row| row.id == pinned.id)
        .expect("a pinned detector row survived a stricter pass");
    assert!(held.locked, "it is still pinned");
    assert_eq!(
        held.provenance,
        Provenance::Declick,
        "and still the detector's"
    );
    assert_eq!(held.start_frame, pinned.start_frame);
    assert_eq!(held.end_frame, pinned.end_frame);

    assert!(
        finally.len() < rows.len(),
        "a stricter pass left {} rows where a looser one wrote {}",
        finally.len(),
        rows.len()
    );
    // At most: everything the strict pass found, plus the two rows it was not
    // allowed to touch. Fewer if the strict pass landed on one of them exactly,
    // which the upsert folds into the locked row rather than duplicating.
    assert!(
        finally.len() <= third.spans.len() + 2,
        "{} rows for {} strict spans and two locked ones",
        finally.len(),
        third.spans.len()
    );
}

/// A dry run predicts the real run, and writes nothing while doing it.
///
/// Counts are not names, so this compares the spans themselves: a preview that
/// reported the right number of the wrong things would be a prediction of nothing.
#[test]
fn a_dry_run_predicts_the_real_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (path, capture_id) = recorded(dir.path());
    let cfg = Config::default();
    let intent = declick::Intent::default();

    let predicted = declick::preview_project(&path, capture_id, &cfg, &intent).expect("preview");
    assert!(!predicted.recorded);
    assert!(!predicted.spans.is_empty());

    let project = Project::open_read_only(&path).expect("open");
    let rows = defect::of_capture(project.conn(), capture_id).expect("read");
    project.close().expect("close");
    assert!(rows.is_empty(), "a dry run wrote {} rows", rows.len());

    let real = declick::scan_project(&path, capture_id, &cfg, &intent).expect("scan");
    assert_eq!(
        predicted.spans, real.spans,
        "the prediction named the spans"
    );
    assert_eq!(predicted.written, real.written, "and counted them");
    assert_eq!(
        predicted.flagged_fraction(),
        real.flagged_fraction(),
        "and reported the same fraction of the side"
    );
}
