/*
 *  corpus.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Reading every Audacity project in the user's real corpus, and agreeing with the oracle.
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
//! Reading every Audacity project in the user's real corpus, and agreeing with
//! the oracle.
//!
//! These are 30 of the user's own irreplaceable rips, so they are not in the
//! repository and never will be: the smallest is 271 MB. They also cannot be
//! replaced by a fixture we wrote, because a fixture written by this crate's
//! author would only prove that our encoder agrees with our decoder. So the
//! corpus tests are `#[ignore]`d and pointed at the real files by an environment
//! variable:
//!
//! ```text
//! VCW_AUP_CORPUS=/data2/vinyl_rips cargo test -p vcw-import --test corpus -- --ignored --nocapture
//! ```
//!
//! **The variable is required, not optional.** A corpus test that quietly
//! becomes a no-op when the corpus is absent is a test that passes by doing
//! nothing, and this project has been bitten by exactly that.
//!
//! What they check, in two halves:
//!
//! 1. Every file parses with every byte consumed, every block reference
//!    resolves, and the aggregate facts S5 measured still hold - the rate trap
//!    in 22 of 25 projects, 24 of 25 stored as float32, 532 references to 456
//!    blocks in the one clip-split project.
//! 2. The Rust reader and `spikes/aup-format-probe/probe.py` produce the same
//!    counts for every file. The oracle is an independent implementation of the
//!    same clean-room grammar, so a disagreement means one of them is wrong and
//!    neither gets the benefit of the doubt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use vcw_import::{Event, Version, audit, model::Project, read::survey, sniff};

/// Where the real projects are. Required; see the module documentation.
const CORPUS_ENV: &str = "VCW_AUP_CORPUS";
/// What the corpus holds, which is itself a thing worth asserting: a run over
/// half the files would otherwise look like a pass.
const EXPECTED_AUP3: usize = 25;
/// AUP4 conversions of five of the AUP3 projects.
const EXPECTED_AUP4: usize = 5;

/// One file, read by this crate.
struct Summary {
    path: PathBuf,
    version: Version,
    dict_entries: usize,
    doc_bytes: usize,
    events: usize,
    elements: BTreeMap<String, usize>,
    editor_rate_preference: Option<f64>,
    track_rates: Vec<f64>,
    formats: Vec<String>,
    audit: vcw_import::Audit,
    autosave: bool,
    history_generations: Option<u32>,
    labels: usize,
}

fn corpus_dir() -> PathBuf {
    let raw = std::env::var(CORPUS_ENV).unwrap_or_else(|_| {
        panic!(
            "{CORPUS_ENV} is not set. These tests read the real Audacity corpus; \
             run them as\n  {CORPUS_ENV}=/data2/vinyl_rips cargo test -p vcw-import \
             --test corpus -- --ignored"
        )
    });
    let dir = PathBuf::from(raw);
    assert!(
        dir.is_dir(),
        "{CORPUS_ENV} points at {dir:?}, which is not a directory"
    );
    dir
}

/// Every project in the corpus, sorted, so a failure names the same file twice
/// in a row.
fn projects(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read the corpus directory")
        .filter_map(|entry| {
            let path = entry.expect("a corpus directory entry").path();
            let extension = path.extension()?.to_str()?;
            // The extension picks the *candidates*. It never decides the
            // version: `identify` does that from `user_version`, and this test
            // then checks the two agree rather than assuming it.
            (extension == "aup3" || extension == "aup4").then_some(path)
        })
        .collect();
    found.sort();
    found
}

/// Reads one project the way the crate is meant to be used.
fn read(path: &Path) -> Summary {
    let (conn, sniffed) = sniff::open(path).unwrap_or_else(|e| panic!("open {path:?}: {e}"));
    let surveyed = survey(&conn, path).unwrap_or_else(|e| panic!("survey {path:?}: {e}"));
    let project = Project::from_events(&surveyed.document.events)
        .unwrap_or_else(|e| panic!("model {path:?}: {e}"));
    let audited = audit(&conn, &project).unwrap_or_else(|e| panic!("audit {path:?}: {e}"));

    let mut elements: BTreeMap<String, usize> = BTreeMap::new();
    for event in &surveyed.document.events {
        if let Event::Start(name) = event {
            *elements.entry(name.to_string()).or_default() += 1;
        }
    }

    let mut formats: Vec<String> = project
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter().map(|clip| clip.sample_format))
        .map(|format| format!("{format:?}"))
        .collect();
    formats.sort();
    formats.dedup();

    Summary {
        path: path.to_path_buf(),
        version: sniffed.version,
        dict_entries: surveyed.document.dict.len(),
        doc_bytes: surveyed.document.doc_bytes,
        events: surveyed.document.events.len(),
        elements,
        editor_rate_preference: project.editor_rate_preference,
        track_rates: project.tracks.iter().map(|track| track.rate).collect(),
        formats,
        audit: audited,
        autosave: surveyed.autosave.is_some(),
        history_generations: surveyed.history_generations,
        labels: project
            .label_tracks
            .iter()
            .map(|track| track.labels.len())
            .sum(),
    }
}

#[test]
#[ignore = "reads the real corpus; set VCW_AUP_CORPUS and run with --ignored"]
fn every_corpus_project_parses_and_every_block_reference_resolves() {
    let dir = corpus_dir();
    let paths = projects(&dir);
    let summaries: Vec<Summary> = paths.iter().map(|path| read(path)).collect();

    let aup3 = summaries
        .iter()
        .filter(|s| s.version == Version::Aup3)
        .count();
    let aup4 = summaries
        .iter()
        .filter(|s| s.version == Version::Aup4)
        .count();
    println!("{} projects: {aup3} AUP3, {aup4} AUP4", summaries.len());
    for s in &summaries {
        println!(
            "  {:<58} {:?} dict {:>3} doc {:>7} events {:>6} refs {:>5}/{:>5} \
             orphans {:>3} lengths {:>5} labels {:>3} rate {:?} pref {:?} {:?}",
            s.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            s.version,
            s.dict_entries,
            s.doc_bytes,
            s.events,
            s.audit.refs,
            s.audit.distinct,
            s.audit.orphans.len(),
            s.audit.lengths_checked,
            s.labels,
            s.track_rates.first(),
            s.editor_rate_preference,
            s.formats,
        );
    }

    assert_eq!(
        aup3, EXPECTED_AUP3,
        "the corpus should hold {EXPECTED_AUP3} .aup3 projects; {dir:?} gave {aup3}"
    );
    assert_eq!(
        aup4, EXPECTED_AUP4,
        "the corpus should hold {EXPECTED_AUP4} .aup4 projects; {dir:?} gave {aup4}"
    );

    for s in &summaries {
        let name = &s.path;
        // The extension is not evidence, but in this corpus it happens to be
        // right, and a mismatch would mean the user had renamed a file - which
        // is the case the reader is built to survive and this test would be the
        // place it showed up.
        let by_extension = match s.path.extension().and_then(|e| e.to_str()) {
            Some("aup3") => Version::Aup3,
            _ => Version::Aup4,
        };
        assert_eq!(
            s.version, by_extension,
            "{name:?}: user_version says {:?} but the extension says {by_extension:?}",
            s.version
        );
        assert!(s.doc_bytes > 0, "{name:?}: the document blob was empty");
        assert!(s.events > 0, "{name:?}: the document held no records");
        assert!(
            s.dict_entries > 0,
            "{name:?}: the document resolved names without a dictionary"
        );
        assert!(
            s.audit.orphans.len() < s.audit.rows,
            "{name:?}: every stored block is unreferenced, so nothing was read"
        );
        assert!(
            s.audit.refs >= s.audit.distinct,
            "{name:?}: {} references cannot reach {} distinct blocks",
            s.audit.refs,
            s.audit.distinct
        );
        assert!(
            !s.track_rates.is_empty(),
            "{name:?}: no wavetrack, so nothing carries a rate"
        );

        // AUP4 declares every block's length and AUP3 declares none. Absence
        // means the older generation, not corruption.
        if s.version.declares_block_lengths() {
            assert_eq!(
                s.audit.lengths_checked, s.audit.refs,
                "{name:?}: AUP4 should declare a length on every waveblock"
            );
        } else {
            assert_eq!(
                s.audit.lengths_checked, 0,
                "{name:?}: AUP3 has no waveblock/@length"
            );
            assert_eq!(
                s.history_generations, None,
                "{name:?}: AUP3 has no project_history table"
            );
        }
    }

    // The rate trap, which is the single most consequential fact in the format.
    let preference_always_192k = summaries
        .iter()
        .all(|s| s.editor_rate_preference == Some(192_000.0));
    assert!(
        preference_always_192k,
        "project/@rate is supposed to read 192000.0 in every corpus file regardless \
         of content; one of them now disagrees, which would make it look like data"
    );
    let disagreeing = summaries
        .iter()
        .filter(|s| s.version == Version::Aup3)
        .filter(|s| s.track_rates.iter().any(|rate| *rate != 192_000.0))
        .count();
    assert_eq!(
        disagreeing, 22,
        "22 of the 25 AUP3 projects were captured at a rate other than the 192000.0 \
         that project/@rate claims; trusting the project attribute plays those at \
         four times speed"
    );

    // 24 of 25 rips are float32, which is the finding that the current Audacity
    // workflow has been converting the converter's integer words to float.
    let float_only = summaries
        .iter()
        .filter(|s| s.version == Version::Aup3)
        .filter(|s| s.formats == ["Float32"])
        .count();
    assert_eq!(
        float_only, 24,
        "24 of the 25 AUP3 rips are stored as float32 and one as int24"
    );

    // Sharing. Only the clip-split project shows it, and it is the reason
    // anything that copies or frees a block must work from the distinct set.
    let shared = summaries
        .iter()
        .find(|s| s.path.file_name().is_some_and(|n| n == "simples_test.aup3"))
        .expect("simples_test.aup3 is the clip-split project and must be in the corpus");
    assert_eq!(
        shared.audit.refs, 532,
        "simples_test's waveblock references"
    );
    assert_eq!(shared.audit.distinct, 456, "simples_test's distinct blocks");
    assert_eq!(
        shared.audit.most_shared.map(|(_, count)| count),
        Some(3),
        "one of simples_test's blocks is referenced three times"
    );

    // Nothing in the corpus was left unsaved, so `autosave` is empty in all 30.
    // Asserted rather than assumed: if one ever is populated, the importer must
    // report it rather than silently reading the older `project` row.
    let unsaved: Vec<&PathBuf> = summaries
        .iter()
        .filter(|s| s.autosave)
        .map(|s| &s.path)
        .collect();
    assert!(
        unsaved.is_empty(),
        "these projects carry unsaved work in `autosave`, which an import must \
         report rather than discard: {unsaved:?}"
    );
}

/// The clip-timing rule, on the one project that can distinguish the two
/// readings of `offset`.
///
/// Four of the five AUP4 projects and 24 of the 25 AUP3 ones are a single clip
/// per channel with no trims, where every reading of `offset` agrees. Only
/// `simples_test` is clip-split, and it is the whole evidence for the rule, so
/// the numbers are written out here rather than derived: a future change that
/// re-derived them from the same misreading would agree with itself.
#[test]
#[ignore = "reads the real corpus; set VCW_AUP_CORPUS and run with --ignored"]
fn a_clips_offset_is_where_its_sequence_begins_not_where_it_plays() {
    let dir = corpus_dir();
    for name in ["simples_test.aup3", "simples_test.aup4"] {
        let path = dir.join(name);
        let (conn, _) = sniff::open(&path).expect("open the clip-split project");
        let surveyed = survey(&conn, &path).expect("survey");
        let project = Project::from_events(&surveyed.document.events).expect("model");

        let track = project.tracks.first().expect("a wavetrack");
        let rate = track.rate;
        assert_eq!(
            rate, 192_000.0,
            "{name}: the clip-split project is at 192 kHz"
        );
        assert_eq!(track.clips.len(), 19, "{name}: clips on the first channel");

        // The clip that settles it: offset 4.31446875 s with a 5.211296875 s
        // left trim. Read as an audible start it would begin at 4.314 s, inside
        // the clip that runs to 8.525765625 s. Read as a sequence origin it
        // begins at 9.525765625 s, exactly where the clip before it ends.
        let trimmed = track
            .clips
            .iter()
            .find(|clip| clip.trim_left > 0.0 && clip.offset < 10.0)
            .expect("the early left-trimmed clip");
        assert_eq!(trimmed.offset, 4.314_468_75);
        assert_eq!(trimmed.trim_left, 5.211_296_875);
        assert_eq!(trimmed.num_samples, 1_299_910);
        assert_eq!(trimmed.first_audible_sample(rate), 1_000_569);
        assert_eq!(trimmed.start_sample(rate), 1_828_947);
        assert_eq!(trimmed.start(rate), 9.525_765_625);
        assert_eq!(trimmed.end(rate), 11.084_833_333_333_334);

        // And the consequence: with `offset` read this way the whole track is a
        // chain of clips that touch, with no gap and no overlap anywhere. That
        // is not something the reader arranges; it is what the file says, and it
        // is only true on one reading of `offset`.
        let mut previous_end = 0_u64;
        for (index, clip) in track.clips.iter().enumerate() {
            assert_eq!(
                clip.start_sample(rate),
                previous_end,
                "{name}: clip {index} starts {} samples from where clip {} ended",
                i64::try_from(clip.start_sample(rate)).unwrap_or(i64::MAX)
                    - i64::try_from(previous_end).unwrap_or(i64::MAX),
                index.saturating_sub(1)
            );
            previous_end = clip.end_sample(rate);
        }

        // Both channels describe the same audio, so they must come out the same
        // length. A stereo pair that disagreed would mean one channel's clips
        // had been laid out differently from the other's.
        let lengths: Vec<u64> = project
            .tracks
            .iter()
            .map(|track| track.clips.last().map_or(0, |c| c.end_sample(track.rate)))
            .collect();
        assert_eq!(lengths.len(), 2, "{name}: a stereo pair is two wavetracks");
        assert_eq!(
            lengths[0], lengths[1],
            "{name}: the channels differ in length"
        );
    }
}

/// The fixture builder, against the projects the fixtures were made from.
///
/// The committed fixtures are tested in `tests/fixtures.rs` and that runs in CI.
/// This is the other half: that the tool which produced them still produces
/// something the reader accepts, so a fixture can be remade rather than being
/// bytes nobody can account for.
#[test]
#[ignore = "reads the real corpus; set VCW_AUP_CORPUS and run with --ignored"]
fn shrinking_a_real_project_produces_one_the_reader_still_accepts() {
    let dir = corpus_dir();
    let scratch = tempfile::tempdir().expect("a temporary directory");

    for name in [
        "simples_test.aup3",
        "simples_test.aup4",
        "Gasp_Stardonas (Transverse).aup3",
    ] {
        let source = dir.join(name);
        let destination = scratch.path().join(name);
        let report = vcw_import::fixture::shrink(
            &source,
            &destination,
            &vcw_import::fixture::Shrink::default(),
        )
        .unwrap_or_else(|e| panic!("shrink {name}: {e}"));

        // It has to be a shrink. A tool that produced a file the same size as a
        // 271 MB project would still pass every assertion below.
        let before = std::fs::metadata(&source).expect("the source's size").len();
        assert!(
            report.file_bytes < before / 100,
            "{name}: {} bytes is not a fixture, the source was {before}",
            report.file_bytes
        );
        assert!(report.blocks_removed > 0, "{name}: no blocks were dropped");
        assert!(
            report.doc_bytes.1 < report.doc_bytes.0,
            "{name}: the document did not shrink"
        );

        // The source is the user's, and it must come out untouched.
        let after = std::fs::metadata(&source).expect("the source's size").len();
        assert_eq!(before, after, "{name}: the source changed size");

        // And the result reads, models and audits, which is everything a
        // fixture is required to do.
        let shrunk = read(&destination);
        assert_eq!(shrunk.audit.refs, report.refs_kept);
        assert_eq!(shrunk.audit.distinct, report.blocks_kept);
        assert!(
            shrunk.audit.orphans.is_empty(),
            "{name}: orphan blocks in a fixture"
        );
        assert!(
            !shrunk.track_rates.is_empty(),
            "{name}: no wavetrack survived"
        );
        assert_eq!(
            shrunk.version,
            read(&source).version,
            "{name}: the generation must survive the copy"
        );
    }
}

#[test]
#[ignore = "reads the real corpus and runs the Python oracle; set VCW_AUP_CORPUS"]
fn the_rust_reader_agrees_with_the_python_oracle() {
    let dir = corpus_dir();
    let probe = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spikes/aup-format-probe/probe.py")
        .canonicalize()
        .expect("the S5 probe should still be in the tree");

    let paths = projects(&dir);
    assert_eq!(
        paths.len(),
        EXPECTED_AUP3 + EXPECTED_AUP4,
        "the oracle diff has to cover the whole corpus"
    );

    for path in &paths {
        let summary = read(path);
        let output = Command::new("python3")
            .arg(&probe)
            .arg("--json")
            .arg(path)
            .output()
            .expect("run the oracle; it needs python3 on PATH");
        assert!(
            output.status.success(),
            "the oracle refused {path:?}:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let oracle: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("the oracle's JSON");
        let oracle = oracle
            .get(0)
            .expect("the oracle reports one object per file");

        let u = |key: &str| -> u64 {
            oracle
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_else(|| panic!("the oracle has no numeric '{key}' for {path:?}"))
        };

        assert_eq!(
            summary.dict_entries as u64,
            u("dict_entries"),
            "{path:?}: dictionary size"
        );
        assert_eq!(
            summary.doc_bytes as u64,
            u("doc_bytes"),
            "{path:?}: document size"
        );
        // Record for record. This is the assertion that would catch a width
        // mistake on a tag that happens not to desynchronize the stream.
        assert_eq!(summary.events as u64, u("events"), "{path:?}: record count");
        assert_eq!(
            summary.audit.refs as u64,
            u("waveblock_refs"),
            "{path:?}: waveblock references"
        );
        assert_eq!(
            summary.audit.distinct as u64,
            u("blocks_referenced"),
            "{path:?}: distinct blocks referenced"
        );
        assert_eq!(
            summary.audit.rows as u64,
            u("blocks_stored"),
            "{path:?}: stored blocks"
        );
        assert_eq!(
            summary.audit.orphans.len() as u64,
            u("orphan_blocks"),
            "{path:?}: orphan blocks"
        );
        assert_eq!(
            summary.audit.lengths_checked as u64,
            u("lengths_declared"),
            "{path:?}: declared block lengths"
        );

        // Element counts, which is the model's own reading of the same stream:
        // the oracle counts start records and this counts the elements the model
        // built from them.
        let expected = oracle
            .get("elements")
            .and_then(serde_json::Value::as_object)
            .expect("the oracle's element counts");
        for (name, count) in expected {
            let count = count.as_u64().expect("an element count");
            let ours = summary.elements.get(name).copied().unwrap_or(0) as u64;
            assert_eq!(ours, count, "{path:?}: <{name}> count");
        }
        assert_eq!(
            summary.elements.len(),
            expected.len(),
            "{path:?}: element kinds; ours {:?} against the oracle's {:?}",
            summary.elements.keys().collect::<Vec<_>>(),
            expected.keys().collect::<Vec<_>>()
        );

        // The rates, both of them, because the whole point is that they differ.
        let oracle_rates: Vec<f64> = oracle
            .get("track_rates")
            .and_then(serde_json::Value::as_array)
            .expect("the oracle's track rates")
            .iter()
            .filter_map(serde_json::Value::as_f64)
            .collect();
        let mut ours = summary.track_rates.clone();
        ours.sort_by(f64::total_cmp);
        ours.dedup();
        assert_eq!(ours, oracle_rates, "{path:?}: wavetrack rates");
        assert_eq!(
            summary.editor_rate_preference,
            oracle
                .get("project_rate")
                .and_then(serde_json::Value::as_f64),
            "{path:?}: project/@rate"
        );
    }

    println!("{} projects agree with the oracle", paths.len());
}

/// Where a landing test writes. Defaults to the system temporary directory,
/// which on this machine is tmpfs - so a run that lands several hundred
/// megabytes should point this at real storage.
const SCRATCH_ENV: &str = "VCW_SCRATCH";

/// A directory to land projects into, on real storage if the operator said where.
fn scratch() -> tempfile::TempDir {
    match std::env::var(SCRATCH_ENV) {
        Ok(dir) => tempfile::tempdir_in(dir).expect("a temporary directory in VCW_SCRATCH"),
        Err(_) => tempfile::tempdir().expect("a temporary directory"),
    }
}

/// Reads a whole capture's audio back through the reader playback uses.
fn landed_audio(path: &Path, capture_id: i64) -> (vcw_project::Layout, Vec<u8>) {
    let project = vcw_project::Project::open_read_only(path).expect("open the landed project");
    let layout = vcw_project::Layout::of(project.conn(), capture_id).expect("layout");
    let mut reader = vcw_project::pcm::Reader::open(project.conn(), capture_id, layout.span())
        .expect("a reader over the whole capture");
    let mut audio = Vec::with_capacity((layout.frames * layout.frame_bytes() as u64) as usize);
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let filled = reader.fill(&mut buffer).expect("fill");
        if filled == 0 {
            break;
        }
        audio.extend_from_slice(&buffer[..filled]);
    }
    (layout, audio)
}

/// One block's samples, straight out of the source project.
fn source_block(conn: &rusqlite::Connection, blockid: i64) -> Vec<u8> {
    conn.query_row(
        "SELECT samples FROM sampleblocks WHERE blockid = ?1",
        [blockid],
        |row| row.get(0),
    )
    .unwrap_or_else(|e| panic!("read block {blockid}: {e}"))
}

/// Landing real projects, and checking the audio arrived where the document said
/// it would.
///
/// The committed-fixture landing tests in `tests/landing.rs` prove the assembly
/// on synthetic audio, because a fixture's samples are zeros and zeros cannot
/// tell a correct layout from a shuffled one. This is the other half and the one
/// that matters: real vinyl, real 24-bit and float32 blocks, real trims, real
/// gaps, compared byte for byte against the blocks they came out of.
///
/// The oracle is deliberately not this crate's timeline code. For every clip on
/// every channel it reads the *source* block directly and compares the head and
/// tail of the clip's audible span against the landed capture at the frame the
/// document puts it at. A bug in the streaming assembly - an off-by-one on a
/// trim, a block stitched at the wrong offset, a channel swapped - moves audio
/// relative to the timeline, and that is exactly what this measures.
#[test]
#[ignore = "lands the real corpus; set VCW_AUP_CORPUS and run with --ignored"]
fn a_real_project_lands_with_its_audio_where_the_document_says() {
    let dir = corpus_dir();
    let scratch = scratch();

    // One 24-bit clip-split project, and one float32 single-clip one. Between
    // them: both stored widths that matter, trims, gaps, shared blocks and the
    // common shape 24 of the 25 rips have.
    for name in ["simples_test.aup3", "Gasp_Stardonas (Transverse).aup3"] {
        let source = dir.join(name);
        let destination = scratch.path().join(format!("{name}.vcw"));
        let landed = vcw_import::land(&source, &destination, &vcw_import::land::Options::default())
            .unwrap_or_else(|e| panic!("land {name}: {e}"));

        let (conn, _) = sniff::open(&source).expect("reopen the source");
        let surveyed = survey(&conn, &source).expect("survey");
        let document = Project::from_events(&surveyed.document.events).expect("model");
        let hz = document.rate().expect("a rate");
        assert_eq!(f64::from(landed.rate.hz()), hz, "{name}: the landed rate");

        let (layout, audio) = landed_audio(&destination, landed.capture_id);
        assert_eq!(layout.frames, landed.frames);
        assert_eq!(layout.channels as usize, document.tracks.len());
        let width = layout.format.bytes_per_sample();
        let frame_bytes = layout.frame_bytes();
        assert_eq!(audio.len(), layout.frames as usize * frame_bytes);

        // The timeline's length is the last clip's audible end, and nothing else.
        let expected_frames = document
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .map(|clip| clip.end_sample(hz))
            .max()
            .expect("at least one clip");
        assert_eq!(
            landed.frames, expected_frames,
            "{name}: the capture has to be exactly as long as the timeline"
        );

        let mut compared = 0;
        for (channel, track) in document.tracks.iter().enumerate() {
            for clip in &track.clips {
                let start = clip.start_sample(hz);
                let audible = clip.audible_samples(hz);
                let head = clip.first_audible_sample(hz);
                // The head of the clip, and the tail, which is where a
                // rounding error on `trimRight` would show up.
                for (into_clip, note) in [(0_u64, "head"), (audible.saturating_sub(16), "tail")] {
                    let sequence = head + into_clip;
                    let reference = clip
                        .blocks
                        .iter()
                        .filter(|block| block.start <= sequence)
                        .max_by_key(|block| block.start)
                        .expect("a block covering the sample");
                    let bytes = source_block(&conn, reference.blockid);
                    let from = ((sequence - reference.start) as usize) * width;
                    let take = 16.min(audible - into_clip) as usize;
                    if from + take * width > bytes.len() {
                        // The 16 samples straddle two blocks. Skipped rather
                        // than stitched: the next clip's head covers the same
                        // ground and stitching here would re-implement the code
                        // under test.
                        continue;
                    }
                    let want = &bytes[from..from + take * width];

                    let frame = (start + into_clip) as usize;
                    let mut got = Vec::with_capacity(take * width);
                    for index in 0..take {
                        let at = (frame + index) * frame_bytes + channel * width;
                        got.extend_from_slice(&audio[at..at + width]);
                    }
                    assert_eq!(
                        got, want,
                        "{name}: channel {channel}, clip '{}', {note} at frame {frame}: \
                         the landed audio is not the block's",
                        clip.name
                    );
                    compared += 1;
                }
            }
        }
        assert!(
            compared >= 2,
            "{name}: nothing was compared, so this proved nothing"
        );

        // Every frame between two clips has to be silence, and it has to be
        // there: closing the gaps would slide every label off its track.
        for (channel, track) in document.tracks.iter().enumerate() {
            for pair in track.clips.windows(2) {
                let gap_start = pair[0].end_sample(hz);
                let gap_end = pair[1].start_sample(hz);
                for frame in gap_start..gap_end.min(gap_start + 8) {
                    let at = frame as usize * frame_bytes + channel * width;
                    assert!(
                        audio[at..at + width].iter().all(|byte| *byte == 0),
                        "{name}: channel {channel} frame {frame} is in a gap and is not silent"
                    );
                }
            }
        }

        // And the result is a project like any other.
        let project = vcw_project::Project::open_read_only(&destination).expect("open");
        let report =
            vcw_project::validate(&project, vcw_project::Options::default()).expect("validate");
        assert!(
            report.findings.is_empty(),
            "{name}: a landed project must validate clean: {:?}",
            report.findings
        );
        drop(project);
        // These are hundreds of megabytes each; the scratch directory should not
        // hold two at once.
        std::fs::remove_file(&destination).expect("remove the landed project");
    }
}

/// The two generations of one project have to land as the same audio.
///
/// S5 proved the AUP3 to AUP4 conversion is byte-identical at the
/// `sampleblocks` layer, and `tests/fixtures.rs` asserts the documents agree.
/// This closes it at the other end: the whole assembled timeline, hashed, from
/// both generations of the same rip.
#[test]
#[ignore = "lands the real corpus; set VCW_AUP_CORPUS and run with --ignored"]
fn both_generations_of_one_project_land_as_the_same_audio() {
    let dir = corpus_dir();
    let scratch = scratch();

    let mut digests = Vec::new();
    for name in ["simples_test.aup3", "simples_test.aup4"] {
        let destination = scratch.path().join(format!("{name}.vcw"));
        let landed = vcw_import::land(
            &dir.join(name),
            &destination,
            &vcw_import::land::Options::default(),
        )
        .unwrap_or_else(|e| panic!("land {name}: {e}"));

        let (layout, audio) = landed_audio(&destination, landed.capture_id);
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&audio);
        digests.push((
            name,
            landed.frames,
            layout.format,
            layout.channels,
            hasher.finalize(),
        ));
        std::fs::remove_file(&destination).expect("remove the landed project");
    }

    let (first, rest) = digests.split_first().expect("two landings");
    for other in rest {
        assert_eq!(
            (first.1, first.2, first.3, first.4),
            (other.1, other.2, other.3, other.4),
            "{} and {} landed as different audio",
            first.0,
            other.0
        );
    }
}
