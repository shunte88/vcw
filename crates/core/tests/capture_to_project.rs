/*
 *  capture_to_project.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A capture and its project, together: the WP-04 exit criterion end to end.
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

//! A capture and its project, together: the WP-04 exit criterion end to end.
//!
//! `vcw-audio` counts what went wrong and `vcw-project` stores it, and neither
//! crate depends on the other - by design, since both sit on `vcw-types` alone.
//! That leaves nothing inside either one able to prove the two halves meet.
//! `vcw-core` is where they are composed, so this is where the proof lives.
//!
//! What is being proved is narrow and specific: the diagnostics a capture
//! produced are in the file afterwards, the frame count survives a crash, and a
//! project holding either is still a valid project.

use std::time::Duration;

use vcw_audio::capture::Negotiated;
use vcw_audio::source::{Faults, Pace, Pattern, Simulated, Source};
use vcw_project::validate::{Options, validate};
use vcw_project::{Project, Session, session};
use vcw_types::{CaptureState, SampleFormat, SampleRate};

/// Every real-time test in this file is a capture running at the speed of a
/// record, and the harness would otherwise start all of them at once. On a
/// four-core hosted runner that is what they measure: a Windows runner lost
/// 2400 frames to five ring overruns here, which is the machine failing to
/// provide real time rather than VCW failing to keep up. One at a time, and
/// the loss assertions mean what they say again. Copied from
/// `vcw-cli`'s `session_from_cli.rs`, which learned it first; a mutex per
/// test binary is as far as this goes, and cargo already runs the binaries
/// one after another.
fn alone() -> std::sync::MutexGuard<'static, ()> {
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GATE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn project(dir: &tempfile::TempDir, name: &str) -> Project {
    Project::create(dir.path().join(name)).expect("create")
}

/// Reads the ring for a while and throws the bytes away, which is what WP-05
/// will do with them properly. Returns the frames the source says it delivered.
fn run_for(
    source: &Simulated,
    reader: &mut vcw_audio::buffers::RingReader,
    limit: Duration,
) -> u64 {
    let mut scratch = vec![0u8; 64 * 1024];
    let deadline = std::time::Instant::now() + limit;
    while std::time::Instant::now() < deadline {
        if reader.read(&mut scratch) == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    source.frames()
}

#[test]
fn a_clean_capture_lands_in_the_project_with_its_counters() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let mut p = project(&dir, "clean.vcw");

    let (source, mut reader) =
        Simulated::deterministic(SampleRate(96_000), 2, Pace::RealTime).expect("start");
    let session = Session::begin(&mut p, &source.info()).expect("begin");
    let frames = run_for(&source, &mut reader, Duration::from_millis(250));
    let diagnostics = source.stop();

    session
        .finish(&mut p, CaptureState::Finalised, frames, diagnostics)
        .expect("finish");

    let path = p.path().to_path_buf();
    p.close().expect("close");

    // Reopened, because a counter that only exists in the process that made it
    // has not been persisted, whatever the in-memory assertion says.
    let reopened = Project::open(&path).expect("reopen");
    let record = session::load(reopened.conn(), session.id())
        .expect("load")
        .expect("row");
    assert_eq!(record.diagnostics, diagnostics);
    assert_eq!(record.frames, frames);
    assert_eq!(record.info.rate, SampleRate(96_000));
    assert_eq!(record.state, CaptureState::Finalised);
    assert!(record.duration_secs() > 0.0);
    assert!(
        validate(&reopened, Options::default())
            .expect("validate")
            .is_clean()
    );
}

#[test]
fn r9_the_counters_from_an_unplugged_device_reach_the_file() {
    let _alone = alone();
    // The fault that WP-03 deferred to here. The capture is interrupted, the
    // stream error is on disk, and the project is still sound.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut p = project(&dir, "unplugged.vcw");

    let (source, mut reader) = Simulated::start(
        Negotiated::simulated(SampleRate(48_000), 2, SampleFormat::S32),
        &Pattern::Deterministic,
        Pace::RealTime,
        Faults::unplug_after(4_800),
        500,
    )
    .expect("start");
    let session = Session::begin(&mut p, &source.info()).expect("begin");
    let frames = run_for(&source, &mut reader, Duration::from_millis(300));
    let diagnostics = source.stop();
    assert_eq!(diagnostics.stream_errors, 1, "the fault did not fire");

    session
        .finish(&mut p, CaptureState::Interrupted, frames, diagnostics)
        .expect("finish");

    let record = session::load(p.conn(), session.id())
        .expect("load")
        .expect("row");
    assert_eq!(record.state, CaptureState::Interrupted);
    assert_eq!(record.diagnostics.stream_errors, 1);
    assert!(!record.diagnostics.is_clean());
    assert!(
        validate(&p, Options::default())
            .expect("validate")
            .is_clean(),
        "a device coming out mid-capture damaged the project"
    );
}

#[test]
fn a_capture_killed_before_it_finished_is_recoverable_from_committed_rows_alone() {
    let _alone = alone();
    // §15. Nothing calls finish(), exactly as nothing would if the power went.
    // What is left has to be enough for recovery to know what happened.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("killed.vcw");
    let frames;
    let session_id;
    {
        let mut p = Project::create(&path).expect("create");
        let (source, mut reader) =
            Simulated::deterministic(SampleRate(44_100), 2, Pace::RealTime).expect("start");
        let session = Session::begin(&mut p, &source.info()).expect("begin");
        session_id = session.id();
        frames = run_for(&source, &mut reader, Duration::from_millis(150));
        session.advance(p.conn(), frames).expect("advance");
        session
            .record(p.conn(), source.diagnostics())
            .expect("record");
        source.stop();
        // No finish, and no close: the handle is dropped where it stands.
    }

    let reopened = Project::open(&path).expect("reopen");
    let open = session::unfinished(reopened.conn()).expect("unfinished");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].id, session_id);
    assert!(open[0].needs_recovery());
    assert_eq!(open[0].frames, frames, "recovery cannot see how far it got");
    assert_eq!(open[0].info.rate, SampleRate(44_100));
    assert!(
        validate(&reopened, Options::default())
            .expect("validate")
            .is_clean(),
        "an interrupted capture must not read as a damaged project"
    );
}

#[test]
fn two_captures_in_one_project_keep_their_own_counters() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let mut p = project(&dir, "two.vcw");

    let (first, mut r1) = Simulated::start(
        Negotiated::simulated(SampleRate(48_000), 2, SampleFormat::S32),
        &Pattern::Silence,
        Pace::RealTime,
        Faults::unplug_after(480),
        500,
    )
    .expect("start");
    let a = Session::begin(&mut p, &first.info()).expect("begin");
    let a_frames = run_for(&first, &mut r1, Duration::from_millis(150));
    let a_diagnostics = first.stop();
    a.finish(&mut p, CaptureState::Interrupted, a_frames, a_diagnostics)
        .expect("finish");

    let (second, mut r2) =
        Simulated::deterministic(SampleRate(48_000), 2, Pace::RealTime).expect("start");
    let b = Session::begin(&mut p, &second.info()).expect("begin");
    let b_frames = run_for(&second, &mut r2, Duration::from_millis(150));
    let b_diagnostics = second.stop();
    b.finish(&mut p, CaptureState::Finalised, b_frames, b_diagnostics)
        .expect("finish");

    let all = session::all(p.conn()).expect("all");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].diagnostics.stream_errors, 1);
    assert_eq!(all[1].diagnostics.stream_errors, 0);
    assert!(
        session::unfinished(p.conn())
            .expect("unfinished")
            .is_empty()
    );
}

#[test]
fn what_the_source_says_about_itself_is_what_the_project_records() {
    // §38's provenance, crossing the crate boundary without losing a field.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut p = project(&dir, "provenance.vcw");

    let (source, reader) =
        Simulated::deterministic(SampleRate(176_400), 1, Pace::Fast).expect("start");
    let info = source.info();
    let session = Session::begin(&mut p, &info).expect("begin");
    source.stop();
    drop(reader);

    let stored = session::load(p.conn(), session.id())
        .expect("load")
        .expect("row")
        .info;
    assert_eq!(stored, info);
    assert!(!stored.os_verified, "a simulated capture verified nothing");
}
