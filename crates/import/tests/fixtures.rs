/*
 *  fixtures.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Reading the committed fixtures, which are real Audacity bytes and run in CI.
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
//! Reading the committed fixtures, which are real Audacity bytes and run in CI.
//!
//! These three files came out of the user's corpus through
//! [`vcw_import::fixture`], which deletes rather than writes: the dictionary,
//! the element structure and every attribute except the four it has to rewrite
//! are bytes Audacity produced. That is the point. A fixture this crate's author
//! wrote by hand would prove only that our encoder agrees with our decoder,
//! which is the mistake these tests exist to avoid.
//!
//! What they cannot prove is sample decoding: the audio in a fixture is zeros,
//! deliberately, because a repository is no place for somebody's commercial
//! vinyl. `tests/corpus.rs` covers that against the real files.

use std::path::{Path, PathBuf};

use vcw_import::{Version, audit, model::Project, read::survey, sniff};

/// Where the fixtures are, relative to the crate.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Everything a fixture test wants, read the way the crate is meant to be used.
struct Read {
    sniffed: vcw_import::Sniffed,
    surveyed: vcw_import::Survey,
    project: Project,
    audit: vcw_import::Audit,
}

fn read(name: &str) -> Read {
    let path = fixture(name);
    assert!(
        path.is_file(),
        "{path:?} is missing; it is committed, so this is not a machine difference"
    );
    let (conn, sniffed) = sniff::open(&path).unwrap_or_else(|e| panic!("open {name}: {e}"));
    let surveyed = survey(&conn, &path).unwrap_or_else(|e| panic!("survey {name}: {e}"));
    let project = Project::from_events(&surveyed.document.events)
        .unwrap_or_else(|e| panic!("model {name}: {e}"));
    let audited = audit(&conn, &project).unwrap_or_else(|e| panic!("audit {name}: {e}"));
    Read {
        sniffed,
        surveyed,
        project,
        audit: audited,
    }
}

#[test]
fn an_aup3_fixture_reads_as_a_clip_split_stereo_project() {
    let read = read("clips.aup3");

    assert_eq!(read.sniffed.version, Version::Aup3);
    assert_eq!(read.sniffed.user_version, 0x0307_0000, "3.7.0.0");
    assert_eq!(
        read.sniffed.page_size, 4096,
        "the smaller of the two in use"
    );

    // Two sibling wavetracks, which is what stereo is in this format.
    assert_eq!(read.project.tracks.len(), 2);
    let left = &read.project.tracks[0];
    let right = &read.project.tracks[1];
    assert_eq!(left.channel, Some(0));
    assert_eq!(left.linked, Some(3));
    assert_eq!(right.channel, Some(1));
    assert_eq!(right.linked, Some(0));

    // 19 clips a channel survive the shrink: it drops blocks, not clips, so the
    // structure a reader has to cope with is still all there.
    assert_eq!(left.clips.len(), 19);
    assert_eq!(right.clips.len(), 19);

    // Sharing, which only a clip-split project has. 38 references reaching 26
    // blocks, so 12 of the references are to a block another clip also uses.
    assert_eq!(read.audit.refs, 38);
    assert_eq!(read.audit.distinct, 26);
    assert!(
        read.audit.most_shared.is_some_and(|(_, count)| count > 1),
        "a shared block must survive into the fixture, or the reference counting \
         this file exists to exercise is untested in CI"
    );
    assert_eq!(read.audit.orphans, Vec::<i64>::new());
    assert_eq!(
        read.audit.lengths_checked, 0,
        "AUP3 has no waveblock/@length"
    );
    assert_eq!(
        read.surveyed.history_generations, None,
        "AUP3 has no history"
    );

    // The user's own track boundaries, which is what a labeltrack is.
    assert_eq!(read.project.label_tracks.len(), 1);
    assert_eq!(read.project.label_tracks[0].labels.len(), 2);
    assert_eq!(read.project.label_tracks[0].labels[0].title, "beep");
}

#[test]
fn an_aup4_fixture_reads_the_whole_delta() {
    let read = read("clips.aup4");

    assert_eq!(read.sniffed.version, Version::Aup4);
    assert_eq!(read.sniffed.user_version, 0x0400_0001, "4.0.0.1");
    assert!(read.sniffed.version.declares_block_lengths());

    // Every waveblock declares its length, and every one of them agrees with the
    // block it points at. `audit` would have refused the file otherwise, so this
    // asserts the count rather than the agreement.
    assert_eq!(read.audit.lengths_checked, read.audit.refs);
    assert_eq!(read.audit.refs, 38);
    assert_eq!(read.audit.distinct, 26);

    // The table AUP4 adds. Emptied by the shrinker, because a second copy of a
    // document that no longer matches the live one is the exact trap a reader has
    // to avoid; its existence is the part that matters.
    assert_eq!(read.surveyed.history_generations, Some(0));

    // Tag 0x10, AUP4's only new record and the only length in the format that is
    // already a byte count. The payload is truncated to 64 bytes, so what this
    // checks is that the record was read as a blob of exactly the length it
    // declared.
    let blob = read
        .surveyed
        .document
        .events
        .iter()
        .find_map(|event| match event {
            vcw_import::Event::Attr {
                value: vcw_import::Value::Blob(bytes),
                ..
            } => Some(bytes),
            _ => None,
        })
        .expect("the thumbnail's 0x10 record");
    assert_eq!(blob.len(), 64);
    assert_eq!(
        &blob[..8],
        b"\x89PNG\r\n\x1a\n",
        "the surviving bytes are Audacity's own, and they are a PNG signature"
    );
}

#[test]
fn the_two_generations_of_one_project_describe_the_same_thing() {
    // An AUP3 and its AUP4 conversion. The conversion is lossless on the audio
    // layer, and this is the assertion that says so in CI: same tracks, same
    // clips, same blocks, same rate.
    let three = read("clips.aup3");
    let four = read("clips.aup4");

    assert_eq!(three.project.tracks.len(), four.project.tracks.len());
    assert_eq!(three.project.rate(), four.project.rate());
    assert_eq!(three.audit.refs, four.audit.refs);
    assert_eq!(three.audit.distinct, four.audit.distinct);
    assert_eq!(
        three.project.distinct_blocks(),
        four.project.distinct_blocks(),
        "the conversion preserves blockids, gaps and all"
    );

    for (three, four) in three.project.tracks.iter().zip(&four.project.tracks) {
        assert_eq!(three.rate, four.rate);
        assert_eq!(three.clips.len(), four.clips.len());
        for (three, four) in three.clips.iter().zip(&four.clips) {
            // In samples, not seconds. f64 timings re-round by up to 2.7e-15 s
            // across a conversion, so `==` on seconds is a test that fails for
            // no reason.
            assert_eq!(three.num_samples, four.num_samples);
            assert_eq!(
                three.origin_sample(192_000.0),
                four.origin_sample(192_000.0)
            );
        }
    }

    // Order is not stable across a conversion, so metadata compares as a map.
    assert_eq!(three.project.tags, four.project.tags);
}

#[test]
fn the_rate_trap_fixture_carries_two_rates_that_disagree() {
    // The single most consequential fact in the format, in CI. `project/@rate`
    // says 192000.0 and the audio is at 48000.0; a reader that trusted the
    // project attribute would play this file at four times speed.
    let read = read("rate-trap.aup3");

    assert_eq!(read.project.editor_rate_preference, Some(192_000.0));
    assert_eq!(read.project.rate(), Some(48_000.0));
    assert_ne!(
        read.project.rate(),
        read.project.editor_rate_preference,
        "this fixture is worthless if the two ever agree"
    );

    // The other page size. Both 4096 and 65536 occur in the corpus, so both are
    // in the fixtures.
    assert_eq!(read.sniffed.page_size, 65_536);

    // float32, which is what 24 of the user's 25 rips actually contain.
    assert_eq!(
        read.project.tracks[0].clips[0].sample_format,
        vcw_import::SampleFormat::Float32
    );

    // And real metadata, which an import has to carry across.
    assert_eq!(
        read.project.tags.get("ALBUM").map(String::as_str),
        Some("Stardonas (Transverse)")
    );
    assert_eq!(read.project.label_tracks[0].labels.len(), 3);
}

#[test]
fn a_file_that_is_not_an_audacity_project_is_refused() {
    // An empty SQLite database: valid SQLite, no application_id, no document.
    // The refusal has to come from the file's own header rather than from a
    // failed query, because the message a user sees for "this is not an Audacity
    // project" should say that.
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("not-audacity.aup3");
    rusqlite::Connection::open(&path)
        .expect("create")
        .execute_batch("CREATE TABLE t(x)")
        .expect("write something so the file exists on disk");

    let error = sniff::open(&path).expect_err("an empty database is not a project");
    assert!(
        matches!(error, vcw_import::Error::NotAudacity { found: 0, .. }),
        "got {error}"
    );
}
