/*
 *  transport_session.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A whole capture session driven through the engine, with no UI and no sound card.
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

//! A whole capture session driven through the engine, with no UI and no sound
//! card.
//!
//! WP-07's exit criterion has two halves. The first - invalid transitions
//! unrepresentable - is proved in `state.rs` by code that does not compile.
//! This is the second: a full capture session driven from outside the core,
//! through nothing but [`Command`] and [`Event`].
//!
//! §4.5 is what makes it worth doing at this level rather than only from the
//! CLI: "the core engine must remain independently testable and usable without
//! the graphical frontend". If these tests need anything the CLI has, the core
//! is not independent.

use std::time::{Duration, Instant};

use vcw_core::commands::{Command, Setup};
use vcw_core::events::{Bus, Event, Events};
use vcw_core::state::Phase;
use vcw_core::{Engine, engine};
use vcw_project::{Options, Project, recovery, session, validate};
use vcw_types::CaptureState;

/// Every test in this file drives a live `Engine`, and an engine's source is
/// a `Pace::RealTime` capture running at the speed of a record. The harness
/// would otherwise start all of them at once: a Windows runner then saw 8 of
/// the 30 meter snapshots a 600 ms capture owes, which is the machine failing
/// to provide real time rather than VCW failing to keep up. One at a time, and
/// the rate and loss assertions mean what they say again. Copied from
/// `vcw-cli`'s `session_from_cli.rs`, which learned it first; a mutex per
/// test binary is as far as this goes, and cargo already runs the binaries
/// one after another.
fn alone() -> std::sync::MutexGuard<'static, ()> {
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GATE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether this machine belongs to somebody else.
///
/// Same gate, and the same reasons, as `vcw-cli`'s `session_from_cli.rs`: how
/// much audio a wall-clock window produced is the machine's to provide, and
/// `Finalised` means `Interrupted`'s opposite only because `Interrupted` is
/// exactly `!diagnostics.is_clean()`. Serialising the engine tests fixed the
/// loss; it cannot fix a 700 ms sleep that a loaded host returns from after
/// 930 ms, which is how a macOS runner read 1.860 s of audio for 1.4 s of
/// recording.
fn shared() -> bool {
    std::env::var("VCW_SHARED").as_deref() == Ok("1")
}

/// The session ended of its own accord and kept what it had.
///
/// `Finalised` says that and says nothing was lost. On a shared machine only
/// the first half is VCW's to prove, so `Interrupted` passes there - it is
/// still an ended capture with its audio committed, and a capture nobody
/// closed would be neither.
#[track_caller]
fn ended(state: CaptureState, why: &str) {
    assert!(
        state == CaptureState::Finalised || (shared() && state == CaptureState::Interrupted),
        "{why} - the capture ended {state:?}"
    );
}

/// Waits for the transport to reach a phase, or gives up and says what it saw.
///
/// Polling the event stream rather than sleeping a guessed interval: the engine
/// publishes a phase change the moment it happens, so this is as fast as the
/// transport is and cannot pass by accident on a slow machine.
fn wait_for(events: &Events, phase: Phase, seen: &mut Vec<Event>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match events.try_next() {
            Some(Event::Phase { to, .. }) if to == phase => {
                seen.push(Event::Phase { from: phase, to });
                return;
            }
            Some(event) => {
                let stop = matches!(event, Event::Closed);
                seen.push(event);
                assert!(!stop, "the engine closed before reaching {phase}: {seen:?}");
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    panic!("never reached {phase}; saw {seen:?}");
}

/// Drives one side: arm, record, pause, resume, record, stop.
#[test]
fn a_whole_session_runs_from_commands_and_reports_through_events() {
    let _alone = alone();

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("side-a.vcw");

    let engine = Engine::start().expect("start");
    let events = engine.events();
    let mut seen = Vec::new();

    engine
        .send(Command::Arm(Box::new(
            Setup::simulated(&path).at(48_000).channels(2),
        )))
        .expect("arm");
    wait_for(&events, Phase::Armed, &mut seen);
    assert!(
        seen.iter().any(|e| matches!(e, Event::Armed { .. })),
        "arming must say what it opened: {seen:?}"
    );

    // Armed is a real phase, not a label: the device is running and the project
    // exists, and nothing has been committed.
    assert!(path.exists(), "arming did not create the project");
    std::thread::sleep(Duration::from_millis(300));
    engine.send(Command::Poll).expect("poll");
    let status = next_status(&events, &mut seen);
    assert_eq!(status, (Phase::Armed, 0), "armed wrote audio: {seen:?}");

    engine.send(Command::Record).expect("record");
    wait_for(&events, Phase::Recording, &mut seen);
    std::thread::sleep(Duration::from_millis(700));

    engine.send(Command::Pause).expect("pause");
    wait_for(&events, Phase::Paused, &mut seen);
    // Pausing flushes the part-filled block, so a few thousand frames land
    // *after* the phase event. That audio belongs to the recording - it was
    // captured before the pause - so the baseline is taken once the flush has
    // settled rather than the instant the phase changed.
    std::thread::sleep(Duration::from_millis(200));
    engine.send(Command::Poll).expect("poll");
    let (_, at_pause) = next_status(&events, &mut seen);
    assert!(at_pause > 0, "nothing was recorded before the pause");

    // The pause is the interesting part: audio keeps arriving from the device
    // and none of it is committed.
    std::thread::sleep(Duration::from_millis(500));
    engine.send(Command::Poll).expect("poll");
    let (phase, during_pause) = next_status(&events, &mut seen);
    assert_eq!(phase, Phase::Paused);
    assert_eq!(
        during_pause, at_pause,
        "the transport recorded while paused: {seen:?}"
    );

    engine.send(Command::Resume).expect("resume");
    wait_for(&events, Phase::Recording, &mut seen);
    std::thread::sleep(Duration::from_millis(700));

    engine.send(Command::Stop).expect("stop");
    wait_for(&events, Phase::Stopped, &mut seen);
    engine.shutdown().expect("shutdown");
    seen.extend(events.collect_until_closed());

    // §35's contract: everything that happened is in the stream.
    let finished = seen
        .iter()
        .find_map(|e| match e {
            Event::Finished {
                capture_id,
                frames,
                state,
                ..
            } => Some((*capture_id, *frames, *state)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no capture-finished event: {seen:?}"));
    let (capture_id, frames, state) = finished;
    ended(state, "the session was stopped, not interrupted");
    // Exactly once. A side that appears to finish twice is a side a consumer
    // would catalog twice, and the report exists in two places - the
    // `Stopped` phase holds it and the reset yields it - so this is worth
    // pinning rather than assuming.
    assert_eq!(
        seen.iter()
            .filter(|e| matches!(e, Event::Finished { .. }))
            .count(),
        1,
        "capture-finished more than once: {seen:?}"
    );
    assert!(frames > at_pause, "the resume recorded nothing");
    assert!(
        seen.iter().any(|e| matches!(e, Event::Position { .. })),
        "no recording-position events: {seen:?}"
    );
    assert!(
        seen.last().is_some_and(Event::is_last),
        "the stream must end with Closed: {:?}",
        seen.last()
    );

    // And the project agrees with the events.
    let project = Project::open(&path).expect("reopen");
    let record = session::load(project.conn(), capture_id)
        .expect("load")
        .expect("the capture row");
    assert_eq!(record.frames, frames);
    ended(record.state, "the stored capture should say it was stopped");
    assert!(record.finished_at.is_some());
    assert!(
        recovery::survey(project.conn()).expect("survey").is_empty(),
        "a cleanly stopped session must not look unfinished"
    );
    let report = validate(
        &project,
        Options {
            verify_checksums: true,
        },
    )
    .expect("validate");
    assert!(report.is_clean(), "{:?}", report.findings);

    // The pause is not in the recording. Two runs of roughly 700 ms with half a
    // second of pause between them is about 1.4 s of audio, and would be about
    // 1.9 s if the pause had leaked in.
    let seconds = record.duration_secs();
    assert!(
        (0.9..1.75).contains(&seconds) || shared(),
        "expected about 1.4 s of audio, got {seconds:.3} s - \
         a pause that is in the recording would read about 1.9 s"
    );
}

/// The runtime half of §11: a command with no meaning here changes nothing.
#[test]
fn a_command_that_does_not_apply_is_reported_and_changes_nothing() {
    let _alone = alone();

    let engine = Engine::start().expect("start");
    let events = engine.events();

    // Every transport verb, from Idle, where only `arm` means anything.
    for command in [
        Command::Record,
        Command::Pause,
        Command::Resume,
        Command::Stop,
        Command::Reset,
        Command::Disarm,
    ] {
        let name = command.as_str();
        engine.send(command).expect("send");
        let event = next_matching(&events, |e| matches!(e, Event::Rejected { .. }));
        let Event::Rejected { command, phase } = event else {
            unreachable!()
        };
        assert_eq!(command, name);
        assert_eq!(phase, Phase::Idle);
    }

    engine.send(Command::Poll).expect("poll");
    let status = next_matching(&events, |e| matches!(e, Event::Status { .. }));
    assert_eq!(
        status,
        Event::Status {
            phase: Phase::Idle,
            frames: 0
        },
        "six illegal commands moved the transport"
    );
    engine.shutdown().expect("shutdown");
}

/// Arming something that cannot be opened is a refusal, not a crash.
#[test]
fn a_device_that_cannot_be_opened_leaves_the_transport_idle() {
    let _alone = alone();

    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::start().expect("start");
    let events = engine.events();

    engine
        .send(Command::Arm(Box::new(Setup::device(
            "no-such-device-anywhere",
            dir.path().join("nope.vcw"),
        ))))
        .expect("arm");

    let event = next_matching(&events, |e| matches!(e, Event::Refused { .. }));
    let Event::Refused { command, phase, .. } = event else {
        unreachable!()
    };
    assert_eq!(command, "arm");
    assert_eq!(
        phase,
        Phase::Idle,
        "a failed arm must not move the transport"
    );

    // Still usable afterwards, which is the point of isolating the failure.
    engine
        .send(Command::Arm(Box::new(Setup::simulated(
            dir.path().join("yes.vcw"),
        ))))
        .expect("arm");
    let mut seen = Vec::new();
    wait_for(&events, Phase::Armed, &mut seen);
    engine.shutdown().expect("shutdown");
}

/// §11: stopping finalizes the capture and does *not* close the project.
#[test]
fn a_second_side_records_into_the_same_project() {
    let _alone = alone();

    // §50's workflow is `Record -> Flip -> Record`, which is the requirement
    // §11's arrow diagram does not draw and this test does.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("album.vcw");
    let engine = Engine::start().expect("start");
    let events = engine.events();
    let mut seen = Vec::new();

    for side in 0..2 {
        engine
            .send(Command::Arm(Box::new(Setup::simulated(&path))))
            .expect("arm");
        wait_for(&events, Phase::Armed, &mut seen);
        engine.send(Command::Record).expect("record");
        wait_for(&events, Phase::Recording, &mut seen);
        std::thread::sleep(Duration::from_millis(400));
        engine.send(Command::Stop).expect("stop");
        wait_for(&events, Phase::Stopped, &mut seen);
        engine.send(Command::Reset).expect("reset");
        wait_for(&events, Phase::Idle, &mut seen);
        assert!(path.exists(), "side {side} lost the project");
    }
    engine.shutdown().expect("shutdown");

    let project = Project::open(&path).expect("reopen");
    let captures = session::all(project.conn()).expect("all");
    assert_eq!(captures.len(), 2, "both sides should be in the project");
    for capture in &captures {
        ended(capture.state, "each side should say it was stopped");
        assert!(capture.frames > 0);
    }
    // Two captures, one project, and the blocks of each belong to their own
    // capture rather than being spliced into one timeline.
    let report = validate(
        &project,
        Options {
            verify_checksums: true,
        },
    )
    .expect("validate");
    assert!(report.is_clean(), "{:?}", report.findings);
}

/// An arm that is thought better of leaves nothing behind.
#[test]
fn arming_and_changing_your_mind_does_not_litter_the_project() {
    let _alone = alone();

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("second-thoughts.vcw");
    let engine = Engine::start().expect("start");
    let events = engine.events();
    let mut seen = Vec::new();

    engine
        .send(Command::Arm(Box::new(Setup::simulated(&path))))
        .expect("arm");
    wait_for(&events, Phase::Armed, &mut seen);
    engine.send(Command::Disarm).expect("disarm");
    wait_for(&events, Phase::Idle, &mut seen);
    engine.shutdown().expect("shutdown");

    let project = Project::open(&path).expect("reopen");
    assert!(
        session::all(project.conn()).expect("all").is_empty(),
        "an abandoned arm left a capture row behind"
    );
    assert!(
        recovery::survey(project.conn()).expect("survey").is_empty(),
        "an abandoned arm left something that looks unfinished"
    );
}

/// Dropping the engine mid-capture must finish the side, not lose it.
#[test]
fn a_shutdown_while_recording_finalizes_rather_than_abandons() {
    let _alone = alone();

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("interrupted.vcw");
    let events;
    {
        let engine = Engine::start().expect("start");
        events = engine.events();
        let mut seen = Vec::new();
        engine
            .send(Command::Arm(Box::new(Setup::simulated(&path))))
            .expect("arm");
        wait_for(&events, Phase::Armed, &mut seen);
        engine.send(Command::Record).expect("record");
        wait_for(&events, Phase::Recording, &mut seen);
        std::thread::sleep(Duration::from_millis(400));
        // No stop, no shutdown: just dropped, the way a process exiting a scope
        // would leave it.
    }

    let tail = events.collect_until_closed();
    assert!(
        tail.iter().any(|e| matches!(e, Event::Finished { .. })),
        "a dropped engine must still finish the capture: {tail:?}"
    );

    let project = Project::open(&path).expect("reopen");
    let captures = session::all(project.conn()).expect("all");
    assert_eq!(captures.len(), 1);
    ended(
        captures[0].state,
        "a dropped engine should still stop the capture",
    );
    assert!(captures[0].frames > 0);
    assert!(
        recovery::survey(project.conn()).expect("survey").is_empty(),
        "a dropped engine left an unfinished capture"
    );
}

/// Every subscriber sees the same session.
#[test]
fn two_subscribers_see_the_same_thing() {
    let _alone = alone();

    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::start().expect("start");
    let first = engine.events();
    let second = engine.events();

    engine
        .send(Command::Arm(Box::new(Setup::simulated(
            dir.path().join("shared.vcw"),
        ))))
        .expect("arm");
    let mut seen = Vec::new();
    wait_for(&first, Phase::Armed, &mut seen);
    engine.shutdown().expect("shutdown");

    let a: Vec<String> = first.collect_until_closed().iter().map(names).collect();
    let b: Vec<String> = second.collect_until_closed().iter().map(names).collect();
    // The first subscriber consumed some events before the shutdown, so `b` is
    // the longer list; what matters is that `b` ends the same way and that
    // neither invented anything.
    assert!(b.ends_with(&a), "subscribers disagreed:\n{a:?}\n{b:?}");
    assert!(b.contains(&"armed".to_owned()), "{b:?}");
    assert!(b.last().is_some_and(|n| n == "closed"), "{b:?}");
}

/// The engine is a plain library type with no framework anywhere near it.
#[test]
fn the_core_is_usable_with_no_frontend_at_all() {
    // §4.5 in one assertion. If this ever needs a runtime, a window or a
    // feature flag, §2 has been eroded and this is where it shows.
    let dir = tempfile::tempdir().expect("tempdir");
    let recorder =
        engine::Recorder::open(&Setup::simulated(dir.path().join("bare.vcw")), &Bus::new())
            .expect("open");
    assert!(recorder.capture_id() > 0);
    assert_eq!(recorder.negotiated().channels, 2);
    recorder.abandon().expect("abandon");
}

fn names(event: &Event) -> String {
    event.name().to_owned()
}

/// The next event matching a predicate, or a panic naming what turned up.
fn next_matching(events: &Events, want: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        match events.try_next() {
            Some(event) if want(&event) => return event,
            Some(event) => seen.push(event),
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    panic!("no matching event; saw {seen:?}");
}

/// The next status event, unpacked.
fn next_status(events: &Events, seen: &mut Vec<Event>) -> (Phase, u64) {
    let event = next_matching(events, |e| matches!(e, Event::Status { .. }));
    let Event::Status { phase, frames } = event else {
        unreachable!()
    };
    seen.push(Event::Status { phase, frames });
    (phase, frames)
}
