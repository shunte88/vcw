/*
 *  error.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What can go wrong turning a project into deliverables.
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

//! What can go wrong turning a project into deliverables.

use std::path::PathBuf;

/// An export failure.
///
/// Specific by design, like the project layer's: an export is a long operation
/// over a lot of audio, and "export failed" after twenty minutes is not a report
/// anyone can act on.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The side has no tracks to export.
    ///
    /// Not an empty success: a caller that asked for a side and got no files
    /// wants to know whether the side is unanalysed or whether the export broke.
    #[error("side {side} has no tracks to export")]
    NothingToExport {
        /// The side asked for.
        side: char,
    },

    /// The project has no tracks at all.
    ///
    /// Separate from [`Error::NothingToExport`] because there is no side to
    /// name: a project in this state has not been through detection yet, and
    /// telling the operator that is more use than telling them side A is empty.
    #[error("this project has no tracks yet, so there is nothing to export")]
    Nothing,

    /// A track's side has no capture behind it.
    #[error("side {side} has no capture attached, so there is no audio to cut")]
    NoAudio {
        /// The side asked for.
        side: char,
    },

    /// The stored format cannot be written in the requested container.
    ///
    /// FLAC is an integer codec. A float capture is real - §8 allows it and
    /// Audacity produces it - and converting one to integers is a decision about
    /// dither and headroom that belongs to a person, not to an exporter that was
    /// asked for a lossless copy.
    #[error("{format:?} audio cannot be written as {container}: {why}")]
    Unencodable {
        /// The stored format.
        format: vcw_types::StorageFormat,
        /// What was asked for.
        container: &'static str,
        /// Why it will not work.
        why: &'static str,
    },

    /// A WAV file would exceed what a RIFF header can describe.
    ///
    /// RIFF sizes are 32-bit, so 4 GiB is the ceiling for the whole file. A
    /// 30-minute side at 192 kHz in 32-bit stereo is 1.4 GiB, so this is reachable
    /// by a long unsplit side rather than by a track. Refused rather than
    /// truncated, and FLAC has no such limit.
    #[error(
        "{bytes} bytes will not fit in a WAV file: RIFF sizes are 32-bit, so the ceiling is \
         {ceiling} bytes. Export this one as FLAC."
    )]
    TooLargeForWav {
        /// How many bytes of audio were to be written.
        bytes: u64,
        /// The largest a data chunk can be.
        ceiling: u64,
    },

    /// A naming template mentions a token that does not exist.
    #[error("the naming template has {} unknown token(s): {}", tokens.len(), tokens.join(", "))]
    UnknownTokens {
        /// Each unknown token, with a suggestion where one was found.
        tokens: Vec<String>,
    },

    /// Two tracks want the same file.
    ///
    /// A template with no track number in it does this the moment a side has two
    /// untitled tracks, and the second silently overwriting the first is the worst
    /// possible outcome. Reported before anything is written.
    ///
    /// The tracks are named by their positions - `A2` and `B2` - because the
    /// number within a side names neither of them: the first real export of a
    /// two-sided project reported `tracks 2 and 2`.
    #[error("tracks {first} and {second} both export to {}", path.display())]
    NameCollision {
        /// The position of the first track to claim the name, `A2`.
        first: String,
        /// The position of the second.
        second: String,
        /// The path they agree on.
        path: PathBuf,
    },

    /// A file that is already there.
    #[error("{} already exists", path.display())]
    Exists {
        /// The file in the way.
        path: PathBuf,
    },

    /// A write arrived that was not a whole number of frames.
    ///
    /// `pcm::Reader::fill` never does this - it stops at a frame boundary on
    /// purpose - so this is a caller that built its own buffer, and handing the
    /// bytes on would tear every frame after the first.
    #[error("{bytes} bytes is not a whole number of {frame_bytes}-byte frames")]
    Partial {
        /// What arrived.
        bytes: usize,
        /// What a frame occupies in the project's blocks.
        frame_bytes: usize,
    },

    /// The FLAC encoder refused.
    #[error("the FLAC encoder refused: {why}")]
    Flac {
        /// What it said.
        why: String,
    },

    /// The tagger refused.
    #[error("tagging {} failed", path.display())]
    Tagging {
        /// The file being tagged.
        path: PathBuf,
        /// The underlying failure. Boxed on purpose: lofty 0.25 has no single
        /// error type - it raises `FileParseError` on the way in and
        /// `FileEncodingError` on the way out - and naming either here would
        /// pin this crate to the tagger's internal type names.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The project said no.
    #[error(transparent)]
    Project(#[from] vcw_project::error::Error),

    /// The filesystem said no.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// An export result.
pub type Result<T> = std::result::Result<T, Error>;
