/*
 *  fingerprint_live.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Fingerprinting, driven by the capture tap and by committed audio (25).
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

//! Fingerprinting, driven by the capture tap and by committed audio (§25).
//!
//! `vcw-fingerprint`'s own tests prove the builder turns bytes into a fingerprint and
//! does not care how the bytes were sliced. This proves the two things it cannot: that
//! the live worker is fed the capture stream through §10's tee while the record turns,
//! takes its regions from the detector's boundaries on the bus, and produces **the
//! fingerprint the same audio produces offline, bit for bit** - which is S4's central
//! claim and the reason there is no staging file anywhere in this path.
//!
//! The stream here is a canned one rather than the simulated device, for one reason:
//! the simulated source is a hash, uniform and loud from the first frame to the last,
//! and a fingerprint of noise is a fingerprint of nothing in particular. A tone with a
//! beat in it is what the algorithm was built to describe.
//!
//! Timing is made irrelevant rather than tuned. The tap is sized to hold the whole
//! stream, so no scheduling accident can drop audio and no assertion here depends on
//! how often the worker woke up; and audio is pumped only *after* the boundary is
//! published, so the cursor cannot have moved before the region opened. What is left
//! is the worker's logic, which is what the tests are about.

use std::time::Duration;

use vcw_audio::buffers::Tee;
use vcw_core::events::{Bus, Event};
use vcw_core::fingerprinting::{self, Fingerprints};
use vcw_fingerprint::chromaprint::Builder;
use vcw_project::Project;
use vcw_project::persistence::{Config as Commits, Writer};
use vcw_types::{
    CaptureEq, CaptureInfo, CaptureMode, CaptureState, Edge, PcmSource, Provenance, SampleRate,
    Span, StorageFormat,
};

const RATE: SampleRate = SampleRate(48_000);
const CHANNELS: u16 = 2;
/// Int16 stereo.
const FRAME: usize = 4;
/// Long enough to clear the algorithm's 2.65 s warm-up and have plenty to say after
/// it: at 8.08 sub-fingerprints a second, about 43 of them.
const SECONDS: u64 = 8;

/// What a capture of this shape would report about itself.
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
        eq: CaptureEq::Unknown,
    }
}

/// A stream with a shape the algorithm can describe: two tones a fifth apart, beating
/// slowly, with the right channel lower and in a different key.
fn music(seconds: u64) -> Vec<u8> {
    let rate = f64::from(RATE.hz());
    let frames = seconds * u64::from(RATE.hz());
    let mut pcm = Vec::with_capacity(frames as usize * FRAME);
    for frame in 0..frames {
        let t = frame as f64 / rate;
        let beat = 0.5 + 0.5 * (std::f64::consts::TAU * 0.37 * t).sin();
        let left = ((std::f64::consts::TAU * 440.0 * t).sin() * 0.3
            + (std::f64::consts::TAU * 660.0 * t).sin() * 0.2 * beat)
            * 0.8;
        let right = (std::f64::consts::TAU * 220.0 * t).sin() * 0.25;
        for value in [left, right] {
            let sample = (value.clamp(-1.0, 1.0) * f64::from(i16::MAX)).round() as i16;
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
    }
    pcm
}

/// A stream the test hands over at its own pace: §10's tee will wrap anything.
struct Canned {
    pcm: Vec<u8>,
    at: usize,
}

impl PcmSource for Canned {
    fn read(&mut self, dst: &mut [u8]) -> usize {
        let take = (self.pcm.len() - self.at).min(dst.len());
        dst[..take].copy_from_slice(&self.pcm[self.at..self.at + take]);
        self.at += take;
        take
    }

    fn is_finished(&self) -> bool {
        self.at >= self.pcm.len()
    }
}

/// Pumps a whole stream through the tee, which is what the writer would have done.
fn pump(tee: &mut Tee<Canned>, bytes: usize) {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut moved = 0;
    while moved < bytes {
        let want = (bytes - moved).min(buffer.len());
        let read = tee.read(&mut buffer[..want]);
        assert!(read > 0, "the canned stream ran out at {moved} of {bytes}");
        moved += read;
    }
}

/// The offline fingerprint of a byte range, which is the answer the live pass has to
/// agree with.
fn offline(pcm: &[u8]) -> Vec<u32> {
    let mut builder = Builder::open(&info()).expect("open");
    builder.push(pcm).expect("push");
    builder.finish().expect("finish").raw
}

/// A live worker on a tap big enough that nothing can be dropped, and the bus it
/// takes its regions from.
fn worker(pcm: Vec<u8>) -> (Tee<Canned>, Bus, Fingerprints) {
    let mut tee = Tee::new(Canned { pcm, at: 0 });
    // Sized for the whole stream rather than for the worker's latency: a dropped byte
    // is a thrown-away region by design, which would make every assertion below about
    // the scheduler rather than about the code.
    let capacity = tee.inner().pcm.len() + FRAME;
    let tap = tee.tap(capacity);
    let bus = Bus::new();
    let fingerprints = Fingerprints::spawn(tap, &info(), &bus).expect("chromaprint takes 48k/2ch");
    (tee, bus, fingerprints)
}

/// Long enough for the worker to have woken and emptied the tap. One drain interval
/// plus a generous margin; nothing below depends on the number, only on the ordering
/// it buys.
const SETTLE: Duration = Duration::from_millis(300);

/// Publishes a boundary the way the live detector does, with the worker given time to
/// be level with the stream on both sides of it.
///
/// Both sleeps matter, and for opposite reasons. The one before: a region closes at
/// the worker's *cursor* rather than at the frame the event names, which is the right
/// behavior for audio arriving in real time - by the time a boundary is announced the
/// cursor is already about 1.2 s past it - but a test that pumps eight seconds in a
/// millisecond would otherwise say "end" while all eight are still in the tap. The one
/// after: the event has to be on the bus before the audio it opens is pumped, or the
/// cursor has moved and the region starts late.
fn boundary(bus: &Bus, frame: u64, edge: Edge) {
    std::thread::sleep(SETTLE);
    bus.publish(&Event::Detected {
        frame,
        seconds: frame as f64 / f64::from(RATE.hz()),
        edge,
        confidence: 0.9,
        provenance: Provenance::Silence,
    });
    std::thread::sleep(SETTLE);
}

/// §25's live half, and S4's claim: off the tap is the same as off the file.
#[test]
fn a_region_off_the_tap_is_what_the_same_audio_fingerprints_to_offline() {
    let pcm = music(SECONDS);
    let (mut tee, bus, fingerprints) = worker(pcm.clone());

    boundary(&bus, 0, Edge::Start);
    pump(&mut tee, pcm.len());
    drop(tee);
    let out = fingerprints.stop();

    assert_eq!(out.holed, 0, "the tap could not have dropped anything");
    assert_eq!(out.refused, 0, "eight seconds is not too short");
    assert_eq!(out.regions.len(), 1, "one boundary, one region");
    let region = &out.regions[0];
    assert_eq!(region.boundary, 0);
    assert_eq!(
        region.from_frame, 0,
        "no audio was pumped before the boundary was published"
    );
    assert_eq!(region.fingerprint.frames, pcm.len() as u64 / FRAME as u64);
    assert!(
        region.fingerprint.raw.len() > 40,
        "eight seconds should be about 43 sub-fingerprints, not {}",
        region.fingerprint.raw.len()
    );
    assert_eq!(
        region.fingerprint.raw,
        offline(&pcm),
        "the live fingerprint differs from the offline one of the same audio"
    );
    assert!(!region.fingerprint.encoded.is_empty());
}

/// A region ends where the detector says it ends, and nothing after it is in there.
#[test]
fn audio_after_the_end_boundary_is_not_in_the_region() {
    let pcm = music(SECONDS * 2);
    let half = pcm.len() / 2;
    let (mut tee, bus, fingerprints) = worker(pcm.clone());

    boundary(&bus, 0, Edge::Start);
    pump(&mut tee, half);
    boundary(&bus, half as u64 / FRAME as u64, Edge::End);
    pump(&mut tee, pcm.len() - half);
    drop(tee);
    let out = fingerprints.stop();

    assert_eq!(
        out.regions.len(),
        1,
        "an end boundary closes a region and does not open one"
    );
    let region = &out.regions[0];
    let frames = region.fingerprint.frames as usize;
    assert!(
        frames <= half / FRAME,
        "the region ran {frames} frames past the {} the boundary allowed",
        half / FRAME
    );
    // Which frames those are is the point: the region holds the stream from its start,
    // contiguously, and stopped where it was told to.
    assert_eq!(region.fingerprint.raw, offline(&pcm[..frames * FRAME]));
    assert_eq!(out.holed, 0);
}

/// §10's rule has a cost, and the cost is not allowed to be a wrong fingerprint.
#[test]
fn a_region_the_tap_lost_audio_from_is_thrown_away_rather_than_published() {
    let pcm = music(SECONDS);
    // Half a second of tap for eight seconds of audio pumped in one go: the tap is
    // lossy by construction and this is what that looks like. A fingerprint with a
    // hole in it is shifted from the hole onwards and would match nothing, while
    // looking exactly like one that works - so it must not be handed on.
    let mut tee = Tee::new(Canned {
        pcm: pcm.clone(),
        at: 0,
    });
    let tap = tee.tap(RATE.hz() as usize * FRAME / 2);
    let bus = Bus::new();
    let fingerprints = Fingerprints::spawn(tap, &info(), &bus).expect("spawn");

    boundary(&bus, 0, Edge::Start);
    pump(&mut tee, pcm.len());
    drop(tee);
    let out = fingerprints.stop();

    assert_eq!(out.holed, 1, "the lost audio was not noticed");
    assert!(
        out.regions.is_empty(),
        "a holed region was published anyway: {:?}",
        out.regions
    );
}

/// §25's other half: a span of committed audio, read back out of the project.
#[test]
fn a_committed_span_fingerprints_to_the_same_thing_as_the_bytes_that_made_it() {
    let pcm = music(SECONDS);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("fingerprint.vcw");
    let project = Project::create(&path).expect("create");
    let mut writer = Writer::begin(project, &info(), Commits::default()).expect("begin");
    writer.push(&pcm).expect("push");
    let (outcome, project, _) = writer
        .finish_with_project(CaptureState::Finalised)
        .expect("finish");
    project.close().expect("close");

    let project = Project::open_read_only(&path).expect("reopen");
    let whole = fingerprinting::of_span(&project, outcome.capture_id, Span::whole(outcome.frames))
        .expect("fingerprint the whole side");
    assert_eq!(whole.raw, offline(&pcm), "the read path changed the audio");
    assert_eq!(whole.frames, outcome.frames);

    // A span shorter than the algorithm's warm-up has nothing to say, and says so
    // rather than returning an empty fingerprint that would match everything.
    let short = fingerprinting::of_span(&project, outcome.capture_id, Span::new(0, 1_000));
    assert!(
        matches!(
            short,
            Err(fingerprinting::Error::Fingerprint(
                vcw_fingerprint::chromaprint::Error::TooShort { frames: 1_000 }
            ))
        ),
        "a 1,000-frame span was not refused: {short:?}"
    );
}
