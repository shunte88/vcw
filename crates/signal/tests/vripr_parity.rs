/*
 *  vripr_parity.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A/B parity against VRipr on the vripr_training corpus (22, WP-11's exit criterion).
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

//! A/B parity against VRipr on the `vripr_training` corpus (§22).
//!
//! WP-11's exit criterion is parity with VRipr, and this is the measurement. It
//! is ignored by default because it reads 294 MB from a path outside the repo:
//! run it with `cargo test -p vcw-signal --test vripr_parity -- --ignored
//! --nocapture` and read the table it prints.
//!
//! # What the labels are
//!
//! `/data2/vripr_training` is 595 snippets VRipr itself cut, one per boundary in
//! its own track tables, by `src/workers/training_samples.rs`: 16 s of mono
//! 16 kHz audio centered on the boundary, peak-normalized, with a JSON sidecar
//! naming the kind. So a label is **where VRipr put a boundary**, not where a
//! boundary provably is. That is the right reference for a parity criterion and
//! the wrong one for an accuracy claim, and the difference matters: a snippet
//! VCW misses is a disagreement with VRipr, and nothing here can say which of
//! the two was right.
//!
//! # What parity is measured against
//!
//! Not the labels. `tests/fixtures/vripr_answers.jsonl` is VRipr's own detectors
//! run over the same 595 snippets under the same two configurations, produced
//! out of tree by `/data2/vcw-scratch/parity` - a scratch crate holding a
//! verbatim copy of `/data2/vripr/src/audio/mod.rs`, so nothing in this repo
//! duplicates VRipr's algorithm and nothing in that copy was improved on the
//! way past. Parity is then boundary for boundary: for every boundary VRipr
//! placed, did VCW place one of the same kind within half a second, and did VCW
//! place any VRipr did not.
//!
//! The fixture is checked in because it is the reference of record. The corpus
//! it was computed from is 294 MB and is not, so on any other machine this test
//! prints that and passes.
//!
//! `kind: mid` is the negative case - the center of a track at least 60 s long,
//! so the whole window is clear of both its boundaries. A boundary found at 8 s
//! in one of those is a false positive on VRipr's own reading.
//!
//! # Why agreement with the labels is low, and why that is not this port's doing
//!
//! It is around a sixth of the labels for the level detector and a third for the
//! HMM. Two reasons, and neither is the port. The corpus is dominated by ambient
//! and drone records whose tracks segue with no silence between them at all, so
//! there is nothing for a level or a flatness detector to find; and the labels
//! are the track table, which for those records came from a release listing or
//! from a person, not from the detector being measured. The `.onnx` file sitting
//! in the corpus directory is the rest of the story: VRipr was training a
//! learned boundary detector on this material precisely because its classical
//! ones could not do it.
//!
//! So VRipr's own answers are scored against the labels beside VCW's, by the
//! same rule. On 2026-09-26 that came out at 16.3% against VCW's 17.1% for the
//! level detector, 10.8% against 11.3% for the spectral one and 30.7% against
//! 30.9% for the HMM: the same wall, a hair on VCW's side of it. A figure here
//! that drops below VRipr's is a regression in the port. A figure that rises
//! well above it would mean this has stopped being a parity test.
//!
//! # What the corpus does to the audio
//!
//! Peak normalization is the part that bites. A quiet side is lifted until its
//! loudest transient is full scale, so the *absolute* level of a run-out groove
//! in a snippet is not the level it had on the record, and a fixed -40 dBFS
//! threshold is being asked a different question here than it is asked of a real
//! capture. That is why the adaptive threshold is measured beside it: §22's
//! adaptive mode exists for exactly this case, and the corpus is the only place
//! we can see the two side by side over 595 sides' worth of material.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use vcw_signal::features::{Frame, Shape, Windows};
use vcw_signal::regions::{Config, Trace};
use vcw_signal::resolve::{Decision, Tolerance, resolve};
use vcw_signal::{hmm, silence, spectral};
use vcw_types::{BoundaryObservation, Edge, Provenance, SampleRate, StorageFormat};

/// Where the corpus lives. Absent on any machine but the author's, and the test
/// says so and passes rather than failing for a missing directory.
const CORPUS: &str = "/data2/vripr_training";

/// How far from the labeled position a boundary may be and still count as the
/// same boundary. The resolver's own clustering tolerance, which is the tightest
/// figure that can be defended: a detector that agrees to within half a second
/// agrees.
const NEAR_SECS: f64 = 0.5;

/// A looser reading, reported beside it. Two analysis windows and the padding.
const LOOSE_SECS: f64 = 1.0;

/// The floor each detector has to clear against VRipr, as a fraction of VRipr's
/// own boundaries reproduced.
///
/// Measured on 2026-09-26: 99.52% for the level detector, 99.71% for the
/// spectral one and 98.01% for the HMM with a fixed threshold, and 97.63%,
/// 98.71% and 98.01% with an adaptive one. The floor sits below the worst of
/// those with room for a rebuild of the reference, and not so far below that a
/// real regression could hide under it.
const PARITY_FLOOR: f64 = 0.97;

/// The floor for snippets where every boundary matched, both ways.
///
/// Measured the same day: 93.95% to 98.66% depending on the detector and the
/// threshold. This is the harsher figure, because one disagreement anywhere in a
/// snippet fails the whole snippet.
const IDENTICAL_FLOOR: f64 = 0.90;

/// One snippet: the audio, and what VRipr said about it.
struct Snippet {
    stem: String,
    kind: String,
    at: f64,
    rate: u32,
    pcm: Vec<u8>,
}

/// The little of the JSON sidecar this needs.
fn label(path: &Path) -> Option<(String, f64, u32)> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some((
        value.get("kind")?.as_str()?.to_string(),
        value.get("boundary_at_secs")?.as_f64()?,
        u32::try_from(value.get("sample_rate")?.as_u64()?).ok()?,
    ))
}

/// Reads a 16-bit PCM WAV and returns its data chunk and its rate.
///
/// Hand-rolled rather than pulled from a crate, because the corpus is one
/// format written by one writer and a dependency added for a test is a
/// dependency all the same. It walks the chunk list rather than assuming the
/// canonical 44-byte header, and refuses anything that is not the format it
/// claims to handle instead of reading it as noise.
fn wav(path: &Path) -> Option<(Vec<u8>, u32, u16)> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);

    let mut at = 12;
    let mut rate = 0;
    let mut channels = 0;
    let mut bits = 0;
    let mut data: Option<Vec<u8>> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32_at(at + 4) as usize;
        let body = at + 8;
        if body + len > bytes.len() {
            break;
        }
        match id {
            b"fmt " if len >= 16 => {
                channels = u16_at(body + 2);
                rate = u32_at(body + 4);
                bits = u16_at(body + 14);
            }
            b"data" => data = Some(bytes[body..body + len].to_vec()),
            _ => {}
        }
        // Chunks are padded to even lengths.
        at = body + len + (len & 1);
    }
    if bits != 16 {
        return None;
    }
    Some((data?, rate, channels))
}

/// Loads the corpus, or returns an empty list if it is not on this machine.
fn corpus() -> Vec<Snippet> {
    let dir = PathBuf::from(CORPUS);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut stems: Vec<String> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "wav").then(|| path.file_stem()?.to_str().map(String::from))?
        })
        .collect();
    stems.sort();

    let mut out = Vec::with_capacity(stems.len());
    for stem in stems {
        let Some((kind, at, rate)) = label(&dir.join(format!("{stem}.json"))) else {
            continue;
        };
        let Some((pcm, wav_rate, channels)) = wav(&dir.join(format!("{stem}.wav"))) else {
            continue;
        };
        assert_eq!(wav_rate, rate, "{stem}: the sidecar and the WAV disagree");
        assert_eq!(channels, 1, "{stem}: the corpus is mono");
        out.push(Snippet {
            stem,
            kind,
            at,
            rate,
            pcm,
        });
    }
    out
}

/// The frames of one snippet, extracted exactly as a capture's would be.
fn frames_of(snippet: &Snippet) -> (Vec<Frame>, Windows, Shape) {
    let shape = Shape::at_default_window(SampleRate(snippet.rate), 1, StorageFormat::Int16);
    let mut windows = Windows::spectral(&shape);
    let mut frames = Vec::new();
    windows.push(&snippet.pcm, &mut frames);
    windows.flush(&mut frames);
    (frames, windows, shape)
}

/// What one detector, or the resolver, did over the whole corpus.
#[derive(Default)]
struct Tally {
    /// Snippets of a boundary kind where a boundary of that kind was found near
    /// the label.
    hit: usize,
    /// The same, at [`LOOSE_SECS`].
    loose: usize,
    /// Snippets of a boundary kind where nothing was found near the label.
    missed: usize,
    /// `mid` snippets with a boundary near the label anyway.
    false_positive: usize,
    /// `mid` snippets left alone.
    clean: usize,
    /// Distance from the label, for the hits, in seconds.
    error: Vec<f64>,
    /// Confidence, for the hits.
    confidence: Vec<f64>,
}

impl Tally {
    fn note(&mut self, snippet: &Snippet, found: &[(u64, Edge, f32)], rate: f64) {
        let wanted = match snippet.kind.as_str() {
            "start" => Some(Edge::Start),
            "end" => Some(Edge::End),
            _ => None,
        };
        let near = |limit: f64, edge: Option<Edge>| {
            found
                .iter()
                .filter(|(_, found_edge, _)| edge.is_none_or(|want| *found_edge == want))
                .map(|(at, _, confidence)| (*at as f64 / rate - snippet.at, *confidence))
                .filter(|(gap, _)| gap.abs() <= limit)
                .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
        };

        match wanted {
            Some(edge) => match near(NEAR_SECS, Some(edge)) {
                Some((gap, confidence)) => {
                    self.hit += 1;
                    self.loose += 1;
                    self.error.push(gap);
                    self.confidence.push(f64::from(confidence));
                }
                None => {
                    self.missed += 1;
                    if near(LOOSE_SECS, Some(edge)).is_some() {
                        self.loose += 1;
                    }
                }
            },
            // Either edge counts against a mid-track window: the claim being
            // tested is that nothing happens there, not that the wrong kind of
            // something happens there.
            None => {
                if near(NEAR_SECS, None).is_some() {
                    self.false_positive += 1;
                } else {
                    self.clean += 1;
                }
            }
        }
    }

    fn agreement(&self) -> f64 {
        let seen = self.hit + self.missed;
        if seen == 0 {
            return 0.0;
        }
        self.hit as f64 / seen as f64
    }

    fn line(&self, name: &str) -> String {
        let mid = self.false_positive + self.clean;
        let mean = |values: &[f64]| {
            if values.is_empty() {
                0.0
            } else {
                values.iter().sum::<f64>() / values.len() as f64
            }
        };
        let spread = |values: &[f64]| {
            let mut sorted: Vec<f64> = values.iter().map(|value| value.abs()).collect();
            sorted.sort_by(f64::total_cmp);
            sorted.get(sorted.len() / 2).copied().unwrap_or_default()
        };
        format!(
            "{name:<16} {:>4}/{:<4} {:>6.1}%   loose {:>5.1}%   mid clean {:>4}/{:<4} \
             {:>5.1}%   median err {:>5.3} s   bias {:>+6.3} s   conf {:>4.2}",
            self.hit,
            self.hit + self.missed,
            self.agreement() * 100.0,
            if self.hit + self.missed == 0 {
                0.0
            } else {
                self.loose as f64 / (self.hit + self.missed) as f64 * 100.0
            },
            self.clean,
            mid,
            if mid == 0 {
                0.0
            } else {
                self.clean as f64 / mid as f64 * 100.0
            },
            spread(&self.error),
            mean(&self.error),
            mean(&self.confidence),
        )
    }
}

/// VRipr's answers, as `(stem, config) -> detector -> track spans in seconds`.
type Reference = BTreeMap<(String, String), BTreeMap<String, Vec<(f64, f64)>>>;

/// Loads the checked-in reference.
fn reference() -> Reference {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vripr_answers.jsonl");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Reference::new();
    };
    let mut out = Reference::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).expect("the reference is JSON");
        let stem = value["stem"].as_str().expect("stem").to_string();
        let config = value["config"].as_str().expect("config").to_string();
        let mut detectors = BTreeMap::new();
        for detector in ["silence", "spectral-change", "hmm"] {
            let spans = value[detector]
                .as_array()
                .map(|spans| {
                    spans
                        .iter()
                        .filter_map(|span| Some((span[0].as_f64()?, span[1].as_f64()?)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            detectors.insert(detector.to_string(), spans);
        }
        out.insert((stem, config), detectors);
    }
    out
}

/// VRipr's track spans as boundaries, in the order VCW emits them.
fn as_boundaries(spans: &[(f64, f64)]) -> Vec<(f64, Edge)> {
    let mut out = Vec::with_capacity(spans.len() * 2);
    for &(start, end) in spans {
        out.push((start, Edge::Start));
        out.push((end, Edge::End));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// How closely one VCW detector tracked the VRipr detector it was ported from.
#[derive(Default)]
struct Parity {
    /// VRipr boundaries VCW also placed, of the same kind, within [`NEAR_SECS`].
    agreed: usize,
    /// VRipr boundaries VCW did not place.
    missing: usize,
    /// VCW boundaries VRipr did not place.
    extra: usize,
    /// Signed offset of every agreement, VCW minus VRipr, in seconds.
    offset: Vec<f64>,
    /// Snippets where every boundary matched both ways.
    identical: usize,
    /// Snippets compared.
    snippets: usize,
}

impl Parity {
    fn note(&mut self, mine: &[(u64, Edge, f32)], theirs: &[(f64, Edge)], rate: f64) {
        self.snippets += 1;
        let ours: Vec<(f64, Edge)> = mine
            .iter()
            .map(|(at, edge, _)| (*at as f64 / rate, *edge))
            .collect();
        let nearest = |from: &[(f64, Edge)], at: f64, edge: Edge| {
            from.iter()
                .filter(|(_, other)| *other == edge)
                .map(|(other, _)| other - at)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))
                .filter(|gap| gap.abs() <= NEAR_SECS)
        };

        let before = (self.missing, self.extra);
        for &(at, edge) in theirs {
            match nearest(&ours, at, edge) {
                Some(gap) => {
                    self.agreed += 1;
                    self.offset.push(gap);
                }
                None => self.missing += 1,
            }
        }
        for &(at, edge) in &ours {
            if nearest(theirs, at, edge).is_none() {
                self.extra += 1;
            }
        }
        if (self.missing, self.extra) == before {
            self.identical += 1;
        }
    }

    /// The fraction of VRipr's boundaries reproduced.
    fn rate(&self) -> f64 {
        let theirs = self.agreed + self.missing;
        if theirs == 0 {
            return 0.0;
        }
        self.agreed as f64 / theirs as f64
    }

    /// The fraction of snippets where nothing differed in either direction.
    fn identical(&self) -> f64 {
        if self.snippets == 0 {
            return 0.0;
        }
        self.identical as f64 / self.snippets as f64
    }

    /// The largest distance between two boundaries counted as agreeing.
    fn worst(&self) -> f64 {
        self.offset
            .iter()
            .map(|gap| gap.abs())
            .max_by(f64::total_cmp)
            .unwrap_or_default()
    }

    fn line(&self, name: &str) -> String {
        let theirs = self.agreed + self.missing;
        let mut sorted: Vec<f64> = self.offset.iter().map(|gap| gap.abs()).collect();
        sorted.sort_by(f64::total_cmp);
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or_default();
        let worst = sorted.last().copied().unwrap_or_default();
        format!(
            "{name:<16} {:>5}/{:<5} {:>6.2}% of VRipr's boundaries   extra {:>4}   \
             whole snippets {:>4}/{:<4} {:>6.2}%   median {:>5.3} s   worst {:>5.3} s",
            self.agreed,
            theirs,
            if theirs == 0 {
                0.0
            } else {
                self.agreed as f64 / theirs as f64 * 100.0
            },
            self.extra,
            self.identical,
            self.snippets,
            if self.snippets == 0 {
                0.0
            } else {
                self.identical as f64 / self.snippets as f64 * 100.0
            },
            median,
            worst,
        )
    }
}

/// Runs every detector and the resolver over the corpus under one configuration.
fn sweep(
    snippets: &[Snippet],
    cfg: &Config,
    config_name: &str,
    reference: &Reference,
) -> (BTreeMap<String, Tally>, BTreeMap<String, Parity>) {
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut parity: BTreeMap<String, Parity> = BTreeMap::new();
    for snippet in snippets {
        let (frames, windows, _shape) = frames_of(snippet);
        let trace = Trace::from_windows(&frames, &windows);
        let rate = f64::from(snippet.rate);

        let passes = [
            (Provenance::Silence, silence::scan(&trace, cfg)),
            (Provenance::SpectralChange, spectral::scan(&trace, cfg)),
            (Provenance::Hmm, hmm::scan(&trace, cfg)),
        ];
        let mut every: Vec<BoundaryObservation> = Vec::new();
        for (provenance, outcome) in passes {
            let found: Vec<(u64, Edge, f32)> = outcome
                .boundaries
                .iter()
                .map(|b| (b.at, b.edge, b.confidence))
                .collect();
            tallies
                .entry(provenance.as_str().to_string())
                .or_default()
                .note(snippet, &found, rate);
            if let Some(theirs) = reference
                .get(&(snippet.stem.clone(), config_name.to_string()))
                .and_then(|detectors| detectors.get(provenance.as_str()))
            {
                let theirs = as_boundaries(theirs);
                parity
                    .entry(provenance.as_str().to_string())
                    .or_default()
                    .note(&found, &theirs, rate);
                // VRipr's own answer, scored against the labels by the same rule.
                // Without this the agreement figures below could be read as a
                // verdict on the port; with it they are a verdict on the corpus.
                let as_found: Vec<(u64, Edge, f32)> = theirs
                    .iter()
                    .map(|(at, edge)| ((at * rate) as u64, *edge, 0.0))
                    .collect();
                tallies
                    .entry(format!("vripr {}", provenance.as_str()))
                    .or_default()
                    .note(snippet, &as_found, rate);
            }
            every.extend(outcome.boundaries);
        }

        let decisions: Vec<Decision> =
            resolve(&every, Tolerance::default_at(SampleRate(snippet.rate)));
        let found: Vec<(u64, Edge, f32)> = decisions
            .iter()
            .map(|d| (d.at, d.edge, d.confidence))
            .collect();
        tallies
            .entry("resolved".to_string())
            .or_default()
            .note(snippet, &found, rate);

        // The resolver's other claim: a boundary two detectors saw is worth more
        // than one only the HMM saw. Counted separately, so the claim is
        // measured rather than asserted.
        let seconded: Vec<(u64, Edge, f32)> = decisions
            .iter()
            .filter(|d| d.agreement() > 1)
            .map(|d| (d.at, d.edge, d.confidence))
            .collect();
        tallies
            .entry("resolved (2+)".to_string())
            .or_default()
            .note(snippet, &seconded, rate);
    }
    (tallies, parity)
}

/// WP-11's exit criterion, measured rather than asserted.
#[test]
#[ignore = "reads the 294 MB corpus at /data2/vripr_training"]
fn the_detectors_agree_with_vripr_on_its_own_corpus() {
    let snippets = corpus();
    if snippets.is_empty() {
        eprintln!("{CORPUS} is not on this machine; nothing measured");
        return;
    }

    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for snippet in &snippets {
        *kinds.entry(snippet.kind.as_str()).or_default() += 1;
    }
    eprintln!("corpus: {} snippets, {kinds:?}", snippets.len());

    let reference = reference();
    assert!(!reference.is_empty(), "the reference fixture is missing");

    for (name, cfg) in [("fixed", Config::new()), ("adaptive", Config::adaptive())] {
        let began = Instant::now();
        let (tallies, parity) = sweep(&snippets, &cfg, name, &reference);
        eprintln!(
            "\n{name} threshold, {} snippet(s) in {:.1} s",
            snippets.len(),
            began.elapsed().as_secs_f64()
        );
        eprintln!("  against VRipr:");
        for (detector, measured) in &parity {
            eprintln!("    {}", measured.line(detector));
        }
        for (detector, measured) in &parity {
            assert_eq!(
                measured.snippets,
                snippets.len(),
                "{detector} was not compared over the whole corpus at the {name} threshold"
            );
            assert!(
                measured.rate() >= PARITY_FLOOR,
                "{detector} at the {name} threshold reproduced {:.2}% of VRipr's \
                 boundaries, under the {:.0}% floor",
                measured.rate() * 100.0,
                PARITY_FLOOR * 100.0
            );
            assert!(
                measured.identical() >= IDENTICAL_FLOOR,
                "{detector} at the {name} threshold matched VRipr exactly on {:.2}% of \
                 snippets, under the {:.0}% floor",
                measured.identical() * 100.0,
                IDENTICAL_FLOOR * 100.0
            );
            // Not a tolerance that happens to hold: every boundary the two agree
            // on is at the identical frame, and a non-zero figure here would mean
            // the ported arithmetic had started to drift rather than to differ.
            assert!(
                measured.worst() < 1e-9,
                "{detector} at the {name} threshold agreed with VRipr to within \
                 {:.4} s rather than exactly",
                measured.worst()
            );
        }
        eprintln!("  against the labels:");
        for (detector, tally) in &tallies {
            eprintln!("    {}", tally.line(detector));
        }
        for detector in ["silence", "spectral-change", "hmm"] {
            let mine = tallies[detector].hit;
            let theirs = tallies[&format!("vripr {detector}")].hit;
            // Slack of two snippets, because a single boundary condition can move
            // one either way and the claim is "no worse", not "identical".
            assert!(
                mine + 2 >= theirs,
                "{detector} at the {name} threshold agreed with {mine} of VRipr's own \
                 labels where VRipr itself agreed with {theirs}"
            );
        }
    }
}
