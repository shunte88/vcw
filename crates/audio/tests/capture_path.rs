/*
 *  capture_path.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The capture path end to end, driven by a simulated source.
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

//! The capture path end to end, driven by a simulated source.
//!
//! `src/capture.rs` tests the pieces and `tests/rt_safety.rs` tests the
//! callback's real-time behaviour. This file tests the thing they compose into:
//! bytes leaving a producer, crossing the ring, and arriving at a consumer with
//! nothing changed and nothing missing - and what the counters and the verdict
//! say when something does go wrong.
//!
//! Every test here runs with no sound card, which is the point. R9 - a device
//! removed mid-capture - cannot be provoked by unplugging a cable in CI, so
//! [`Faults`] provokes it instead.

use std::time::{Duration, Instant};

use vcw_audio::buffers::RingReader;
use vcw_audio::capture::{self, BitPerfect, Negotiated};
use vcw_audio::source::{Faults, Pace, Pattern, Simulated, Source};
use vcw_audio::verify::Verification;
use vcw_types::{CaptureMode, SampleFormat, SampleRate};

/// Reads until the predicate holds or the deadline passes, accumulating
/// everything that arrives. Returns what was read.
///
/// Wall-clock bounded rather than frame bounded: a test for a device that has
/// stopped delivering must terminate when the device stops delivering.
fn drain_until(
    reader: &mut RingReader,
    limit: Duration,
    mut done: impl FnMut(&[u8]) -> bool,
) -> Vec<u8> {
    let mut got = Vec::new();
    let mut scratch = vec![0u8; 64 * 1024];
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        let n = reader.read(&mut scratch);
        if n == 0 {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        got.extend_from_slice(&scratch[..n]);
        if done(&got) {
            break;
        }
    }
    got
}

/// The sample a deterministic source should have produced, as stored bytes.
fn expected_bytes(frames: u64, channels: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(frames as usize * channels as usize * 4);
    for frame in 0..frames {
        for channel in 0..channels {
            out.extend_from_slice(&Simulated::expected_sample(frame, channel).to_le_bytes());
        }
    }
    out
}

#[test]
fn every_byte_arrives_unaltered_and_in_order() {
    // The whole of §9 in one assertion: what the producer made is what the
    // consumer got, byte for byte, with no gap and no reordering.
    let (source, mut reader) =
        Simulated::deterministic(SampleRate(48_000), 2, Pace::Fast).expect("start");
    let want = expected_bytes(20_000, 2);
    let got = drain_until(&mut reader, Duration::from_secs(5), |g| {
        g.len() >= want.len()
    });
    source.stop();

    assert!(got.len() >= want.len(), "only {} bytes arrived", got.len());
    assert_eq!(
        &got[..want.len()],
        &want[..],
        "the stream was altered in transit"
    );
}

#[test]
fn a_simulated_capture_can_never_be_called_bit_perfect() {
    // Defence in depth against the worst possible bug in this crate: a CI run
    // with no hardware reporting a confirmed bit-perfect capture.
    let (source, mut reader) =
        Simulated::deterministic(SampleRate(96_000), 2, Pace::Fast).expect("start");
    drain_until(&mut reader, Duration::from_millis(200), |g| {
        g.len() > 100_000
    });
    let verdict = source.verdict();
    let diagnostics = source.stop();

    assert!(diagnostics.is_clean(), "the run itself was clean");
    assert!(
        !verdict.is_confirmed(),
        "a clean simulated run still claimed bit-perfection: {}",
        verdict.summary()
    );
    let BitPerfect::Refuted { reasons } = verdict else {
        panic!("expected a refusal, got {verdict:?}");
    };
    assert!(reasons.iter().any(|r| r.contains("shared")), "{reasons:?}");

    // And the refusal does not rest on that one ground alone. Grant the
    // simulated configuration the mode it could never really have: the claim
    // still does not go through, because nothing knows what is in front of a
    // device that does not exist.
    let mut pretending = Negotiated::simulated(SampleRate(96_000), 2, SampleFormat::S32);
    pretending.mode = CaptureMode::Exclusive;
    pretending.mode_requested = CaptureMode::Exclusive;
    let second = capture::verdict(
        &pretending,
        diagnostics,
        &Verification::Unavailable {
            why: "no hardware".to_owned(),
        },
    );
    assert!(!second.is_confirmed(), "{}", second.summary());
}

#[test]
fn r9_a_device_unplugged_mid_capture_goes_quiet_and_is_counted() {
    // "Device removal mid-capture corrupts project". It must not: the capture
    // stops receiving, the error is counted, and everything already delivered
    // stays intact and readable.
    let negotiated = Negotiated::simulated(SampleRate(48_000), 2, SampleFormat::S32);
    let frame_bytes = negotiated.frame_bytes();
    let unplug_at = 4_800;
    let (source, mut reader) = Simulated::start(
        negotiated,
        &Pattern::Deterministic,
        Pace::RealTime,
        Faults::unplug_after(unplug_at),
        500,
    )
    .expect("start");

    let got = drain_until(&mut reader, Duration::from_millis(600), |_| false);
    let diagnostics = source.stop();

    assert_eq!(diagnostics.stream_errors, 1, "the removal was not reported");
    assert!(diagnostics.overruns == 0 && diagnostics.dropped_frames == 0);

    let frames = got.len() / frame_bytes;
    assert_eq!(got.len() % frame_bytes, 0, "a partial frame was delivered");
    assert!(
        frames >= unplug_at as usize,
        "delivery stopped early at {frames} frames"
    );
    // What arrived before the cable came out is still exactly right. This is
    // the part that decides whether the project is salvageable.
    assert_eq!(
        &got[..unplug_at as usize * frame_bytes],
        &expected_bytes(unplug_at, 2)[..]
    );
}

#[test]
fn an_unplugged_device_refutes_bit_perfection_rather_than_going_unnoticed() {
    let (source, mut reader) = Simulated::start(
        Negotiated::simulated(SampleRate(48_000), 2, SampleFormat::S32),
        &Pattern::Silence,
        Pace::RealTime,
        Faults::unplug_after(480),
        500,
    )
    .expect("start");
    drain_until(&mut reader, Duration::from_millis(300), |_| false);
    let verdict = source.verdict();
    source.stop();

    let BitPerfect::Refuted { reasons } = verdict else {
        panic!("a capture that lost its device was not refuted");
    };
    assert!(
        reasons.iter().any(|r| r.contains("stream errors")),
        "the data loss was not among the reasons: {reasons:?}"
    );
}

#[test]
fn a_reader_that_falls_behind_overruns_instead_of_blocking_the_producer() {
    // §10: capture never waits for the consumer. A stalled writer costs data,
    // and the cost is counted rather than absorbed silently.
    let (source, reader) = Simulated::start(
        Negotiated::simulated(SampleRate(192_000), 2, SampleFormat::S32),
        &Pattern::Silence,
        Pace::Fast,
        Faults::none(),
        500,
    )
    .expect("start");

    // Never read. The ring fills within its own capacity and stays full.
    std::thread::sleep(Duration::from_millis(200));
    let diagnostics = source.stop();
    drop(reader);

    assert!(diagnostics.overruns > 0, "a full ring reported no overrun");
    assert_eq!(
        diagnostics.dropped_frames % 1_920,
        0,
        "an overrun discarded part of a callback rather than all of it"
    );
}

#[test]
fn a_metered_source_waits_for_room_instead_of_dropping() {
    // WP-17. `vcw soak --fast` ran at Pace::Fast and could never pass: the
    // source outruns the writer, the ring overruns, and because an overrun
    // discards a whole callback the written frame index stops agreeing with the
    // source's - so the byte verifier compared frame n against the sample
    // belonging to a later frame and reported a mismatch every time.
    //
    // **The two arms are measured together on purpose.** A metered run that
    // reported no loss proves nothing on its own: it is also exactly what a
    // machine fast enough to keep up would look like at Pace::Fast. The claim
    // is that the two paces differ, so the dropping arm has to drop for this
    // test to mean anything, and it is asserted rather than assumed.
    let counters = |pace| {
        let (source, reader) = Simulated::start(
            Negotiated::simulated(SampleRate(192_000), 2, SampleFormat::S32),
            &Pattern::Deterministic,
            pace,
            Faults::none(),
            500,
        )
        .expect("start");
        // Nobody reads, in either arm. The ring fills and stays full, which is
        // the condition the two paces answer differently.
        std::thread::sleep(Duration::from_millis(200));
        // Read before the stop: `stop` consumes the source, and the frame count
        // is the evidence that back-pressure is not a deadlock.
        let delivered = source.frames();
        let diagnostics = source.stop();
        drop(reader);
        (diagnostics, delivered)
    };

    let (flooded, _) = counters(Pace::Fast);
    let (metered, delivered) = counters(Pace::Metered);

    assert!(
        flooded.overruns > 0 && flooded.dropped_frames > 0,
        "the dropping arm did not drop, so this test proves nothing: {flooded:?}"
    );
    assert_eq!(
        (metered.overruns, metered.dropped_frames),
        (0, 0),
        "a metered source overran; it is supposed to wait for room"
    );
    assert!(
        delivered > 0,
        "a metered source delivered nothing at all, which is a deadlock and not \
         back-pressure"
    );
}

#[test]
fn a_starved_device_is_an_underrun_and_not_the_end_of_the_capture() {
    let (source, mut reader) = Simulated::start(
        Negotiated::simulated(SampleRate(48_000), 2, SampleFormat::S32),
        &Pattern::Deterministic,
        Pace::Fast,
        Faults {
            starve_after: Some(480),
            ..Faults::none()
        },
        500,
    )
    .expect("start");
    let got = drain_until(&mut reader, Duration::from_millis(200), |g| {
        g.len() > 200_000
    });
    let frames_before = source.frames();
    let diagnostics = source.stop();

    assert_eq!(diagnostics.underruns, 1);
    assert!(!diagnostics.is_clean());
    assert!(
        frames_before > 480 && got.len() > 480 * 8,
        "the capture ended at the starvation instead of continuing"
    );
}

#[test]
fn the_provenance_a_source_reports_is_the_one_it_is_running() {
    let (source, reader) =
        Simulated::deterministic(SampleRate(176_400), 1, Pace::Fast).expect("start");
    let info = source.info();
    source.stop();
    drop(reader);

    assert_eq!(info.rate, SampleRate(176_400));
    assert_eq!(info.channels, 1);
    assert_eq!(info.frame_bytes(), 4);
    assert!(!info.os_verified, "nothing verified a simulated capture");
}
