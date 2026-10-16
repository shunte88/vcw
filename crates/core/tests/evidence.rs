/*
 *  evidence.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Reading a project's own knowledge back out as resolver evidence.
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

//! Reading a project's own knowledge back out as resolver evidence (§23, §26).
//!
//! [`vcw_core::evidence::observe`] is a seam like [`vcw_core::identity`] is: the
//! resolver never opens a database and the project layer has never heard of a
//! resolver, so the interesting behavior is all in the translation. Two things
//! are worth a test. A typed fact and a measured one have to come out under
//! different sources, because §26 only lets a candidate contradict the first.
//! And a capture that holds two faces has to measure neither, because half a
//! capture is not a side and a candidate would then disagree with a figure the
//! project never really had.
//!
//! Audio is written straight in rather than recorded, the way `reanalysis.rs`
//! does it: what is under test is what the rows say, and a real capture would
//! only make the fixture slower.

use std::path::PathBuf;

use vcw_identify::evidence::{Fact, Source};
use vcw_project::persistence::{Config as Commits, Writer};
use vcw_project::{Project, release, side, track};
use vcw_types::vinyl::Side;
use vcw_types::{CaptureInfo, CaptureMode, CaptureState, SampleRate, StorageFormat};

const RATE: SampleRate = SampleRate(48_000);
/// Int16 stereo.
const FRAME: u64 = 4;

fn info() -> CaptureInfo {
    CaptureInfo {
        rate: RATE,
        channels: 2,
        storage_format: StorageFormat::Int16,
        capture_mode: CaptureMode::Exclusive,
        host_api: Some("test".into()),
        device_id: None,
        device_name: None,
        os_verified: false,
        os_report: None,
        eq: vcw_types::CaptureEq::Unknown,
    }
}

/// Side B. `Side` names only `A`, every other side being derived from a letter.
fn b() -> Side {
    Side::from_letter('B').expect("B is a side")
}

/// A project holding `seconds` of digital black, as one finished capture.
fn recorded(dir: &tempfile::TempDir, seconds: u64) -> (PathBuf, i64) {
    let path = dir.path().join("evidence.vcw");
    let project = Project::create(&path).expect("create");
    let mut writer = Writer::begin(project, &info(), Commits::default()).expect("begin");
    let frames = seconds * u64::from(RATE.hz());
    writer
        .push(&vec![0u8; (frames * FRAME) as usize])
        .expect("push");
    let (outcome, project, _) = writer
        .finish_with_project(CaptureState::Finalised)
        .expect("finish");
    project.close().expect("close");
    (path, outcome.capture_id)
}

/// Every side length the project offered, in seconds.
fn sides(known: &vcw_core::Known) -> Vec<u64> {
    known
        .observed
        .items()
        .iter()
        .filter_map(|item| match item.fact {
            Fact::Side(length) => Some(length.as_secs()),
            _ => None,
        })
        .collect()
}

#[test]
fn what_was_typed_and_what_was_measured_come_out_under_different_sources() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (path, capture) = recorded(&dir, 60);
    let mut project = Project::open(&path).expect("open");

    let mut record = release::ensure(&mut project).expect("ensure");
    record.album_artist = "Ultravox".into();
    record.album = "Vienna".into();
    record.catalog = "CHR 1296".into();
    record.label = "Chrysalis".into();
    record.year = Some(1980);
    release::store(&mut project, &record).expect("store");

    side::ensure(&mut project, Side::A).expect("side");
    side::attach(&mut project, Side::A, capture).expect("attach");
    for n in 0..3u64 {
        let start = n * 20 * u64::from(RATE.hz());
        track::add_track(
            &mut project,
            Side::A,
            start,
            start + 10 * u64::from(RATE.hz()),
        )
        .expect("track");
    }

    let known = vcw_core::observe(project.conn()).expect("observe");
    assert!(
        !known.confirmed,
        "nobody accepted a provider release, so the text is a person's"
    );
    assert_eq!(known.captured, [(Side::A, capture)]);

    let typed: Vec<&Fact> = known
        .observed
        .items()
        .iter()
        .filter(|item| item.source == Source::Stated)
        .map(|item| &item.fact)
        .collect();
    assert_eq!(
        typed.len(),
        5,
        "artist, album, catalog, label and year were all typed: {typed:?}"
    );
    assert!(typed.contains(&&Fact::Catalog("CHR 1296".into())));
    assert!(typed.contains(&&Fact::Year(1980)));

    let measured: Vec<&Fact> = known
        .observed
        .items()
        .iter()
        .filter(|item| item.source == Source::Signal)
        .map(|item| &item.fact)
        .collect();
    assert!(
        measured.contains(&&Fact::Count(3)),
        "how many tracks there are is measured, not stated: {measured:?}"
    );
    // Three ten-second tracks, and not the minute the capture runs: the side is
    // what the record's face holds, which is the figure a candidate states.
    assert_eq!(sides(&known), [30]);
}

#[test]
fn a_release_already_accepted_says_so() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("confirmed.vcw");
    let mut project = Project::create(&path).expect("create");
    let mut record = release::ensure(&mut project).expect("ensure");
    record.album = "Vienna".into();
    record.confirmed = true;
    release::store(&mut project, &record).expect("store");

    let known = vcw_core::observe(project.conn()).expect("observe");
    assert!(
        known.confirmed,
        "this text came back from a provider, and a walk over it is partly circular"
    );
}

#[test]
fn a_capture_that_holds_two_faces_measures_neither() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (path, capture) = recorded(&dir, 60);
    let mut project = Project::open(&path).expect("open");
    side::ensure(&mut project, Side::A).expect("side");
    side::attach(&mut project, Side::A, capture).expect("attach");

    // One face, no tracks: the capture is the side, so it measures.
    let alone = vcw_core::observe(project.conn()).expect("observe");
    assert_eq!(sides(&alone), [60]);

    // The second face of the same pass. Neither side is 60 seconds long now, and
    // nothing here can say where the flip was.
    side::ensure(&mut project, b()).expect("side");
    side::attach(&mut project, b(), capture).expect("attach");
    let shared = vcw_core::observe(project.conn()).expect("observe");
    assert!(
        sides(&shared).is_empty(),
        "half a capture is not a side: {:?}",
        sides(&shared)
    );
    assert_eq!(
        shared.captured,
        [(Side::A, capture), (b(), capture)],
        "both faces still hold audio an AcoustID lookup could run over"
    );
}
