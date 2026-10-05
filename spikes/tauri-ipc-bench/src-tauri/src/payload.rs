/*
 *  payload.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The three payloads §35 names for high-rate traffic, in the three encodings
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

//! The three payloads §35 names for high-rate traffic, in the three encodings
//! D6 is choosing between.
//!
//! §35 forbids high-frequency PCM crossing the boundary, so none of these carry
//! samples: the meter is reduced, the waveform delta is a summary, the position
//! is a counter. The sizes here are therefore the real sizes, not a stand-in.

use serde::Serialize;

/// Sent at the meter rate. Peak and RMS per channel plus clip flags is what
/// §44's VU meters need and nothing more.
#[derive(Serialize, Clone, Copy, Debug)]
pub struct MeterFrame {
    /// Monotonic per-stream, so the receiver can detect loss and reordering.
    pub seq: u32,
    /// Microseconds since the bench started, on the Rust clock.
    pub t_us: u64,
    pub peak: [f32; 2],
    pub rms: [f32; 2],
    /// Bit 0 = left clipped, bit 1 = right clipped.
    pub clip: u8,
}

/// One append to the progressive waveform: a run of summary buckets starting at
/// `start_bucket` in pyramid `level`. Min and max per channel per bucket, which
/// is the AUP4 `summary256` shape S5 found and D1 adopted.
#[derive(Serialize, Clone, Debug)]
pub struct WaveDelta {
    pub seq: u32,
    pub t_us: u64,
    pub level: u8,
    pub start_bucket: u64,
    /// Interleaved `[min_l, max_l, min_r, max_r]` per bucket.
    pub buckets: Vec<i16>,
}

#[derive(Serialize, Clone, Copy, Debug)]
pub struct PositionUpdate {
    pub seq: u32,
    pub t_us: u64,
    pub frame: u64,
    pub dropped: u32,
}

/// Little-endian binary encodings. Deliberately hand-rolled and fixed-layout:
/// the point of the `raw` arm is to be the smallest thing that could work, so
/// that if it still loses to JSON we know the loss is transport, not encoding.
pub trait Binary {
    fn write_binary(&self, out: &mut Vec<u8>);
}

impl Binary for MeterFrame {
    fn write_binary(&self, out: &mut Vec<u8>) {
        out.clear();
        out.push(1);
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.t_us.to_le_bytes());
        for v in self.peak.iter().chain(self.rms.iter()) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.push(self.clip);
    }
}

impl Binary for WaveDelta {
    fn write_binary(&self, out: &mut Vec<u8>) {
        out.clear();
        out.push(2);
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.t_us.to_le_bytes());
        out.push(self.level);
        out.extend_from_slice(&self.start_bucket.to_le_bytes());
        out.extend_from_slice(&(self.buckets.len() as u32).to_le_bytes());
        for v in &self.buckets {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
}

impl Binary for PositionUpdate {
    fn write_binary(&self, out: &mut Vec<u8>) {
        out.clear();
        out.push(3);
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.t_us.to_le_bytes());
        out.extend_from_slice(&self.frame.to_le_bytes());
        out.extend_from_slice(&self.dropped.to_le_bytes());
    }
}

/// Hand-built compact JSON into a reused buffer - D6's "pre-serialized compact
/// payload". Short keys and fixed precision, so it is both smaller than serde's
/// output and free of per-frame allocation.
pub trait ManualJson {
    fn write_json(&self, out: &mut String);
}

fn push_f32(out: &mut String, v: f32) {
    use std::fmt::Write;
    // 4 decimals is ~0.0009 dB of meter resolution; more is wasted bytes.
    let _ = write!(out, "{v:.4}");
}

impl ManualJson for MeterFrame {
    fn write_json(&self, out: &mut String) {
        use std::fmt::Write;
        out.clear();
        let _ = write!(out, r#"{{"k":1,"s":{},"t":{},"p":["#, self.seq, self.t_us);
        push_f32(out, self.peak[0]);
        out.push(',');
        push_f32(out, self.peak[1]);
        out.push_str(r#"],"r":["#);
        push_f32(out, self.rms[0]);
        out.push(',');
        push_f32(out, self.rms[1]);
        let _ = write!(out, r#"],"c":{}}}"#, self.clip);
    }
}

impl ManualJson for WaveDelta {
    fn write_json(&self, out: &mut String) {
        use std::fmt::Write;
        out.clear();
        let _ = write!(
            out,
            r#"{{"k":2,"s":{},"t":{},"l":{},"b":{},"v":["#,
            self.seq, self.t_us, self.level, self.start_bucket
        );
        for (i, v) in self.buckets.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "{v}");
        }
        out.push_str("]}");
    }
}

impl ManualJson for PositionUpdate {
    fn write_json(&self, out: &mut String) {
        use std::fmt::Write;
        out.clear();
        let _ = write!(
            out,
            r#"{{"k":3,"s":{},"t":{},"f":{},"d":{}}}"#,
            self.seq, self.t_us, self.frame, self.dropped
        );
    }
}
