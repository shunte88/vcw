/*
 *  verify.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Asking the operating system what the hardware is really doing (§9).
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

//! Asking the operating system what the hardware is really doing (§9).
//!
//! §9 says the application must never claim bit-perfect operation solely because
//! CPAL is in use, and S1 finding 1 is why that sentence exists: CPAL 0.16
//! reported an honored 48 kHz stereo I32 request while the hardware ran 8 kHz
//! mono I16. The bug is fixed, the class of bug is not. A backend reports what it
//! asked for; only the kernel knows what it got.
//!
//! So bit-perfection is a claim with evidence attached or it is not made.
//! [`Verification`] has three outcomes and only one of them supports a claim:
//!
//! - [`Verification::Agrees`] - the OS was asked, answered, and matches.
//! - [`Verification::Disagrees`] - the OS was asked and something differs. The
//!   capture is *not* bit-perfect, and the divergence is named.
//! - [`Verification::Unavailable`] - nobody was asked, or the answer could not be
//!   read. This is not a pass. It is the honest absence of a check, and it reads
//!   as such everywhere it surfaces.
//!
//! Linux is implemented here: while a PCM is open, ALSA publishes the negotiated
//! hardware parameters under `/proc/asound`. Windows and macOS return
//! `Unavailable` with a reason rather than pretending. WASAPI's exclusive-mode
//! format is the Windows equivalent and is the obvious next one to write.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::devices::{DeviceKey, Direction};

/// What the operating system reported about an open stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsReport {
    /// Where the answer came from, so it can be gone and looked at.
    pub source: String,
    /// The OS's own name for the sample format: `S32_LE`, `S24_3LE`.
    pub format: Option<String>,
    /// Sample rate in Hz.
    pub rate: Option<u32>,
    /// Channel count.
    pub channels: Option<u16>,
    /// Frames per hardware period.
    pub period_frames: Option<u32>,
    /// Frames in the hardware ring.
    pub buffer_frames: Option<u32>,
    /// Interleaving and mapping, e.g. `RW_INTERLEAVED`.
    pub access: Option<String>,
    /// The whole answer, unedited, because a summary of evidence is not evidence.
    pub raw: String,
}

impl fmt::Display for OsReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.source)?;
        match (&self.format, self.rate, self.channels) {
            (Some(fmt_), Some(rate), Some(ch)) => write!(f, "{fmt_} {rate} Hz {ch} ch"),
            _ => write!(f, "incomplete"),
        }
    }
}

/// The configuration the backend says it negotiated, which is the claim under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expected {
    /// Sample rate in Hz.
    pub rate: u32,
    /// Channel count.
    pub channels: u16,
    /// The sample format CPAL built the stream with.
    pub format: cpal::SampleFormat,
}

/// The result of asking. Only [`Verification::Agrees`] supports a bit-perfect claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// The OS answered and every field it reported matches what was expected.
    Agrees(Box<OsReport>),
    /// The OS answered and something differs. Each divergence is spelled out.
    Disagrees {
        /// What the OS said.
        report: Box<OsReport>,
        /// One line per field that does not match, expected against actual.
        divergences: Vec<String>,
    },
    /// No check happened. Not a pass.
    Unavailable {
        /// Why, in terms a user can act on.
        why: String,
    },
}

impl Verification {
    /// Whether this result supports a bit-perfect claim. False for anything but
    /// a positive answer from the operating system.
    pub const fn confirms(&self) -> bool {
        matches!(self, Self::Agrees(_))
    }

    /// Whether the OS actively contradicted the backend.
    pub const fn refutes(&self) -> bool {
        matches!(self, Self::Disagrees { .. })
    }

    /// Whether a check happened at all, either way.
    pub const fn was_checked(&self) -> bool {
        !matches!(self, Self::Unavailable { .. })
    }

    /// The report, where there is one.
    pub fn report(&self) -> Option<&OsReport> {
        match self {
            Self::Agrees(r) => Some(r),
            Self::Disagrees { report, .. } => Some(report),
            Self::Unavailable { .. } => None,
        }
    }

    /// A one-line audit trail for the `os_report` column: what was checked,
    /// against what, and what came back.
    pub fn evidence(&self) -> String {
        match self {
            Self::Agrees(r) => format!("confirmed by {r}"),
            Self::Disagrees {
                report,
                divergences,
            } => {
                format!("contradicted by {report}: {}", divergences.join("; "))
            }
            Self::Unavailable { why } => format!("not checked: {why}"),
        }
    }
}

/// Asks the operating system what an open stream is actually running.
///
/// Call it *while the stream is open*: on Linux a closed PCM reports the literal
/// string `closed`, which is the only honest thing it could say and no use to us.
/// ALSA also needs a moment after `play()` before the parameters appear, so the
/// caller is responsible for not asking too early.
pub fn against(key: &DeviceKey, direction: Direction, expected: Expected) -> Verification {
    match key.host() {
        "alsa" => alsa(key, direction, expected),
        host => Verification::Unavailable {
            why: format!(
                "no format verifier for the {host} host yet, so bit-perfect operation \
                 cannot be confirmed on this platform"
            ),
        },
    }
}

/// Linux: read the negotiated parameters the kernel publishes for an open PCM.
fn alsa(key: &DeviceKey, direction: Direction, expected: Expected) -> Verification {
    let Some(path) = alsa_hw_params_path(key.id(), direction) else {
        return Verification::Unavailable {
            why: format!(
                "cannot work out which PCM {:?} is, so there is nothing to read",
                key.id()
            ),
        };
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) => {
            return Verification::Unavailable {
                why: format!("{} could not be read: {e}", path.display()),
            };
        }
    };
    if raw.trim() == "closed" || raw.trim().is_empty() {
        return Verification::Unavailable {
            why: format!(
                "{} reports the PCM is closed - the stream must be running when the \
                 check is made",
                path.display()
            ),
        };
    }
    compare(parse(path.display().to_string(), &raw), expected)
}

/// Resolves an ALSA PCM id to the kernel's `hw_params` file.
///
/// `hw:CARD=0,DEV=0` becomes `/proc/asound/card0/pcm0c/sub0/hw_params`, and
/// `hw:CARD=PCH,DEV=0` becomes `/proc/asound/PCH/pcm0c/sub0/hw_params` - the
/// kernel keeps a symlink under the card's id, so both spellings CPAL enumerates
/// resolve without a lookup table.
///
/// Only `hw:` ids resolve. A `plughw:` or `default` stream is running through a
/// converter by definition, and pointing the verifier at the hardware underneath
/// it would confirm a format the application never received.
fn alsa_hw_params_path(id: &str, direction: Direction) -> Option<PathBuf> {
    let rest = id.strip_prefix("hw:")?;
    let mut card = None;
    let mut dev = "0";
    for field in rest.split(',') {
        match field.split_once('=') {
            Some(("CARD", v)) => card = Some(v),
            Some(("DEV", v)) => dev = v,
            _ => {}
        }
    }
    let card = card?;
    if card.is_empty()
        || card.contains('/')
        || dev.contains('/')
        || !dev.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    // A bare number is a card index; anything else is the card's id, which the
    // kernel exposes as a symlink of the same name.
    let dir = if card.bytes().all(|b| b.is_ascii_digit()) {
        format!("card{card}")
    } else {
        card.to_owned()
    };
    let suffix = match direction {
        Direction::Input => 'c',
        Direction::Output => 'p',
    };
    let pcm = Path::new("/proc/asound")
        .join(dir)
        .join(format!("pcm{dev}{suffix}"));
    // Substream 0 unless the kernel only made others; ordinary captures use sub0.
    for sub in 0..4 {
        let candidate = pcm.join(format!("sub{sub}")).join("hw_params");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Parses ALSA's `hw_params`, which is `key: value` a line at a time.
fn parse(source: String, raw: &str) -> OsReport {
    let field = |key: &str| -> Option<String> {
        raw.lines()
            .find_map(|l| l.trim().strip_prefix(key)?.strip_prefix(':'))
            .map(|v| v.trim().to_owned())
    };
    OsReport {
        format: field("format"),
        // "rate: 96000 (96000/1)" - the leading integer is the one that matters.
        rate: field("rate").and_then(|v| v.split_whitespace().next()?.parse().ok()),
        channels: field("channels").and_then(|v| v.parse().ok()),
        period_frames: field("period_size").and_then(|v| v.parse().ok()),
        buffer_frames: field("buffer_size").and_then(|v| v.parse().ok()),
        access: field("access"),
        raw: raw.trim().to_owned(),
        source,
    }
}

/// Compares a report against the claim, field by field.
///
/// A field the OS did not report is not a match and not a mismatch. It counts as
/// a gap in the evidence, and a report with gaps in the fields that matter cannot
/// confirm anything.
fn compare(report: OsReport, expected: Expected) -> Verification {
    let mut divergences = Vec::new();
    let mut checked = 0;

    match report.rate {
        Some(rate) if rate == expected.rate => checked += 1,
        Some(rate) => divergences.push(format!(
            "rate: asked {} Hz, hardware {rate} Hz",
            expected.rate
        )),
        None => {}
    }
    match report.channels {
        Some(ch) if ch == expected.channels => checked += 1,
        Some(ch) => divergences.push(format!(
            "channels: asked {}, hardware {ch}",
            expected.channels
        )),
        None => {}
    }
    match report
        .format
        .as_deref()
        .map(|f| (f, alsa_format_matches(f, expected.format)))
    {
        Some((_, Some(true))) => checked += 1,
        Some((name, Some(false))) => divergences.push(format!(
            "format: asked {:?}, hardware {name}",
            expected.format
        )),
        // A format name we have no mapping for cannot be said to agree or differ.
        Some((name, None)) => divergences.push(format!(
            "format: hardware reports {name}, which does not map to any format we know, \
             so agreement cannot be established"
        )),
        None => {}
    }

    if !divergences.is_empty() {
        return Verification::Disagrees {
            report: Box::new(report),
            divergences,
        };
    }
    if checked < 3 {
        return Verification::Unavailable {
            why: format!(
                "{} did not report rate, channels and format together, so there is \
                 nothing conclusive to compare",
                report.source
            ),
        };
    }
    Verification::Agrees(Box::new(report))
}

/// ALSA's format names against CPAL's.
///
/// `None` where we have no mapping, which is not the same as a mismatch: an
/// unknown name means we cannot establish agreement, and saying so is the point.
/// `I24` maps to both `S24_LE` and `S24_3LE` because CPAL does not distinguish
/// the padded and packed layouts that ALSA names separately.
#[must_use]
pub fn alsa_format_matches(alsa: &str, format: cpal::SampleFormat) -> Option<bool> {
    use cpal::SampleFormat as F;
    let expected: &[&str] = match format {
        F::I8 => &["S8"],
        F::U8 => &["U8"],
        F::I16 => &["S16_LE"],
        F::U16 => &["U16_LE"],
        F::I24 => &["S24_LE", "S24_3LE"],
        F::I32 => &["S32_LE"],
        F::F32 => &["FLOAT_LE"],
        F::F64 => &["FLOAT64_LE"],
        _ => return None,
    };
    Some(expected.contains(&alsa))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "access: RW_INTERLEAVED\nformat: S32_LE\nsubformat: STD\n\
                        channels: 2\nrate: 96000 (96000/1)\nperiod_size: 1024\n\
                        buffer_size: 4096";

    fn expected() -> Expected {
        Expected {
            rate: 96_000,
            channels: 2,
            format: cpal::SampleFormat::I32,
        }
    }

    #[test]
    fn a_matching_report_confirms_and_carries_its_evidence() {
        let v = compare(parse("p".into(), GOOD), expected());
        assert!(v.confirms());
        assert!(!v.refutes());
        assert!(v.was_checked());
        assert_eq!(v.report().unwrap().period_frames, Some(1024));
        assert!(v.evidence().contains("S32_LE 96000 Hz 2 ch"));
    }

    #[test]
    fn s1_finding_1_is_exactly_what_this_catches() {
        // The backend claimed 48 kHz stereo I32; the hardware ran 8 kHz mono I16.
        let hardware = "access: RW_INTERLEAVED\nformat: S16_LE\nchannels: 1\nrate: 8000 (8000/1)";
        let v = compare(
            parse("p".into(), hardware),
            Expected {
                rate: 48_000,
                channels: 2,
                format: cpal::SampleFormat::I32,
            },
        );
        assert!(v.refutes());
        assert!(!v.confirms());
        let Verification::Disagrees { divergences, .. } = &v else {
            unreachable!()
        };
        assert_eq!(divergences.len(), 3, "rate, channels and format all differ");
        assert!(v.evidence().contains("8000 Hz"));
    }

    #[test]
    fn an_incomplete_report_confirms_nothing() {
        // Rate agrees, but nothing else was reported. One matching field out of
        // three is not evidence of a bit-perfect path.
        let v = compare(parse("p".into(), "rate: 96000 (96000/1)"), expected());
        assert!(!v.confirms());
        assert!(!v.was_checked());
        assert!(v.evidence().starts_with("not checked"));
    }

    #[test]
    fn a_format_name_we_do_not_know_is_a_gap_and_never_a_pass() {
        let odd = "format: S20_3LE\nchannels: 2\nrate: 96000 (96000/1)";
        let v = compare(parse("p".into(), odd), expected());
        assert!(!v.confirms());
        assert!(v.refutes(), "an unmappable name is reported, not swallowed");
        assert!(v.evidence().contains("S20_3LE"));
    }

    #[test]
    fn cpals_i24_accepts_both_of_alsas_two_spellings() {
        use cpal::SampleFormat as F;
        assert_eq!(alsa_format_matches("S24_LE", F::I24), Some(true));
        assert_eq!(alsa_format_matches("S24_3LE", F::I24), Some(true));
        assert_eq!(alsa_format_matches("S32_LE", F::I24), Some(false));
        assert_eq!(alsa_format_matches("MU_LAW", F::I24), Some(false));
    }

    #[test]
    fn both_spellings_of_a_card_resolve_to_a_path() {
        let by_index = alsa_hw_params_path("hw:CARD=0,DEV=0", Direction::Input);
        let by_name = alsa_hw_params_path("hw:CARD=PCH,DEV=0", Direction::Input);
        // Whether they exist depends on the machine; the shape must be right
        // wherever they do.
        for p in [by_index, by_name].into_iter().flatten() {
            let s = p.display().to_string();
            assert!(s.starts_with("/proc/asound/"), "{s}");
            assert!(s.ends_with("/hw_params"), "{s}");
            assert!(s.contains("pcm0c"), "capture PCMs end in c: {s}");
        }
    }

    #[test]
    fn direction_picks_the_capture_or_playback_pcm() {
        // Only meaningful where the device exists, which on a CI runner it may
        // not; the assertion is on the spelling, not on presence.
        if let Some(p) = alsa_hw_params_path("hw:CARD=0,DEV=0", Direction::Output) {
            assert!(p.display().to_string().contains("pcm0p"));
        }
    }

    #[test]
    fn a_converting_path_is_never_verified_against_the_hardware_beneath_it() {
        // plughw: and default: are conversions by definition. Confirming the
        // hardware format under a converter would confirm a format the
        // application never received, which is worse than not checking.
        for id in ["plughw:CARD=0,DEV=0", "default", "pipewire", "pulse"] {
            assert!(
                alsa_hw_params_path(id, Direction::Input).is_none(),
                "{id} must not resolve"
            );
        }
    }

    #[test]
    fn a_malicious_id_cannot_walk_out_of_proc_asound() {
        for id in [
            "hw:CARD=../../etc,DEV=0",
            "hw:CARD=0,DEV=../0",
            "hw:CARD=,DEV=0",
        ] {
            assert!(alsa_hw_params_path(id, Direction::Input).is_none(), "{id}");
        }
    }

    #[test]
    fn an_unknown_host_says_so_rather_than_passing() {
        let v = against(
            &DeviceKey::new("wasapi", "some-device"),
            Direction::Input,
            expected(),
        );
        assert!(!v.confirms());
        assert!(!v.was_checked());
        assert!(v.evidence().contains("wasapi"));
    }
}
