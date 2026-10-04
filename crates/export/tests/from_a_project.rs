/*
 *  from_a_project.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-14's exit criterion: bit-exact extraction from a real project.
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

//! WP-14's exit criterion: bit-exact extraction from a real project.
//!
//! *"Bit-exact WAV extraction verified against source blocks"* - so this builds a
//! project the way the application does, records a known pattern into it through
//! `vcw-project`'s own capture writer, cuts it into tracks, exports, and compares
//! the data chunk of each file against the exact slice of what went in.
//!
//! Nothing here mocks the project layer. A splitter tested against a fake reader
//! proves the arithmetic and not the claim: the claim is that the bytes on the
//! turntable's side of the pipeline come back out unchanged, and every layer that
//! touched them in between is part of what is being tested. It is the same shape
//! of test as WP-10's gapless render, and for the same reason.

use std::path::Path;

use vcw_export::encoder::Container;
use vcw_export::error::Error;
use vcw_export::splitter::{self, Artwork, Progress, Request};
use vcw_project::persistence::{Config, Writer};
use vcw_project::{Project, release, side, track};
use vcw_types::vinyl::Side;
use vcw_types::{CaptureInfo, CaptureMode, CaptureState, SampleRate, StorageFormat};

/// A 1x1 RGB PNG, so the artwork path has real bytes to move.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x10, 0x50, 0x30, 0x00,
    0x00, 0x00, 0xa4, 0x00, 0x61, 0x34, 0x66, 0x7d, 0x72, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Every byte is a function of its own offset, so one wrong byte says where it
/// came from and a channel swap is not a subtle failure. The same pattern
/// `pcm`'s own tests use, on purpose.
fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// A project with one side of recorded audio, and the bytes that went in.
///
/// The tracks are deliberately not block-aligned: 1000, 40_001 and 90_123 land
/// inside blocks, which is the case the splitter exists to get right.
fn recorded(
    dir: &Path,
    format: StorageFormat,
    channels: u16,
    frames: u64,
) -> (Project, Vec<u8>, usize) {
    recorded_at(dir, 96_000, format, channels, frames)
}

/// The same, at a named sample rate.
///
/// Split out for the lossy containers, which are the first ones that care: 96
/// kHz is a rate MPEG never defined, so a project recorded at the default
/// cannot be exported as MP3 at all and the successful case needs 44.1.
fn recorded_at(
    dir: &Path,
    rate: u32,
    format: StorageFormat,
    channels: u16,
    frames: u64,
) -> (Project, Vec<u8>, usize) {
    let info = CaptureInfo {
        rate: SampleRate(rate),
        channels,
        storage_format: format,
        capture_mode: CaptureMode::Exclusive,
        host_api: Some("ALSA".into()),
        device_id: Some("hw:CARD=0,DEV=0".into()),
        device_name: Some("Cirrus Analog".into()),
        os_verified: false,
        os_report: None,
    };
    let frame_bytes = format.bytes_per_sample() * channels as usize;
    let project = Project::create(dir.join("export.vcw")).expect("create");
    let mut writer = Writer::begin(project, &info, Config::default()).expect("begin");
    let sent = pattern(frames as usize * frame_bytes);
    writer.push(&sent).expect("push");
    let (outcome, mut project, _) = writer
        .finish_with_project(CaptureState::Finalised)
        .expect("finish");

    side::ensure(&mut project, Side::A).expect("side");
    side::attach(&mut project, Side::A, outcome.capture_id).expect("attach");
    (project, sent, frame_bytes)
}

/// Fills in the release, so the naming template and the tags have something to
/// work with.
fn described(project: &mut Project) {
    let mut record = release::ensure(project).expect("release");
    record.album = "Trans-Europe Express".to_owned();
    record.album_artist = "Kraftwerk".to_owned();
    record.year = Some(1977);
    record.genres = vec!["Electronic".to_owned(), "Krautrock".to_owned()];
    record.label = "Kling Klang".to_owned();
    record.catalog = "1C 064-82 306".to_owned();
    record.country = "Germany".to_owned();
    record.discogs_id = Some("1234567".to_owned());
    release::store(project, &record).expect("store");
    release::put_artwork(project, release::Artwork::FRONT, "image/png", PNG, None).expect("art");
}

/// Adds titled tracks over frame ranges, in order, on side A.
fn titled(project: &mut Project, ranges: &[(u64, u64, &str)]) {
    titled_on(project, Side::A, ranges);
}

/// The same, on a named side.
fn titled_on(project: &mut Project, side: Side, ranges: &[(u64, u64, &str)]) {
    for &(start, end, title) in ranges {
        let id = track::add_track(project, side, start, end).expect("track");
        track::update(project, id, &track::Update::title(title)).expect("title");
    }
}

/// The data chunk of a WAV file, found by walking the chunks.
fn data_chunk(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("read");
    assert_eq!(
        &bytes[0..4],
        b"RIFF",
        "{} is not a RIFF file",
        path.display()
    );
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"data" {
            return bytes[at + 8..at + 8 + size].to_vec();
        }
        at += 8 + size + size % 2;
    }
    panic!("no data chunk in {}", path.display());
}

/// Finds a tool on `PATH`, or `None`.
fn tool(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// A callback that keeps nothing, for the tests that do not care.
fn quiet(_: Progress<'_>) {}

#[test]
fn every_wav_we_export_is_the_blocks_we_recorded() {
    // The exit criterion. Two formats where a stored frame and a WAV frame are
    // the same bytes, so the comparison is an identity and not a transform: if
    // this passes, the splitter, the reader, the block reassembly and the
    // container header are all handing on exactly what the device produced.
    for (format, channels) in [
        (StorageFormat::Int16, 2),
        (StorageFormat::Int32, 2),
        (StorageFormat::Int32, 4),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (mut project, sent, frame_bytes) = recorded(dir.path(), format, channels, 120_000);
        described(&mut project);
        let ranges = [
            (1_000u64, 40_001u64, "Europe Endless"),
            (40_001, 90_123, "The Hall of Mirrors"),
            (90_123, 120_000, "Showroom Dummies"),
        ];
        titled(
            &mut project,
            &[
                (ranges[0].0, ranges[0].1, ranges[0].2),
                (ranges[1].0, ranges[1].1, ranges[1].2),
                (ranges[2].0, ranges[2].1, ranges[2].2),
            ],
        );

        let out = dir.path().join("out");
        let request = Request::new(&out, Container::Wav);
        let plan = splitter::plan(project.conn(), &request).expect("plan");
        assert_eq!(plan.items.len(), 3);
        let report = splitter::run(project.conn(), &plan, &mut quiet).expect("run");
        assert_eq!(report.files, 3);
        assert_eq!(report.frames, plan.frames());

        for (item, &(start, end, title)) in plan.items.iter().zip(ranges.iter()) {
            let expected = &sent[start as usize * frame_bytes..end as usize * frame_bytes];
            assert_eq!(
                data_chunk(&item.path),
                expected,
                "{format:?} {channels}ch: {title} is not the frames {start}..{end} that went in"
            );
        }
    }
}

#[test]
fn a_padded_capture_exports_the_three_bytes_that_matter() {
    // `Int24Padded` is the one stored format a WAV frame is *not* a copy of: it
    // is a little-endian i32 holding a value in +-2^23, so the fourth byte is
    // sign extension and dropping it is lossless. This asserts both halves -
    // that the three bytes are the three bytes, and that the byte dropped was
    // only ever the sign.
    let dir = tempfile::tempdir().unwrap();
    let (mut project, sent, frame_bytes) =
        recorded(dir.path(), StorageFormat::Int24Padded, 2, 60_000);
    described(&mut project);
    titled(&mut project, &[(5_000, 55_555, "Franz Schubert")]);

    let out = dir.path().join("out");
    let plan = splitter::plan(project.conn(), &Request::new(&out, Container::Wav)).expect("plan");
    splitter::run(project.conn(), &plan, &mut quiet).expect("run");

    let stored = &sent[5_000 * frame_bytes..55_555 * frame_bytes];
    let expected: Vec<u8> = stored
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|sample| sample[..3].to_vec())
        .collect();
    assert_eq!(data_chunk(&plan.items[0].path), expected);
    // The pattern is not sign-extended, so this documents what the test is and
    // is not proving: the pad byte is dropped whatever is in it, which is only
    // lossless because a real capture writes the sign there. `convert`'s own
    // tests are where that invariant lives.
    assert_eq!(expected.len(), stored.len() / 4 * 3);
}

#[test]
fn a_flac_export_decodes_back_to_the_blocks() {
    // The same claim for the container people will actually archive in, checked
    // by the reference decoder rather than by us.
    let Some(flac) = tool("flac") else {
        eprintln!("skipped: no flac");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut project, sent, frame_bytes) = recorded(dir.path(), StorageFormat::Int16, 2, 90_000);
    described(&mut project);
    titled(&mut project, &[(2_048, 61_237, "Metropolis")]);

    let out = dir.path().join("out");
    let plan = splitter::plan(project.conn(), &Request::new(&out, Container::Flac)).expect("plan");
    splitter::run(project.conn(), &plan, &mut quiet).expect("run");

    let decoded = dir.path().join("decoded.wav");
    let said = std::process::Command::new(&flac)
        .args([
            "-d",
            "-f",
            "--totally-silent",
            "-o",
            decoded.to_str().unwrap(),
            plan.items[0].path.to_str().unwrap(),
        ])
        .output()
        .expect("flac -d");
    assert!(
        said.status.success(),
        "{}",
        String::from_utf8_lossy(&said.stderr)
    );
    assert_eq!(
        data_chunk(&decoded),
        &sent[2_048 * frame_bytes..61_237 * frame_bytes]
    );
}

#[test]
fn the_template_names_the_files_and_the_release_fills_it_in() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 60_000);
    described(&mut project);
    titled(
        &mut project,
        &[
            (0, 30_000, "Europe Endless"),
            (30_000, 60_000, "Hall of Mirrors"),
        ],
    );

    let out = dir.path().join("out");
    let mut request = Request::new(&out, Container::Flac);
    request.template = "{album_artist}/{year} {album}/{position} - {title}".to_owned();
    let plan = splitter::plan(project.conn(), &request).expect("plan");

    let relative: Vec<String> = plan
        .items
        .iter()
        .map(|item| {
            item.path
                .strip_prefix(&out)
                .expect("under the output directory")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        relative,
        [
            "Kraftwerk/1977 Trans-Europe Express/A1 - Europe Endless.flac",
            "Kraftwerk/1977 Trans-Europe Express/A2 - Hall of Mirrors.flac",
        ]
    );
}

#[test]
fn two_tracks_that_want_the_same_file_are_refused_before_anything_is_written() {
    // A template with no track number in it does this the moment a side has two
    // untitled tracks, and the second silently overwriting the first is the
    // worst outcome available. Refused at plan time, so the output directory
    // does not exist at all afterwards.
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 60_000);
    described(&mut project);
    titled(&mut project, &[(0, 30_000, ""), (30_000, 60_000, "")]);

    let out = dir.path().join("out");
    let mut request = Request::new(&out, Container::Wav);
    request.template = "{album}/{title}".to_owned();
    match splitter::plan(project.conn(), &request) {
        Err(Error::NameCollision { first, second, .. }) => {
            // The positions, so the message names the tracks rather than
            // their numbers within a side.
            assert_eq!((first.as_str(), second.as_str()), ("A1", "A2"));
        }
        other => panic!("expected a collision, got {other:?}"),
    }
    assert!(!out.exists(), "a refused plan left files behind");
}

#[test]
fn a_two_sided_record_numbers_its_tracks_across_the_disc() {
    // The first export of a real two-sided project refused with `tracks 2 and 2
    // both export to .../02 -.flac`, and the message named neither track
    // because both numbers were 2. `track::Record::number` is the number
    // *within its side* (§29), so on any record with a B side it repeats: one
    // file name for two tracks, and one album carrying two tracks numbered 2.
    // Nothing the operator did caused it and nothing they could do would avoid
    // it, short of titling every track.
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 60_000);
    described(&mut project);
    // Two faces on one capture, which is what a real flip produces: a side has
    // no extent of its own.
    let b = Side::from_letter('B').expect("B is a side");
    side::ensure(&mut project, b).expect("side B");
    let capture = side::load(project.conn(), Side::A)
        .expect("side A")
        .and_then(|row| row.capture)
        .expect("a capture behind side A");
    side::attach(&mut project, b, capture).expect("attach B");
    // Untitled on purpose: the title is the only thing that was hiding this.
    titled_on(
        &mut project,
        Side::A,
        &[(0, 20_000, ""), (20_000, 30_000, "")],
    );
    titled_on(
        &mut project,
        b,
        &[(30_000, 50_000, ""), (50_000, 60_000, "")],
    );

    let out = dir.path().join("out");
    let plan = splitter::plan(project.conn(), &Request::new(&out, Container::Flac))
        .expect("four untitled tracks over two sides collided");

    // The default template is VRipr's, and VRipr's `{tracknum}` was the alpha
    // position: `A1 - Title`, the number printed on the label.
    let names: Vec<String> = plan
        .items
        .iter()
        .map(|item| {
            item.path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    // `Untitled` rather than a dangling separator: these four names came off a
    // real record whose provider row had no titles, and `A1 -.flac` is what a
    // person then has to look at in a file manager.
    assert_eq!(
        names,
        [
            "A1 - Untitled.flac",
            "A2 - Untitled.flac",
            "B1 - Untitled.flac",
            "B2 - Untitled.flac"
        ]
    );

    // A tag's track number is unique within its disc and says nothing about
    // which face it came from, which is what every player assumes.
    let numbers: Vec<Option<u32>> = plan
        .items
        .iter()
        .map(|item| item.tags.track_number)
        .collect();
    assert_eq!(numbers, [Some(1), Some(2), Some(3), Some(4)]);
    let positions: Vec<String> = plan
        .items
        .iter()
        .map(|item| item.tags.extra[0].1.clone())
        .collect();
    assert_eq!(positions, ["A1", "A2", "B1", "B2"]);
    assert!(
        plan.items
            .iter()
            .all(|item| item.tags.disc_number == Some(1)),
        "one disc, two faces"
    );

    // Numeric numbering moves `{tracknum}` and leaves `{position}` alone,
    // because the position is provenance rather than presentation: it is the
    // one fact a vinyl rip has that a CD rip does not.
    let mut record = release::load(project.conn())
        .expect("load")
        .expect("a release");
    record.numbering = vcw_types::vinyl::Numbering::Numeric;
    release::store(&mut project, &record).expect("store");
    let mut request = Request::new(&out, Container::Flac);
    request.template = "{tracknum} {position}".to_owned();
    let plan = splitter::plan(project.conn(), &request).expect("plan");
    let names: Vec<String> = plan
        .items
        .iter()
        .map(|item| {
            item.path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        names,
        ["01 A1.flac", "02 A2.flac", "03 B1.flac", "04 B2.flac"]
    );
}

#[test]
fn a_template_with_a_typo_in_it_is_refused_with_a_suggestion() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 30_000);
    described(&mut project);
    titled(&mut project, &[(0, 30_000, "Neon Lights")]);

    let mut request = Request::new(dir.path().join("out"), Container::Wav);
    request.template = "{titel}".to_owned();
    match splitter::plan(project.conn(), &request) {
        Err(Error::UnknownTokens { tokens }) => {
            assert_eq!(tokens.len(), 1);
            assert!(tokens[0].contains("{title}"), "{tokens:?}");
        }
        other => panic!("expected unknown tokens, got {other:?}"),
    }
}

#[test]
fn a_project_with_no_tracks_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 30_000);
    described(&mut project);
    let request = Request::new(dir.path().join("out"), Container::Wav);
    assert!(matches!(
        splitter::plan(project.conn(), &request),
        Err(Error::Nothing)
    ));
}

#[test]
fn a_side_with_no_capture_behind_it_has_no_audio_to_cut() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 30_000);
    described(&mut project);
    titled(&mut project, &[(0, 30_000, "Neon Lights")]);
    side::detach(&mut project, Side::A).expect("detach");

    let request = Request::new(dir.path().join("out"), Container::Wav);
    assert!(matches!(
        splitter::plan(project.conn(), &request),
        Err(Error::NoAudio { side: 'A' })
    ));
}

#[test]
fn an_export_does_not_overwrite_what_is_already_there() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 30_000);
    described(&mut project);
    titled(&mut project, &[(0, 30_000, "Neon Lights")]);

    let out = dir.path().join("out");
    let request = Request::new(&out, Container::Wav);
    let first = splitter::export(project.conn(), &request, &mut quiet).expect("first");
    assert_eq!(first.files, 1);

    match splitter::plan(project.conn(), &request) {
        Err(Error::Exists { path }) => assert!(path.starts_with(&out)),
        other => panic!("expected a refusal, got {other:?}"),
    }

    // And with permission, the same export again.
    let mut again = request.clone();
    again.overwrite = true;
    let second = splitter::export(project.conn(), &again, &mut quiet).expect("second");
    assert_eq!(second.files, 1);
}

#[test]
fn the_cover_goes_in_the_tags_and_beside_the_files() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 60_000);
    described(&mut project);
    titled(
        &mut project,
        &[
            (0, 30_000, "Europe Endless"),
            (30_000, 60_000, "Hall of Mirrors"),
        ],
    );

    let out = dir.path().join("out");
    let plan = splitter::plan(project.conn(), &Request::new(&out, Container::Flac)).expect("plan");
    // One image per directory, not one per track.
    assert_eq!(plan.covers.len(), 1);
    let report = splitter::run(project.conn(), &plan, &mut quiet).expect("run");
    assert_eq!(report.covers, 1);

    let beside = &plan.covers[0];
    assert_eq!(beside.file_name().unwrap(), "folder.png");
    assert_eq!(std::fs::read(beside).expect("cover"), PNG);
    assert_eq!(
        plan.items[0]
            .tags
            .cover
            .as_ref()
            .map(|c| c.bytes.as_slice()),
        Some(PNG)
    );

    // And with artwork off, neither.
    let mut bare = Request::new(dir.path().join("bare"), Container::Flac);
    bare.artwork = Artwork::None;
    let plan = splitter::plan(project.conn(), &bare).expect("plan");
    assert!(plan.covers.is_empty());
    assert!(plan.cover.is_none());
    assert!(plan.items[0].tags.cover.is_none());
}

#[test]
fn the_tags_come_from_the_release_and_the_track() {
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 60_000);
    described(&mut project);
    titled(&mut project, &[(0, 30_000, "Europe Endless")]);

    let plan = splitter::plan(
        project.conn(),
        &Request::new(dir.path().join("out"), Container::Flac),
    )
    .expect("plan");
    let tags = &plan.items[0].tags;
    assert_eq!(tags.title, "Europe Endless");
    // No track artist on the row, so the release's, which is what a player
    // showing one artist per file needs.
    assert_eq!(tags.artist, "Kraftwerk");
    assert_eq!(tags.album, "Trans-Europe Express");
    assert_eq!(tags.year, Some(1977));
    assert_eq!(tags.genre, "Electronic;Krautrock");
    assert_eq!(tags.catalog, "1C 064-82 306");
    assert_eq!(tags.discogs_id, "1234567");
    assert_eq!(tags.track_number, Some(1));
    assert_eq!(tags.disc_number, Some(1));
    assert_eq!(tags.extra, [("VINYL_POSITION".to_owned(), "A1".to_owned())]);
}

#[test]
fn progress_counts_up_to_the_plans_own_total() {
    // What §35's `export-progress` is built on, so it has to be monotonic and it
    // has to arrive at the number the plan promised. A progress bar that stops
    // at 97% is a bug report.
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int32, 2, 120_000);
    described(&mut project);
    titled(
        &mut project,
        &[
            (0, 50_000, "Europe Endless"),
            (50_000, 120_000, "Hall of Mirrors"),
        ],
    );

    let out = dir.path().join("out");
    let plan = splitter::plan(project.conn(), &Request::new(&out, Container::Wav)).expect("plan");
    let mut seen: Vec<(usize, u64)> = Vec::new();
    let report = splitter::run(project.conn(), &plan, &mut |progress| {
        assert_eq!(progress.of, 2);
        seen.push((progress.index, progress.total));
    })
    .expect("run");

    assert!(
        seen.len() > 2,
        "only {} updates for 120k frames",
        seen.len()
    );
    assert!(
        seen.windows(2).all(|pair| pair[0].1 <= pair[1].1),
        "progress went backwards: {seen:?}"
    );
    assert_eq!(seen.last().expect("an update").1, plan.frames());
    assert_eq!(report.frames, plan.frames());
    assert_eq!(report.frames, 120_000);
}

#[test]
fn a_title_with_a_dot_in_it_still_gets_its_extension() {
    // `with_extension` would read this title as `Symphony No` plus an extension
    // of ` 5` and replace the ` 5`, which is how a classical side loses its
    // movement numbers. The extension is appended for exactly this reason.
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 30_000);
    described(&mut project);
    titled(&mut project, &[(0, 30_000, "Symphony No. 5")]);

    let out = dir.path().join("out");
    let mut request = Request::new(&out, Container::Flac);
    request.template = "{title}".to_owned();
    let plan = splitter::plan(project.conn(), &request).expect("plan");
    assert_eq!(
        plan.items[0].path.strip_prefix(&out).expect("under out"),
        Path::new("Symphony No. 5.flac")
    );
}

#[test]
fn a_track_number_carries_the_total_for_its_own_disc() {
    // `disc 2/2` beside a bare `track 6` is the shape a real export went out
    // in: `track_total` was hardcoded `None`, so every player showed the
    // numerator and nothing to divide it by. The denominator is per disc for
    // the same reason the numerator is - a tag's track number restarts on the
    // next record - so a double album is 1..9 of 9 then 1..8 of 8, and not
    // 1..17 of 17.
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 80_000);
    described(&mut project);
    let capture = side::load(project.conn(), Side::A)
        .expect("side A")
        .and_then(|row| row.capture)
        .expect("a capture behind side A");
    for letter in ['B', 'C', 'D'] {
        let side = Side::from_letter(letter).expect("a side");
        side::ensure(&mut project, side).expect("side");
        side::attach(&mut project, side, capture).expect("attach");
    }
    // Two tracks a face, so disc one holds four and disc two holds four, and a
    // release-wide total would be eight and wrong on both.
    titled_on(
        &mut project,
        Side::A,
        &[(0, 10_000, ""), (10_000, 20_000, "")],
    );
    titled_on(
        &mut project,
        Side::from_letter('B').expect("B"),
        &[(20_000, 30_000, ""), (30_000, 40_000, "")],
    );
    titled_on(
        &mut project,
        Side::from_letter('C').expect("C"),
        &[(40_000, 50_000, ""), (50_000, 60_000, "")],
    );
    titled_on(
        &mut project,
        Side::from_letter('D').expect("D"),
        &[(60_000, 70_000, ""), (70_000, 80_000, "")],
    );

    let out = dir.path().join("out");
    let plan = splitter::plan(project.conn(), &Request::new(&out, Container::Flac)).expect("plan");
    let numbered: Vec<(Option<u32>, Option<u32>, Option<u32>)> = plan
        .items
        .iter()
        .map(|item| {
            (
                item.tags.disc_number,
                item.tags.track_number,
                item.tags.track_total,
            )
        })
        .collect();
    assert_eq!(
        numbered,
        [
            (Some(1), Some(1), Some(4)),
            (Some(1), Some(2), Some(4)),
            (Some(1), Some(3), Some(4)),
            (Some(1), Some(4), Some(4)),
            (Some(2), Some(1), Some(4)),
            (Some(2), Some(2), Some(4)),
            (Some(2), Some(3), Some(4)),
            (Some(2), Some(4), Some(4)),
        ],
        "each disc numbers 1..4 of 4, and the second disc restarts"
    );

    // And the total does not depend on what else was in the export: a side
    // exported on its own is still `of 4`, or a file could not be re-made.
    let mut one_side = Request::new(&out, Container::Flac);
    one_side.sides = vec![Side::from_letter('D').expect("D")];
    let plan = splitter::plan(project.conn(), &one_side).expect("plan");
    assert_eq!(plan.items.len(), 2, "side D only");
    assert_eq!(
        plan.items
            .iter()
            .map(|item| (item.tags.track_number, item.tags.track_total))
            .collect::<Vec<_>>(),
        [(Some(3), Some(4)), (Some(4), Some(4))],
        "side D is tracks 3 and 4 of disc two, whatever was exported with it"
    );
}

#[test]
fn a_container_that_cannot_carry_the_capture_is_refused_by_the_plan() {
    // The plan is the thing an operator is invited to argue with for free, so a
    // container the capture cannot go into has to be refused there and not by
    // the first `Writer::create`. A `--dry-run` that printed `3 file(s) in
    // FLAC` for a float32 rip and then died on file one had already made the
    // directories by the time it told the truth.
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Float32, 2, 30_000);
    described(&mut project);
    titled(&mut project, &[(0, 30_000, "Franz Schubert")]);

    let out = dir.path().join("out");
    let error = splitter::plan(project.conn(), &Request::new(&out, Container::Flac))
        .expect_err("FLAC is an integer codec");
    assert!(
        matches!(error, Error::Unencodable { .. }),
        "the refusal should name the format and the container, got {error:?}"
    );
    assert!(
        !out.exists(),
        "a refused plan must not have made the output directory"
    );

    // The same capture as WAV is fine, which is the point of saying which
    // container rather than refusing the project.
    splitter::plan(project.conn(), &Request::new(&out, Container::Wav)).expect("WAV takes float32");
}

#[test]
#[cfg(any(feature = "mp3", feature = "ogg"))]
fn a_lossy_export_writes_the_span_of_audio_the_track_asked_for() {
    // The round-trip claim a lossy container can actually make. There is no
    // byte comparison to do - that is what lossy means - so what is checked is
    // the thing the splitter is responsible for either way: that the file holds
    // the cut the track describes and not the whole side, and that the plan's
    // paths and the report's byte count describe what is on disk.
    let Some(ffprobe) = tool("ffprobe") else {
        eprintln!("skipped: no ffprobe");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded_at(dir.path(), 44_100, StorageFormat::Int16, 2, 441_000);
    described(&mut project);
    // Ten seconds of capture cut into two tracks of four and three, with a gap
    // between them and lead-out after - so a writer that ignored the range
    // would come back at ten seconds and a writer that ran the two together
    // would come back at seven.
    titled(
        &mut project,
        &[
            (0, 176_400, "Europe Endless"),
            (220_500, 352_800, "The Hall of Mirrors"),
        ],
    );

    for (container, seconds) in [
        #[cfg(feature = "mp3")]
        (
            Container::Mp3(vcw_export::encoder::Quality::High),
            [4.0, 3.0],
        ),
        #[cfg(feature = "ogg")]
        (
            Container::OggVorbis(vcw_export::encoder::Quality::High),
            [4.0, 3.0],
        ),
    ] {
        let out = dir.path().join(format!("out-{}", container.extension()));
        let plan = splitter::plan(project.conn(), &Request::new(&out, container)).expect("plan");
        assert_eq!(plan.items.len(), 2);
        for item in &plan.items {
            assert_eq!(
                item.path.extension().and_then(|ext| ext.to_str()),
                Some(container.extension()),
                "the plan named {}",
                item.path.display()
            );
        }
        let report = splitter::run(project.conn(), &plan, &mut quiet).expect("run");

        let on_disk: u64 = plan
            .items
            .iter()
            .map(|item| std::fs::metadata(&item.path).expect("stat").len())
            .sum();
        assert_eq!(
            report.bytes, on_disk,
            "{container}: the report and the filesystem disagree"
        );
        assert_eq!(report.files, 2);
        assert_eq!(report.frames, 176_400 + 132_300);

        for (item, expected) in plan.items.iter().zip(seconds) {
            let said = std::process::Command::new(&ffprobe)
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "default=nw=1:nk=1",
                    item.path.to_str().unwrap(),
                ])
                .output()
                .expect("ffprobe");
            let duration: f64 = String::from_utf8_lossy(&said.stdout)
                .trim()
                .parse()
                .unwrap_or_else(|why| {
                    panic!(
                        "ffprobe said {:?}: {why}",
                        String::from_utf8_lossy(&said.stdout)
                    )
                });
            assert!(
                (duration - expected).abs() < 0.1,
                "{container}: {} is {duration} s and the track is {expected} s",
                item.path.display()
            );
        }
    }
}

#[test]
#[cfg(feature = "mp3")]
fn a_rate_mp3_cannot_carry_is_refused_by_the_plan_and_offered_the_others() {
    // The same shape as the float32-into-FLAC refusal above, for the limit that
    // will actually come up: 96 kHz is a perfectly ordinary capture rate and the
    // one `recorded` uses by default, and MPEG stops at 48. Resampling it would
    // be a filter choice, which belongs to a person and not to an export.
    let dir = tempfile::tempdir().unwrap();
    let (mut project, _, _) = recorded(dir.path(), StorageFormat::Int16, 2, 30_000);
    described(&mut project);
    titled(&mut project, &[(0, 30_000, "Showroom Dummies")]);

    let out = dir.path().join("out");
    let error = splitter::plan(
        project.conn(),
        &Request::new(&out, Container::Mp3(vcw_export::encoder::Quality::High)),
    )
    .expect_err("96 kHz is not an MPEG rate");
    assert!(matches!(error, Error::Unencodable { .. }), "got {error:?}");
    let said = error.to_string();
    assert!(
        said.contains("96000"),
        "the refusal has to name the rate: {said}"
    );
    assert!(
        !out.exists(),
        "a refused plan must not have made the output directory"
    );

    // And the containers that do take it, which is the point of refusing the
    // one rather than the project. Ogg has no rate table at all.
    #[cfg(feature = "ogg")]
    splitter::plan(
        project.conn(),
        &Request::new(
            &out,
            Container::OggVorbis(vcw_export::encoder::Quality::High),
        ),
    )
    .expect("Ogg takes 96 kHz");
    splitter::plan(project.conn(), &Request::new(&out, Container::Flac))
        .expect("FLAC takes it too");
}
