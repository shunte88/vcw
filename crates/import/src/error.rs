/*
 *  error.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What can go wrong reading an Audacity project.
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
//! What can go wrong reading an Audacity project (§12).
//!
//! Every refusal names the offset or the attribute at fault. The reason is in
//! the format: the record stream is self-delimiting, so a reader that guesses a
//! width for a tag it does not know **desynchronizes and produces plausible
//! garbage** rather than stopping. S5 found AUP4's one new record because the
//! AUP3 grammar refused it by offset and name instead of mis-parsing it.

use std::path::PathBuf;

/// An import-layer failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The file is not an Audacity project.
    #[error("{path} is not an Audacity project: application_id is 0x{found:08X}")]
    NotAudacity {
        /// The file we were asked to read.
        path: PathBuf,
        /// The `application_id` the file actually carries.
        found: u32,
    },

    /// The file is an Audacity project of a version this reader does not know.
    ///
    /// Refused rather than attempted. `user_version` is the only thing in the
    /// file that carries the version at all: `application_id` is `"AUDY"` for
    /// both AUP3 and AUP4, and the extension is whatever the user typed.
    #[error(
        "{path} is Audacity project version {}.{}.{}.{} (user_version 0x{found:08X}), \
         which this reader does not know. It reads 3.7.x.x and 4.0.x.x.",
        found >> 24, (found >> 16) & 0xFF, (found >> 8) & 0xFF, found & 0xFF
    )]
    UnknownVersion {
        /// The file we were asked to read.
        path: PathBuf,
        /// The `user_version` the file carries.
        found: u32,
    },

    /// A record carried a tag the grammar has no rule for.
    ///
    /// The offset and the attribute name are both here because both were what
    /// made S5's AUP4 delta cheap to find.
    #[error(
        "unknown record tag 0x{tag:02X} at byte {offset} of the document{}",
        name.as_ref().map(|n| format!(" (name '{n}')")).unwrap_or_default()
    )]
    UnknownTag {
        /// The tag byte we do not know.
        tag: u8,
        /// Where in the blob it was found.
        offset: usize,
        /// The name the record refers to, when it has one and the dictionary
        /// defines it. This is the human clue: `tag 0x10 at 1237 (name 'data')`
        /// said what the new AUP4 record was for, not merely that it existed.
        name: Option<String>,
    },

    /// A record claimed more bytes than the blob holds.
    #[error(
        "record 0x{tag:02X} at byte {offset} claims {claimed} bytes but only \
         {available} remain in the document"
    )]
    Truncated {
        /// The tag that made the claim.
        tag: u8,
        /// Where the record started.
        offset: usize,
        /// How many bytes it asked for.
        claimed: usize,
        /// How many were left.
        available: usize,
    },

    /// A record referenced a dictionary id the dictionary does not define.
    ///
    /// Not recoverable and not ignorable: the id *is* the name, so without it
    /// there is no way to know what the value means.
    #[error("record 0x{tag:02X} at byte {offset} references undefined name id {id}")]
    UnknownName {
        /// The tag that referenced it.
        tag: u8,
        /// Where the record started.
        offset: usize,
        /// The id that is not in the dictionary.
        id: u16,
    },

    /// The dictionary did not start with the two-byte prologue.
    ///
    /// Reported as bytes rather than as a number: the prologue is `00 04` in
    /// file order, and calling that `0x0004` or `0x0400` would be asserting an
    /// endianness the format never states.
    #[error(
        "the dictionary starts with {:02X} {:02X} rather than the expected 00 04",
        found[0], found[1]
    )]
    BadDictPrologue {
        /// What the first two bytes actually were.
        found: [u8; 2],
    },

    /// The dictionary blob ended in the middle of an entry.
    #[error("the dictionary ended mid-entry at byte {offset} of {length}")]
    DictTruncated {
        /// Where the incomplete entry started.
        offset: usize,
        /// How long the blob was.
        length: usize,
    },

    /// A dictionary entry carried a tag other than `0x0F`.
    #[error("dictionary entry tag 0x{tag:02X} at byte {offset}, expected 0x0F")]
    BadDictTag {
        /// The tag found.
        tag: u8,
        /// Where it was.
        offset: usize,
    },

    /// A string field's byte length was not a whole number of UTF-32 units.
    ///
    /// All strings in the format are UTF-32LE and **all lengths are in bytes**.
    /// A length that is not a multiple of four means the stream is already lost,
    /// and reading on would compound it.
    #[error("a UTF-32 string at byte {offset} claims {bytes} bytes, which is not a multiple of 4")]
    BadUtf32Length {
        /// Where the string started.
        offset: usize,
        /// The byte length it claimed.
        bytes: usize,
    },

    /// A UTF-32 code unit was not a scalar value.
    #[error("a UTF-32 string at byte {offset} contains 0x{unit:08X}, which is not a character")]
    BadUtf32Char {
        /// Where the string started.
        offset: usize,
        /// The offending code unit.
        unit: u32,
    },

    /// The document ended in the middle of an element.
    #[error(
        "the document ended with {depth} element(s) still open, the innermost being '{innermost}'"
    )]
    UnclosedElement {
        /// How many were open.
        depth: usize,
        /// The innermost one's name.
        innermost: String,
    },

    /// An end-element record did not match the element it closed.
    #[error("'{found}' closed at byte {offset} but '{expected}' was open")]
    MismatchedElement {
        /// The name the record closed.
        found: String,
        /// The name that was actually open.
        expected: String,
        /// Where the record started.
        offset: usize,
    },

    /// The project row was missing, so there is no document to read.
    ///
    /// The `project` table, never `project_history`: the history table holds a
    /// complete document per save and its first generation is byte-identical to
    /// the live row, so a reader that falls back to it works by accident on a
    /// freshly converted file and returns a stale document later.
    #[error("{path} has no row in its `project` table, so it carries no document")]
    NoDocument {
        /// The file we were asked to read.
        path: PathBuf,
    },

    /// An element did not carry an attribute the model needs.
    ///
    /// Never defaulted. A missing `rate` is the difference between a rip at
    /// 48 kHz and a rip played at four times speed, and a default would make
    /// that silent - see [`crate::model`].
    #[error("<{element}> has no '{attr}' attribute")]
    MissingAttr {
        /// The element that should have carried it.
        element: String,
        /// The attribute name.
        attr: &'static str,
    },

    /// An attribute was there but not of a usable type.
    #[error("<{element}> has '{attr}' = {found}, which is not usable as {wanted}")]
    BadAttr {
        /// The element it was on.
        element: String,
        /// The attribute name.
        attr: &'static str,
        /// What was stored, debug-printed with its record width.
        found: String,
        /// What the model needed.
        wanted: &'static str,
    },

    /// A `sampleformat` code this reader does not know.
    ///
    /// The code is `(bytes_per_sample << 16) | type_code`, and only three exist:
    /// int16, int24-in-4-bytes and float32. There is **no 32-bit integer
    /// format**, so a plausible-looking fourth code is a reason to stop rather
    /// than to infer a width.
    #[error(
        "sample format 0x{found:08X} ({} bytes per sample, type code {}) is not one this          reader knows: int16 (0x00020001), int24 (0x00040001) or float32 (0x0004000F)",
        found >> 16, found & 0xFFFF
    )]
    UnknownSampleFormat {
        /// The code the file carried.
        found: u32,
    },

    /// Two clips on one track claimed the same stretch of the timeline.
    ///
    /// Not a grammar error, which is why it is checked: clips are **not stored
    /// in time order** - one corpus track ends with a clip at 11.08 s after one
    /// at 256.63 s - so a reader that trusted document order would build an
    /// overlapping timeline and never notice.
    #[error(
        "on track '{track}', a clip starting at {start:.6} s overlaps the one before it,          which runs to {previous_end:.6} s"
    )]
    OverlappingClips {
        /// The track's name.
        track: String,
        /// Where the later clip starts.
        start: f64,
        /// Where the earlier one ended.
        previous_end: f64,
    },

    /// A `waveblock` referenced a `sampleblocks` row that does not exist.
    #[error("clip '{clip}' references block {blockid}, which is not in `sampleblocks`")]
    DanglingBlock {
        /// The clip that referenced it.
        clip: String,
        /// The id that is not there.
        blockid: i64,
    },

    /// `waveblock/@length` disagreed with the stored block.
    ///
    /// AUP4 only, and worth checking precisely because it is redundant: it is
    /// the one place the document restates a size the audio table already
    /// knows, so a disagreement means one of the two is wrong.
    #[error("block {blockid} is declared {declared} samples by the document but holds {actual}")]
    BlockLengthMismatch {
        /// The block in question.
        blockid: i64,
        /// What `waveblock/@length` said.
        declared: u64,
        /// What `sampleblocks` holds.
        actual: u64,
    },

    /// A number the fixture builder has to rewrite will not fit its record.
    ///
    /// Only reachable from [`crate::fixture`], and it means the document's shape
    /// is not what this reader thinks: a `numsamples` in a one-byte record, or a
    /// blob length on a record that is not a blob.
    #[error("record 0x{tag:02X} at byte {offset} cannot be rewritten to hold {value}")]
    UnpatchableRecord {
        /// The record's tag.
        tag: u8,
        /// Where it starts.
        offset: usize,
        /// The value that would not fit.
        value: u64,
    },

    /// The project has no wave tracks, so there is no audio to import.
    ///
    /// A label-only project is a real thing a user can save, and it is not a
    /// capture. Refused here rather than landed as a zero-frame one, which
    /// recovery would then read as an interrupted recording.
    #[error("the project has no wave tracks, so there is no audio to import")]
    NoAudio,

    /// The channels disagree about something a single capture cannot hold.
    ///
    /// One `captures` row carries one rate and one storage format for every
    /// channel, which is what a stereo pair off one converter always is. Two
    /// tracks at different rates are two recordings, and mixing them would mean
    /// resampling - which D4 forbids on the way in as firmly as on the way out.
    #[error("the wave tracks disagree about {what}: the first says {first}, another says {found}")]
    MixedTracks {
        /// Which property disagreed.
        what: &'static str,
        /// What the first track said.
        first: String,
        /// What a later one said.
        found: String,
    },

    /// More wave tracks than a capture row can describe.
    #[error("the project has {found} wave tracks, more than a capture can hold")]
    TooManyChannels {
        /// How many there were.
        found: usize,
    },

    /// `wavetrack/@rate` was not a whole number of Hz.
    ///
    /// Refused rather than rounded. A `captures` row holds an integer rate, and
    /// silently turning 44,100.5 into 44,100 is a pitch error of a thousandth of
    /// a percent - far too small for anyone to notice and far too large to
    /// introduce on purpose.
    #[error("the track rate {found} is not a whole number of Hz")]
    FractionalRate {
        /// The rate the document carried.
        found: f64,
    },

    /// A clip's blocks do not cover a sample its audible span needs.
    ///
    /// The sequence is meant to be tiled by its `waveblock`s with no gap. A hole
    /// means the document and the audio table describe different recordings, and
    /// filling it with silence would put a click in the middle of a track
    /// instead of saying so.
    #[error("no block holds sequence sample {sample}, across the clip's {blocks} block(s)")]
    SequenceHole {
        /// The sample nothing covered.
        sample: u64,
        /// How many blocks the clip declared.
        blocks: usize,
    },

    /// A block held fewer samples than the sequence asked of it.
    #[error("block {blockid} was asked for {wanted} samples but holds {held}")]
    BlockTooShort {
        /// The block in question.
        blockid: i64,
        /// How many samples were needed.
        wanted: usize,
        /// How many it has.
        held: usize,
    },

    /// A `sampleblocks` row exists but carries no audio.
    #[error("block {blockid} has no samples")]
    NoSamples {
        /// The block in question.
        blockid: i64,
    },

    /// The project being written refused.
    #[error("project: {0}")]
    Project(#[from] vcw_project::Error),

    /// SQLite refused.
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// The import crate's result type.
pub type Result<T> = std::result::Result<T, Error>;
