/*
 *  capture_writes_audio.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The audio itself, from the ring to the file and back out unchanged.
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

//! The audio itself, from the ring to the file and back out unchanged.
//!
//! WP-04 proved the counters reach the project. This proves the audio does, and
//! it is a different claim: a capture whose diagnostics are perfect and whose
//! samples are in the wrong order is a worse failure than one that admits to a
//! dropout.
//!
//! The composition under test is the one the product actually uses.
//! `vcw-audio`'s ring is on one side, `vcw-project`'s writer on the other, and
//! the only thing joining them is [`vcw_types::PcmSource`] - which is why
//! neither crate depends on the other and why this test can only live here.
//!
//! Every assertion about the samples is byte-for-byte against
//! [`Simulated::expected_sample`], recomputed from the frame index stored in the
//! block rather than from a running total. A block written at the wrong offset,
//! on the wrong channel, or after a gap fails here rather than passing on its
//! own internal consistency.

use std::time::{Duration, Instant};

use rusqlite::Connection;
use vcw_audio::capture::Negotiated;
use vcw_audio::source::{Faults, Pace, Pattern, Simulated, Source};
use vcw_project::persistence::{self, Config};
use vcw_project::validate::{Options, validate};
use vcw_project::{Project, Writer, session};
use vcw_types::{CaptureInfo, CaptureState, SampleFormat, SampleRate, StorageFormat};

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

/// Whether this machine belongs to somebody else.
///
/// The soak harness's word for it, and CI sets it on the test job.
fn shared() -> bool {
    std::env::var("VCW_SHARED").as_deref() == Ok("1")
}

/// 32-bit is what `Simulated::deterministic` produces, and the widest thing §8
/// asks for. Four bytes a sample, so every generated word is stored whole.
const WIDTH: usize = 4;

/// Checks every stored sample against the value the source must have produced
/// for that frame and channel, and returns the frames checked per channel.
///
/// Streams block by block: the point is that this scales to a full side, and a
/// verifier that needed the capture in memory would be no use on the run it was
/// written for.
fn audit(conn: &Connection, capture_id: i64, channels: u16, width: usize) -> u64 {
    let mut stmt = conn
        .prepare(
            "SELECT b.channel, b.start_frame, b.frame_count, s.samples
             FROM capture_blocks b JOIN sampleblocks s ON s.blockid = b.blockid
             WHERE b.capture_id = ?1 ORDER BY b.sequence, b.channel",
        )
        .expect("prepare");
    let mut rows = stmt.query([capture_id]).expect("query");
    let mut next = vec![0u64; channels as usize];
    while let Some(row) = rows.next().expect("row") {
        let channel: i64 = row.get(0).expect("channel");
        let start_frame: u64 = row.get::<_, i64>(1).expect("start_frame") as u64;
        let frame_count: u64 = row.get::<_, i64>(2).expect("frame_count") as u64;
        let samples: Vec<u8> = row.get(3).expect("samples");

        assert!(
            channel >= 0 && (channel as u16) < channels,
            "block on channel {channel} of a {channels}-channel capture"
        );
        assert_eq!(
            start_frame, next[channel as usize],
            "channel {channel} has a gap or an overlap in its timeline"
        );
        assert_eq!(
            samples.len(),
            frame_count as usize * width,
            "channel {channel} block at {start_frame} is the wrong length"
        );
        for i in 0..frame_count {
            let want = Simulated::expected_sample(start_frame + i, channel as u16).to_le_bytes();
            let at = i as usize * width;
            assert_eq!(
                &samples[at..at + width],
                &want[..width],
                "channel {channel} frame {} is not what the source produced",
                start_frame + i
            );
        }
        next[channel as usize] = start_frame + frame_count;
    }
    let frames = next[0];
    for (channel, count) in next.iter().enumerate() {
        assert_eq!(
            *count, frames,
            "channel {channel} holds {count} frames where channel 0 holds {frames}"
        );
    }
    frames
}

#[test]
fn every_sample_the_source_produced_is_in_the_project_and_in_order() {
    let _alone = alone();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("audio.vcw");

    let (source, reader) =
        Simulated::deterministic(SampleRate(48_000), 2, Pace::RealTime).expect("start");
    let info = source.info();
    let handle = persistence::spawn(
        Project::create(&path).expect("create"),
        &info,
        Config::default(),
        reader,
    )
    .expect("spawn");
    let capture_id = handle.capture_id();

    std::thread::sleep(Duration::from_millis(1_200));
    let diagnostics = source.stop();
    handle.set_result(CaptureState::Finalised, diagnostics);
    let outcome = handle.stop().expect("stop");

    assert!(
        diagnostics.is_clean(),
        "the ring lost audio before the writer ever saw it: {diagnostics:?}"
    );
    assert!(outcome.frames > 0, "nothing was written at all");

    let p = Project::open(&path).expect("reopen");
    assert_eq!(audit(p.conn(), capture_id, 2, WIDTH), outcome.frames);
    let report = validate(
        &p,
        Options {
            verify_checksums: true,
        },
    )
    .expect("validate");
    assert!(report.is_clean(), "{:?}", report.findings);
    assert!(report.blocks > 0);
}

#[test]
fn the_frames_the_device_delivered_are_the_frames_that_were_written() {
    let _alone = alone();
    // The two counts come from opposite ends of the pipeline and nothing
    // reconciles them at runtime. If they can drift apart, every duration VCW
    // ever displays is a guess.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("counts.vcw");

    let (source, reader) =
        Simulated::deterministic(SampleRate(96_000), 2, Pace::RealTime).expect("start");
    let info = source.info();
    let handle = persistence::spawn(
        Project::create(&path).expect("create"),
        &info,
        Config::default(),
        reader,
    )
    .expect("spawn");
    let capture_id = handle.capture_id();

    std::thread::sleep(Duration::from_millis(900));
    // The count is only final once the feeder thread has joined, which `stop`
    // does. Reading it a line earlier races the device: the callback that lands
    // in between is written, and the writer then legitimately reports more
    // frames than the snapshot saw.
    let counters = std::sync::Arc::clone(source.counters());
    let diagnostics = source.stop();
    let delivered = counters.frames();
    handle.set_result(CaptureState::Finalised, diagnostics);
    let outcome = handle.stop().expect("stop");

    assert!(diagnostics.is_clean(), "{diagnostics:?}");
    assert_eq!(
        outcome.frames, delivered,
        "the device delivered {delivered} frames and {} were written",
        outcome.frames
    );

    let p = Project::open(&path).expect("reopen");
    let record = session::load(p.conn(), capture_id)
        .expect("load")
        .expect("row");
    assert_eq!(record.frames, delivered);
    assert_eq!(record.state, CaptureState::Finalised);
    assert!(!record.needs_recovery());
}

#[test]
fn a_device_that_vanishes_leaves_everything_it_did_deliver() {
    let _alone = alone();
    // R9. The half of a side that reached the disc before the cable came out is
    // the half worth keeping, and it has to be intact rather than merely present.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("unplugged.vcw");
    let rate = SampleRate(48_000);
    let cut = 24_000; // half a second in

    let (source, reader) = Simulated::start(
        Negotiated::simulated(rate, 2, SampleFormat::S32),
        &Pattern::Deterministic,
        Pace::RealTime,
        Faults::unplug_after(cut),
        vcw_audio::buffers::DEFAULT_MILLIS,
    )
    .expect("start");
    let info = source.info();
    let handle = persistence::spawn(
        Project::create(&path).expect("create"),
        &info,
        Config::default(),
        reader,
    )
    .expect("spawn");
    let capture_id = handle.capture_id();

    std::thread::sleep(Duration::from_millis(1_200));
    let diagnostics = source.stop();
    assert!(
        diagnostics.stream_errors > 0,
        "the unplug should have been reported"
    );
    handle.set_result(CaptureState::Interrupted, diagnostics);
    let outcome = handle.stop().expect("stop");

    let p = Project::open(&path).expect("reopen");
    let written = audit(p.conn(), capture_id, 2, WIDTH);
    assert_eq!(written, outcome.frames);
    // Everything up to the cut, and nothing invented after it. The writer is
    // allowed to hold back less than a block; it is not allowed to make one up.
    assert!(
        written <= cut && written + u64::from(rate.hz()) / 4 >= cut,
        "{written} frames written against a cut at {cut}; \
         outcome {outcome:?}, device {diagnostics:?}"
    );

    let record = session::load(p.conn(), capture_id)
        .expect("load")
        .expect("row");
    assert_eq!(record.state, CaptureState::Interrupted);
    assert_eq!(record.diagnostics, diagnostics);
    assert!(
        validate(
            &p,
            Options {
                verify_checksums: true
            }
        )
        .expect("validate")
        .is_clean(),
        "an interrupted capture still has to leave a valid project"
    );
}

#[test]
fn a_writer_that_never_finished_leaves_a_recoverable_project() {
    // The crash case, staged rather than simulated: blocks are committed, the
    // session is never closed, and the process goes away. §15 says what is left
    // must be openable, valid, and identifiable as needing recovery - which is
    // what WP-06 will pick up.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("crashed.vcw");
    let info = CaptureInfo::unverified(
        SampleRate(48_000),
        2,
        StorageFormat::Int32,
        vcw_types::CaptureMode::Shared,
    );

    let capture_id = {
        let mut writer = Writer::begin(
            Project::create(&path).expect("create"),
            &info,
            Config::default(),
        )
        .expect("begin");
        let id = writer.session().id();
        let block = writer.block_frames() as usize * info.frame_bytes();
        let mut frame = 0u64;
        for _ in 0..3 {
            let mut chunk = Vec::with_capacity(block);
            for _ in 0..writer.block_frames() {
                for channel in 0..2u16 {
                    chunk.extend_from_slice(
                        &Simulated::expected_sample(frame, channel).to_le_bytes(),
                    );
                }
                frame += 1;
            }
            writer.push(&chunk).expect("push");
        }
        // No flush, no finish. The last partial block is lost, which is exactly
        // the commit-granularity loss D3 chose; everything committed survives.
        drop(writer);
        id
    };

    let p = Project::open(&path).expect("reopen");
    let record = session::load(p.conn(), capture_id)
        .expect("load")
        .expect("row");
    assert!(record.needs_recovery(), "finished_at should still be null");
    assert_eq!(record.state, CaptureState::Recording);
    assert_eq!(record.frames, audit(p.conn(), capture_id, 2, WIDTH));
    assert_eq!(record.frames, 3 * 48_000 / 4);
    assert!(
        validate(
            &p,
            Options {
                verify_checksums: true
            }
        )
        .expect("validate")
        .is_clean(),
        "a project nobody closed is still a valid project"
    );
    assert_eq!(session::unfinished(p.conn()).expect("unfinished").len(), 1);
}

#[test]
fn the_summaries_describe_the_audio_that_is_really_in_the_block() {
    let _alone = alone();
    // The waveform view is built from these and nothing else. A summary that
    // does not match its block is a picture of a recording that does not exist.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("summaries.vcw");

    let (source, reader) =
        Simulated::deterministic(SampleRate(48_000), 2, Pace::RealTime).expect("start");
    let handle = persistence::spawn(
        Project::create(&path).expect("create"),
        &source.info(),
        Config::default(),
        reader,
    )
    .expect("spawn");
    std::thread::sleep(Duration::from_millis(700));
    let diagnostics = source.stop();
    handle.set_result(CaptureState::Finalised, diagnostics);
    let outcome = handle.stop().expect("stop");
    assert!(outcome.blocks >= 2);

    let p = Project::open(&path).expect("reopen");
    let mut stmt = p
        .conn()
        .prepare(
            "SELECT s.summin, s.summax, s.sumrms, s.summary256, s.samples, s.sampleformat
             FROM capture_blocks b JOIN sampleblocks s ON s.blockid = b.blockid",
        )
        .expect("prepare");
    let mut rows = stmt.query([]).expect("query");
    let mut seen = 0;
    while let Some(row) = rows.next().expect("row") {
        let stored_min: f64 = row.get(0).expect("summin");
        let stored_max: f64 = row.get(1).expect("summax");
        let stored_rms: f64 = row.get(2).expect("sumrms");
        let s256: Vec<u8> = row.get(3).expect("summary256");
        let samples: Vec<u8> = row.get(4).expect("samples");
        let code: i64 = row.get(5).expect("sampleformat");
        assert_eq!(code as u32, StorageFormat::Int32.code());

        let n = StorageFormat::Int32.samples_in(samples.len());
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        let mut squares = 0f64;
        for i in 0..n {
            let v = StorageFormat::Int32
                .decode_sample(&samples, i)
                .expect("decode");
            min = min.min(v);
            max = max.max(v);
            squares += f64::from(v) * f64::from(v);
        }
        assert!(
            (stored_min - f64::from(min)).abs() < 1e-6,
            "{stored_min} vs {min}"
        );
        assert!(
            (stored_max - f64::from(max)).abs() < 1e-6,
            "{stored_max} vs {max}"
        );
        let rms = (squares / n as f64).sqrt();
        assert!((stored_rms - rms).abs() < 1e-5, "{stored_rms} vs {rms}");
        // One triplet per 256 samples, and nothing for a group that is not
        // there: capacity padding is Audacity's convention, not ours.
        assert_eq!(s256.len(), n.div_ceil(256) * 12);
        seen += 1;
    }
    assert!(seen >= 4);
}

#[test]
fn the_writer_keeps_up_with_a_192k_device_in_real_time() {
    let _alone = alone();
    // The soak in miniature, short enough for CI. It cannot prove the tail over
    // 90 minutes, but it does prove that the steady state is a steady state and
    // that nothing in the per-block work has become quadratic.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("keeping-up.vcw");

    let (source, reader) =
        Simulated::deterministic(SampleRate(192_000), 2, Pace::RealTime).expect("start");
    let info = source.info();
    let config = Config::default();
    let handle = persistence::spawn(
        Project::create(&path).expect("create"),
        &info,
        config,
        reader,
    )
    .expect("spawn");
    let capture_id = handle.capture_id();

    let started = Instant::now();
    std::thread::sleep(Duration::from_secs(3));
    let diagnostics = source.stop();
    handle.set_result(CaptureState::Finalised, diagnostics);
    let outcome = handle.stop().expect("stop");
    let wall = started.elapsed().as_secs_f64();

    assert!(diagnostics.is_clean(), "audio was lost: {diagnostics:?}");
    let audio = outcome.duration_secs(192_000);
    // Nothing was lost - that gates everywhere, above. This is the other
    // claim, that the audio arrived about as fast as the clock, and the wall
    // clock it is measured against includes both stops draining the ring. A
    // Windows runner took 1.27 s over that drain and read 3.13 s of audio in
    // 4.27 s, which is the machine not keeping up rather than the writer.
    assert!(
        audio / wall > 0.9 || shared(),
        "{audio:.2} s of audio in {wall:.2} s of wall clock"
    );
    // The loss assertions above hold anywhere. This one is a latency claim, and
    // a shared runner cannot make one: the same code measured 359 ms against
    // this 250 ms budget on a hosted runner with nothing lost. `VCW_SHARED=1`
    // is the soak harness's word for "this machine is not ours", and it means
    // the same thing here.
    if shared() {
        eprintln!(
            "commit tail not gated (VCW_SHARED=1): max {:?} us",
            outcome.commit.max()
        );
    } else {
        assert!(
            outcome.commits_within_budget(&config),
            "a commit took {:?} us against a {} ms budget",
            outcome.commit.max(),
            config.commit_granularity_millis()
        );
    }
    // The point of expressing the ceiling in bytes: 1000 pages of 64 KiB would
    // be a 64 MiB log for three seconds of audio.
    assert!(
        outcome.peak_wal_bytes < 3 * config.wal_bytes,
        "the log reached {} bytes",
        outcome.peak_wal_bytes
    );

    let p = Project::open(&path).expect("reopen");
    assert_eq!(audit(p.conn(), capture_id, 2, WIDTH), outcome.frames);
}
