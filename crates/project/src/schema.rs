/*
 *  schema.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Schema v1, and the constants that identify a `.vcw` file.
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

//! Schema v1, and the constants that identify a `.vcw` file.
//!
//! The DDL lives here as text rather than being built by a query builder, because
//! it is the thing the format *is*. It is diffed against a real Audacity project in
//! CI (`tests/aup4_shape.rs`) and rendered into `docs/SCHEMA.md` by
//! [`crate::doc::markdown`], so there is one definition and two checks on it.

/// The format is little-endian, and this is where that stops being an assumption.
///
/// D4 forbids the write path from converting a sample, so on a big-endian host a
/// capture would land in the file big-endian and `docs/SCHEMA.md`'s promise to a
/// third-party reader would be false. WP-18 made that promise explicit, so the
/// build refuses rather than writing a file nobody else can read: every Tier 1
/// and Tier 2 target is little-endian, so this costs nothing today and turns a
/// silent corruption into a compile error if that ever changes.
///
/// The fix, if a big-endian port is ever wanted, is a byte swap on the write and
/// read paths - not a relaxation of this line.
const _: () = assert!(
    cfg!(target_endian = "little"),
    "the .vcw format is little-endian; see docs/SCHEMA.md, 'Sample format codes'"
);

/// SQLite `application_id` for a `.vcw` project: ASCII `"VCW\0"`.
///
/// Distinct from Audacity's `0x41554459` (`"AUDY"`), which both AUP3 and AUP4 use.
pub const APPLICATION_ID: u32 = 0x5643_5700;

/// The newest schema version this build understands.
///
/// Stored in SQLite's `user_version`. A plain ascending integer, deliberately not
/// Audacity's packed dotted quad - we have one number to express and no reason to
/// pack four into it.
///
/// v1 is capture; v2 adds the §29 vinyl data model; v3 adds `captures.capture_eq`
/// (§51); v4 adds the two release-level intents the new-project prompt asks for,
/// `releases.is_mono` and `releases.riaa_eq`; v5 replaces that second one with a
/// named curve, `playback_eq`, on releases, sides and tracks; v6 adds `defects`,
/// the §46 Phase 3 repair instructions. [`FORMAT_VERSION`] has not moved with any
/// of them, because nothing an older version wrote means anything different now: a
/// v1 or v2 capture has an unrecorded equalization provenance, and `'unknown'` is
/// what that is, not a changed meaning for a column that already existed; a pre-v4
/// release has no stated intent, and `0` is what that is; v4's `riaa_eq = 1` means
/// exactly `playback_eq = 'riaa'`, which is why v5 can carry it across rather than
/// asking again; and a project with no `defects` rows is one nobody has run a
/// detector over, which is what every project before v6 is.
pub const SCHEMA_VERSION: u32 = 7;

/// The project-format version: the *meaning* of the schema, as opposed to its shape.
///
/// §16 asks for both. They move independently: adding a table bumps
/// [`SCHEMA_VERSION`] alone, while changing what an existing column means - a unit,
/// an encoding, an invariant - bumps this too, and that is the one that makes an
/// older reader unsafe rather than merely incomplete.
pub const FORMAT_VERSION: u32 = 1;

/// The file extension. One file, movable, self-contained (§12).
pub const EXTENSION: &str = "vcw";

/// Page size for a new project.
///
/// 64 KiB, which is what Audacity uses for full-length projects and what S2
/// benchmarked against. Sample blobs are ~1 MiB, so a large page keeps a block to a
/// short chain of overflow pages. Must be set before the first table exists, which
/// is why it is applied on a freshly created file and never on open.
///
/// S2's page-size sweep is still outstanding, so treat this as measured-adjacent
/// rather than tuned: it is Audacity's choice, validated by not being a problem
/// across a 90-minute soak.
pub const PAGE_SIZE: u32 = 65_536;

/// Samples per block, per channel, at 48 kHz - 250 ms (D3, provisional from S2).
///
/// The number is a recovery-granularity decision, not a throughput one. S2 found
/// throughput to be a non-issue at 24/192, so the budget buys a smaller worst-case
/// loss window instead: crash loss is commit granularity plus the driver buffer,
/// and this is the commit granularity.
pub const BLOCK_MILLIS: u32 = 250;

/// Samples per `summary256` triplet, matching Audacity exactly.
pub const SUMMARY_256_STRIDE: u32 = 256;

/// Samples per `summary64k` triplet, matching Audacity exactly.
pub const SUMMARY_64K_STRIDE: u32 = 65_536;

/// Schema v1.
///
/// Two halves. `sampleblocks` is Audacity's table, column for column and
/// constraint for constraint - including the absence of `NOT NULL`, so the diff
/// against a real `.aup4` is literal rather than approximate. Everything else is
/// the superset: the provenance, diagnostics and versioning Audacity has nowhere to
/// put, and which §10, §15 and §16 require.
pub const SCHEMA_V1: &str = r#"
-- Audacity's table, reproduced exactly. Column names, types, order, the
-- AUTOINCREMENT primary key and the absence of NOT NULL are all deliberate:
-- tests/aup4_shape.rs diffs this against DDL extracted from a real project.
-- Integrity is enforced next door in capture_blocks and by validate(), not by
-- constraints that would break the match.
CREATE TABLE sampleblocks (
    -- Never reused and never updated (D4). A block is written once; edits
    -- produce new blocks and leave the old ones for the undo history.
    blockid      INTEGER PRIMARY KEY AUTOINCREMENT,
    -- (bytes_per_sample << 16) | type_code. See the format table above.
    sampleformat INTEGER,
    -- Minimum sample value in the block, as f32. Audacity's whole-block summary.
    summin       REAL,
    -- Maximum sample value in the block, as f32.
    summax       REAL,
    -- RMS across the block, as f32.
    sumrms       REAL,
    -- (min, max, rms) f32 triplets, one per 256 samples. The waveform pyramid.
    summary256   BLOB,
    -- The same, one triplet per 65536 samples.
    summary64k   BLOB,
    -- The audio itself, native-endian, exactly as captured (D4: no conversion
    -- on the write path).
    samples      BLOB
);

-- One row per capture session (§13, §15). A session is unfinished exactly when
-- finished_at IS NULL, which is what recovery looks for on the next launch.
CREATE TABLE captures (
    -- Stable for the life of the project; referenced by every block.
    capture_id     INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Hz, as negotiated with the device. Authoritative, unlike Audacity's
    -- project/@rate, which is only an editor preference.
    sample_rate    INTEGER NOT NULL,
    -- Channel count. Blocks are stored per channel, never interleaved.
    channels       INTEGER NOT NULL,
    -- StorageFormat code, matching sampleblocks.sampleformat for this capture.
    storage_format INTEGER NOT NULL,
    -- How the stream was opened: 'exclusive', 'native' or 'shared' (§9), or
    -- 'imported' where no stream was opened because the audio came out of an
    -- Audacity project (§12). CaptureMode's spellings; a request, not a
    -- confirmed outcome.
    capture_mode   TEXT    NOT NULL,
    -- CPAL host, e.g. 'ALSA', 'WASAPI', 'CoreAudio'. Recorded for provenance.
    host_api       TEXT,
    -- Stable device identifier where the platform offers one.
    device_id      TEXT,
    -- Human-readable device name at the time of capture.
    device_name    TEXT,
    -- §9 and S1: bit-perfection is never claimed on CPAL's word. 0 until the
    -- platform verifier has confirmed the negotiated format against the OS.
    os_verified    INTEGER NOT NULL DEFAULT 0,
    -- What the OS reported, verbatim, so a later reader can audit the claim.
    os_report      TEXT,
    -- Unix seconds at stream start.
    started_at     INTEGER NOT NULL,
    -- Unix seconds at clean stop. NULL means interrupted: recovery's signal.
    finished_at    INTEGER,
    -- Frames committed per channel. Updated as blocks land, so it survives a crash.
    frames         INTEGER NOT NULL DEFAULT 0,
    -- 'recording', 'finalised' or 'interrupted'. validate() rejects anything else.
    state          TEXT    NOT NULL DEFAULT 'recording'
    -- Schema v3 adds capture_eq here: 'flat', 'riaa' or 'unknown' (§51), the
    -- equalization applied by the hardware upstream. Added by migration rather
    -- than written into this statement, because migration 1 *is* this text and a
    -- fresh project has to take the same path an upgrade does.
);

-- The superset half: where each sample block came from and how to find it again.
-- Audacity keeps this in its document blob; we keep it in a table, because
-- recovery has to work from committed rows alone with no document to parse.
CREATE TABLE capture_blocks (
    -- One row per sample block, sharing its key. Every block a .vcw holds has
    -- one, imported audio included: WP-20 re-blocks an Audacity timeline through
    -- the capture writer rather than adopting its rows, because a clip's trim
    -- almost never falls on a 262,144-sample boundary and an adopted block would
    -- need a sample offset this table has no column for and validate() no way to
    -- check. The cost is one copy at import; the gain is that an imported
    -- capture is structurally identical to a recorded one.
    blockid      INTEGER PRIMARY KEY REFERENCES sampleblocks(blockid),
    -- The session this block belongs to.
    capture_id   INTEGER NOT NULL REFERENCES captures(capture_id),
    -- Zero-based channel index.
    channel      INTEGER NOT NULL,
    -- Zero-based position within the channel. Contiguous, with no gaps.
    sequence     INTEGER NOT NULL,
    -- Frame offset from the start of the capture. Redundant against sequence and
    -- frame_count by design: validate() checks them against each other.
    start_frame  INTEGER NOT NULL,
    -- Frames in this block. The last block of a capture is usually short.
    frame_count  INTEGER NOT NULL,
    -- CRC32 of the samples blob, computed before the write. Detects bit rot that
    -- SQLite's own integrity check cannot see.
    checksum     INTEGER NOT NULL,
    -- Unix seconds at commit. Bounds how much a crash can have cost.
    committed_at INTEGER NOT NULL,
    UNIQUE (capture_id, channel, sequence)
);

-- Ordered playback and recovery both walk the timeline, and both do it per channel.
CREATE INDEX capture_blocks_timeline
    ON capture_blocks (capture_id, channel, start_frame);

-- Ours, not Audacity's, and the difference between a zoomed-out waveform drawing
-- in milliseconds and in seconds (§37).
--
-- A sampleblocks row carries a 192 KB samples blob at 24/192, so it occupies
-- several 64 KiB pages. Reading nothing but summin/summax/sumrms still costs one
-- page fault per block, and a 26-minute side is 12,528 blocks: 784 MiB of page
-- reads to obtain 150 KB of triplets. Measured cold on ext4, that is 3.77 s.
-- This index holds the three values beside the key, so the block level is served
-- entirely from it: 9 pages, 576 KiB against a 2.3 GiB project, and 14 ms.
CREATE INDEX sampleblocks_levels
    ON sampleblocks (blockid, summin, summax, sumrms);

-- The same trick one rung finer, and it is not free: this one duplicates the
-- 256-frame triplets rather than three floats, which on a 24/192 side is 28 MiB
-- against 2.3 GiB - 1.2%. It buys the zoom an editor actually works at. An
-- eight-minute span at 1920 px is 1,916 blocks per channel, and reaching their
-- summary256 through the row costs 594 ms per channel cold; through this index,
-- 29 ms. Without it that drawing takes 1.35 s, and §37 asks for sub-second.
--
-- It repeats the whole-block triplet as well, twelve bytes beside two kilobytes,
-- because the reader falls back to it when a block has no summary blob. Leaving
-- those three columns out makes the index a lookup rather than a covering one,
-- SQLite fetches the row after all, and the whole 28 MiB buys nothing - measured.
--
-- There is no matching index for summary64k. Nothing VCW writes ever reads that
-- rung - it is coarser than a 250 ms block and exists for AUP4 compatibility
-- (§49) - and the blocks that do need it, imported from Audacity, have no
-- `capture_blocks` row and so never reach this query. If import ever makes it
-- hot, measure it then.
CREATE INDEX sampleblocks_summary256
    ON sampleblocks (blockid, summin, summax, sumrms, summary256);

-- §10 requires overruns, underruns, dropped frames and stream errors counted and
-- *persisted*: they belong to the recording, not to the process that made it.
CREATE TABLE capture_diagnostics (
    -- One row per capture, created with it.
    capture_id     INTEGER PRIMARY KEY REFERENCES captures(capture_id),
    -- Times the capture ring filled before the writer drained it.
    overruns       INTEGER NOT NULL DEFAULT 0,
    -- Times the device callback found no data ready.
    underruns      INTEGER NOT NULL DEFAULT 0,
    -- Frames known to be lost. Non-zero means the capture is not bit-perfect.
    dropped_frames INTEGER NOT NULL DEFAULT 0,
    -- Stream errors reported by the host.
    stream_errors  INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds of the last update to this row.
    updated_at     INTEGER NOT NULL
);

-- §16's four versions, plus project configuration. Key/value because the set grows
-- with every work package and a migration per setting is not a good trade.
CREATE TABLE meta (
    -- Dotted key, e.g. 'created.by', 'format_version'. See meta.rs for the
    -- required set, which is checked on open.
    key   TEXT PRIMARY KEY,
    -- Always text. Callers parse; the schema does not pretend to types it
    -- cannot enforce.
    value TEXT NOT NULL
);

-- What has been applied, when, and by which build. §16 requires transactional
-- migrations; this is the record that makes a half-applied one detectable.
CREATE TABLE schema_migrations (
    -- Matches user_version once the run completes.
    version     INTEGER PRIMARY KEY,
    -- The migration's own description, copied at apply time.
    description TEXT    NOT NULL,
    -- Unix seconds.
    applied_at  INTEGER NOT NULL,
    -- Crate name and version of the build that applied it.
    applied_by  TEXT    NOT NULL
);
"#;

/// Schema v2: the §29 vinyl data model, and §31's non-destructive edits.
///
/// Nothing here touches the capture half, and that is the point. §4.1 and §31 make
/// every edit a record *about* the audio rather than a change to it: a split writes
/// two boundary rows, a merge deletes two, and `sampleblocks` is not read, let alone
/// written, by any of it. `tests/editing.rs` asserts that as a property rather than
/// as an intention.
///
/// # Why a track has no frame columns
///
/// A track *is* its two boundaries, so it holds their ids and no positions of its
/// own. The alternative - `start_frame` and `end_frame` on the track, beside an
/// `at_frame` on the boundary - is two places to store one fact, and the first edit
/// that updates one and not the other leaves a project that disagrees with itself.
/// Moving a boundary therefore moves whichever tracks it bounds, which is not a
/// consequence to work around: it is what moving a boundary means.
pub const SCHEMA_V2: &str = r#"
-- The release being captured (§29, §32). One per project: §29's topology is
-- Project -> Release -> Disc -> Side -> Track, and a project is one record. The
-- CHECK enforces it rather than a convention doing so, because every query below
-- would otherwise have to decide what two releases in one project mean. Relaxing
-- it later is a table rebuild, which is what migrations are for.
CREATE TABLE releases (
    -- Always 1. See above.
    release_id     INTEGER PRIMARY KEY CHECK (release_id = 1),
    -- The release title. Empty string rather than NULL for the text a person
    -- types, so a caller never has to distinguish "not set" from "set to nothing".
    album          TEXT    NOT NULL DEFAULT '',
    -- The credited artist for the release. A track carries its own only when it
    -- differs, which is how a compilation is told from an album.
    album_artist   TEXT    NOT NULL DEFAULT '',
    -- Release year. NULL is unknown, and unknown is common on a reissue.
    year           INTEGER,
    -- Normalized genres (§32), '; '-separated in order. The same spelling
    -- `vcw metadata genres` prints, and the order a tagger writes them in.
    genres         TEXT    NOT NULL DEFAULT '',
    -- The record label, as the provider or the person spells it. Discogs and
    -- MusicBrainz disagree about this more often than about anything else.
    label          TEXT    NOT NULL DEFAULT '',
    -- The catalog number off the label: the one identifier a vinyl pressing
    -- reliably carries, and the one a person searches by.
    catalog        TEXT    NOT NULL DEFAULT '',
    -- Country of the pressing. Part of telling two pressings apart (§28).
    country        TEXT    NOT NULL DEFAULT '',
    -- Barcode, where a sleeve has one. NULL on anything old enough not to.
    barcode        TEXT,
    -- Composer (§32), for the classical and soundtrack cases where it is the
    -- field that matters.
    composer       TEXT    NOT NULL DEFAULT '',
    -- Free text a person added about this copy: the pressing, the condition, the
    -- shop. Exported as a comment tag.
    comments       TEXT    NOT NULL DEFAULT '',
    -- How many discs the release has. The sides follow from it (§29), and it is
    -- stored because a project may hold fewer sides than the release has. It is
    -- raised to cover any side that is created, so it is never lower than the
    -- sides on disk; identification and the operator are what set it higher.
    discs          INTEGER NOT NULL DEFAULT 1,
    -- 'alpha' (A1, B2), 'numeric' (01 within the side) or 'sequence' (01..0n
    -- across the disc). Numbering's three spellings; alpha is VRipr's and the
    -- one printed on the label. An unknown spelling reads back as alpha.
    numbering      TEXT    NOT NULL DEFAULT 'alpha',
    -- MusicBrainz release id, where identification found one (§32).
    musicbrainz_id TEXT,
    -- Discogs release id, likewise. Both are kept rather than the search that
    -- found them, because the id is what a later lookup can use again.
    discogs_id     TEXT,
    -- §26: automatic identification shall never silently replace what a person
    -- confirmed. 1 once a person has accepted this metadata.
    confirmed      INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds of the last change to this row.
    updated_at     INTEGER NOT NULL
);

-- Cover art (§32), stored in the project because §12 makes the file
-- self-contained: a project that referenced an image on disk would export
-- differently on a different machine.
CREATE TABLE release_artwork (
    -- Surrogate key. A release may hold several images.
    artwork_id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- The release it belongs to, which is always release 1.
    release_id INTEGER NOT NULL REFERENCES releases(release_id),
    -- 'front', 'back', 'label' or 'other'. Front is what a tagger embeds.
    role       TEXT    NOT NULL DEFAULT 'front',
    -- The sniffed type, not the one the server claimed: vcw-metadata refuses a
    -- download whose bytes do not start with an image it recognizes.
    mime       TEXT    NOT NULL,
    -- Pixel width, where the provider stated one. NULL is "not known", never 0.
    width      INTEGER,
    -- Pixel height, likewise.
    height     INTEGER,
    -- Where it came from, for provenance. Never carries a credential (§39),
    -- because vcw-metadata puts those in headers and not in URLs.
    source_url TEXT,
    -- Unix seconds at download.
    fetched_at INTEGER NOT NULL,
    -- The image itself. Last in the row, so reading the columns above does not
    -- fault in the image.
    bytes      BLOB    NOT NULL
);

-- One side of one disc (§29), and the capture that produced it. A side is the
-- natural unit of vinyl capture, which is why the recording attaches here and not
-- to the release.
CREATE TABLE sides (
    -- Surrogate key, referenced by boundaries and tracks.
    side_id    INTEGER PRIMARY KEY AUTOINCREMENT,
    -- The release this side belongs to, which is always release 1.
    release_id INTEGER NOT NULL REFERENCES releases(release_id),
    -- Zero-based side index: A is 0, D is 3. The disc is index / 2 + 1 and the
    -- letter is 'A' + index, so neither is stored - a stored letter cannot say
    -- that C follows B. See vcw_types::vinyl::Side.
    side_index INTEGER NOT NULL,
    -- The capture holding this side's audio, or NULL for a side not yet recorded.
    -- Two sides may share one capture: recording both faces in a single take is a
    -- real thing people do, and then the frames tell them apart.
    capture_id INTEGER REFERENCES captures(capture_id),
    -- A side title where the label prints one. Rare, and not the same as a track.
    title      TEXT,
    -- Unix seconds at creation.
    created_at INTEGER NOT NULL,
    -- A side letter appears once per release.
    UNIQUE (release_id, side_index)
);

-- Track boundaries, with the evidence behind them (§23, §24). A boundary is a
-- record in its own right, not a column on a track: §24 requires position,
-- confidence, provenance and supporting evidence, and §31 requires locking one.
CREATE TABLE track_boundaries (
    -- Surrogate key. Referenced by the track it bounds, if any.
    boundary_id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- The side it is on.
    side_id     INTEGER NOT NULL REFERENCES sides(side_id),
    -- Frames from the start of the side's capture. Frames, not seconds: a rate is
    -- a property of the capture, and a boundary in seconds would drift with it.
    at_frame    INTEGER NOT NULL,
    -- 'start' or 'end'. Edge's two spellings. A track's end is not the next
    -- track's start - on vinyl there is a gap between them, and it belongs to
    -- neither.
    edge        TEXT    NOT NULL,
    -- 0..=1, as the resolver decided it. Never a product of agreeing detectors:
    -- see vcw_signal::resolve, rule 3.
    confidence  REAL    NOT NULL DEFAULT 1.0,
    -- Provenance's kebab-case name: what decided the position.
    provenance  TEXT    NOT NULL,
    -- Every provenance that reported this boundary, '+'-joined, so "three
    -- detectors agree" survives into the project. The resolver's sources list.
    sources     TEXT    NOT NULL DEFAULT '',
    -- The measurements behind it (§24), as 'name=value;name=value'. Names are the
    -- resolver's, provenance-prefixed, so silence.contrast-db and hmm.posterior sit
    -- side by side. Text rather than a table because it is read whole, by a person,
    -- and never queried by name.
    evidence    TEXT    NOT NULL DEFAULT '',
    -- 1 if automatic analysis may not move it (§24). Set by confirming a boundary,
    -- and the reason re-analysis is safe to re-run over a side someone has edited.
    locked      INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds at creation.
    created_at  INTEGER NOT NULL,
    -- Unix seconds at the last move, lock or unlock.
    updated_at  INTEGER NOT NULL,
    -- One boundary per frame per edge. Two detectors landing on the same frame is
    -- one boundary, which the resolver already decided before this row was written.
    UNIQUE (side_id, at_frame, edge)
);

-- Every read of a side walks it in time order: drawing it, playing it, splitting it.
CREATE INDEX track_boundaries_timeline
    ON track_boundaries (side_id, at_frame);

-- A track: two boundaries, and the metadata naming what is between them (§29,
-- §31, §32).
CREATE TABLE tracks (
    -- Surrogate key. Stable across renumbering, which is why the number is not it.
    track_id       INTEGER PRIMARY KEY AUTOINCREMENT,
    -- The side it is on. Changing it moves the track, boundaries and all.
    side_id        INTEGER NOT NULL REFERENCES sides(side_id),
    -- One-based within the side: the 1 of A1. Contiguous after any edit, which is
    -- what renumbering maintains.
    number         INTEGER NOT NULL,
    -- Where the track begins. The track *is* its boundaries and holds no frame
    -- positions of its own, so there is nothing that can disagree with them.
    start_boundary INTEGER NOT NULL REFERENCES track_boundaries(boundary_id),
    -- Where it ends.
    end_boundary   INTEGER NOT NULL REFERENCES track_boundaries(boundary_id),
    -- The track title. Empty until something names it.
    title          TEXT    NOT NULL DEFAULT '',
    -- NULL means the release's album artist, which is the common case. Set only
    -- where the track credits someone else.
    artist         TEXT,
    -- Composer, where it differs from the release's or matters per track.
    composer       TEXT,
    -- Free text a person added about this track.
    comments       TEXT,
    -- MusicBrainz recording id, where identification found one (§32).
    musicbrainz_id TEXT,
    -- 1 once a person has accepted this track's metadata (§26).
    confirmed      INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds of the last change to this row.
    updated_at     INTEGER NOT NULL,
    -- Numbering is contiguous and unique within a side.
    UNIQUE (side_id, number),
    -- A boundary bounds at most one track, and a second one claiming it is a bug
    -- worth failing on rather than validating after the fact.
    UNIQUE (start_boundary),
    UNIQUE (end_boundary)
);
"#;

/// Schema v3: the capture's equalization provenance (§51).
///
/// One column, and the whole of its design is the default. `'unknown'` is what a
/// capture written by v1 or v2 gets, which is the truth about those rows: nobody was
/// asked and nothing was recorded. Defaulting to `'riaa'` would have been the
/// plausible choice - an ordinary phono stage is overwhelmingly the common case - and
/// would have silently fabricated provenance for every project already on disk,
/// which a Phase 3 processing chain would then read as a fact and invert.
///
/// `ALTER TABLE ... ADD COLUMN` with a `NOT NULL DEFAULT` is a metadata-only change
/// in SQLite: it rewrites no rows and so costs nothing on a 2.33 GiB project, which
/// is the only reason a migration can be run on open rather than offered as a job.
pub const SCHEMA_V3: &str = r#"
-- Equalization the signal already carried when it reached the sound card (§51):
-- 'flat' for a preamp that applied no curve, 'riaa' for one that applied RIAA,
-- 'unknown' when nobody said. Playback equalization needs it and it cannot be recovered
-- from the audio, so it is recorded from the first capture, years before the curves ship.
ALTER TABLE captures ADD COLUMN capture_eq TEXT NOT NULL DEFAULT 'unknown';
"#;

/// Schema v4: what the operator said about the record, before a note was captured.
///
/// Two booleans on the one release row, because both are answers to the setup
/// prompt rather than anything identification or analysis can find out. A mono
/// pressing is not detectable from a stereo transfer of it - a mono groove played
/// with a stereo cartridge gives two channels that are *nearly* identical and
/// never exactly so - and no equalization curve is recoverable from the audio it
/// was applied to.
///
/// Neither one changes a byte of what is captured. §9 governs the capture path and
/// §51 says equalization is "a non-destructive stored decision, held with the
/// project's other edit instructions and applied on playback, render and export";
/// the fold to mono is the same kind of decision, made at export and never on the
/// way in. The stereo capture of a mono record stays stereo in the project, which
/// is what makes the flag free to change your mind about.
///
/// `riaa_eq` is distinct from `captures.capture_eq` and the two are easy to
/// confuse: `capture_eq` records what the signal *already carried* when it reached
/// the sound card, which is the input to this decision, and this is the decision.
///
/// ponytail: one boolean where §51 eventually wants a named curve selectable per
/// side and overridable per track. RIAA is the curve for every record cut after
/// 1954, which is nearly all of them, so a boolean buys the common case now;
/// widening it to a curve reference is another additive migration when Phase 3
/// ships the other eight curves.
pub const SCHEMA_V4: &str = r#"
-- Whether this pressing is mono, as the operator stated it at setup. Export sums
-- the captured channels to one; capture and the project stay stereo, because the
-- two channels of a stereo cartridge in a mono groove are not identical and
-- discarding one on the way in cannot be undone.
ALTER TABLE releases ADD COLUMN is_mono INTEGER NOT NULL DEFAULT 0;
-- Whether the RIAA curve is to be applied on playback and on export (§51). Not
-- what the capture arrived with - that is captures.capture_eq, and it is the input
-- to this decision rather than the decision. 0 on every project written before v4,
-- which is the truth about them: nobody was asked.
ALTER TABLE releases ADD COLUMN riaa_eq INTEGER NOT NULL DEFAULT 0;
"#;

/// Schema v5: the cutting characteristic, named rather than implied (§51).
///
/// v4's `riaa_eq` was a boolean, and its own note said what was wrong with it: one
/// bit can say "RIAA or nothing" and cannot name any of the fifteen characteristics
/// that came before RIAA or ran alongside it. §51 asks for nine by name, a
/// user-defined one, and a selection that is "selectable per side, overridable per
/// track". So the boolean becomes a curve id - the `id` of a row in the catalog
/// `vcw eq list` prints - on all three levels.
///
/// **`riaa_eq` is dropped rather than kept.** It would otherwise be a second place
/// the same decision could be written, and the two could disagree; `riaa_eq = 1`
/// migrates to `playback_eq = 'riaa'`, which is all the information it held.
/// Dropping a column is forward-only, which these migrations already are.
///
/// **NULL means nobody has said**, at every level, and that is why the column is
/// nullable rather than `NOT NULL DEFAULT 'riaa'`. A track inherits its side, a side
/// inherits the release, and a release with nothing stated gets the application's
/// default - see `vcw_eq::in_force`. Writing `'riaa'` into every existing row would
/// put a statement in the file that nobody made, which is the mistake
/// [`SCHEMA_V3`] deliberately avoided for `capture_eq`.
///
/// **It is not `captures.capture_eq` and the two are easy to confuse.** This names
/// the characteristic the *record was cut with*, which is a property of the pressing
/// and revisable forever. `capture_eq` records what the *signal already carried* when
/// it reached the sound card, which is a property of the chain, unrecoverable after
/// the fact, and the input to this decision. The filter needs both: a flat capture of
/// an RIAA pressing has the curve applied, an RIAA capture of the same pressing needs
/// nothing done, and an RIAA capture of a Columbia LP has RIAA divided out first.
pub const SCHEMA_V5: &str = r#"
-- The characteristic this release was cut with (§51), as a curve id from the
-- catalog `vcw eq list` prints: 'riaa', 'columbia_lp', 'decca_ffrr_78' and so on.
-- NULL means nobody has said, and playback falls back to the application default.
-- Not what the capture arrived with - that is captures.capture_eq, which is the
-- input to this decision rather than the decision.
ALTER TABLE releases ADD COLUMN playback_eq TEXT;
-- v4's boolean carried one bit of that: RIAA, or nothing said.
UPDATE releases SET playback_eq = 'riaa' WHERE riaa_eq = 1;
ALTER TABLE releases DROP COLUMN riaa_eq;
-- The characteristic for one side, where it differs from the release's. NULL
-- inherits the release. Two faces of a reissue are sometimes cut from masters of
-- different ages, and one capture can hold a face of each of two pressings.
ALTER TABLE sides ADD COLUMN playback_eq TEXT;
-- The characteristic for one track, which §51 asks for by name: a compilation can
-- carry transfers made from pressings cut years apart. NULL inherits the side.
ALTER TABLE tracks ADD COLUMN playback_eq TEXT;
"#;

/// Schema v6: the defect list (§46 Phase 3, WP-30).
///
/// A defect is a span of damage on a capture. It is not repaired audio and it is
/// not a repaired file: §4.1 keeps the sample blocks immutable, so the repair is
/// computed from these rows every time the audio leaves the project. Detection
/// writes rows here; a person edits them; nothing else moves them.
pub const SCHEMA_V6: &str = r#"
-- Damage found on a capture, and what to do about it (WP-30 §3). One row is one
-- span: detection proposes them, a person may veto or change any one of them, and
-- the repair is applied on playback, render and export rather than stored.
CREATE TABLE defects (
    -- Surrogate key. Stable across re-detection, which is what lets a person's
    -- edit to one row survive another analysis pass.
    defect_id   INTEGER PRIMARY KEY AUTOINCREMENT,
    -- The capture the damage is in. Deliberately not the track: frames index a
    -- capture, so a defect survives re-splitting, re-adoption and a side being
    -- laid out again, none of which change what frame 4,812,330 is.
    capture_id  INTEGER NOT NULL REFERENCES captures(capture_id),
    -- First damaged frame, counted from the start of the capture.
    start_frame INTEGER NOT NULL,
    -- One past the last damaged frame. Exclusive, so the length is a subtraction
    -- and an empty span is an error rather than a convention.
    end_frame   INTEGER NOT NULL,
    -- Which channels are damaged, as a bitmask: bit 0 is channel 0. A bitmask and
    -- not a boolean because a vinyl click is physical damage to one groove wall far
    -- more often than both, and repairing the good channel throws away half a good
    -- stereo image - which a mono fold would then hear the repair of twice.
    channels    INTEGER NOT NULL,
    -- What kind of damage: 'click', 'crackle', 'dropout'. Free text rather than a
    -- CHECK, because whether crackle is the same detector as clicks is open (WP-30
    -- §6) and a fourth kind should not need a migration.
    kind        TEXT    NOT NULL,
    -- 0..=1, as the detector scored it. A margin rather than a probability unless
    -- the provenance says otherwise, which is the same rule track_boundaries uses.
    confidence  REAL    NOT NULL DEFAULT 1.0,
    -- Which detector found it, kebab-case, or 'user'. §24.
    provenance  TEXT    NOT NULL,
    -- The measurements behind it (§24), as 'name=value;name=value'. Read whole, by
    -- a person, and never queried by name.
    evidence    TEXT    NOT NULL DEFAULT '',
    -- What to do about it: 'interpolate', 'mute', or 'none'. No default, because a
    -- repair nobody chose is a repair nobody can be held to - the writer states it.
    -- 'none' is the veto, and the row stays so that re-analysis does not propose it
    -- again and the reason is still readable.
    repair      TEXT    NOT NULL,
    -- 1 if automatic analysis may not change it (§24). The same bit, the same
    -- meaning and the same spelling as on a boundary; a second spelling would be
    -- the bug.
    locked      INTEGER NOT NULL DEFAULT 0,
    -- Unix seconds at creation.
    created_at  INTEGER NOT NULL,
    -- Unix seconds at the last edit, veto, lock or unlock.
    updated_at  INTEGER NOT NULL,
    -- A span with no frames in it is a bug in whatever wrote it, not a defect.
    CHECK (end_frame > start_frame),
    -- So is damage to no channel.
    CHECK (channels > 0),
    -- One defect per span per channel set. Two detectors agreeing about the same
    -- damage is one defect, which is the rule track_boundaries already follows, and
    -- it is what makes a detector re-runnable: a second pass updates rather than
    -- duplicates.
    UNIQUE (capture_id, start_frame, end_frame, channels)
);

-- Every read walks one capture in frame order: drawing the overlay, playing it,
-- exporting it. The same shape as track_boundaries_timeline, for the same reason.
CREATE INDEX defects_timeline
    ON defects (capture_id, start_frame);
"#;

/// Schema v7: the archivist's transfer log (§23/§24, WP-31).
///
/// Four columns that say what the audio came out of. VCW already records what the
/// *software* did - the curve, the chain, the host API - and none of that names the
/// deck, the cartridge or the copy. An archival transfer is only citable if the
/// chain that made it is on the record.
///
/// The rig itself is not here: a turntable does not change between sides, so the
/// eight rig fields live in Settings (§39) and are read at export. What lands in the
/// project is the part that is about *this* copy and *this* session, which is the
/// test the design note applies throughout - a fact about the rig belongs to the rig,
/// a fact about the copy to the release, a fact about the session to the capture.
pub const SCHEMA_V7: &str = r#"
-- The physical item identifier: a shelf mark, an accession number, or whatever the
-- holder calls this copy. Not the catalog number, which identifies the pressing;
-- this identifies the one object that was played.
ALTER TABLE releases ADD COLUMN item TEXT NOT NULL DEFAULT '';
-- The known condition of that copy, in the holder's own words. Free text because
-- the grading vocabularies disagree and a project should not pick one.
ALTER TABLE releases ADD COLUMN condition TEXT NOT NULL DEFAULT '';
-- Who made the transfer, overriding the operator named in Settings. NULL is the
-- ordinary case and means "whoever Settings says": a per-capture value is for the
-- session somebody else ran, and an override that starts life pre-filled is an
-- override nobody notices is wrong.
ALTER TABLE captures ADD COLUMN operator TEXT;
-- The transfer date as ISO 8601, date only, where started_at is not it. NULL is the
-- ordinary case, because started_at is Unix seconds at stream start and that is the
-- transfer date more accurately than a person retyping it. The exception is the
-- reason this column exists: a capture with capture_mode 'imported' is dated by the
-- import, and an Audacity project carried over from 2019 should not export as 2026.
ALTER TABLE captures ADD COLUMN transferred_on TEXT;
"#;

/// Tables the current schema must contain. Checked on open, so a truncated or
/// partially-migrated file is refused rather than half-read.
pub const REQUIRED_TABLES: &[&str] = &[
    "capture_blocks",
    "capture_diagnostics",
    "captures",
    "defects",
    "meta",
    "release_artwork",
    "releases",
    "sampleblocks",
    "schema_migrations",
    "sides",
    "track_boundaries",
    "tracks",
];
