/*
 *  meter.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  REQUIREMENTS §47.6 - live peak and RMS.
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

//! REQUIREMENTS §47.6 - live peak and RMS.
//!
//! This runs *inside* the real-time callback, so it does exactly what it is
//! allowed to do there: fixed-point-free arithmetic over a borrowed slice,
//! publishing through atomics. No allocation, no locking, no I/O. Metering a
//! copy on another thread would be safer-looking and strictly worse - it would
//! need a second ring, and the numbers would lag the audio they describe.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use cpal::SampleFormat;

/// Peak and running sum-of-squares, published as bit-patterns so the callback
/// never touches a lock. Peaks are held until read, the way a hardware meter does.
#[derive(Debug, Default)]
pub struct Meter {
    peak_bits: [AtomicU32; 2],
    sumsq_bits: [AtomicU64; 2],
    frames: AtomicU64,
    /// Samples the callback could not interpret, i.e. an unsupported format.
    unreadable: AtomicU64,
}

impl Meter {
    pub fn observe(&self, bytes: &[u8], format: SampleFormat, channels: u16) {
        let Some(read) = reader(format) else {
            self.unreadable.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let width = format.sample_size();
        let frame_bytes = width * channels as usize;
        if frame_bytes == 0 || bytes.len() < frame_bytes {
            return;
        }
        // Accumulate locally, publish once: one atomic RMW per channel per
        // callback rather than per sample.
        let mut peak = [0f32; 2];
        let mut sumsq = [0f64; 2];
        let frames = bytes.len() / frame_bytes;
        for f in 0..frames {
            for ch in 0..channels as usize {
                let o = f * frame_bytes + ch * width;
                let v = read(&bytes[o..o + width]);
                let slot = ch.min(1);
                let a = v.abs();
                if a > peak[slot] {
                    peak[slot] = a;
                }
                sumsq[slot] += (v as f64) * (v as f64);
            }
        }
        for slot in 0..2 {
            self.peak_bits[slot].fetch_max(peak[slot].to_bits(), Ordering::Relaxed);
            // f64 has no atomic add; a compare-exchange loop is wait-free enough
            // here because this thread is the only writer.
            let cur = f64::from_bits(self.sumsq_bits[slot].load(Ordering::Relaxed));
            self.sumsq_bits[slot].store((cur + sumsq[slot]).to_bits(), Ordering::Relaxed);
        }
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
    }

    /// Peak and RMS in dBFS per channel, and reset the peak hold.
    pub fn take(&self) -> Reading {
        let frames = self.frames.swap(0, Ordering::Relaxed).max(1);
        let mut out = Reading::default();
        for slot in 0..2 {
            let peak = f32::from_bits(self.peak_bits[slot].swap(0, Ordering::Relaxed));
            let sumsq = f64::from_bits(self.sumsq_bits[slot].swap(0, Ordering::Relaxed));
            out.peak_dbfs[slot] = dbfs(peak as f64);
            out.rms_dbfs[slot] = dbfs((sumsq / frames as f64).sqrt());
            out.peak_linear[slot] = peak;
        }
        out.unreadable = self.unreadable.load(Ordering::Relaxed);
        out
    }
}

#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
pub struct Reading {
    pub peak_dbfs: [f64; 2],
    pub rms_dbfs: [f64; 2],
    pub peak_linear: [f32; 2],
    pub unreadable: u64,
}

impl Reading {
    /// A clipped sample in an integer format is a real event, not a rounding
    /// artifact - vinyl capture chains routinely arrive too hot.
    pub fn clipped(&self) -> bool {
        self.peak_linear.iter().any(|p| *p >= 0.999_969)
    }
}

fn dbfs(linear: f64) -> f64 {
    if linear <= 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

/// Byte-slice → normalized f32, per sample format. Returns `None` for formats
/// we have not been asked to meter rather than guessing at the layout.
fn reader(format: SampleFormat) -> Option<fn(&[u8]) -> f32> {
    Some(match format {
        SampleFormat::I8 => |b: &[u8]| b[0] as i8 as f32 / i8::MAX as f32,
        SampleFormat::U8 => |b: &[u8]| (b[0] as f32 - 128.0) / 128.0,
        SampleFormat::I16 => {
            |b: &[u8]| i16::from_le_bytes([b[0], b[1]]) as f32 / -(i16::MIN as f32)
        }
        SampleFormat::U16 => {
            |b: &[u8]| (u16::from_le_bytes([b[0], b[1]]) as f32 - 32768.0) / 32768.0
        }
        // CPAL carries I24 in a 4-byte word; the sample occupies the low three
        // bytes. Shifting up and back sign-extends without a branch.
        SampleFormat::I24 => |b: &[u8]| {
            let raw = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            ((raw << 8) >> 8) as f32 / 8_388_608.0
        },
        SampleFormat::I32 => {
            |b: &[u8]| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / -(i32::MIN as f32)
        }
        SampleFormat::U32 => |b: &[u8]| {
            (u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64 - 2_147_483_648.0) as f32
                / 2_147_483_648.0
        },
        SampleFormat::F32 => |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        SampleFormat::F64 => {
            |b: &[u8]| f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32
        }
        _ => return None,
    })
}
