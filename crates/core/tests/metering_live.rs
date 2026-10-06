/*
 *  metering_live.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The meters, fed by a live engine rather than by a test signal (§17, §18).
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

//! The meters, fed by a live engine rather than by a test signal (§17, §18).
//!
//! `tests/known_levels.rs` in `vcw-signal` proves the meter measures correctly.
//! This proves the other half: that what reaches it is the capture stream, that
//! it reaches it while the transport is only *armed*, and that it stops before
//! the capture is declared finished.
//!
//! The level is not arbitrary. The deterministic source is a hash, so its
//! output is uniform over the whole code range, and uniform noise on [-1, 1)
//! has an RMS of 1/sqrt(3) - **-4.771 dBFS**, a figure that comes from the
//! distribution and not from a previous run of this test. If the fan-out ever
//! drops, duplicates or reorders a byte, the RMS moves off it.

use std::time::{Duration, Instant};

use vcw_core::Engine;
use vcw_core::commands::{Command, Setup};
use vcw_core::events::{Event, Events};
use vcw_core::state::Phase;

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
/// Same gate as `vcw-cli`'s `session_from_cli.rs`, for the same reason: how
/// many snapshots arrive in a wall-clock window, and how much audio arrives
/// with them, are claims about the machine's scheduler rather than about VCW.
/// What a snapshot *contains* - the level, the peak, the ordering - is VCW's
/// and still gates everywhere, for however many of them arrived.
fn shared() -> bool {
    std::env::var("VCW_SHARED").as_deref() == Ok("1")
}

/// The RMS of a uniform distribution over full scale: 20*log10(1/sqrt(3)).
const UNIFORM_RMS_DB: f32 = -4.771_213;

/// Collects events until `done` says so, or panics with what it saw.
fn collect(events: &Events, done: impl Fn(&Event) -> bool) -> Vec<Event> {
    // Thirty seconds, which is not a claim about anything: this is the guard
    // that stops a broken engine hanging a CI job, and a guard that fires on a
    // slow machine is a false alarm. A Windows runner waited out ten seconds
    // for a capture-finished that the same tree had delivered twice that hour.
    let deadline = Instant::now() + Duration::from_secs(30);
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

/// Every meter snapshot in a run, in order.
fn meters(seen: &[Event]) -> Vec<&vcw_signal::meter::Snapshot> {
    seen.iter()
        .filter_map(|e| match e {
            Event::Meter { levels } => Some(levels),
            _ => None,
        })
        .collect()
}

/// §50 asks for the level to be set before the needle goes down.
#[test]
fn the_meters_are_live_while_the_transport_is_only_armed() {
    let _alone = alone();

    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::start().expect("start");
    let events = engine.events();

    engine
        .send(Command::Arm(Box::new(
            Setup::simulated(dir.path().join("level.vcw"))
                .at(48_000)
                .channels(2),
        )))
        .expect("arm");

    // Nothing is being recorded here and nothing will be. Everything the meter
    // reports in this window is audio the writer drained while paused.
    let armed = collect(&events, |e| matches!(e, Event::Meter { .. }));
    let levels = meters(&armed);
    assert!(!levels.is_empty(), "no meter update before RECORD");
    assert_eq!(levels[0].channels.len(), 2, "one reading per channel");

    let mut more = collect(&events, |e| matches!(e, Event::Meter { .. }));
    more.extend(collect(&events, |e| matches!(e, Event::Meter { .. })));
    assert!(
        meters(&more).len() >= 2,
        "the meters must keep running while armed, not fire once"
    );

    // Still armed: metering did not move the transport.
    engine.send(Command::Poll).expect("poll");
    let status = collect(&events, |e| matches!(e, Event::Status { .. }));
    let Some(Event::Status { phase, frames }) = status.last() else {
        unreachable!()
    };
    assert_eq!(*phase, Phase::Armed);
    assert_eq!(*frames, 0, "armed commits nothing");

    engine.send(Command::Disarm).expect("disarm");
    engine.shutdown().expect("shutdown");
}

/// The level that arrives is the level the source is producing.
#[test]
fn the_engine_meters_the_stream_it_is_recording() {
    let _alone = alone();

    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::start().expect("start");
    let events = engine.events();

    engine
        .send(Command::Arm(Box::new(
            Setup::simulated(dir.path().join("noise.vcw"))
                .at(48_000)
                .channels(2),
        )))
        .expect("arm");
    engine.send(Command::Record).expect("record");

    // Past the meter's own 300 ms RMS window, so the figure is a full window
    // of audio rather than a window that is still filling.
    std::thread::sleep(Duration::from_millis(600));
    engine.send(Command::Stop).expect("stop");
    let seen = collect(&events, |e| matches!(e, Event::Finished { .. }));

    let levels = meters(&seen);
    assert!(
        levels.len() > 10 || shared(),
        "600 ms at 50 Hz should be about thirty snapshots, got {}",
        levels.len()
    );

    let Some(last) = levels.last() else {
        assert!(shared(), "no meter update at all during a 600 ms capture");
        eprintln!("no snapshots to check (VCW_SHARED=1)");
        engine.shutdown().expect("shutdown");
        return;
    };
    for (index, channel) in last.channels.iter().enumerate() {
        assert!(
            (channel.rms_db() - UNIFORM_RMS_DB).abs() < 0.5,
            "channel {index} reads {:.3} dBFS, not the {UNIFORM_RMS_DB:.3} a \
             uniform source must produce - the fan-out is not carrying the \
             stream faithfully",
            channel.rms_db()
        );
        // A hash covers the whole code range, so full scale is reached often.
        assert!(
            channel.peak_db() > -0.1,
            "channel {index} peaks at {:.3} dBFS",
            channel.peak_db()
        );
    }

    // The frame count the meter has seen tracks the recording, within the
    // pause and the flush at each end. How much audio a wall-clock window
    // produced is the machine's to provide, so this is a floor the shared
    // runner does not have to meet; that the meter saw *the* stream rather
    // than some other one is the RMS assertion above, and that gates.
    assert!(
        last.frames > 20_000 || shared(),
        "the meter saw only {} frames of a 600 ms capture",
        last.frames
    );

    engine.shutdown().expect("shutdown");
}

/// A needle that twitches after the capture is over is a bug in a UI's lap.
#[test]
fn no_meter_update_arrives_after_the_capture_is_finished() {
    let _alone = alone();

    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::start().expect("start");
    let events = engine.events();

    engine
        .send(Command::Arm(Box::new(
            Setup::simulated(dir.path().join("order.vcw")).at(48_000),
        )))
        .expect("arm");
    engine.send(Command::Record).expect("record");
    std::thread::sleep(Duration::from_millis(200));
    engine.send(Command::Stop).expect("stop");
    engine.send(Command::Reset).expect("reset");
    engine.shutdown().expect("shutdown");

    let seen = collect(&events, Event::is_last);
    let finished = seen
        .iter()
        .position(|e| matches!(e, Event::Finished { .. }))
        .expect("the capture must finish");
    let Some(last_meter) = seen.iter().rposition(|e| matches!(e, Event::Meter { .. })) else {
        // A 200 ms capture owes ten snapshots and a Windows runner produced
        // none. There is no ordering to check without one, and the thing this
        // test is about - that nothing follows Finished - is not in question
        // when nothing was published at all.
        assert!(shared(), "the meters must have run");
        eprintln!("no meter updates to order (VCW_SHARED=1)");
        return;
    };
    assert!(
        last_meter < finished,
        "a meter update followed capture-finished: {:?}",
        &seen[finished..]
    );
}

/// The meter is a passenger, and a passenger cannot steer.
#[test]
fn the_capture_is_unaffected_by_the_fan_out() {
    let _alone = alone();

    // The fan-out reads nothing the writer does not, and the writer reads
    // everything it did before. A capture with the meters attached must still
    // commit a plausible number of frames and report no dropped audio.
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = Engine::start().expect("start");
    let events = engine.events();

    engine
        .send(Command::Arm(Box::new(
            Setup::simulated(dir.path().join("intact.vcw"))
                .at(96_000)
                .channels(2),
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
    assert_eq!(diagnostics.dropped_frames, 0, "the fan-out cost audio");
    assert_eq!(diagnostics.overruns, 0, "the fan-out slowed the writer");
    assert!(
        *frames > 48_000,
        "800 ms at 96 kHz committed only {frames} frames"
    );

    engine.shutdown().expect("shutdown");
}
