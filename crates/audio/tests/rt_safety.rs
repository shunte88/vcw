/*
 *  rt_safety.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Proving the §10 real-time contract instead of asserting it in a comment.
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

//! Proving the §10 real-time contract instead of asserting it in a comment.
//!
//! §10 says the callback shall not allocate, lock, or do I/O. Every audio
//! codebase says that; most of them are wrong somewhere, because nothing checks.
//! This file checks.
//!
//! Both callbacks are covered: [`Sink::on_data`], which takes bytes off the
//! device, and [`Source::on_data`], which puts them back. Playback's has more to
//! go wrong in it, because a seek is serviced *on the audio thread* - the
//! callback is the only thing that can discard audio it has already been handed.
//!
//! **Allocation is measured.** A counting global allocator is installed for this
//! test binary, armed only around the callback body, and the assertion is that
//! the count does not move. `the_harness_itself_can_see_an_allocation` is the
//! control: without it, a broken counter would silently "prove" every other test
//! in the file.
//!
//! **Blocking is measured.** The consumer thread is parked and never drains, so
//! the ring fills and stays full. A callback that waits on a stalled reader
//! would hang here; a wait-free one overruns, counts it, and returns. That is
//! the operational property §10 is really asking for.
//!
//! What is *not* proven here is lock-freedom as a formal property. There is no
//! lock in the path - `rtrb` is a wait-free SPSC ring and the counters are
//! relaxed atomics - but that is an argument from construction, and this file
//! only claims what it measures.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use vcw_audio::capture::{Counters, Sink};
use vcw_audio::playback::{Counters as PlaybackCounters, Cursor, Source};
use vcw_audio::{buffers, chunks};

/// An allocator that counts, but only while this thread has armed it.
///
/// Both the switch and the tally are thread-local and const-initialized: arming
/// cannot itself allocate, which would make the measurement measure the
/// measurement, and one test's armed window cannot pollute another's. Cargo runs
/// these in parallel, and a shared counter made the starvation test fail about
/// one run in five with two allocations it never made.
struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc_zeroed(layout) }
    }
}

fn note() {
    // `try_with` rather than `with`: during thread teardown the local is gone,
    // and panicking inside the allocator would be a poor way to find out.
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        }
    });
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Runs `body` with the allocator counting, and reports how many it saw.
fn allocations_during(body: impl FnOnce()) -> u64 {
    // Touch the local first: its first access on a thread is not something we
    // want inside the measured window.
    ARMED.with(|a| a.set(false));
    let before = ALLOCATIONS.with(Cell::get);
    ARMED.with(|a| a.set(true));
    body();
    ARMED.with(|a| a.set(false));
    ALLOCATIONS.with(Cell::get) - before
}

/// 2 ch of 32-bit: 8 bytes a frame.
const FRAME: usize = 8;
const RATE: u32 = 192_000;

fn sink(millis: u32) -> (Sink, buffers::RingReader, Arc<Counters>) {
    let counters = Arc::new(Counters::default());
    let (writer, reader) = buffers::ring(FRAME, RATE, millis);
    (
        Sink::new(writer, Arc::clone(&counters), FRAME),
        reader,
        counters,
    )
}

#[test]
fn the_harness_itself_can_see_an_allocation() {
    // The control. If this ever fails, every other assertion in this file is
    // vacuous and the real-time contract is unproven rather than proven.
    let seen = allocations_during(|| {
        let v: Vec<u8> = Vec::with_capacity(4096);
        std::hint::black_box(&v);
    });
    assert!(seen > 0, "the counting allocator is not counting");
}

#[test]
fn the_callback_allocates_nothing_on_the_ordinary_path() {
    let (mut s, mut r, c) = sink(buffers::MIN_MILLIS);
    let payload = vec![0x5Au8; FRAME * 480];
    let mut drain = vec![0u8; FRAME * 480];

    // Warm every path once outside the measured window.
    s.on_data(&payload);
    r.read(&mut drain);

    let seen = allocations_during(|| {
        for _ in 0..1_000 {
            s.on_data(&payload);
            r.read(&mut drain);
        }
    });
    assert_eq!(seen, 0, "§10: the callback allocated {seen} times");
    assert!(c.frames() > 0);
}

#[test]
fn the_callback_allocates_nothing_when_it_overruns() {
    // The failure path is the one that tempts an implementation into building an
    // error, formatting a message, or pushing to a log. None of that may happen
    // on the audio thread.
    let (mut s, _r, c) = sink(buffers::MIN_MILLIS);
    let payload = vec![0u8; FRAME * 480];
    while c.snapshot().overruns == 0 {
        s.on_data(&payload);
    }

    let seen = allocations_during(|| {
        for _ in 0..1_000 {
            s.on_data(&payload);
        }
    });
    assert_eq!(seen, 0, "an overrun allocated {seen} times");
    assert!(c.snapshot().overruns >= 1_000);
    assert_eq!(c.snapshot().dropped_frames % 480, 0);
}

#[test]
fn the_callback_allocates_nothing_when_the_device_starves() {
    let (mut s, _r, c) = sink(buffers::MIN_MILLIS);
    s.on_data(&[]);
    let seen = allocations_during(|| {
        for _ in 0..1_000 {
            s.on_data(&[]);
        }
    });
    assert_eq!(seen, 0, "an empty callback allocated {seen} times");
    assert_eq!(c.snapshot().underruns, 1_001);
}

#[test]
fn recording_a_stream_error_is_kept_off_the_callback_path() {
    // record_error formats and lowercases a string, so it certainly allocates.
    // That is fine - CPAL calls the error callback from its own thread, not the
    // audio one - but it must never be reachable from on_data. This pins the
    // split in place so a later refactor cannot quietly merge them.
    let c = Arc::new(Counters::default());
    let seen = allocations_during(|| c.record_error("ALSA xrun"));
    assert!(
        seen > 0,
        "if this ever stops allocating the test is no longer pinning anything"
    );

    let (mut s, _r, sink_counters) = sink(buffers::MIN_MILLIS);
    let payload = vec![0u8; FRAME * 64];
    s.on_data(&payload);
    let seen = allocations_during(|| s.on_data(&payload));
    assert_eq!(seen, 0);
    assert_eq!(sink_counters.snapshot().stream_errors, 0);
}

#[test]
fn a_stalled_reader_cannot_block_the_callback() {
    // The consumer is parked and never drains. With a lock-based queue and an
    // unlucky interleaving this is where an audio thread waits; with a wait-free
    // one the ring fills, callbacks are dropped and counted, and nothing stops.
    let (mut s, reader, c) = sink(buffers::MIN_MILLIS);
    let stop = Arc::new(AtomicBool::new(false));
    let parked = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            // Holds the reading end and deliberately does nothing with it.
            let _reader = reader;
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };

    let payload = vec![0u8; FRAME * 480];
    let mut worst = Duration::ZERO;
    for _ in 0..2_000 {
        let at = Instant::now();
        s.on_data(&payload);
        worst = worst.max(at.elapsed());
    }
    stop.store(true, Ordering::Relaxed);
    parked.join().unwrap();

    // Generous by three orders of magnitude against a 2.5 ms callback period:
    // this is here to catch a callback that *waits*, not one that is slow.
    assert!(
        worst < Duration::from_millis(100),
        "a callback took {worst:?} with the reader stalled"
    );
    assert!(
        c.snapshot().overruns > 0,
        "the ring should have filled, which is the correct outcome"
    );
}

/// Playback's callback, wired to a queue with nothing else attached.
fn source(chunks: usize) -> (chunks::Feeder, Source, Arc<Cursor>, Arc<PlaybackCounters>) {
    let (feeder, drain) = chunks::queue(chunks::chunk_bytes(FRAME, RATE), chunks);
    let cursor = Arc::new(Cursor::default());
    let counters = Arc::new(PlaybackCounters::default());
    cursor.set_playing(true);
    let source = Source::new(drain, Arc::clone(&cursor), Arc::clone(&counters), FRAME);
    (feeder, source, cursor, counters)
}

/// Fills every spare chunk the feeder holds, outside any measured window.
fn top_up(feeder: &mut chunks::Feeder, epoch: u64, frame: &mut u64) {
    top_up_with(feeder, epoch, frame, 0x5A);
}

fn top_up_with(feeder: &mut chunks::Feeder, epoch: u64, frame: &mut u64, byte: u8) {
    while let Some(mut chunk) = feeder.take() {
        let capacity = chunk.capacity();
        chunk.spare_mut().fill(byte);
        chunk.mark(epoch, *frame, capacity);
        *frame += (capacity / FRAME) as u64;
        feeder.send(chunk).expect("send");
    }
}

#[test]
fn the_playback_callback_allocates_nothing_on_the_ordinary_path() {
    // Chunks are handed over by move and handed back by move, so the audio
    // thread never sees an allocator. This is what pays for the chunk design:
    // a byte ring would have been simpler and could not discard a seek.
    let (mut feeder, mut source, _cursor, counters) = source(8);
    let mut out = vec![0u8; FRAME * 480];
    let mut frame = 0u64;

    // Warm every path once outside the window.
    top_up(&mut feeder, 0, &mut frame);
    source.on_data(&mut out);

    let mut seen = 0;
    for _ in 0..50 {
        top_up(&mut feeder, 0, &mut frame);
        seen += allocations_during(|| {
            for _ in 0..8 {
                source.on_data(&mut out);
            }
        });
    }
    assert_eq!(seen, 0, "§10: the playback callback allocated {seen} times");
    assert!(counters.snapshot().frames > 0);
    assert_eq!(counters.snapshot().underruns, 0);
}

#[test]
fn reaching_past_what_a_seek_invalidated_allocates_nothing() {
    // The arrangement a gapless seek actually produces on a device: the queue
    // holds the audio the seek invalidated *and*, behind it, the audio for the
    // new position, filled out of the feeder's reserve before the callback ran.
    // The callback has to walk past the first to reach the second, in one pass,
    // without allocating and without counting a starvation it did not suffer.
    let (mut feeder, mut source, cursor, counters) = source(9);
    feeder.hold_back(4);
    let mut frame = 0u64;
    top_up(&mut feeder, 0, &mut frame);

    let epoch = cursor.seek(500_000);
    feeder.release_reserve();
    let mut at = 500_000;
    top_up_with(&mut feeder, epoch, &mut at, 0xA5);

    // One callback: four chunks, which is exactly the reserve.
    let mut out = vec![0u8; chunks::chunk_bytes(FRAME, RATE) * 4];
    let seen = allocations_during(|| source.on_data(&mut out));
    assert_eq!(seen, 0, "walking past stale audio allocated {seen} times");

    assert!(
        out.iter().all(|&b| b == 0xA5),
        "the callback played the old position, or silence, or both"
    );
    let health = counters.snapshot();
    assert_eq!(
        health.stale_chunks, 5,
        "the invalidated audio was not dropped"
    );
    assert_eq!(health.underruns, 0, "a served seek was called a starvation");
    assert_eq!(health.silence_frames, 0);
    assert_eq!(cursor.frame(), 500_000 + (out.len() / FRAME) as u64);
}

#[test]
fn discarding_the_audio_a_seek_invalidated_allocates_nothing() {
    // The seek path is the one that tempts an implementation into clearing a
    // collection, and it runs on the audio thread by design - the callback is
    // the only thing that can drop what is already queued.
    let (mut feeder, mut source, cursor, counters) = source(8);
    let mut out = vec![0u8; FRAME * 64];
    let mut frame = 0u64;
    top_up(&mut feeder, 0, &mut frame);
    source.on_data(&mut out);

    let mut seen = 0;
    for round in 1..=50u64 {
        // Everything queued belongs to the old epoch and must be discarded.
        let epoch = cursor.seek(round * 100_000);
        seen += allocations_during(|| source.on_data(&mut out));
        let mut at = round * 100_000;
        top_up(&mut feeder, epoch, &mut at);
        seen += allocations_during(|| source.on_data(&mut out));
    }
    assert_eq!(seen, 0, "a seek allocated {seen} times on the audio thread");
    assert!(counters.snapshot().stale_chunks >= 50);
}

#[test]
fn the_playback_callback_allocates_nothing_when_it_starves() {
    // The failure path again, and here it also has to write silence - which is
    // a fill over a borrowed slice, not a new buffer.
    let (_feeder, mut source, _cursor, counters) = source(4);
    let mut out = vec![0u8; FRAME * 480];
    source.on_data(&mut out);

    let seen = allocations_during(|| {
        for _ in 0..1_000 {
            source.on_data(&mut out);
        }
    });
    assert_eq!(seen, 0, "a starved callback allocated {seen} times");
    assert_eq!(counters.snapshot().underruns, 1_001);
    assert_eq!(out, vec![0u8; FRAME * 480], "starvation made noise");
}

#[test]
fn a_stalled_feeder_cannot_block_the_playback_callback() {
    // The mirror of the stalled-reader test. A feeder thread that parks - stuck
    // on a slow disk, or descheduled - must cost the listener silence, not a
    // stopped audio thread.
    let (feeder, mut source, _cursor, counters) = source(8);
    let stop = Arc::new(AtomicBool::new(false));
    let parked = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let _feeder = feeder;
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };

    let mut out = vec![0u8; FRAME * 480];
    let mut worst = Duration::ZERO;
    for _ in 0..2_000 {
        let at = Instant::now();
        source.on_data(&mut out);
        worst = worst.max(at.elapsed());
    }
    stop.store(true, Ordering::Relaxed);
    parked.join().unwrap();

    assert!(
        worst < Duration::from_millis(100),
        "a playback callback took {worst:?} with the feeder stalled"
    );
    assert!(
        counters.snapshot().underruns > 0,
        "silence and a counter is the correct outcome"
    );
}
