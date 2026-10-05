/*
 *  error.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What can go wrong opening, creating or validating a project.
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

//! What can go wrong opening, creating or validating a project.

use std::path::PathBuf;

/// A project-layer failure.
///
/// Refusals are deliberately specific. §15 makes recovery a correctness
/// requirement, and a recovery tool that reports "could not open project" has
/// thrown away the information the user needs.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The file exists but is not a VCW project.
    #[error(
        "{path} is not a VCW project: application_id is 0x{found:08X}{}",
        describe_application_id(*found)
    )]
    NotAProject {
        /// The file we were asked to open.
        path: PathBuf,
        /// The `application_id` the file actually carries.
        found: u32,
    },

    /// The project was written by a newer VCW than this one.
    ///
    /// Refused rather than guessed at. §16 allows newer versions to upgrade older
    /// projects; it says nothing about older versions reading newer ones, and a
    /// partial read of a schema we do not know is how data gets lost.
    #[error(
        "{path} has schema version {found}, and this build understands up to {supported}. \
         Upgrade VCW to open it."
    )]
    SchemaTooNew {
        /// The file we were asked to open.
        path: PathBuf,
        /// The schema version in the file.
        found: u32,
        /// The newest schema version this build can read.
        supported: u32,
    },

    /// The project is older than this build, and the caller needed it current.
    ///
    /// Not raised by opening: v1 is a schema this build reads perfectly well, and
    /// a capture-only project has nothing missing from it. It is raised by a
    /// reader that needs a v2 table - a release, a side, a track - and found a
    /// file written before those existed.
    ///
    /// Separate from [`Error::SchemaTooNew`] because the answer is the opposite
    /// one. A project from the future cannot be read at all; this one upgrades
    /// (§16), so the message says so rather than telling a person their project
    /// is broken.
    #[error(
        "{path} was written by an older VCW (schema version {found}, this build uses \
         {supported}). Open it to upgrade it."
    )]
    SchemaNeedsUpgrade {
        /// The file we were asked to read.
        path: PathBuf,
        /// The schema version in the file.
        found: u32,
        /// The version the reader needed.
        supported: u32,
    },

    /// A migration failed, and was rolled back.
    #[error("migration to schema version {version} ({description}) failed and was rolled back")]
    Migration {
        /// The version being migrated to.
        version: u32,
        /// That migration's description.
        description: String,
        /// The underlying SQLite failure.
        #[source]
        source: rusqlite::Error,
    },

    /// The project opened, but its contents are not self-consistent.
    #[error("{path} failed validation: {} problem(s)", findings.len())]
    Invalid {
        /// The project that failed.
        path: PathBuf,
        /// Every problem found, not just the first.
        findings: Vec<String>,
    },

    /// A capture cannot be written as it is described.
    ///
    /// Only reachable through the library, never from a device: a stream with no
    /// channels has no frames, and a writer asked for zero-length blocks would
    /// spin forever rather than fail. Refusing at the door is cheaper than making
    /// every loop defend itself.
    #[error(
        "a capture with {channels} channel(s) at {frame_bytes} bytes per frame cannot be written"
    )]
    Unwritable {
        /// The channel count asked for.
        channels: u16,
        /// The frame width that implies.
        frame_bytes: usize,
    },

    /// The capture writer thread ended without reporting an outcome.
    ///
    /// Only reachable if it panicked. Its ordinary failure path closes the
    /// session as interrupted and returns the error, so this variant means
    /// something worse happened than a refused write, and the project should be
    /// treated as needing recovery rather than as merely short.
    #[error("the capture writer ended without reporting; the project needs recovery")]
    WriterLost,

    /// A write was attempted through a read-only handle.
    ///
    /// Not a programming slip worth a panic: recovery is routinely offered a
    /// project the caller opened read-only to inspect it, and "reopen it for
    /// writing" is a better answer than a crash.
    #[error("{path} is open read-only and cannot be written")]
    ReadOnly {
        /// The project in question.
        path: PathBuf,
    },

    /// Recovery found blocks it is not allowed to remove without being asked.
    ///
    /// Blocks are immutable and never deleted to tidy up (D4), so recovery
    /// refuses rather than quietly destroying captured audio. See
    /// [`crate::recovery::Plan::Repair`].
    #[error(
        "capture {capture_id} has {blocks} block(s) stranded past the recoverable end; \
         recovering would discard them, so it needs Plan::Repair"
    )]
    StrandedBlocks {
        /// The capture concerned.
        capture_id: i64,
        /// How many blocks would be discarded.
        blocks: usize,
    },

    /// A capture id that is not in this project.
    ///
    /// Its own variant rather than an `Option`, because every caller that asks
    /// for a capture by id already believes it exists - it came from a list, a
    /// command line or a row - so `None` would only ever be unwrapped.
    #[error("capture {capture_id} is not in this project")]
    NoSuchCapture {
        /// The id that was asked for.
        capture_id: i64,
    },

    /// A capture's blocks do not describe audio that can be played.
    ///
    /// Distinct from [`Error::Invalid`], which is what a whole-project
    /// validation returns: this one is raised by the reader, in the middle of
    /// playback, about one block it was asked for and cannot honor. Playback
    /// refusing to invent audio for a hole in the timeline is the point -
    /// silence in place of a missing block would be indistinguishable from
    /// silence that was recorded.
    #[error("capture {capture_id} cannot be played: channel {channel} of block {sequence} {why}")]
    Unplayable {
        /// The capture concerned.
        capture_id: i64,
        /// The channel whose block is wrong.
        channel: u16,
        /// The block's sequence number within the capture.
        sequence: u64,
        /// What is wrong with it.
        why: String,
    },

    /// A side letter this project has no row for.
    ///
    /// Sides are created deliberately, by [`crate::side::ensure`], because a side
    /// row is the thing a capture attaches to and conjuring one on demand would
    /// hide a mislabeled recording rather than report it.
    #[error("side {side} is not in this project")]
    NoSuchSide {
        /// The letter that was asked for.
        side: char,
    },

    /// A side letter that is already taken.
    #[error("side {side} already exists in this project")]
    SideOccupied {
        /// The letter that was asked for.
        side: char,
    },

    /// A side that still holds tracks or boundaries.
    ///
    /// Deleting it would take them with it, and §4.1 does not let this layer decide
    /// that on a caller's behalf.
    #[error("side {side} still holds {tracks} track(s) and {boundaries} boundary/ies")]
    SideNotEmpty {
        /// The letter concerned.
        side: char,
        /// Tracks still on it.
        tracks: usize,
        /// Boundaries still on it.
        boundaries: usize,
    },

    /// A track id that is not in this project.
    #[error("track {track_id} is not in this project")]
    NoSuchTrack {
        /// The id that was asked for.
        track_id: i64,
    },

    /// A boundary id that is not in this project.
    #[error("boundary {boundary_id} is not in this project")]
    NoSuchBoundary {
        /// The id that was asked for.
        boundary_id: i64,
    },

    /// A locked boundary was asked to move or to go (§24).
    ///
    /// The one rule in the editing model that is not advisory: a boundary a person
    /// placed or confirmed is not moved by anything except that person unlocking it.
    /// Re-analysis relies on this, so it is enforced here rather than in each caller.
    #[error("boundary {boundary_id} at frame {at_frame} is locked, so analysis may not move it")]
    BoundaryLocked {
        /// The boundary concerned.
        boundary_id: i64,
        /// Where it is.
        at_frame: u64,
    },

    /// A boundary that bounds a track was asked to go.
    ///
    /// Deleting it would leave the track with one end, so the track is what has to
    /// be edited: merge it with its neighbor, or delete it.
    #[error("boundary {boundary_id} bounds track {track_id} and cannot be deleted on its own")]
    BoundaryInUse {
        /// The boundary concerned.
        boundary_id: i64,
        /// The track that needs it.
        track_id: i64,
    },

    /// Two tracks that cannot be merged because something sits between them.
    #[error("tracks {left} and {right} are not adjacent, so merging them would swallow a track")]
    NotAdjacent {
        /// The earlier track.
        left: i64,
        /// The later track.
        right: i64,
    },

    /// A split point that is not inside the track being split.
    #[error("frame {at} is not inside track {track_id} ({start}..{end})")]
    OutsideTrack {
        /// The track concerned.
        track_id: i64,
        /// The frame asked for.
        at: u64,
        /// Where the track starts.
        start: u64,
        /// Where it ends.
        end: u64,
    },

    /// A track was asked to move to a side holding different audio.
    ///
    /// A track's boundaries are frames into its side's capture, so moving it to a
    /// side recorded separately would point them at audio that is not the track.
    /// Moving between two sides that share one capture - both faces in a single
    /// take - is the case this allows.
    #[error("track {track_id} cannot move from side {from} to side {to}: different captures")]
    DifferentCapture {
        /// The track concerned.
        track_id: i64,
        /// The side it is on.
        from: char,
        /// The side asked for.
        to: char,
    },

    /// SQLite said no.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    /// The filesystem said no.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Adds a hint when the `application_id` is one we recognize.
fn describe_application_id(id: u32) -> &'static str {
    match id {
        0x4155_4459 => {
            " - that is an Audacity project. Import it instead; VCW reads .aup3 and .aup4."
        }
        0 => {
            " - the file has no application_id, so it is a plain SQLite database or not SQLite at all."
        }
        _ => "",
    }
}

/// A project-layer result.
pub type Result<T> = std::result::Result<T, Error>;
