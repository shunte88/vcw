/*
 *  capture.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What a capture session is, and the counters that belong to the recording.
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

//! What a capture session is, and the counters that belong to the recording.
//!
//! These types are the contract between the crate that makes a capture and the
//! crate that stores one. `vcw-audio` produces them from a live stream; the
//! `captures` and `capture_diagnostics` tables in `vcw-project` are their
//! resting place. Neither crate depends on the other, which is why the
//! vocabulary lives here.
//!
//! §10 is the reason the counters are a persisted type rather than a process
//! statistic: overruns and dropped frames are facts about the *recording*, and a
//! recording outlives the run that made it.

use serde::{Deserialize, Serialize};

use crate::{CaptureEq, CaptureMode, SampleRate, StorageFormat};

/// How far a capture session got. Mirrors the `state` column, and `validate()`
/// rejects any value outside this set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureState {
    /// The stream is open, or was open when the process died.
    Recording,
    /// Stopped cleanly, every block committed.
    Finalised,
    /// Stopped by something other than a request: a stream error or a device
    /// that went away. The writer was still running and shut down tidily, so
    /// the frame count and the counters are the writer's own.
    Interrupted,
    /// Reconstructed after the process died mid-capture (§15).
    ///
    /// Distinct from [`CaptureState::Interrupted`] because the difference
    /// matters to whoever reads the row later: an interrupted capture stopped
    /// for a reason the writer *observed and recorded*, while a recovered one
    /// stopped without warning and everything known about it was inferred
    /// afterwards from the blocks that had already been committed. The audio is
    /// no less trustworthy - it was fsynced before the process died - but the
    /// counters are only as current as the last time they were written, and
    /// whatever the device produced after the final commit is gone and
    /// unmeasurable.
    Recovered,
}

impl CaptureState {
    /// The `state` column's spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Finalised => "finalised",
            Self::Interrupted => "interrupted",
            Self::Recovered => "recovered",
        }
    }

    /// Reads the `state` column. `None` for a value no version of VCW wrote.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "recording" => Some(Self::Recording),
            "finalised" => Some(Self::Finalised),
            "interrupted" => Some(Self::Interrupted),
            "recovered" => Some(Self::Recovered),
            _ => None,
        }
    }

    /// Whether the session's own row claims it is still going.
    ///
    /// Advisory only. Recovery keys on `finished_at IS NULL` instead, because
    /// the absence of a write is the one thing a crash cannot forge, while this
    /// column says `recording` both for a process that died and for a bug that
    /// forgot to update it.
    pub const fn is_unfinished(self) -> bool {
        matches!(self, Self::Recording)
    }

    /// Whether the capture ran to a clean, requested stop.
    ///
    /// The three ways of not doing so are worth keeping apart: still running,
    /// stopped by a fault the writer saw, and stopped by a process death nobody
    /// saw.
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Finalised)
    }
}

/// §10's four counters, which §38 requires persisted.
///
/// Saturating rather than wrapping throughout. A counter that wraps to zero
/// turns a catastrophic capture into a clean-looking one, and the whole purpose
/// of these numbers is to stop a damaged recording being mistaken for a good
/// one. Reaching `u64::MAX` overruns is not a situation worth being precise in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostics {
    /// Times the ring filled before the writer drained it. Each one is samples
    /// the device produced and nothing collected.
    pub overruns: u64,
    /// Times the device callback found no data ready.
    pub underruns: u64,
    /// Frames known to be lost. Non-zero means the capture is not bit-perfect,
    /// whatever the format verifier says.
    pub dropped_frames: u64,
    /// Stream errors reported by the host, counted here and described in the
    /// capture's `os_report`.
    pub stream_errors: u64,
}

impl Diagnostics {
    /// Whether the capture is free of every defect these counters can see.
    ///
    /// Necessary for a bit-perfect claim and nowhere near sufficient: a capture
    /// can be clean by all four counts and still have been silently resampled,
    /// which is what the format verifier is for.
    pub const fn is_clean(self) -> bool {
        self.overruns == 0
            && self.underruns == 0
            && self.dropped_frames == 0
            && self.stream_errors == 0
    }

    /// Adds another set, saturating. Used to fold a worker's counters into the
    /// session's.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self {
            overruns: self.overruns.saturating_add(other.overruns),
            underruns: self.underruns.saturating_add(other.underruns),
            dropped_frames: self.dropped_frames.saturating_add(other.dropped_frames),
            stream_errors: self.stream_errors.saturating_add(other.stream_errors),
        }
    }
}

/// §38's provenance: what was recorded, through what, and whether anyone checked.
///
/// Every field here answers a question a reader might ask years later about a
/// file whose origin is otherwise unrecoverable. [`CaptureInfo::os_verified`] is
/// the load-bearing one: §9 forbids claiming bit-perfect operation on the
/// backend's own report, so the claim is stored as *what was checked*, not as a
/// boolean opinion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureInfo {
    /// Rate as negotiated with the device, not as requested.
    pub rate: SampleRate,
    /// Channel count. Blocks are stored per channel, never interleaved.
    pub channels: u16,
    /// How the samples are laid out on disk. D4 stores them as the device gave
    /// them, so this follows the device's format rather than a house style.
    pub storage_format: StorageFormat,
    /// How the stream was opened (§9).
    pub capture_mode: CaptureMode,
    /// CPAL host: `alsa`, `wasapi`, `coreaudio`.
    pub host_api: Option<String>,
    /// The device's stable id, as `vcw-audio` spells it.
    pub device_id: Option<String>,
    /// The device's human-readable name at the time of capture. Names change and
    /// are not unique; this is provenance, not identity.
    pub device_name: Option<String>,
    /// Whether the operating system confirmed the negotiated format. False until
    /// a platform verifier has actually run and agreed.
    pub os_verified: bool,
    /// What the OS reported, verbatim, so the claim can be audited rather than
    /// believed.
    pub os_report: Option<String>,
    /// What equalisation the hardware upstream had already applied (§51).
    ///
    /// Provenance, like the three fields above it, and the one piece of
    /// provenance VCW cannot observe for itself: a phono stage does not announce
    /// its curve over USB. It is whatever the operator said, and
    /// [`CaptureEq::Unknown`] where they said nothing.
    pub eq: CaptureEq,
}

impl CaptureInfo {
    /// A minimal record for a capture whose format nobody has verified.
    ///
    /// Deliberately the only constructor that does not take a verification
    /// result: making the unverified case the explicit, named one keeps
    /// `os_verified: true` from ever being reached by a struct literal filled in
    /// on autopilot.
    pub const fn unverified(
        rate: SampleRate,
        channels: u16,
        storage_format: StorageFormat,
        capture_mode: CaptureMode,
    ) -> Self {
        Self {
            rate,
            channels,
            storage_format,
            capture_mode,
            host_api: None,
            device_id: None,
            device_name: None,
            os_verified: false,
            os_report: None,
            // Unverified provenance is unknown provenance, which is the same rule
            // `os_verified: false` follows one line up.
            eq: CaptureEq::Unknown,
        }
    }

    /// The same record with the operator's stated equalisation on it.
    ///
    /// Separate from [`CaptureInfo::unverified`] so that stating the provenance is
    /// a visible act at the call site. §51 wants it stated rather than defaulted,
    /// and a fifth positional argument of an enum whose common value is `Unknown`
    /// is how it would end up defaulted in practice.
    #[must_use]
    pub const fn with_eq(mut self, eq: CaptureEq) -> Self {
        self.eq = eq;
        self
    }

    /// Bytes one frame occupies: one sample per channel, at the storage width.
    pub const fn frame_bytes(&self) -> usize {
        self.storage_format.bytes_per_sample() * self.channels as usize
    }
}

/// The reading end of a live PCM stream, as the writer sees it.
///
/// This exists to keep a dependency from being created. `vcw-audio` owns the
/// ring buffer and `vcw-project` owns the writer that drains it, and neither
/// crate depends on the other - both sit on this one. Declaring the trait here
/// lets `vcw-audio` implement it for its own ring and `vcw-project` consume it
/// without either learning about SQLite or ALSA respectively.
///
/// The other thing it buys is that the writer can be driven by anything: a file,
/// a generator, a test fixture holding a `Vec<u8>`. WP-05's soak and WP-06's
/// kill-at-random-point suite both need that, because neither can rely on a
/// sound card being present.
pub trait PcmSource: Send {
    /// Takes up to `dst.len()` bytes and returns how many were taken.
    ///
    /// Never blocks. Zero means "nothing ready *now*", which is not the same as
    /// end of stream - see [`PcmSource::is_finished`]. A caller that treats zero
    /// as the end will truncate every capture that has a quiet moment.
    ///
    /// Implementations must return whole samples' worth where they can; the
    /// writer reassembles frames from the byte stream and a partial sample would
    /// shift every channel after it.
    fn read(&mut self, dst: &mut [u8]) -> usize;

    /// Whether the producer has gone for good.
    ///
    /// Once this is true *and* [`PcmSource::read`] returns zero, nothing more
    /// will ever arrive. Both conditions matter: a producer can finish with data
    /// still in flight, and dropping it would lose the end of the side.
    fn is_finished(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SampleFormat;

    #[test]
    fn states_round_trip_and_reject_nonsense() {
        for s in [
            CaptureState::Recording,
            CaptureState::Finalised,
            CaptureState::Interrupted,
            CaptureState::Recovered,
        ] {
            assert_eq!(CaptureState::parse(s.as_str()), Some(s));
        }
        assert_eq!(CaptureState::parse("Recording"), None);
        assert_eq!(CaptureState::parse("done"), None);
    }

    #[test]
    fn only_recording_is_unfinished() {
        assert!(CaptureState::Recording.is_unfinished());
        assert!(!CaptureState::Finalised.is_unfinished());
        assert!(!CaptureState::Interrupted.is_unfinished());
    }

    #[test]
    fn a_fresh_diagnostics_is_clean_and_any_counter_spoils_it() {
        assert!(Diagnostics::default().is_clean());
        for spoil in [
            Diagnostics {
                overruns: 1,
                ..Default::default()
            },
            Diagnostics {
                underruns: 1,
                ..Default::default()
            },
            Diagnostics {
                dropped_frames: 1,
                ..Default::default()
            },
            Diagnostics {
                stream_errors: 1,
                ..Default::default()
            },
        ] {
            assert!(!spoil.is_clean(), "{spoil:?} should not be clean");
        }
    }

    #[test]
    fn counters_saturate_rather_than_wrapping_to_a_clean_looking_zero() {
        let full = Diagnostics {
            overruns: u64::MAX,
            underruns: u64::MAX,
            dropped_frames: u64::MAX,
            stream_errors: u64::MAX,
        };
        let sum = full.saturating_add(Diagnostics {
            overruns: 9,
            ..Default::default()
        });
        assert_eq!(sum.overruns, u64::MAX);
        assert!(!sum.is_clean());
    }

    #[test]
    fn frame_bytes_follows_the_storage_width_not_the_logical_one() {
        // 24-bit is the case that differs: 3 bytes packed, 4 padded.
        let packed = CaptureInfo::unverified(
            SampleRate(96_000),
            2,
            StorageFormat::native_for(SampleFormat::S24),
            CaptureMode::Exclusive,
        );
        assert_eq!(packed.frame_bytes(), 6);
        let padded = CaptureInfo {
            storage_format: StorageFormat::Int24Padded,
            ..packed
        };
        assert_eq!(padded.frame_bytes(), 8);
    }

    #[test]
    fn the_unverified_constructor_leaves_no_claim_behind() {
        let info = CaptureInfo::unverified(
            SampleRate(192_000),
            2,
            StorageFormat::Int32,
            CaptureMode::Shared,
        );
        assert!(!info.os_verified);
        assert!(info.os_report.is_none());
    }

    /// A source backed by a slice, which is all a test usually needs.
    struct Canned {
        bytes: Vec<u8>,
        at: usize,
        done: bool,
    }

    impl PcmSource for Canned {
        fn read(&mut self, dst: &mut [u8]) -> usize {
            let n = dst.len().min(self.bytes.len() - self.at);
            dst[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
            self.at += n;
            n
        }
        fn is_finished(&self) -> bool {
            self.done
        }
    }

    #[test]
    fn a_source_that_is_empty_now_is_not_a_source_that_has_finished() {
        // The distinction the whole writer loop turns on. Treating a quiet
        // moment as the end of the stream would truncate the side.
        let mut source = Canned {
            bytes: vec![1, 2, 3, 4],
            at: 0,
            done: false,
        };
        let mut buffer = [0u8; 8];
        assert_eq!(source.read(&mut buffer), 4);
        assert_eq!(source.read(&mut buffer), 0);
        assert!(!source.is_finished(), "drained is not finished");
        source.done = true;
        assert!(source.is_finished());
    }
}
