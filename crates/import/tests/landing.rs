/*
 *  landing.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Landing a project as a capture: the write half of import, in CI.
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
//! Landing a project as a capture: the write half of import, in CI (§12).
//!
//! # Why the source here is synthetic, when `fixtures.rs` insists it must not be
//!
//! Because the claim is different. `fixtures.rs` tests whether we can read
//! Audacity's bytes, and for that a file we wrote ourselves would only prove our
//! encoder agrees with our decoder. These tests ask something else: given a
//! timeline, does the writer lay it out on VCW's side correctly? The subject is
//! our code at both ends, so the input may as well be ours - and it has to be,
//! for two reasons the committed fixtures cannot get around.
//!
//! The first is size. A fixture keeps a real project's clip *offsets* while
//! shrinking its audio to 256 samples a block, so `clips.aup3` describes a
//! 273-second timeline holding about 40 ms of sound. Landing it means writing
//! the silence in between - 419 MB of it - which is not a thing to do in CI.
//!
//! The second is that fixture audio is zeros, so no fixture can distinguish
//! audio laid down in the right order from audio laid down in the wrong one. A
//! synthetic source with a recognisable ramp in every block can, and does: these
//! tests read the landed capture back through
//! [`vcw_project::pcm::Reader`] - the same reader playback uses - and compare it
//! sample for sample against what the Audacity timeline said should be there.
//!
//! One real-bytes landing test is here too, on the smallest fixture, because
//! "the synthetic path works" and "a genuine Audacity file lands" are also two
//! different claims. `tests/corpus.rs` lands the real 30.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use vcw_import::land::{Options, Source};
use vcw_import::model::{BlockRef, Clip, Label, LabelTrack, Project, SampleFormat, Track};
use vcw_import::{Version, land_from};
use vcw_types::{StorageFormat, vinyl::Side};

/// Audacity's own DDL for the audio table, which is what makes a synthetic
/// source a fair stand-in for a real one on the read side.
const SAMPLEBLOCKS: &str = "CREATE TABLE sampleblocks (
        blockid      INTEGER PRIMARY KEY AUTOINCREMENT,
        sampleformat INTEGER,
        summin       REAL,
        summax       REAL,
        sumrms       REAL,
        summary256   BLOB,
        summary64k   BLOB,
        samples      BLOB
    )";

/// The rate the synthetic sources run at. Low on purpose: every assertion in
/// this file is in frames, and a test that has to write 192,000 frames to cover
/// a second of timeline is a test nobody reads twice.
const RATE: f64 = 8_000.0;

/// A source built in a temporary directory, kept alive by its `_dir`.
struct Synthetic {
    _dir: tempfile::TempDir,
    conn: Connection,
    path: PathBuf,
}

/// Opens an Audacity-shaped database with no blocks in it yet.
fn synthetic() -> Synthetic {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("synthetic.aup3");
    let conn = Connection::open(&path).expect("open");
    conn.execute_batch(SAMPLEBLOCKS).expect("Audacity's DDL");
    Synthetic {
        _dir: dir,
        conn,
        path,
    }
}

impl Synthetic {
    /// Writes one block of 16-bit samples counting up from `first`, and returns
    /// its blockid.
    ///
    /// A ramp rather than noise: if a chunk lands at the wrong offset, the
    /// failure message says which sample is where instead of "bytes differ".
    fn ramp(&self, first: i16, samples: u64) -> i64 {
        let mut bytes = Vec::with_capacity(samples as usize * 2);
        for index in 0..samples {
            let value = first.wrapping_add(index as i16);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.conn
            .execute(
                "INSERT INTO sampleblocks (sampleformat, summin, summax, sumrms, samples)
                 VALUES (?1, 0.0, 0.0, 0.0, ?2)",
                rusqlite::params![SampleFormat::Int16.code(), bytes],
            )
            .expect("insert a block");
        self.conn.last_insert_rowid()
    }

    /// Lands it, with the default options.
    fn land(&self, document: &Project) -> (vcw_import::Landed, PathBuf) {
        self.land_with(document, &Options::default())
    }

    /// Lands it, with options.
    fn land_with(&self, document: &Project, options: &Options) -> (vcw_import::Landed, PathBuf) {
        self.land_into(document, options, "landed")
    }

    /// Lands it, with options, under a name of its own.
    ///
    /// One fixture, two destinations: the batching test has to land the *same*
    /// document twice and compare, and two fixtures would compare two ramps.
    fn land_into(
        &self,
        document: &Project,
        options: &Options,
        name: &str,
    ) -> (vcw_import::Landed, PathBuf) {
        let destination = self.path.with_file_name(format!("{name}.vcw"));
        let landed = land_from(
            &Source {
                conn: &self.conn,
                document,
                path: &self.path,
                version: Version::Aup3,
            },
            &destination,
            options,
        )
        .unwrap_or_else(|error| panic!("land: {error}"));
        (landed, destination)
    }
}

/// A clip with no trims, at `offset` seconds, made of `blocks`.
fn clip(name: &str, offset: f64, blocks: &[(u64, i64)], samples: u64) -> Clip {
    Clip {
        name: name.to_owned(),
        offset,
        trim_left: 0.0,
        trim_right: 0.0,
        num_samples: samples,
        max_samples: 262_144,
        sample_format: SampleFormat::Int16,
        stretch_ratio: 1.0,
        blocks: blocks
            .iter()
            .map(|(start, blockid)| BlockRef {
                start: *start,
                blockid: *blockid,
                length: None,
            })
            .collect(),
        envelope: Vec::new(),
    }
}

/// A wave track carrying `clips`, as the nth channel.
fn track(channel: u32, clips: Vec<Clip>) -> Track {
    Track {
        name: format!("Audio {}", channel + 1),
        rate: RATE,
        channel: Some(channel),
        // Audacity's own coding: 3 on the first of a linked pair, 0 on the second.
        linked: Some(if channel == 0 { 3 } else { 0 }),
        sample_format: SampleFormat::Int16,
        gain: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        clips,
    }
}

/// A project with these tracks and nothing else.
fn project(tracks: Vec<Track>) -> Project {
    Project {
        audacity_version: Some("3.7.5".to_owned()),
        tags: BTreeMap::new(),
        tracks,
        label_tracks: Vec::new(),
        editor_rate_preference: Some(192_000.0),
    }
}

/// Reads a landed capture back through the reader playback uses.
fn played_back(path: &Path, capture_id: i64) -> (vcw_project::Layout, Vec<i16>) {
    let project = vcw_project::Project::open_read_only(path).expect("open the landed project");
    let layout = vcw_project::Layout::of(project.conn(), capture_id).expect("layout");
    let mut reader = vcw_project::pcm::Reader::open(project.conn(), capture_id, layout.span())
        .expect("a reader over the whole capture");
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 4096];
    loop {
        let filled = reader.fill(&mut buffer).expect("fill");
        if filled == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..filled]);
    }
    let samples = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect();
    (layout, samples)
}

#[test]
fn a_landed_capture_is_indistinguishable_from_a_recorded_one() {
    let source = synthetic();
    // Two channels, two clips each, with a 500-frame gap between the clips: the
    // shape the whole design turns on. Left and right get different ramps so a
    // swapped channel cannot pass.
    let left = vec![
        clip("L.1", 0.0, &[(0, source.ramp(1_000, 1_000))], 1_000),
        clip(
            "L.2",
            1_500.0 / RATE,
            &[(0, source.ramp(2_000, 1_000))],
            1_000,
        ),
    ];
    let right = vec![
        clip("R.1", 0.0, &[(0, source.ramp(5_000, 1_000))], 1_000),
        clip(
            "R.2",
            1_500.0 / RATE,
            &[(0, source.ramp(6_000, 1_000))],
            1_000,
        ),
    ];
    let document = project(vec![track(0, left), track(1, right)]);

    let (landed, path) = source.land(&document);

    assert_eq!(landed.frames, 2_500, "1000 + a 500-frame gap + 1000");
    assert_eq!(landed.channels, 2);
    assert_eq!(landed.rate.hz(), 8_000);
    assert_eq!(landed.storage_format, StorageFormat::Int16);
    assert_eq!(landed.clips, 4);
    assert_eq!(landed.side, Side::A);
    assert!(landed.blocks > 0);

    // Read back through the playback reader, not through our own query: if the
    // landing writes something only this crate can read, that is a failure.
    let (layout, samples) = played_back(&path, landed.capture_id);
    assert_eq!(layout.rate.hz(), 8_000);
    assert_eq!(layout.channels, 2);
    assert_eq!(layout.format, StorageFormat::Int16);
    assert_eq!(layout.frames, 2_500);
    assert_eq!(samples.len(), 2_500 * 2);

    let mut expected: Vec<i16> = Vec::with_capacity(5_000);
    for frame in 0..2_500_u64 {
        let (l, r) = match frame {
            0..1_000 => (1_000 + frame as i16, 5_000 + frame as i16),
            1_000..1_500 => (0, 0),
            _ => {
                let into = (frame - 1_500) as i16;
                (2_000 + into, 6_000 + into)
            }
        };
        expected.push(l);
        expected.push(r);
    }
    assert_eq!(samples, expected, "the timeline, frame for frame");

    // And the project is a valid project, by the same check every other capture
    // gets.
    let project = vcw_project::Project::open_read_only(&path).expect("reopen");
    let report =
        vcw_project::validate(&project, vcw_project::Options::default()).expect("validate runs");
    assert!(
        report.findings.is_empty(),
        "an imported capture must validate clean: {:?}",
        report.findings
    );
}

#[test]
fn an_import_batches_its_commits_without_changing_what_lands() {
    // The knob and its safety property in one test. `Config::default()` is D3 -
    // one block a transaction, which is the audio a power cut costs a live
    // capture - and an import overrides it, because an import has every frame
    // in hand and the fsyncs are then the runtime rather than free. What it may
    // not do is change a single byte of what lands, so the two projects are
    // compared frame for frame and only the transaction count is allowed to
    // differ.
    // Stated as an inequality against the live default, and deliberately not as
    // `default().config.batch_blocks == BATCH_BLOCKS`: that reads like a check
    // and is a tautology, because both halves of it move together when the
    // constant changes. This fails if import is reverted to `Config::default()`
    // *and* if `BATCH_BLOCKS` is ever set to a value that does not batch.
    let live = vcw_project::persistence::Config::default().batch_blocks;
    assert_eq!(live, 1, "D3 commits one block at a time");
    assert!(
        Options::default().config.batch_blocks > live,
        "import must batch more than a live capture does, not {}",
        Options::default().config.batch_blocks,
    );
    assert_eq!(
        Options::default().config.batch_blocks,
        vcw_import::land::BATCH_BLOCKS,
        "and the constant is where it is set",
    );

    // Twelve writer blocks: 8 kHz at D3's 250 ms is 2000 frames, so 24,000
    // frames is enough for a batch of four to be three transactions and not one.
    const FRAMES: u64 = 24_000;
    const BLOCK: u64 = 2_000;
    let source = synthetic();
    let left = vec![clip("L", 0.0, &[(0, source.ramp(1_000, FRAMES))], FRAMES)];
    let right = vec![clip("R", 0.0, &[(0, source.ramp(5_000, FRAMES))], FRAMES)];
    let document = project(vec![track(0, left), track(1, right)]);

    let with = |batch: usize, name: &str| {
        let options = Options {
            config: vcw_project::persistence::Config {
                batch_blocks: batch,
                ..vcw_project::persistence::Config::default()
            },
            ..Options::default()
        };
        source.land_into(&document, &options, name)
    };

    let (one, one_path) = with(1, "one");
    let (four, four_path) = with(4, "four");

    assert_eq!(one.commits, FRAMES / BLOCK, "one transaction a block");
    assert_eq!(
        four.commits,
        FRAMES / BLOCK / 4,
        "four blocks a transaction"
    );
    assert_eq!(one.batch_blocks, 1);
    assert_eq!(four.batch_blocks, 4);

    // Everything else about the two is the same, including the block count: a
    // batch is a transaction boundary and nothing else, so batching four blocks
    // must not produce one block four times the size.
    assert_eq!(one.blocks, four.blocks);
    assert_eq!(one.frames, four.frames);
    assert_eq!(one.frames, FRAMES);

    let (one_layout, one_samples) = played_back(&one_path, one.capture_id);
    let (four_layout, four_samples) = played_back(&four_path, four.capture_id);
    assert_eq!(one_layout.frames, four_layout.frames);
    assert_eq!(one_samples, four_samples, "the audio, frame for frame");

    // And both validate, which is the check that would catch a half-written
    // final batch.
    for path in [&one_path, &four_path] {
        let project = vcw_project::Project::open_read_only(path).expect("reopen");
        let report =
            vcw_project::validate(&project, vcw_project::Options::default()).expect("validate");
        assert!(
            report.findings.is_empty(),
            "{}: {:?}",
            path.display(),
            report.findings
        );
    }
}

#[test]
fn the_capture_row_says_the_audio_was_imported() {
    // Not 'shared' or 'native'. The column is where a reader finds out where the
    // audio came from, and the other three spellings all name a stream that was
    // opened, which for an import never happened.
    let source = synthetic();
    let document = project(vec![track(
        0,
        vec![clip("A.1", 0.0, &[(0, source.ramp(1, 64))], 64)],
    )]);

    let (landed, path) = source.land(&document);

    let project = vcw_project::Project::open_read_only(&path).expect("open");
    let mode: String = project
        .conn()
        .query_row(
            "SELECT capture_mode FROM captures WHERE capture_id = ?1",
            [landed.capture_id],
            |row| row.get(0),
        )
        .expect("the capture row");
    assert_eq!(mode, "imported");
    let state: String = project
        .conn()
        .query_row(
            "SELECT state FROM captures WHERE capture_id = ?1",
            [landed.capture_id],
            |row| row.get(0),
        )
        .expect("the capture row");
    assert_eq!(state, "finalised");

    // And the source is recorded in the file it produced, not in a log beside it.
    assert_eq!(
        vcw_project::meta::get(project.conn(), vcw_import::land::SOURCE_KEY).expect("meta"),
        Some(source.path.display().to_string())
    );
    assert_eq!(
        vcw_project::meta::get(project.conn(), vcw_import::land::VERSION_KEY).expect("meta"),
        Some("aup3".to_owned())
    );
    assert_eq!(
        vcw_project::meta::get(project.conn(), vcw_import::land::WRITER_KEY).expect("meta"),
        Some("3.7.5".to_owned())
    );
}

#[test]
fn a_trim_is_honoured_in_the_sample_domain() {
    // The finding that made `model` rewrite every clip accessor: `offset` is
    // where the clip's *sequence* begins, and `trimLeft` is how much of it is
    // hidden. So this clip's audio starts 100 samples into its block and lands
    // 100 frames later than its offset, and the last 50 samples are trimmed away.
    let source = synthetic();
    let blockid = source.ramp(0, 1_000);
    let mut only = clip("A.1", 0.0, &[(0, blockid)], 1_000);
    only.trim_left = 100.0 / RATE;
    only.trim_right = 50.0 / RATE;
    let document = project(vec![track(0, vec![only])]);

    let (landed, path) = source.land(&document);

    // 100 frames of silence, then samples 100..950 of the ramp.
    assert_eq!(landed.frames, 950);
    let (_, samples) = played_back(&path, landed.capture_id);
    let mut expected: Vec<i16> = vec![0; 100];
    expected.extend(100..950_i16);
    assert_eq!(samples, expected);
}

#[test]
fn clips_are_read_in_audible_order_not_document_order() {
    // The corpus proved document order is not even offset order. Here the
    // document lists the later clip first, and the landed audio must still be in
    // time order.
    let source = synthetic();
    let first = source.ramp(100, 100);
    let second = source.ramp(200, 100);
    let document = project(vec![track(
        0,
        vec![
            clip("A.2", 100.0 / RATE, &[(0, second)], 100),
            clip("A.1", 0.0, &[(0, first)], 100),
        ],
    )]);

    let (landed, path) = source.land(&document);

    assert_eq!(landed.frames, 200);
    let (_, samples) = played_back(&path, landed.capture_id);
    let mut expected: Vec<i16> = (100..200).collect();
    expected.extend(200..300_i16);
    assert_eq!(samples, expected);
}

#[test]
fn a_clip_spanning_several_blocks_is_stitched_in_sequence_order() {
    // Blocks are listed by sequence start, and a real sequence is tiled by
    // several of them at uneven lengths. Listed here out of order on purpose.
    let source = synthetic();
    let head = source.ramp(0, 300);
    let middle = source.ramp(300, 200);
    let tail = source.ramp(500, 100);
    let document = project(vec![track(
        0,
        vec![clip(
            "A.1",
            0.0,
            &[(300, middle), (0, head), (500, tail)],
            600,
        )],
    )]);

    let (landed, path) = source.land(&document);

    assert_eq!(landed.frames, 600);
    let (_, samples) = played_back(&path, landed.capture_id);
    assert_eq!(samples, (0..600_i16).collect::<Vec<_>>());
}

#[test]
fn a_shared_block_is_read_twice_rather_than_copied_once() {
    // 532 references to 456 blocks in the corpus, one block used three times. A
    // reader that consumed a block on first use would produce silence the second
    // time; one that de-duplicated would produce the wrong length.
    let source = synthetic();
    let shared = source.ramp(7, 100);
    let document = project(vec![track(
        0,
        vec![
            clip("A.1", 0.0, &[(0, shared)], 100),
            clip("A.2", 100.0 / RATE, &[(0, shared)], 100),
            clip("A.3", 200.0 / RATE, &[(0, shared)], 100),
        ],
    )]);

    let (landed, path) = source.land(&document);

    assert_eq!(landed.frames, 300);
    let (_, samples) = played_back(&path, landed.capture_id);
    let once: Vec<i16> = (7..107).collect();
    let expected: Vec<i16> = once.iter().chain(&once).chain(&once).copied().collect();
    assert_eq!(samples, expected);
}

#[test]
fn labels_become_titled_tracks_and_the_unusable_ones_are_reported() {
    let source = synthetic();
    let document = {
        let mut document = project(vec![track(
            0,
            vec![clip("A.1", 0.0, &[(0, source.ramp(0, 8_000))], 8_000)],
        )]);
        document.label_tracks.push(LabelTrack {
            name: "Labels 1".to_owned(),
            labels: vec![
                // Out of order in the document, to prove the ordering is ours.
                Label {
                    t: 0.5,
                    t1: 0.9,
                    title: "second".to_owned(),
                },
                Label {
                    t: 0.0,
                    t1: 0.5,
                    title: "first".to_owned(),
                },
                // A position, not a span: Audacity writes these when a user
                // drops a marker.
                Label {
                    t: 0.95,
                    t1: 0.95,
                    title: "a marker".to_owned(),
                },
                // Over audio that is no longer in the project, which is what
                // deleting a clip and keeping its labels leaves behind.
                Label {
                    t: 2.0,
                    t1: 3.0,
                    title: "orphaned".to_owned(),
                },
                // Running off the end: kept, and clipped to the audio.
                Label {
                    t: 0.95,
                    t1: 4.0,
                    title: "overhanging".to_owned(),
                },
            ],
        });
        document
    };

    let (landed, path) = source.land(&document);

    assert_eq!(landed.tracks, 3);
    assert_eq!(landed.labels_skipped.len(), 2);
    let skipped: Vec<&str> = landed
        .labels_skipped
        .iter()
        .map(|(title, _)| title.as_str())
        .collect();
    assert_eq!(skipped, ["a marker", "orphaned"]);

    let project = vcw_project::Project::open_read_only(&path).expect("open");
    let tracks = vcw_project::track::tracks(project.conn(), Side::A).expect("tracks");
    let named: Vec<(u32, &str, u64, u64)> = tracks
        .iter()
        .map(|record| {
            (
                record.number,
                record.title.as_str(),
                record.start,
                record.end,
            )
        })
        .collect();
    assert_eq!(
        named,
        [
            (1, "first", 0, 4_000),
            (2, "second", 4_000, 7_200),
            (3, "overhanging", 7_600, 8_000),
        ],
        "in timeline order, numbered from one, with the last one clipped to the \
         end of the audio"
    );

    // A person placed these in Audacity, so §24 reserves them: detection may
    // add boundaries beside them but not move them.
    let boundaries = vcw_project::track::boundaries(project.conn(), Side::A).expect("boundaries");
    assert!(
        boundaries.iter().all(|boundary| boundary.locked),
        "labels a person drew must land locked"
    );
}

#[test]
fn tags_become_release_metadata_and_every_tag_is_kept_verbatim() {
    let source = synthetic();
    let document = {
        let mut document = project(vec![track(
            0,
            vec![clip("A.1", 0.0, &[(0, source.ramp(0, 64))], 64)],
        )]);
        document
            .tags
            .insert("ALBUM".to_owned(), "Atomos".to_owned());
        document.tags.insert(
            "ARTIST".to_owned(),
            "A Winged Victory For The Sullen".to_owned(),
        );
        document.tags.insert("YEAR".to_owned(), "2014".to_owned());
        document
            .tags
            .insert("GENRE".to_owned(), "Folk Pop".to_owned());
        // Not one of Audacity's six. It has to survive anyway.
        document
            .tags
            .insert("ENGINEER".to_owned(), "someone".to_owned());
        document
    };

    let (landed, _path) = source.land(&document);
    assert_eq!(landed.tags, 5);

    let project = vcw_project::Project::open_read_only(&landed.path).expect("open");
    let release = vcw_project::release::load(project.conn())
        .expect("load")
        .expect("a release row");
    assert_eq!(release.album, "Atomos");
    assert_eq!(release.album_artist, "A Winged Victory For The Sullen");
    assert_eq!(release.year, Some(2014));
    // Normalised on the way in (§32), which is what the column claims to hold.
    assert_eq!(release.genres, ["Folk Pop", "Folk", "Pop"]);

    // The lossy mapping is not the only copy: every tag is in `meta` as it was.
    assert_eq!(
        vcw_project::meta::get(
            project.conn(),
            &format!("{}ENGINEER", vcw_import::land::TAG_PREFIX)
        )
        .expect("meta"),
        Some("someone".to_owned())
    );
}

#[test]
fn a_project_with_no_wave_tracks_is_refused_rather_than_landed_empty() {
    // A label-only project is a real thing a user can save. Landing it as a
    // zero-frame capture would leave recovery looking at something that reads
    // like an interrupted recording.
    let source = synthetic();
    let document = project(Vec::new());
    let destination = source.path.with_extension("vcw");

    let error = land_from(
        &Source {
            conn: &source.conn,
            document: &document,
            path: &source.path,
            version: Version::Aup3,
        },
        &destination,
        &Options::default(),
    )
    .expect_err("no audio");

    assert!(matches!(error, vcw_import::Error::NoAudio), "got {error}");
    assert!(
        !destination.exists(),
        "a refused import must not leave a project behind"
    );
}

#[test]
fn the_rate_trap_fixture_lands_at_the_rate_its_tracks_declare() {
    // Real Audacity bytes, all the way into a .vcw. This file says
    // project/@rate 192000.0 and wavetrack/@rate 48000.0; the landed capture has
    // to say 48000, because the other reading would play the user's rip at four
    // times speed. It is the single most consequential fact in the format and
    // this is the end-to-end assertion of it.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rate-trap.aup3");
    let dir = tempfile::tempdir().expect("a temporary directory");
    let destination = dir.path().join("rate-trap.vcw");

    let landed = vcw_import::land(&path, &destination, &Options::default()).expect("land");

    assert_eq!(landed.rate.hz(), 48_000);
    assert_eq!(landed.storage_format, StorageFormat::Float32);
    assert_eq!(landed.version, Version::Aup3);
    // The tags are real, and they came across.
    let project = vcw_project::Project::open_read_only(&destination).expect("open");
    let release = vcw_project::release::load(project.conn())
        .expect("load")
        .expect("a release row");
    assert_eq!(release.album, "Stardonas (Transverse)");

    // Every label in this fixture sits over audio the shrinker removed, so all
    // three are reported rather than landed as tracks past the end of the sound.
    assert_eq!(landed.tracks, 0);
    assert_eq!(landed.labels_skipped.len(), 3);
    assert!(
        landed
            .labels_skipped
            .iter()
            .all(|(_, why)| why.contains("end of the audio")),
        "got {:?}",
        landed.labels_skipped
    );

    let report =
        vcw_project::validate(&project, vcw_project::Options::default()).expect("validate runs");
    assert!(
        report.findings.is_empty(),
        "a landed fixture must validate clean: {:?}",
        report.findings
    );
}
