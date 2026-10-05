/*
 *  meter.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Peak, RMS, peak-hold and clip latch (§17, §18).
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

//! Peak, RMS, peak-hold and clip latch (§17, §18).
//!
//! §17 asks for four measurements per channel and snapshots the UI can read 30
//! to 60 times a second. §18 adds two constraints that are really one: clipping
//! must *latch*, and nothing here may alter the incoming level. The second is
//! enforced by the signatures - every entry point takes a shared slice, so a
//! meter that modified its input would not compile.
//!
//! # What is measured, and over what
//!
//! - **Peak** is the highest magnitude *since the last snapshot was taken*.
//!   Reading resets it. That is deliberate: a decaying peak can hide a transient
//!   that happened between two reads, and the one thing a recording meter must
//!   never do is fail to show the sample that clipped. The cost is that the
//!   value depends slightly on how often you look, which is the right trade.
//! - **RMS** is over a true sliding window of [`Config::rms_window_millis`],
//!   held in a ring of buckets. It does *not* depend on how often snapshots are
//!   taken - a meter whose RMS changed with the UI's frame rate would be
//!   measuring the UI.
//! - **Peak hold** is the slow needle: it takes any new maximum immediately,
//!   holds it for [`Config::hold_millis`], then falls at
//!   [`Config::fall_db_per_second`].
//! - **Clip** latches until [`Meter::clear_clip`] is called, and counts the
//!   samples that caused it, so "one sample touched full scale" and "eight
//!   seconds of flat tops" are distinguishable.
//!
//! # Full scale is a property of the format
//!
//! A clip detector that compares against 1.0 never fires on integer input: the
//! largest positive code of an `i16` is 32767/32768, which is 0.99997 and not
//! one. So the threshold comes from the [`StorageFormat`], positive and negative
//! separately, because two's complement is asymmetric - the most negative code
//! *is* exactly -1.0 and the most positive one is a whole LSB short of it.
//!
//! One measured limitation, at 32-bit only: `f32` has 24 bits of mantissa, so
//! the top few hundred `i32` codes all decode to exactly 1.0 and a sample within
//! about -0.0000005 dBFS of full scale latches as a clip. For a meter that is
//! the right answer anyway, and the alternative - clip detection in integer
//! space, parallel to the decoder - would be a second definition of full scale
//! to keep in step with the first.

use std::fmt;

use vcw_types::StorageFormat;

/// The level a meter shows when there is nothing to show.
///
/// Digital silence has no decibel value, and -inf is not a number a UI can lay
/// out. Every conversion here floors at this instead.
pub const SILENCE_DB: f32 = -120.0;

/// Buckets the sliding RMS window is divided into.
///
/// The window advances a bucket at a time, so this is the granularity of the
/// slide: 16 buckets of a 300 ms window means the window is accurate to about
/// 19 ms. More buckets cost a little arithmetic per rotation and nothing per
/// sample.
const BUCKETS: usize = 16;

/// How a meter is set up.
///
/// The defaults are a recording meter: a 300 ms RMS window, which is the
/// integration time a VU-like needle wants, and a hold-and-fall that leaves a
/// transient visible long enough to read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// Channels in the interleaved stream.
    pub channels: usize,
    /// Sample rate, which is what turns the millisecond settings into samples.
    pub rate: u32,
    /// How the samples are laid out. Also where full scale comes from.
    pub format: StorageFormat,
    /// The RMS integration window.
    pub rms_window_millis: u32,
    /// How long the hold needle stays at a new maximum before it starts to fall.
    pub hold_millis: u32,
    /// How fast it falls afterwards.
    pub fall_db_per_second: f32,
    /// Consecutive samples at or beyond full scale needed to latch a clip.
    ///
    /// One by default: on a recording meter a single sample at full scale is
    /// worth knowing about, because it means the analog gain is already too
    /// high. Raise it to three for the broadcast convention.
    pub clip_consecutive: u32,
}

impl Config {
    /// A meter for a stream of this shape, with the default dynamics.
    #[must_use]
    pub const fn new(channels: usize, rate: u32, format: StorageFormat) -> Self {
        Self {
            channels,
            rate,
            format,
            rms_window_millis: 300,
            hold_millis: 1_500,
            fall_db_per_second: 20.0,
            clip_consecutive: 1,
        }
    }

    /// Sets the RMS integration window.
    #[must_use]
    pub const fn rms_window(mut self, millis: u32) -> Self {
        self.rms_window_millis = millis;
        self
    }

    /// Sets the hold time and fall rate of the peak-hold needle.
    #[must_use]
    pub const fn hold(mut self, millis: u32, db_per_second: f32) -> Self {
        self.hold_millis = millis;
        self.fall_db_per_second = db_per_second;
        self
    }

    /// Sets how many consecutive full-scale samples latch a clip.
    #[must_use]
    pub const fn clip_after(mut self, samples: u32) -> Self {
        self.clip_consecutive = samples;
        self
    }
}

/// What one channel is doing, as of the moment the snapshot was taken.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    /// Highest magnitude since the previous snapshot, 0.0 to about 1.0.
    pub peak: f32,
    /// Root mean square over the configured window.
    pub rms: f32,
    /// The hold needle: a recent maximum, falling.
    pub hold: f32,
    /// Whether this channel has clipped since the latch was last cleared.
    pub clipped: bool,
    /// Samples at or beyond full scale since the latch was last cleared.
    pub clipped_samples: u64,
}

impl Levels {
    /// Instantaneous peak in dBFS, floored at [`SILENCE_DB`].
    #[must_use]
    pub fn peak_db(&self) -> f32 {
        db(self.peak)
    }

    /// RMS in dBFS, floored at [`SILENCE_DB`].
    #[must_use]
    pub fn rms_db(&self) -> f32 {
        db(self.rms)
    }

    /// The hold needle in dBFS, floored at [`SILENCE_DB`].
    #[must_use]
    pub fn hold_db(&self) -> f32 {
        db(self.hold)
    }
}

impl fmt::Display for Levels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "peak {:>7.1} rms {:>7.1} hold {:>7.1}{}",
            self.peak_db(),
            self.rms_db(),
            self.hold_db(),
            if self.clipped { " CLIP" } else { "" }
        )
    }
}

/// Every channel at one instant: §17's "Rust-generated snapshot".
///
/// Cheap enough to take 60 times a second and small enough to cross the IPC
/// boundary as JSON, which is what §35 requires of `meter-update` - the PCM it
/// was computed from never leaves Rust.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// One entry per channel, in stream order.
    pub channels: Vec<Levels>,
    /// Frames metered since the meter was made or last reset.
    pub frames: u64,
}

impl Snapshot {
    /// Whether any channel has clipped.
    #[must_use]
    pub fn clipped(&self) -> bool {
        self.channels.iter().any(|c| c.clipped)
    }

    /// The loudest instantaneous peak across all channels.
    #[must_use]
    pub fn peak(&self) -> f32 {
        self.channels.iter().fold(0.0_f32, |m, c| m.max(c.peak))
    }
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (channel, levels) in self.channels.iter().enumerate() {
            if channel > 0 {
                write!(f, " | ")?;
            }
            write!(f, "ch{channel} {levels}")?;
        }
        Ok(())
    }
}

/// Amplitude to dBFS, floored at [`SILENCE_DB`].
///
/// Takes the magnitude, so a negative sample is as loud as its positive twin.
#[must_use]
pub fn db(amplitude: f32) -> f32 {
    let magnitude = amplitude.abs();
    if magnitude <= 0.0 {
        return SILENCE_DB;
    }
    (20.0 * magnitude.log10()).max(SILENCE_DB)
}

/// dBFS back to amplitude. The inverse of [`db`] above the floor.
#[must_use]
pub fn amplitude(db: f32) -> f32 {
    if db <= SILENCE_DB {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    }
}

/// One channel's running state.
#[derive(Clone, Debug)]
struct Channel {
    /// Peak since the last snapshot.
    peak: f32,
    /// The hold needle.
    hold: f32,
    /// Frames since the hold needle last took a new maximum.
    held_for: u64,
    /// Sum of squares per bucket, oldest to newest by rotation.
    sums: [f64; BUCKETS],
    /// Samples counted per bucket.
    counts: [u32; BUCKETS],
    /// Which bucket is being filled.
    current: usize,
    /// Consecutive samples at or beyond full scale, right now.
    run: u32,
    /// Whether the latch is set.
    clipped: bool,
    /// Samples at or beyond full scale since the latch was cleared.
    clipped_samples: u64,
}

impl Channel {
    fn new() -> Self {
        Self {
            peak: 0.0,
            hold: 0.0,
            held_for: 0,
            sums: [0.0; BUCKETS],
            counts: [0; BUCKETS],
            current: 0,
            run: 0,
            clipped: false,
            clipped_samples: 0,
        }
    }

    /// The sliding window's RMS over whatever it currently holds.
    ///
    /// Divides by the samples actually counted rather than by the window's
    /// nominal length, so a meter is correct from its first sample instead of
    /// ramping up out of a window full of assumed silence.
    fn rms(&self) -> f32 {
        let total: f64 = self.sums.iter().sum();
        let count: u64 = self.counts.iter().map(|c| u64::from(*c)).sum();
        if count == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            (total / count as f64).sqrt() as f32
        }
    }
}

/// A stereo (or any width) meter over an interleaved stream.
///
/// Feed it whatever arrives, as often as it arrives; take a snapshot whenever
/// the UI is ready for one. The two rates are independent, which is the point:
/// §10 forbids the callback from doing UI work, and §17 wants a steady 30-60 Hz
/// regardless of the device's buffer size.
#[derive(Clone, Debug)]
pub struct Meter {
    config: Config,
    channels: Vec<Channel>,
    /// Positive full scale for the format, and its negative twin. Asymmetric.
    ceiling: f32,
    floor: f32,
    /// Frames per RMS bucket.
    bucket_frames: u64,
    /// Frames into the current bucket.
    bucket_position: u64,
    /// Frames after which the hold needle starts to fall.
    hold_frames: u64,
    /// Frames metered in total.
    frames: u64,
}

impl Meter {
    /// A meter for this stream.
    ///
    /// # Panics
    ///
    /// If `channels` is zero. A meter over no channels has no meaning, and
    /// returning a `Result` for a caller error that cannot happen at runtime
    /// would push the check into every call site.
    #[must_use]
    pub fn new(config: Config) -> Self {
        assert!(config.channels > 0, "a meter needs at least one channel");
        let rate = u64::from(config.rate.max(1));
        let window = u64::from(config.rms_window_millis.max(1));
        let bucket_frames = (rate * window / 1_000 / BUCKETS as u64).max(1);
        let hold_frames = rate * u64::from(config.hold_millis) / 1_000;
        let (ceiling, floor) = full_scale(config.format);
        Self {
            channels: vec![Channel::new(); config.channels],
            ceiling,
            floor,
            bucket_frames,
            bucket_position: 0,
            hold_frames,
            frames: 0,
            config,
        }
    }

    /// How the meter was set up.
    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// Frames metered since the meter was made or last reset.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Meters a buffer of interleaved samples in the configured storage format.
    ///
    /// Returns the frames consumed. A trailing partial frame is ignored rather
    /// than metered as though the missing channels were silent, because half a
    /// frame of stereo read as a whole one would show the right channel dipping
    /// on every ragged buffer.
    ///
    /// Takes a shared slice: §18 forbids altering the incoming level, and this
    /// is where that is guaranteed rather than promised.
    pub fn feed(&mut self, bytes: &[u8]) -> u64 {
        let width = self.config.format.bytes_per_sample();
        let frame_bytes = width * self.config.channels;
        if frame_bytes == 0 {
            return 0;
        }
        let frames = bytes.len() / frame_bytes;
        for frame in 0..frames {
            let base = frame * self.config.channels;
            for channel in 0..self.config.channels {
                let sample = self
                    .config
                    .format
                    .decode_sample(bytes, base + channel)
                    .unwrap_or(0.0);
                self.sample(channel, sample);
            }
            self.advance();
        }
        frames as u64
    }

    /// Meters interleaved samples that have already been decoded.
    ///
    /// The path a synthesised test signal takes, and the one a future float
    /// pipeline would. Full-scale detection still uses the configured format's
    /// ceiling, so a meter set up for 16-bit input reports clipping where 16-bit
    /// input would clip.
    pub fn feed_samples(&mut self, interleaved: &[f32]) -> u64 {
        let frames = interleaved.len() / self.config.channels;
        for frame in 0..frames {
            let base = frame * self.config.channels;
            for channel in 0..self.config.channels {
                self.sample(channel, interleaved[base + channel]);
            }
            self.advance();
        }
        frames as u64
    }

    /// Takes a snapshot, and resets the instantaneous peak.
    ///
    /// The peak-hold needle and the clip latch survive: they are the two
    /// measurements whose whole job is to outlive the moment they happened in.
    pub fn snapshot(&mut self) -> Snapshot {
        let channels = self
            .channels
            .iter_mut()
            .map(|channel| {
                let levels = Levels {
                    peak: channel.peak,
                    rms: channel.rms(),
                    hold: channel.hold,
                    clipped: channel.clipped,
                    clipped_samples: channel.clipped_samples,
                };
                channel.peak = 0.0;
                levels
            })
            .collect();
        Snapshot {
            channels,
            frames: self.frames,
        }
    }

    /// Reads the levels without resetting anything.
    ///
    /// For a second observer - a log line, a test - that must not disturb what
    /// the UI is about to read.
    #[must_use]
    pub fn peek(&self) -> Snapshot {
        Snapshot {
            channels: self
                .channels
                .iter()
                .map(|channel| Levels {
                    peak: channel.peak,
                    rms: channel.rms(),
                    hold: channel.hold,
                    clipped: channel.clipped,
                    clipped_samples: channel.clipped_samples,
                })
                .collect(),
            frames: self.frames,
        }
    }

    /// Clears the clip latch on every channel. §18's "until acknowledged".
    pub fn clear_clip(&mut self) {
        for channel in &mut self.channels {
            channel.clipped = false;
            channel.clipped_samples = 0;
            channel.run = 0;
        }
    }

    /// Forgets everything, as though the meter had just been made.
    pub fn reset(&mut self) {
        for channel in &mut self.channels {
            *channel = Channel::new();
        }
        self.bucket_position = 0;
        self.frames = 0;
    }

    /// Meters one sample on one channel.
    fn sample(&mut self, channel: usize, value: f32) {
        let ceiling = self.ceiling;
        let floor = self.floor;
        let needed = self.config.clip_consecutive.max(1);
        let state = &mut self.channels[channel];

        let magnitude = value.abs();
        if magnitude > state.peak {
            state.peak = magnitude;
        }
        if magnitude > state.hold {
            state.hold = magnitude;
            state.held_for = 0;
        }

        let bucket = state.current;
        state.sums[bucket] += f64::from(value) * f64::from(value);
        state.counts[bucket] = state.counts[bucket].saturating_add(1);

        // Two's complement is asymmetric, so the two ends are tested separately
        // rather than against one magnitude.
        if value >= ceiling || value <= floor {
            state.clipped_samples += 1;
            state.run += 1;
            if state.run >= needed {
                state.clipped = true;
            }
        } else {
            state.run = 0;
        }
    }

    /// Moves the clock on by one frame: rotates the RMS window and falls the
    /// hold needle when its time is up.
    fn advance(&mut self) {
        self.frames += 1;
        self.bucket_position += 1;

        for state in &mut self.channels {
            state.held_for += 1;
        }

        if self.bucket_position >= self.bucket_frames {
            self.bucket_position = 0;
            for state in &mut self.channels {
                let next = (state.current + 1) % BUCKETS;
                // The bucket being rotated into is the oldest one, and clearing
                // it is what makes the window slide rather than accumulate.
                state.sums[next] = 0.0;
                state.counts[next] = 0;
                state.current = next;
            }
            self.fall();
        }
    }

    /// Lets the hold needle fall, once per bucket rather than once per sample.
    ///
    /// Per sample would be 384,000 `powf` calls a second at 192 kHz stereo for a
    /// needle that moves a few pixels; per bucket is a few hundred and looks
    /// identical.
    fn fall(&mut self) {
        if self.config.fall_db_per_second <= 0.0 {
            return;
        }
        let seconds = self.bucket_frames as f32 / self.config.rate.max(1) as f32;
        let step = self.config.fall_db_per_second * seconds;
        for state in &mut self.channels {
            if state.held_for < self.hold_frames || state.hold <= 0.0 {
                continue;
            }
            let fallen = db(state.hold) - step;
            state.hold = if fallen <= SILENCE_DB {
                0.0
            } else {
                amplitude(fallen)
            };
        }
    }
}

/// The largest positive and most negative values this format can carry.
///
/// Asymmetric on purpose: in two's complement the most negative code is exactly
/// -1.0 and the most positive is one LSB short of +1.0. A meter that tested
/// `abs() >= 1.0` would never see a positive clip on integer input.
fn full_scale(format: StorageFormat) -> (f32, f32) {
    let bits = match format {
        StorageFormat::Int16 => 16,
        StorageFormat::Int24Packed | StorageFormat::Int24Padded => 24,
        StorageFormat::Int32 => 32,
        // Float has no code ceiling. Anything at or past unity is clipping, and
        // a float sample can legitimately exceed it, which an integer one
        // cannot.
        StorageFormat::Float32 => return (1.0, -1.0),
    };
    let full = 2.0_f32.powi(bits - 1);
    ((full - 1.0) / full, -1.0)
}
