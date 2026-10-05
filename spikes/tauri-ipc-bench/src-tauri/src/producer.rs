/*
 *  producer.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The producer: one dedicated OS thread standing in for the meter and
 *  waveform
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

//! The producer: one dedicated OS thread standing in for the meter and waveform
//! workers §36 names, feeding the webview at a scheduled rate.
//!
//! D8 says no async on this path, so this is a plain thread with an absolute
//! schedule (`next += period`, not `sleep(period)`) - drift-free, and its own
//! lateness is recorded so webview-side jitter can be attributed to the right
//! side of the boundary.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{Emitter, Manager, State, Webview};

use crate::metrics::Hist;
use crate::payload::{Binary, ManualJson, MeterFrame, PositionUpdate, WaveDelta};
use crate::{Bench, BenchConfig, Encoding, SendStats, Transport};

/// Shared time origin, so Rust-side timestamps in payloads and the webview's
/// clock-offset probe agree.
pub struct Epoch(Instant);

impl Epoch {
    pub fn new() -> Self {
        Self(Instant::now())
    }
    pub fn elapsed_us(&self) -> u64 {
        self.0.elapsed().as_micros() as u64
    }
}

/// One outbound stream. Holds the reusable encode buffers so no frame allocates.
struct Sink {
    event: &'static str,
    channel: Channel<InvokeResponseBody>,
    json: String,
    bytes: Vec<u8>,
    sent: u64,
    total_bytes: u64,
    /// Size of the payload as it appears *in the evaluated JavaScript source*.
    /// For JSON that equals `total_bytes`; for `Raw` under tauri's 1024-byte
    /// threshold it is the decimal array tauri renders, which is the whole
    /// point of measuring it separately.
    total_wire: u64,
    errors: u64,
}

impl Sink {
    fn new(event: &'static str, channel: Channel<InvokeResponseBody>) -> Self {
        Self {
            event,
            channel,
            json: String::with_capacity(1 << 16),
            bytes: Vec::with_capacity(1 << 16),
            sent: 0,
            total_bytes: 0,
            total_wire: 0,
            errors: 0,
        }
    }

    /// Encode and send one frame, returning how long the call blocked the
    /// producer thread.
    fn send<T>(&mut self, webview: &Webview, cfg: &BenchConfig, frame: &T) -> Duration
    where
        T: Serialize + Binary + ManualJson,
    {
        // Everything that is *preparation* happens before the timer starts,
        // because moving that work off the IPC path is what D6 proposes. The
        // `serde` arm is the exception on purpose: its serialization is inside
        // the timed region because tauri does it there, and that is the
        // difference being measured.
        //
        // `RawValue::from_string` is here rather than at the call site because
        // it *validates* the string it wraps. Leaving it inside the timed
        // region would have charged the event-manual arm for a JSON parse that
        // the channel-manual arm never pays, and the comparison would have
        // been measuring my own plumbing.
        enum Ready {
            /// Let tauri serialize, inside the timed region.
            Serde,
            Json(InvokeResponseBody),
            Raw(InvokeResponseBody),
            EventJson(Box<serde_json::value::RawValue>),
        }

        let ready = match (cfg.transport, cfg.encoding) {
            (_, Encoding::Serde) => Ready::Serde,
            (Transport::Channel, Encoding::Manual) => {
                frame.write_json(&mut self.json);
                Ready::Json(InvokeResponseBody::Json(self.json.clone()))
            }
            (Transport::Event, Encoding::Manual) => {
                frame.write_json(&mut self.json);
                match serde_json::value::RawValue::from_string(self.json.clone()) {
                    Ok(r) => Ready::EventJson(r),
                    Err(_) => {
                        self.errors += 1;
                        return Duration::ZERO;
                    }
                }
            }
            (_, Encoding::Raw) => {
                frame.write_binary(&mut self.bytes);
                Ready::Raw(InvokeResponseBody::Raw(self.bytes.clone()))
            }
        };

        let (size, wire) = match &ready {
            Ready::Json(InvokeResponseBody::Json(s)) => (s.len(), s.len()),
            Ready::Raw(InvokeResponseBody::Raw(b)) => {
                // Exactly what tauri does under MAX_RAW_DIRECT_EXECUTE_THRESHOLD:
                // `serde_json::to_string(&bytes)`, then eval
                // `new Uint8Array([...]).buffer`. The 25-character wrapper is
                // added so the figure is the whole cost, not just the array.
                let arr = serde_json::to_string(b).map(|s| s.len()).unwrap_or(0);
                (b.len(), arr + 25)
            }
            Ready::EventJson(r) => (r.get().len(), r.get().len()),
            // The serde arm's size is taken from the same serializer tauri
            // uses, outside the timed region, so the byte columns stay
            // comparable across arms.
            Ready::Serde => {
                let n = serde_json::to_string(frame).map(|s| s.len()).unwrap_or(0);
                (n, n)
            }
            _ => (0, 0),
        };

        let t = Instant::now();
        let result = match (cfg.transport, ready) {
            (Transport::Channel, Ready::Json(b) | Ready::Raw(b)) => {
                self.channel.send(b).map_err(|e| e.to_string())
            }
            (Transport::Channel, Ready::Serde) => serde_json::to_string(frame)
                .map_err(|e| e.to_string())
                .and_then(|s| {
                    self.channel
                        .send(InvokeResponseBody::Json(s))
                        .map_err(|e| e.to_string())
                }),
            (Transport::Event, Ready::EventJson(r)) => {
                webview.emit(self.event, r).map_err(|e| e.to_string())
            }
            (Transport::Event, Ready::Serde) => {
                webview.emit(self.event, frame).map_err(|e| e.to_string())
            }
            // The event bus takes `impl Serialize` and has no binary path;
            // `start_bench` rejects that combination before it gets here.
            (Transport::Event, Ready::Raw(_) | Ready::Json(_)) => {
                Err("no binary path on the event bus".to_string())
            }
            (Transport::Channel, Ready::EventJson(_)) => unreachable!(),
        };
        let elapsed = t.elapsed();

        match result {
            Ok(()) => {
                self.sent += 1;
                self.total_bytes += size as u64;
                self.total_wire += wire as u64;
            }
            Err(_) => self.errors += 1,
        }
        elapsed
    }
}

/// Deterministic pseudo-audio for the meter and waveform, so every transport
/// arm sees identical payload *values* and any difference is transport, not data.
struct Source {
    phase: f64,
    rate: f64,
    noise: u64,
}

impl Source {
    fn new(rate: u32) -> Self {
        Self {
            phase: 0.0,
            rate: rate as f64,
            noise: 0x2545F4914F6CDD1D,
        }
    }

    fn next_noise(&mut self) -> f64 {
        // xorshift64*, so there is no rand dependency and runs are reproducible.
        let mut x = self.noise;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.noise = x;
        (x.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Advance by `frames` and return (peak, rms) per channel.
    fn advance(&mut self, frames: u64) -> ([f32; 2], [f32; 2]) {
        self.phase += frames as f64 / self.rate;
        // A slow program-level sweep plus per-frame noise: the meter needs to
        // move convincingly so the canvas work is representative.
        let env = 0.5 + 0.45 * (std::f64::consts::TAU * 0.05 * self.phase).sin();
        let n = self.next_noise();
        let pl = (env * (0.9 + 0.1 * n)) as f32;
        let pr = (env * (0.85 + 0.15 * (1.0 - n))) as f32;
        ([pl, pr], [pl * 0.63, pr * 0.61])
    }

    fn bucket(&mut self) -> [i16; 4] {
        let (p, _) = self.advance(0);
        let j = self.next_noise() as f32;
        let l = (p[0] * 32000.0 * (0.8 + 0.2 * j)) as i16;
        let r = (p[1] * 32000.0 * (0.8 + 0.2 * (1.0 - j))) as i16;
        [-l, l, -r, r]
    }
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn start_bench(
    webview: Webview,
    cfg: BenchConfig,
    meter: Channel<InvokeResponseBody>,
    wave: Channel<InvokeResponseBody>,
    position: Channel<InvokeResponseBody>,
    bench: State<'_, Bench>,
    epoch: State<'_, Epoch>,
) -> Result<(), String> {
    if bench.running.swap(true, Ordering::SeqCst) {
        return Err("a run is already in progress".into());
    }
    bench.stop.store(false, Ordering::SeqCst);
    *bench.rtt.lock().unwrap() = crate::metrics::Hist::default();

    if cfg.transport == Transport::Event && cfg.encoding == Encoding::Raw {
        bench.running.store(false, Ordering::SeqCst);
        return Err("the event bus has no binary payload path".into());
    }

    let handle = webview.clone();

    // The epoch is shared with `now_us` so payload timestamps and the webview's
    // clock-offset estimate are on the same origin.
    let t0_us = epoch.elapsed_us();
    let origin = Instant::now();
    let cfg2 = cfg.clone();

    std::thread::Builder::new()
        .name("s3-producer".into())
        .spawn(move || {
            run(handle, cfg2, meter, wave, position, origin, t0_us);
        })
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn run(
    webview: Webview,
    cfg: BenchConfig,
    meter_ch: Channel<InvokeResponseBody>,
    wave_ch: Channel<InvokeResponseBody>,
    pos_ch: Channel<InvokeResponseBody>,
    origin: Instant,
    t0_us: u64,
) {
    let bench = webview.state::<Bench>();

    let meter_hz = if cfg.coalesce {
        cfg.meter_hz
    } else {
        // The uncoalesced arm: whatever the audio callback rate is.
        cfg.capture_rate as f64 / cfg.callback_frames as f64
    };

    let mut meter = Sink::new("meter-update", meter_ch);
    let mut wave = Sink::new("waveform-update", wave_ch);
    let mut pos = Sink::new("recording-position", pos_ch);

    let mut stats = SendStats::default();
    let mut m_send = Hist::default();
    let mut w_send = Hist::default();
    let mut p_send = Hist::default();
    let mut lateness = Hist::default();

    let mut src = Source::new(cfg.capture_rate);

    // Absolute schedules, so a slow send does not push every later tick back.
    let meter_period = Duration::from_secs_f64(1.0 / meter_hz);
    let wave_period = Duration::from_secs_f64(1.0 / cfg.wave_hz);
    let pos_period = Duration::from_secs_f64(1.0 / cfg.position_hz);
    let mut next_meter = origin + meter_period;
    let mut next_wave = origin + wave_period;
    let mut next_pos = origin + pos_period;

    let deadline = origin + Duration::from_secs_f64(cfg.duration_secs);
    let frames_per_meter = (cfg.capture_rate as f64 / meter_hz) as u64;
    let buckets_per_wave = (cfg.capture_rate as f64 / cfg.wave_hz / cfg.bucket_frames as f64)
        .max(1.0)
        .round() as usize;

    let mut meter_seq = 0u32;
    let mut wave_seq = 0u32;
    let mut pos_seq = 0u32;
    let mut frame = 0u64;
    let mut bucket = 0u64;

    while Instant::now() < deadline && !bench.stop.load(Ordering::Relaxed) {
        let next = next_meter.min(next_wave).min(next_pos);
        let now = Instant::now();
        if next > now {
            // Capped, so the loop still observes `deadline` and the stop flag
            // between ticks. Without the cap the idle control arms - whose
            // periods are longer than the whole run - slept straight past the
            // end of the run and never recorded their stats at all.
            std::thread::sleep((next - now).min(Duration::from_millis(20)));
        }
        let now = Instant::now();
        lateness.record(now.saturating_duration_since(next).as_micros() as u64);
        let t_us = t0_us + now.duration_since(origin).as_micros() as u64;

        if now >= next_meter {
            let (peak, rms) = src.advance(frames_per_meter);
            frame += frames_per_meter;
            let f = MeterFrame {
                seq: meter_seq,
                t_us,
                peak,
                rms,
                clip: u8::from(peak[0] >= 0.999) | (u8::from(peak[1] >= 0.999) << 1),
            };
            meter_seq += 1;
            m_send.record(meter.send(&webview, &cfg, &f).as_micros() as u64);
            next_meter += meter_period;
        }

        if now >= next_wave {
            let mut buckets = Vec::with_capacity(buckets_per_wave * 4);
            for _ in 0..buckets_per_wave {
                buckets.extend_from_slice(&src.bucket());
            }
            let d = WaveDelta {
                seq: wave_seq,
                t_us,
                level: 0,
                start_bucket: bucket,
                buckets,
            };
            bucket += buckets_per_wave as u64;
            wave_seq += 1;
            w_send.record(wave.send(&webview, &cfg, &d).as_micros() as u64);
            next_wave += wave_period;
        }

        if now >= next_pos {
            let u = PositionUpdate {
                seq: pos_seq,
                t_us,
                frame,
                dropped: 0,
            };
            pos_seq += 1;
            p_send.record(pos.send(&webview, &cfg, &u).as_micros() as u64);
            next_pos += pos_period;
        }
    }

    stats.meter_send = m_send.summary();
    stats.wave_send = w_send.summary();
    stats.position_send = p_send.summary();
    stats.tick_lateness = lateness.summary();
    stats.meter_sent = meter.sent;
    stats.wave_sent = wave.sent;
    stats.position_sent = pos.sent;
    stats.meter_bytes = meter.total_bytes;
    stats.wave_bytes = wave.total_bytes;
    stats.position_bytes = pos.total_bytes;
    stats.meter_wire = meter.total_wire;
    stats.wave_wire = wave.total_wire;
    stats.position_wire = pos.total_wire;
    stats.send_errors = meter.errors + wave.errors + pos.errors;
    stats.elapsed_secs = origin.elapsed().as_secs_f64();

    *bench.last.lock().unwrap() = Some((cfg, stats));
    bench.running.store(false, Ordering::SeqCst);
}
