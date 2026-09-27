/*
 *  reanalysis.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-13's second exit criterion: a locked boundary survives re-analysis.
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

//! WP-13's second exit criterion: a locked boundary survives re-analysis.
//!
//! §24 says a boundary a person placed or confirmed shall not be moved by
//! automatic analysis. Three things have to hold for that to be true of the
//! finished system rather than of one function:
//!
//! 1. The detectors' own resolver keeps a user boundary where it is, which
//!    `detection_live.rs` already asserts about one pass.
//! 2. The project layer refuses to move a locked row, which `vcw-project`'s own
//!    tests assert.
//! 3. **Adoption writes the second pass into a project that already holds the
//!    first, without disturbing what the operator settled in between.** That is
//!    what this file is for, and it is the only place all three meet.
//!
//! The method is the real loop: capture a side, refine it, adopt it, have the
//! operator move and lock a boundary somewhere the detectors did not put one, then
//! refine and adopt again - with a *different, more sensitive configuration*, so
//! the second pass genuinely disagrees with the first - and check the operator's
//! boundary is untouched in position, provenance and lock.
//!
//! Audio is written straight into a project rather than recorded, for the same
//! reason `detection_live.rs` does it: nothing in the engine can yet record a side
//! with real gaps in it.

use std::path::PathBuf;

use vcw_core::adopt::{self, Policy};
use vcw_project::persistence::{Config as Commits, Writer};
use vcw_project::track::NewBoundary;
use vcw_project::{Project, track, validate};
use vcw_signal::regions::Config;
use vcw_signal::resolve::Decision;
use vcw_types::observation::{Edge, Provenance};
use vcw_types::vinyl::Side;
use vcw_types::{CaptureInfo, CaptureMode, CaptureState, SampleRate, StorageFormat};

const RATE: SampleRate = SampleRate(48_000);
const CHANNELS: u16 = 2;
/// Int16 stereo.
const FRAME: usize = 4;

fn info() -> CaptureInfo {
    CaptureInfo {
        rate: RATE,
        channels: CHANNELS,
        storage_format: StorageFormat::Int16,
        capture_mode: CaptureMode::Exclusive,
        host_api: Some("test".into()),
        device_id: None,
        device_name: None,
        os_verified: false,
        os_report: None,
    }
}

/// A side of alternating music and run-out noise, as bytes.
///
/// The same generator `detection_live.rs` uses: a 440 Hz tone for the music and
/// hiss 55 dB down for the gaps, because a gap of digital black has no spectrum to
/// be flat and is not what a record sounds like.
fn side(plan: &[(f64, bool)]) -> Vec<u8> {
    let mut pcm = Vec::new();
    let mut phase = 0.0_f64;
    let mut noise = 0x2545_F491_4F6C_DD1D_u64;
    for &(secs, music) in plan {
        let frames = (secs * f64::from(RATE.hz())).round() as u64;
        for _ in 0..frames {
            noise = noise
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let hiss = (noise >> 40) as i32 as f64 / 16_777_216.0 - 0.5;
            let value = if music {
                phase += std::f64::consts::TAU * 440.0 / f64::from(RATE.hz());
                phase.sin() * 0.25
            } else {
                hiss * 0.003_5
            };
            let sample = (value * f64::from(i16::MAX)).round() as i16;
            for _ in 0..CHANNELS {
                pcm.extend_from_slice(&sample.to_le_bytes());
            }
        }
    }
    pcm
}

/// Writes a side into a project the way a capture would have.
fn recorded(dir: &tempfile::TempDir, name: &str, pcm: &[u8]) -> (PathBuf, i64) {
    let path = dir.path().join(name);
    let project = Project::create(&path).expect("create");
    let mut writer = Writer::begin(project, &info(), Commits::default()).expect("begin");
    writer.push(pcm).expect("push");
    let (outcome, project, _) = writer
        .finish_with_project(CaptureState::Finalised)
        .expect("finish");
    project.close().expect("close");
    assert_eq!(pcm.len() % FRAME, 0);
    (path, outcome.capture_id)
}

/// Runs a refine pass over a side that already has rows, and adopts the result.
///
/// The whole of re-analysis in one function, and the shape a UI's "analyse again"
/// button will have: hand the pass what the project already believes, let the
/// resolver weigh it against what the detectors found, then write the result back
/// under the policy.
fn analyse(project: &mut Project, capture: i64, cfg: &Config, policy: &Policy) -> adopt::Adopted {
    let already = adopt::observations(project, Side::A).unwrap_or_default();
    let refined = vcw_core::detection::refine(project, capture, cfg, &already).expect("refine");
    adopt::adopt(project, Side::A, &refined, policy).expect("adopt")
}

#[test]
fn a_locked_boundary_survives_a_second_analysis_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plan = [
        (3.0, true),
        (1.2, false),
        (3.0, true),
        (1.2, false),
        (3.0, true),
    ];
    let (path, capture) = recorded(&dir, "reanalysis.vcw", &side(&plan));
    let mut project = Project::open(&path).expect("open");
    vcw_project::side::attach(&mut project, Side::A, capture).expect("attach");

    let policy = Policy::at(RATE);
    let first = analyse(&mut project, capture, &Config::new(), &policy);
    assert!(
        first.written() > 0,
        "the first pass found nothing to adopt: {first:?}"
    );

    // The operator moves a boundary to where they think the track really ends,
    // 200 ms off what the detectors said, and that act locks it.
    let ends = track::boundaries(project.conn(), Side::A).expect("boundaries");
    let chosen = ends
        .iter()
        .find(|b| b.edge == Edge::End && b.at_frame > RATE.hz() as u64)
        .expect("an end boundary to argue with")
        .clone();
    let moved_to = chosen.at_frame + u64::from(RATE.hz()) / 5;
    track::move_boundary_forced(&mut project, chosen.id, moved_to).expect("move");

    let settled = track::boundary(project.conn(), chosen.id)
        .expect("read")
        .expect("row");
    assert_eq!(settled.at_frame, moved_to);
    assert_eq!(settled.provenance, Provenance::User);
    assert!(settled.locked);

    // A deliberately different configuration, so the second pass is a real second
    // opinion rather than a replay: it will report boundaries the first did not.
    let keener = Config {
        threshold_db: Config::new().threshold_db + 6.0,
        ..Config::new()
    };
    let second = analyse(&mut project, capture, &keener, &policy);

    let after = track::boundary(project.conn(), chosen.id)
        .expect("read")
        .expect("row");
    assert_eq!(
        after.at_frame, moved_to,
        "a second analysis pass moved a boundary a person placed"
    );
    assert_eq!(
        after.provenance,
        Provenance::User,
        "a second analysis pass took the credit for a person's boundary"
    );
    assert!(after.locked, "a second analysis pass unlocked it");

    // No second boundary appeared beside it. Worth checking separately from the
    // position, because §24 would be just as broken by a detector's boundary
    // sitting 200 ms away from the operator's as by the operator's having moved:
    // the side would then have two candidate ends for one track.
    let nearby = track::boundaries(project.conn(), Side::A)
        .expect("boundaries")
        .into_iter()
        .filter(|b| {
            b.edge == Edge::End && b.at_frame.abs_diff(moved_to) <= policy.tolerance.frames()
        })
        .count();
    assert_eq!(nearby, 1, "the pass left a rival end beside the locked one");

    // `already_locked` is zero here and that is not a failure: the resolver saw
    // the operator's boundary alongside the detectors' - `analyse` hands it in -
    // and merged them into one decision that `Provenance::User` won, so there was
    // never a separate detector decision for adoption to skip. §24 is being
    // honoured one layer earlier than this counter measures. The path the counter
    // does measure is covered by
    // `a_detector_landing_beside_a_locked_boundary_is_skipped`.
    assert_eq!(second.already_locked, 0, "{second:?}");

    // And the side is still coherent: the locked boundary is one of the ones the
    // tracks are built from, not an orphan the pass routed around.
    let report = validate(&project, vcw_project::Options::default()).expect("validate");
    assert!(report.is_clean(), "{:?}", report.findings);
}

/// One decision, as a detector pass would have produced it.
fn decision(at: u64, edge: Edge, sources: &[Provenance]) -> Decision {
    Decision {
        at,
        edge,
        confidence: 0.8,
        provenance: *sources.last().expect("a decision has a source"),
        sources: sources.to_vec(),
        evidence: Vec::new(),
        locked: false,
    }
}

/// A project with a side attached to a capture, for tests that supply their own
/// decisions rather than detecting any.
fn a_side(dir: &tempfile::TempDir, name: &str) -> Project {
    let (path, capture) = recorded(dir, name, &side(&[(8.0, true)]));
    let mut project = Project::open(&path).expect("open");
    vcw_project::side::attach(&mut project, Side::A, capture).expect("attach");
    project
}

#[test]
fn adoption_will_not_promote_a_boundary_only_one_detector_saw() {
    // The policy's whole purpose: a level dip that no other detector agrees with
    // is a quiet passage, and splitting a track there costs an operator more to
    // undo than a missed split costs to add.
    //
    // The decisions are supplied rather than detected, because a synthetic side
    // with clean gaps is one all three detectors agree about - which is the right
    // answer for that audio and no test of a policy about disagreement. What a
    // worn pressing produces is a decision with one source, so that is what this
    // hands in.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = a_side(&dir, "policy.vcw");
    let rate = u64::from(RATE.hz());
    let decisions = [
        decision(0, Edge::Start, &[Provenance::Silence, Provenance::Hmm]),
        decision(3 * rate, Edge::End, &[Provenance::Silence, Provenance::Hmm]),
        // The lonely one: a level dip in the middle of a quiet passage.
        decision(4 * rate, Edge::Start, &[Provenance::Silence]),
        decision(5 * rate, Edge::End, &[Provenance::Silence]),
    ];

    let strict = adopt::adopt_decisions(&mut project, Side::A, &decisions, &Policy::at(RATE))
        .expect("adopt");
    assert_eq!(strict.rejected, 2, "{strict:?}");
    assert_eq!(strict.written(), 2);
    for boundary in track::boundaries(project.conn(), Side::A).expect("boundaries") {
        assert!(
            boundary.agreement() >= 2,
            "a boundary only one detector saw got in: {boundary:?}"
        );
    }

    // And the permissive policy does take them, so the difference is the policy
    // rather than the decisions.
    let loose = Policy {
        min_sources: 1,
        ..Policy::at(RATE)
    };
    let permissive =
        adopt::adopt_decisions(&mut project, Side::A, &decisions, &loose).expect("adopt");
    assert_eq!(permissive.rejected, 0, "{permissive:?}");
    assert!(
        track::boundaries(project.conn(), Side::A)
            .expect("boundaries")
            .iter()
            .any(|b| b.agreement() == 1),
        "the permissive policy should have let the lonely ones in"
    );
}

#[test]
fn a_source_filter_narrows_what_is_written_without_dropping_the_operator() {
    // §39's `detection.algorithm`. The point worth testing is not that the
    // filter works but that it does not apply to a locked boundary: the only
    // source an operator's boundary carries is `User`, so a naive
    // `sources.contains(required)` would turn down a person's own marker the
    // moment they narrowed the setting to one detector.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = a_side(&dir, "filter.vcw");
    let rate = u64::from(RATE.hz());

    let mut spectral_only = decision(
        4 * rate,
        Edge::Start,
        &[Provenance::SpectralChange, Provenance::Hmm],
    );
    spectral_only.locked = false;
    // What `locked_observations` hands back for a boundary a person placed.
    let mut mine = decision(8 * rate, Edge::Start, &[Provenance::User]);
    mine.locked = true;

    let both = [spectral_only, mine];
    let policy = Policy {
        require_source: Some(Provenance::Silence),
        ..Policy::at(RATE)
    };
    let adopted = adopt::adopt_decisions(&mut project, Side::A, &both, &policy).expect("adopt");
    assert_eq!(
        adopted.rejected, 1,
        "the spectral-only boundary should have been turned down: {adopted:?}"
    );
    assert_eq!(
        adopted.written(),
        1,
        "the operator's own boundary is not a detector's to filter: {adopted:?}"
    );
    let rows = track::boundaries(project.conn(), Side::A).expect("boundaries");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at_frame, 8 * rate);

    // And with no filter, both go in, so the difference is the setting.
    let adopted =
        adopt::adopt_decisions(&mut project, Side::A, &both, &Policy::at(RATE)).expect("adopt");
    assert_eq!(adopted.rejected, 0, "{adopted:?}");
    assert_eq!(
        track::boundaries(project.conn(), Side::A)
            .expect("boundaries")
            .len(),
        2
    );
}

#[test]
fn a_detector_landing_beside_a_locked_boundary_is_skipped() {
    // The case the resolver cannot catch: a decision that arrives at adoption
    // without having been weighed against the operator's boundary, because the
    // caller did not hand the project's rows in. Adoption still must not put a
    // rival end 200 ms from a confirmed one.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = a_side(&dir, "beside.vcw");
    let rate = u64::from(RATE.hz());
    let settled = track::add_boundary(
        &mut project,
        Side::A,
        &NewBoundary::by_user(3 * rate, Edge::End),
    )
    .expect("place");

    let near = decision(
        3 * rate + rate / 5,
        Edge::End,
        &[Provenance::Silence, Provenance::Hmm],
    );
    let adopted =
        adopt::adopt_decisions(&mut project, Side::A, &[near], &Policy::at(RATE)).expect("adopt");
    assert_eq!(adopted.already_locked, 1, "{adopted:?}");
    assert_eq!(adopted.written(), 0);

    let ends = track::boundaries(project.conn(), Side::A).expect("boundaries");
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0].id, settled);
    assert_eq!(ends[0].at_frame, 3 * rate);

    // Far enough away and it is a different boundary, which it has to be, or a
    // locked boundary would blind the pass to everything within half a second of
    // it for the rest of the project's life.
    let far = decision(6 * rate, Edge::End, &[Provenance::Silence, Provenance::Hmm]);
    let adopted =
        adopt::adopt_decisions(&mut project, Side::A, &[far], &Policy::at(RATE)).expect("adopt");
    assert_eq!(adopted.written(), 1, "{adopted:?}");
}

#[test]
fn running_the_same_analysis_twice_changes_nothing() {
    // Idempotence is what makes an analyse button safe to press twice, and it is
    // not free: an adoption that inserted rather than upserted would double every
    // boundary, and a pairing pass that ignored existing tracks would double
    // those.
    let dir = tempfile::tempdir().expect("tempdir");
    let plan = [(3.0, true), (1.2, false), (3.0, true), (1.2, false)];
    let (path, capture) = recorded(&dir, "twice.vcw", &side(&plan));
    let mut project = Project::open(&path).expect("open");
    vcw_project::side::attach(&mut project, Side::A, capture).expect("attach");

    let policy = Policy::at(RATE);
    analyse(&mut project, capture, &Config::new(), &policy);
    let boundaries = track::boundaries(project.conn(), Side::A).expect("boundaries");
    let tracks = track::tracks(project.conn(), Side::A).expect("tracks");

    analyse(&mut project, capture, &Config::new(), &policy);
    assert_eq!(
        track::boundaries(project.conn(), Side::A)
            .expect("boundaries")
            .iter()
            .map(|b| (b.at_frame, b.edge))
            .collect::<Vec<_>>(),
        boundaries
            .iter()
            .map(|b| (b.at_frame, b.edge))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        track::tracks(project.conn(), Side::A)
            .expect("tracks")
            .iter()
            .map(|t| (t.number, t.start, t.end))
            .collect::<Vec<_>>(),
        tracks
            .iter()
            .map(|t| (t.number, t.start, t.end))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn re_analysis_does_not_grow_the_evidence_column() {
    // The other half of idempotence, and the half that was wrong. Positions
    // settle after one pass because the writes are upserts; the *case* for each
    // boundary settles only if handing a stored row back to the resolver is
    // reversible. It was not: the row holds a decision, so every measurement in
    // it is already named after the detector that took it, and `resolve` prefixes
    // an observation's evidence with its provenance as it absorbs it. Pass two
    // turned `hmm.posterior` into `hmm.hmm.posterior`, pass three into
    // `hmm.hmm.hmm.posterior`, and a side someone analyses while tuning a
    // threshold accumulated text without accumulating knowledge. Caught on the
    // real side, where one boundary was carrying 40 measurements, most of them the
    // same number under a longer name.
    let dir = tempfile::tempdir().expect("tempdir");
    let plan = [(3.0, true), (1.2, false), (3.0, true), (1.2, false)];
    let (path, capture) = recorded(&dir, "evidence.vcw", &side(&plan));
    let mut project = Project::open(&path).expect("open");
    vcw_project::side::attach(&mut project, Side::A, capture).expect("attach");

    let policy = Policy::at(RATE);
    let names = |project: &Project| -> Vec<Vec<String>> {
        track::boundaries(project.conn(), Side::A)
            .expect("boundaries")
            .iter()
            .map(|b| b.evidence.iter().map(|e| e.name.clone()).collect())
            .collect()
    };

    analyse(&mut project, capture, &Config::new(), &policy);
    analyse(&mut project, capture, &Config::new(), &policy);
    let settled = names(&project);
    analyse(&mut project, capture, &Config::new(), &policy);
    assert_eq!(
        names(&project),
        settled,
        "a third identical pass changed the evidence a second one had settled"
    );

    for boundary in &settled {
        for name in boundary {
            let parts: Vec<&str> = name.split('.').collect();
            for pair in parts.windows(2) {
                assert_ne!(
                    pair[0], pair[1],
                    "evidence name {name} repeats a detector, so a prefix was applied twice"
                );
            }
        }
        let mut seen = boundary.clone();
        seen.sort();
        let before = seen.len();
        seen.dedup();
        assert_eq!(
            seen.len(),
            before,
            "the same measurement is stored twice under one name: {boundary:?}"
        );
    }
}
