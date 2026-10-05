/*
 *  detection_live.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Detection, driven by a real capture rather than by a synthetic trace (22, 23, 24).
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

//! Detection, driven by a real capture rather than by a synthetic trace (§22, §23).
//!
//! `vcw-signal`'s own tests prove the detectors classify correctly, given frames.
//! This proves the two things they cannot: that the live worker is fed the capture
//! stream while the record turns and publishes §35's `track-detected` before the
//! capture is declared finished, and that the refine pass can read a committed side
//! back out of a project and resolve it.
//!
//! The live half runs against the simulated source, which is a hash: uniform over
//! the code range, **-4.771 dBFS**, and never quiet. A side that is loud from the
//! first window to the last has exactly one boundary that can ever settle - the one
//! at frame 0 - because its other edge keeps moving as long as audio arrives. That
//! is the honest thing to assert about a live pass, and it is also the §22
//! requirement: a provisional marker that is never retracted.
//!
//! The refine half is given a side with real gaps in it, written straight into a
//! project, because nothing in the engine can yet record one.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use vcw_core::commands::{Command, Setup};
use vcw_core::events::{Event, Events};
use vcw_core::{Engine, detection};
use vcw_project::Project;
use vcw_project::persistence::{Config as Commits, Writer};
use vcw_signal::regions::Config;
use vcw_types::{
    CaptureInfo, CaptureMode, CaptureState, Edge, Provenance, SampleRate, StorageFormat,
};

const RATE: SampleRate = SampleRate(48_000);
const CHANNELS: u16 = 2;
/// Int16 stereo.
const FRAME: usize = 4;

/// Collects events until `done` says so, or panics with what it saw.
fn collect(events: &Events, done: impl Fn(&Event) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        match events.try_next() {
            Some(event) => {
                let stop = done(&event);
                seen.push(event);
                if stop {
                    return seen;
                }
            }
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    panic!("gave up waiting; saw {} events", seen.len());
}

/// Every marker in a run, in the order it was published.
fn markers(seen: &[Event]) -> Vec<(u64, Edge, f32, Provenance)> {
    seen.iter()
        .filter_map(|e| match e {
            Event::Detected {
                frame,
                edge,
                confidence,
                provenance,
                ..
            } => Some((*frame, *edge, *confidence, *provenance)),
            _ => None,
        })
        .collect()
}

/// §22's live half: a marker arrives while the record is still turning.
#[test]
fn a_marker_is_published_before_the_capture_is_finished() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::start().expect("start");
    let events = engine.events();

    engine
        .send(Command::Arm(Box::new(
            Setup::simulated(dir.path().join("live.vcw"))
                .at(RATE.hz())
                .channels(CHANNELS),
        )))
        .expect("arm");
    engine.send(Command::Record).expect("record");

    // Past the settling lag - min_silence + gap_fill + both paddings, 1.2 s at the
    // defaults - plus a drain interval, so a marker that is going to be announced
    // has had its chance.
    std::thread::sleep(Duration::from_millis(2_200));
    engine.send(Command::Stop).expect("stop");
    let seen = collect(&events, |e| matches!(e, Event::Finished { .. }));

    let found = markers(&seen);
    assert!(
        !found.is_empty(),
        "no track-detected in a 2.2 s capture; saw {} events",
        seen.len()
    );

    let (frame, edge, confidence, provenance) = found[0];
    assert_eq!(
        edge,
        Edge::Start,
        "the side opens in music, so it opens a region"
    );
    assert_eq!(frame, 0, "the opening edge of a capture is frame 0");
    assert_eq!(
        provenance,
        Provenance::Silence,
        "the live pass is levels only - no FFT on the capture thread"
    );
    assert!(
        (0.0..=1.0).contains(&confidence),
        "confidence {confidence} is not a confidence"
    );

    // Announced once. A UI that has to de-duplicate markers has been handed a bug.
    assert_eq!(
        found
            .iter()
            .filter(|(f, e, ..)| *f == 0 && *e == Edge::Start)
            .count(),
        1,
        "the same marker was published more than once: {found:?}"
    );
    // And the moving end was never announced, because it was never settled.
    assert!(
        !found.iter().any(|(_, e, ..)| *e == Edge::End),
        "an end was published while it could still move: {found:?}"
    );

    // Ordering: a marker after capture-finished reaches a UI that has already drawn
    // its final waveform.
    let finished = seen
        .iter()
        .position(|e| matches!(e, Event::Finished { .. }))
        .expect("the capture must finish");
    let last = seen
        .iter()
        .rposition(|e| matches!(e, Event::Detected { .. }))
        .expect("a marker must have been published");
    assert!(last < finished, "track-detected followed capture-finished");

    engine.shutdown().expect("shutdown");
}

/// A capture with the detectors attached is still the capture it would have been.
#[test]
fn detection_costs_the_capture_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::start().expect("start");
    let events = engine.events();

    engine
        .send(Command::Arm(Box::new(
            Setup::simulated(dir.path().join("intact.vcw"))
                .at(96_000)
                .channels(CHANNELS),
        )))
        .expect("arm");
    engine.send(Command::Record).expect("record");
    std::thread::sleep(Duration::from_millis(800));
    engine.send(Command::Stop).expect("stop");

    let seen = collect(&events, |e| matches!(e, Event::Finished { .. }));
    let Some(Event::Finished {
        frames,
        diagnostics,
        ..
    }) = seen.iter().find(|e| matches!(e, Event::Finished { .. }))
    else {
        unreachable!()
    };
    // Two taps on the fan-out now, the meter's and the detectors'.
    assert_eq!(diagnostics.dropped_frames, 0, "the second tap cost audio");
    assert_eq!(diagnostics.overruns, 0, "the second tap slowed the writer");
    assert!(
        *frames > 48_000,
        "800 ms at 96 kHz committed only {frames} frames"
    );

    engine.shutdown().expect("shutdown");
}

fn info() -> CaptureInfo {
    CaptureInfo {
        rate: RATE,
        channels: CHANNELS,
        storage_format: StorageFormat::Int16,
        capture_mode: CaptureMode::Exclusive,
        host_api: Some("ALSA".into()),
        device_id: Some("hw:CARD=0,DEV=0".into()),
        device_name: Some("Cirrus Analog".into()),
        os_verified: false,
        os_report: None,
        eq: vcw_types::CaptureEq::Unknown,
    }
}

/// A side: music, groove, music, groove, music, as interleaved Int16 stereo.
///
/// The music is a tone, so its spectrum is a line and the flatness detector has
/// something to see. The gaps are noise 55 dB down rather than digital black,
/// because that is what the run-out of a record actually sounds like - and a gap
/// with no signal at all has no spectrum to be flat.
fn side(plan: &[(f64, bool)]) -> Vec<u8> {
    let mut pcm = Vec::new();
    let mut phase = 0.0_f64;
    let mut noise = 0x2545_F491_4F6C_DD1D_u64;
    let mut frame = 0_u64;
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
            frame += 1;
        }
    }
    assert_eq!(pcm.len(), frame as usize * FRAME);
    pcm
}

/// Writes a side into a project the way a capture would have, and returns it.
fn recorded(dir: &tempfile::TempDir, name: &str, pcm: &[u8]) -> (PathBuf, i64) {
    let path = dir.path().join(name);
    let project = Project::create(&path).expect("create");
    let mut writer = Writer::begin(project, &info(), Commits::default()).expect("begin");
    writer.push(pcm).expect("push");
    let (outcome, project, _) = writer
        .finish_with_project(CaptureState::Finalised)
        .expect("finish");
    project.close().expect("close");
    (path, outcome.capture_id)
}

/// §22's other half: the post-capture pass, over the complete recording.
#[test]
fn the_refine_pass_reads_a_side_back_out_of_the_project() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plan = [
        (3.0, true),
        (1.2, false),
        (3.0, true),
        (1.2, false),
        (3.0, true),
    ];
    let (path, capture_id) = recorded(&dir, "refine.vcw", &side(&plan));

    let refined =
        detection::refine_project(&path, capture_id, &Config::new(), &[]).expect("refine");

    // Three detectors ran, each over the same frames.
    let ran: Vec<Provenance> = refined.diagnostics.iter().map(|(p, _)| *p).collect();
    assert_eq!(
        ran,
        vec![
            Provenance::Silence,
            Provenance::SpectralChange,
            Provenance::Hmm
        ]
    );
    for (provenance, diagnostics) in &refined.diagnostics {
        assert_eq!(
            diagnostics.windows,
            refined.windows,
            "{} saw a different side from the others",
            provenance.as_str()
        );
    }
    // 11.4 s at a 100 ms window.
    assert!(
        (113..=115).contains(&refined.windows),
        "{} windows for an 11.4 s side",
        refined.windows
    );

    // Three tracks: three starts and three ends, at the frame.
    //
    // The tone runs 0-3, 4.2-7.2 and 8.4-11.4 s. The positions below are those
    // edges with §22's padding applied - a start pulled 0.1 s earlier and an end
    // pushed 0.1 s later, clamped at each end of the capture - which is why the
    // first start is 0 rather than -0.1 and the last end is the final frame rather
    // than 11.5 s. They are pinned exactly because the whole chain is
    // deterministic: a window that moves means a detector moved, not that a float
    // rounded.
    let at = |edge| -> Vec<u64> { refined.edges(edge).iter().map(|d| d.at).collect() };
    let second = u64::from(RATE.hz());
    assert_eq!(
        at(Edge::Start),
        vec![0, 41 * second / 10, 83 * second / 10],
        "starts, against ends at {:?}",
        at(Edge::End)
    );
    assert_eq!(
        at(Edge::End),
        vec![31 * second / 10, 73 * second / 10, 114 * second / 10],
        "ends, against starts at {:?}",
        at(Edge::Start)
    );

    // Every boundary was found by all three detectors, independently, from the same
    // frames - which is the property the shared extraction in `refine` buys.
    for decision in &refined.decisions {
        assert_eq!(
            decision.agreement(),
            3,
            "{:.3} s was seen by {:?} alone",
            decision.at as f64 / f64::from(RATE.hz()),
            decision.sources
        );
    }

    // §22's HMM, and the one rule in it that only an end-to-end case exercises:
    // the posterior is a veto, and a capture edge has no transition to veto.
    let applies = |d: &vcw_signal::resolve::Decision| d.measurement("hmm.posterior_applies");
    assert_eq!(
        applies(&refined.decisions[0]),
        Some(0.0),
        "the opening edge"
    );
    assert_eq!(
        applies(refined.decisions.last().expect("a last decision")),
        Some(0.0),
        "the closing edge"
    );
    for decision in &refined.decisions[1..refined.decisions.len() - 1] {
        assert_eq!(
            applies(decision),
            Some(1.0),
            "an interior boundary at {:.3} s escaped the veto",
            decision.at as f64 / f64::from(RATE.hz())
        );
    }

    // §24: every boundary carries provenance, confidence and its evidence.
    for decision in &refined.decisions {
        assert!(
            decision.confidence > 0.0 && decision.confidence <= 1.0,
            "{decision:?} has no usable confidence"
        );
        assert!(
            !decision.sources.is_empty(),
            "{decision:?} came from nowhere"
        );
        assert!(
            !decision.evidence.is_empty(),
            "{decision:?} shows no evidence"
        );
        assert!(!decision.locked, "nothing here was placed by a person");
    }

    assert!(
        refined.took < Duration::from_secs(5),
        "took {:?}",
        refined.took
    );
}

/// §24: a boundary a person placed survives the pass that runs around it.
#[test]
fn a_confirmed_boundary_is_not_moved_by_the_refine_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plan = [(3.0, true), (1.2, false), (3.0, true)];
    let (path, capture_id) = recorded(&dir, "locked.vcw", &side(&plan));

    // Slightly off where the detectors would put it, and inside their tolerance,
    // so the only way it stays put is if §24 is honored.
    let at = (3.1 * f64::from(RATE.hz())) as u64;
    let confirmed = vcw_types::BoundaryObservation::new(at, Edge::End, 1.0, Provenance::User);

    let refined =
        detection::refine_project(&path, capture_id, &Config::new(), &[confirmed]).expect("refine");

    let ends = refined.edges(Edge::End);
    let mine = ends
        .iter()
        .find(|d| d.provenance == Provenance::User)
        .expect("the boundary a person placed must still be there");
    assert_eq!(mine.at, at, "an automatic pass moved a confirmed boundary");
    assert!(mine.locked, "a user boundary must come back locked");
    assert!(
        mine.confidence >= 1.0,
        "a confirmed boundary is not a guess: {}",
        mine.confidence
    );
}
