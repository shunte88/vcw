/*
 *  model.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The project a document describes: tracks, clips, block references and labels.
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
//! The project a document describes: tracks, clips, block references and labels.
//!
//! [`doc`](crate::doc) gets the bytes right. This module is where the bytes
//! start meaning something, and the meanings that are easy to get wrong are the
//! ones written down here:
//!
//! - **`wavetrack/@rate` is the sample rate. `project/@rate` is not.** The
//!   project attribute reads exactly `192000.0` in all 30 corpus files,
//!   including the 22 rips captured at 48 kHz: it is a stored editor
//!   preference. It is kept, as [`Project::editor_rate_preference`], purely so
//!   that the thing being ignored is visible rather than absent.
//! - **Clips are not in time order.** One corpus track's last clip in document
//!   order starts at 11.08 s, after one starting at 256.63 s. [`Track::clips`]
//!   is sorted by offset and checked for overlap; document order is the order to
//!   read, never the order to trust.
//! - **A clip's `offset` is where its sequence *begins*, not where it starts
//!   playing.** `trimLeft` and `trimRight` are audio the sequence still holds
//!   and does not play, so the audible span is
//!   `[offset + trimLeft, offset + numsamples/rate - trimRight]`. Reading
//!   `offset` as the audible start looks right on every clip with no left trim,
//!   which is most of them, and then puts a trimmed clip five seconds early.
//!   The corpus settles it: one clip has `offset="4.31446875"`,
//!   `trimLeft="5.211296875"` and 1,299,910 samples at 192 kHz, and the clip
//!   before it ends at 9.525765625 s while the clip after it starts at
//!   11.084833333 s. Both boundaries land exactly, and only on this reading.
//!   Because of it, **document order is not even offset order**: that clip's
//!   `offset` sorts it third and it plays fifth.
//! - **Blocks are shared.** 532 `waveblock` references to 456 distinct blocks in
//!   one project, one block used three times. Anything that counts, copies or
//!   frees blocks has to work from [`Project::distinct_blocks`], not from the
//!   reference list.
//! - **An all-unity envelope is not an envelope.** A conversion gave 16 clips a
//!   single `val="1.0"` control point each; the user drew none of them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::doc::{Event, Value};
use crate::error::{Error, Result};

/// How samples are stored, from `sampleformat`.
///
/// The code is `(bytes_per_sample << 16) | type_code` and exactly three exist.
/// **There is no 32-bit integer format**, which is a genuine conflict with
/// REQUIREMENTS §8 and the reason D1 chose an AUP4 superset rather than literal
/// AUP4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    /// `0x00020001`. Two bytes per sample.
    Int16,
    /// `0x00040001`. 24-bit samples stored in four bytes, so the stored width
    /// and the meaningful width differ.
    Int24,
    /// `0x0004000F`. What 24 of the 25 corpus rips actually contain, because
    /// the existing Audacity workflow converts the converter's integer words to
    /// float on the way in.
    Float32,
}

impl SampleFormat {
    /// The format for a stored code.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownSampleFormat`] for anything else. Refused rather than
    /// inferred from the high half: a fourth code would mean the format grew,
    /// and guessing a width from `bytes_per_sample` alone would read int32 as
    /// float32 and produce noise.
    pub const fn from_code(code: u32) -> Result<Self> {
        match code {
            0x0002_0001 => Ok(Self::Int16),
            0x0004_0001 => Ok(Self::Int24),
            0x0004_000F => Ok(Self::Float32),
            found => Err(Error::UnknownSampleFormat { found }),
        }
    }

    /// The code Audacity stores for this format.
    #[must_use]
    pub const fn code(self) -> u32 {
        match self {
            Self::Int16 => 0x0002_0001,
            Self::Int24 => 0x0004_0001,
            Self::Float32 => 0x0004_000F,
        }
    }

    /// Bytes each sample occupies in a `samples` blob.
    ///
    /// Four for [`Self::Int24`]: the stored width, not the meaningful one.
    #[must_use]
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::Int16 => 2,
            Self::Int24 | Self::Float32 => 4,
        }
    }
}

/// One `waveblock`: a clip's use of a row in `sampleblocks`.
///
/// A *reference*, not a block. The same `blockid` may appear in several of these
/// across clips and tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockRef {
    /// Sample offset of this block within its sequence.
    pub start: u64,
    /// The `sampleblocks` row it uses.
    pub blockid: i64,
    /// `waveblock/@length`, which is AUP4 only.
    ///
    /// The one place the document restates a size the audio table already
    /// knows, so it is worth checking against and its absence means AUP3 rather
    /// than corruption. It matched `sampleblocks` in all 5,664 AUP4 cases.
    pub length: Option<u64>,
}

/// A stretch of audio placed on a track's timeline.
#[derive(Clone, Debug)]
pub struct Clip {
    /// The clip's name, which Audacity derives from the track's.
    pub name: String,
    /// Seconds from the start of the timeline to the first audible sample.
    pub offset: f64,
    /// Seconds of the sequence held before `offset` and not played.
    pub trim_left: f64,
    /// Seconds of the sequence held after the audible part and not played.
    pub trim_right: f64,
    /// The whole sequence's length in samples, trims included.
    pub num_samples: u64,
    /// The sequence's block capacity, 262144 throughout this corpus.
    pub max_samples: u64,
    /// How this sequence's samples are stored.
    pub sample_format: SampleFormat,
    /// `clipStretchRatio`. `1.0` is unstretched.
    ///
    /// This, not `clipStretchToMatchTempo`, says whether the audio is
    /// transformed: the flag is a mode and the ratio is the transform.
    pub stretch_ratio: f64,
    /// The blocks this clip's sequence is made of, in sequence order.
    pub blocks: Vec<BlockRef>,
    /// Envelope control points as `(seconds from clip start, gain)`.
    ///
    /// Empty when the clip has no envelope **and** when every point is unity,
    /// which a conversion can manufacture.
    pub envelope: Vec<(f64, f64)>,
}

impl Clip {
    /// Where sequence sample 0 sits on the timeline, in samples.
    ///
    /// This is `offset`, which is **not** where the clip starts playing. See the
    /// module documentation.
    #[must_use]
    pub fn origin_sample(&self, rate: f64) -> u64 {
        to_samples(self.offset, rate)
    }

    /// The first audible sample's position on the timeline.
    #[must_use]
    pub fn start_sample(&self, rate: f64) -> u64 {
        self.origin_sample(rate) + self.first_audible_sample(rate)
    }

    /// One past the last audible sample's position on the timeline.
    #[must_use]
    pub fn end_sample(&self, rate: f64) -> u64 {
        self.origin_sample(rate) + self.num_samples - self.trimmed_right(rate)
    }

    /// Samples this clip actually plays.
    #[must_use]
    pub fn audible_samples(&self, rate: f64) -> u64 {
        self.num_samples
            .saturating_sub(self.first_audible_sample(rate) + self.trimmed_right(rate))
    }

    /// Where in the *sequence* the audible audio begins, in samples.
    ///
    /// The offset to read from, once the sequence's blocks have been assembled.
    #[must_use]
    pub fn first_audible_sample(&self, rate: f64) -> u64 {
        to_samples(self.trim_left, rate).min(self.num_samples)
    }

    /// Seconds from the timeline start to the first audible sample.
    #[must_use]
    pub fn start(&self, rate: f64) -> f64 {
        seconds(self.start_sample(rate), rate)
    }

    /// Seconds from the timeline start to the end of the audible audio.
    #[must_use]
    pub fn end(&self, rate: f64) -> f64 {
        seconds(self.end_sample(rate), rate)
    }

    /// The right trim in samples, clamped so it can never cross the left one.
    fn trimmed_right(&self, rate: f64) -> u64 {
        let left = self.first_audible_sample(rate);
        to_samples(self.trim_right, rate).min(self.num_samples - left)
    }
}

/// Seconds to samples, rounded.
///
/// Rounded, not truncated. The timings are stored as seconds and the counts they
/// describe are exact, so the corpus's `trimRight` of `8.845239583333333` at
/// 192 kHz is 1,698,286 samples and truncation would lose one. That one sample
/// is the difference between clips that come out contiguous and clips that come
/// out overlapping, and the corpus has 38 clips to be wrong about.
fn to_samples(seconds: f64, rate: f64) -> u64 {
    let samples = (seconds * rate).round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a negative or non-finite timing clamps to zero rather than wrapping"
    )]
    if samples.is_finite() && samples > 0.0 {
        samples as u64
    } else {
        0
    }
}

/// Samples to seconds.
fn seconds(samples: u64, rate: f64) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a sample count past 2^53 is 594 years of 48 kHz audio"
    )]
    let samples = samples as f64;
    samples / rate
}

/// One `wavetrack`: a single channel of audio.
///
/// Stereo is **two sibling tracks**, not one track with two channels: the left
/// carries `channel="0" linked="3"` and the right `channel="1" linked="0"`.
#[derive(Clone, Debug)]
pub struct Track {
    /// The track's name, which is what Audacity shows in its label.
    pub name: String,
    /// **The sample rate.** Per track, and the only authoritative one in the
    /// file.
    pub rate: f64,
    /// `channel`: 0 for the left or mono channel, 1 for the right.
    pub channel: Option<u32>,
    /// `linked`, which is how a stereo pair is joined.
    pub linked: Option<u32>,
    /// The track's declared storage format. A clip's `sequence` restates it and
    /// the clip's is the one that describes the blocks.
    pub sample_format: SampleFormat,
    /// Playback gain, `1.0` being unity.
    pub gain: f64,
    /// Pan, `0.0` being center.
    pub pan: f64,
    /// Whether the track is muted.
    pub muted: bool,
    /// Whether the track is soloed.
    pub solo: bool,
    /// The track's clips, **sorted by offset** and checked not to overlap.
    pub clips: Vec<Clip>,
}

impl Track {
    /// Seconds from the timeline start to the end of the last clip.
    ///
    /// Not the sum of the clips: a track may have gaps between them, and in VCW
    /// those gaps are meaningful, because a track's end is not the next track's
    /// start.
    #[must_use]
    pub fn duration(&self) -> f64 {
        self.clips.last().map_or(0.0, |clip| clip.end(self.rate))
    }

    /// Every block reference on this track, in clip then sequence order.
    pub fn block_refs(&self) -> impl Iterator<Item = &BlockRef> {
        self.clips.iter().flat_map(|clip| clip.blocks.iter())
    }
}

/// One entry in a `labeltrack`.
///
/// This is where the user's own track boundaries already live, in the projects
/// they have been making for years: `t` and `t1` are the start and end in
/// seconds and `title` is what they called it.
#[derive(Clone, Debug)]
pub struct Label {
    /// Start, in seconds from the timeline start.
    pub t: f64,
    /// End, in seconds. May equal `t` for a point label.
    pub t1: f64,
    /// The label's text.
    pub title: String,
}

/// One `labeltrack`.
///
/// **In AUP4 a `labeltrack` carries `wavetrack`'s whole attribute set** - gain,
/// color index, spectrogram parameters and all - so it must be recognized by
/// its element name and never by which attributes it has.
#[derive(Clone, Debug)]
pub struct LabelTrack {
    /// The track's name.
    pub name: String,
    /// Its labels, in document order.
    pub labels: Vec<Label>,
}

/// An Audacity project, as its document describes it.
#[derive(Clone, Debug, Default)]
pub struct Project {
    /// `project/@audacityversion`, the application that wrote it.
    pub audacity_version: Option<String>,
    /// The `tags` element's metadata.
    ///
    /// A map because **order is not stable**: an AUP3 to AUP4 conversion
    /// reorders `tag` elements with no change of content, so this must be
    /// compared as a set and never as a sequence.
    pub tags: BTreeMap<String, String>,
    /// The audio tracks, in document order, one per channel.
    pub tracks: Vec<Track>,
    /// The label tracks, in document order.
    pub label_tracks: Vec<LabelTrack>,
    /// `project/@rate`, which is **not** the sample rate.
    ///
    /// Recorded so that ignoring it is a visible decision. It reads `192000.0`
    /// in every corpus file regardless of what the file contains; use
    /// [`Project::rate`].
    pub editor_rate_preference: Option<f64>,
}

impl Project {
    /// Builds the project from a parsed document.
    ///
    /// # Errors
    ///
    /// [`Error::MissingAttr`] or [`Error::BadAttr`] for an element that does not
    /// carry what the model needs, [`Error::UnknownSampleFormat`] for a storage
    /// format this reader does not know, and [`Error::OverlappingClips`] if a
    /// track's clips cannot be laid out on a timeline.
    pub fn from_events(events: &[Event]) -> Result<Self> {
        let mut reader = Reader { events, at: 0 };
        let mut project = Self::default();

        // Everything is inside one `project` element, and anything outside it is
        // not something this reader claims to understand.
        while let Some(name) = reader.open() {
            if &*name == "project" {
                project.read_project(&mut reader)?;
            } else {
                reader.skip();
            }
        }
        Ok(project)
    }

    /// The project's sample rate, from the tracks that carry audio.
    ///
    /// `None` when there are no audio tracks. A stereo pair agrees, so the
    /// first track's rate is the project's; a file whose tracks disagreed would
    /// be a resampling question rather than an import question, and this returns
    /// the first rather than pretending otherwise.
    #[must_use]
    pub fn rate(&self) -> Option<f64> {
        self.tracks.first().map(|track| track.rate)
    }

    /// Every block reference in the project, in document order.
    ///
    /// Longer than [`Self::distinct_blocks`], and deliberately: the difference
    /// between the two counts is the sharing.
    pub fn block_refs(&self) -> impl Iterator<Item = &BlockRef> {
        self.tracks.iter().flat_map(Track::block_refs)
    }

    /// The distinct blocks the project uses.
    ///
    /// Sorted and deduplicated, which is the set a copy or a free has to work
    /// from. `blockid`s are sparse and not 1-based - one corpus project runs
    /// 73..2005 for 456 blocks - so never derive a count from a range.
    #[must_use]
    pub fn distinct_blocks(&self) -> BTreeSet<i64> {
        self.block_refs().map(|r| r.blockid).collect()
    }

    /// Reads the `project` element, whose Start has been consumed.
    fn read_project(&mut self, reader: &mut Reader) -> Result<()> {
        let mut attrs = Attrs::new("project");
        while let Some(child) = reader.child(&mut attrs) {
            match &*child {
                "tags" => self.read_tags(reader)?,
                "wavetrack" => {
                    let track = read_wavetrack(reader)?;
                    self.tracks.push(track);
                }
                "labeltrack" => {
                    let track = read_labeltrack(reader)?;
                    self.label_tracks.push(track);
                }
                // `effects`, `thumbnail` and the view state: editor UI, none of
                // it describing the audio.
                _ => reader.skip(),
            }
        }
        self.audacity_version = attrs.text_opt("audacityversion")?.map(str::to_owned);
        self.editor_rate_preference = attrs.f64_opt("rate")?;
        Ok(())
    }

    /// Reads a `tags` element into the metadata map.
    fn read_tags(&mut self, reader: &mut Reader) -> Result<()> {
        let mut attrs = Attrs::new("tags");
        while let Some(child) = reader.child(&mut attrs) {
            if &*child == "tag" {
                let mut tag = Attrs::new("tag");
                reader.drain(&mut tag);
                self.tags
                    .insert(tag.text("name")?.to_owned(), tag.text("value")?.to_owned());
            } else {
                reader.skip();
            }
        }
        Ok(())
    }
}

/// Reads a `wavetrack` element, whose Start has been consumed.
fn read_wavetrack(reader: &mut Reader) -> Result<Track> {
    let mut attrs = Attrs::new("wavetrack");
    let mut clips = Vec::new();
    while let Some(child) = reader.child(&mut attrs) {
        if &*child == "waveclip" {
            clips.push(read_waveclip(reader)?);
        } else {
            reader.skip();
        }
    }

    let name = attrs.text_or("name", "").to_owned();
    let track = Track {
        // Required, and never defaulted: this is the attribute the whole import
        // turns on. See the module documentation.
        rate: attrs.f64("rate")?,
        channel: attrs.u32_opt("channel")?,
        linked: attrs.u32_opt("linked")?,
        sample_format: SampleFormat::from_code(attrs.u32("sampleformat")?)?,
        gain: attrs.f64_or("gain", 1.0)?,
        pan: attrs.f64_or("pan", 0.0)?,
        muted: attrs.bool_or("mute", false)?,
        solo: attrs.bool_or("solo", false)?,
        clips: order_clips(clips, attrs.f64("rate")?, &name)?,
        name,
    };
    Ok(track)
}

/// Sorts a track's clips onto the timeline and refuses an impossible layout.
///
/// Sorted by **audible start**, not by `offset`. Those differ whenever a clip
/// has a left trim, and in the corpus one clip's `offset` sorts it two places
/// before where it plays - so this is not a tidy-up. Without it a consumer
/// walking the clips in order emits audio out of sequence, and the overlap check
/// below compares unrelated pairs and passes.
fn order_clips(mut clips: Vec<Clip>, rate: f64, track: &str) -> Result<Vec<Clip>> {
    clips.sort_by_key(|clip| clip.start_sample(rate));

    // In samples, so the comparison is exact. The timings are stored as seconds
    // and f64 re-rounds by up to 2.7e-15 s across a conversion, which is
    // invisible at sample resolution and would not be if this compared seconds.
    let mut previous_end = 0_u64;
    for clip in &clips {
        let start = clip.start_sample(rate);
        if start < previous_end {
            return Err(Error::OverlappingClips {
                track: track.to_owned(),
                start: seconds(start, rate),
                previous_end: seconds(previous_end, rate),
            });
        }
        previous_end = clip.end_sample(rate);
    }
    Ok(clips)
}

/// Reads a `waveclip` element, whose Start has been consumed.
fn read_waveclip(reader: &mut Reader) -> Result<Clip> {
    let mut attrs = Attrs::new("waveclip");
    let mut sequence: Option<Sequence> = None;
    let mut envelope = Vec::new();

    while let Some(child) = reader.child(&mut attrs) {
        match &*child {
            "sequence" => sequence = Some(read_sequence(reader)?),
            "envelope" => envelope = read_envelope(reader)?,
            _ => reader.skip(),
        }
    }

    let sequence = sequence.ok_or_else(|| Error::MissingAttr {
        element: "waveclip".to_owned(),
        attr: "sequence",
    })?;

    Ok(Clip {
        name: attrs.text_or("name", "").to_owned(),
        offset: attrs.f64("offset")?,
        trim_left: attrs.f64_or("trimLeft", 0.0)?,
        trim_right: attrs.f64_or("trimRight", 0.0)?,
        num_samples: sequence.num_samples,
        max_samples: sequence.max_samples,
        sample_format: sequence.sample_format,
        stretch_ratio: attrs.f64_or("clipStretchRatio", 1.0)?,
        blocks: sequence.blocks,
        envelope,
    })
}

/// A clip's `sequence`: the storage behind it, before the clip's trims apply.
struct Sequence {
    num_samples: u64,
    max_samples: u64,
    sample_format: SampleFormat,
    blocks: Vec<BlockRef>,
}

/// Reads a `sequence` element, whose Start has been consumed.
fn read_sequence(reader: &mut Reader) -> Result<Sequence> {
    let mut attrs = Attrs::new("sequence");
    let mut blocks = Vec::new();

    while let Some(child) = reader.child(&mut attrs) {
        if &*child == "waveblock" {
            let mut block = Attrs::new("waveblock");
            reader.drain(&mut block);
            blocks.push(BlockRef {
                start: block.u64("start")?,
                #[expect(
                    clippy::cast_possible_wrap,
                    reason = "blockid is an INTEGER PRIMARY KEY, which is signed in SQLite"
                )]
                blockid: block.u64("blockid")? as i64,
                length: block.u64_opt("length")?,
            });
        } else {
            reader.skip();
        }
    }

    Ok(Sequence {
        num_samples: attrs.u64("numsamples")?,
        max_samples: attrs.u64("maxsamples")?,
        // `sampleformat` rather than `effectivesampleformat`: the former
        // describes the bytes in the blocks, the latter the narrowest format
        // those samples would fit in, which is an editor hint.
        sample_format: SampleFormat::from_code(attrs.u32("sampleformat")?)?,
        blocks,
    })
}

/// Reads an `envelope` element, whose Start has been consumed.
///
/// Returns no points for an envelope that does nothing. A conversion gave 16
/// clips a single unity point each, so a point does not mean the user drew one,
/// and an all-unity envelope is the same thing as no envelope.
fn read_envelope(reader: &mut Reader) -> Result<Vec<(f64, f64)>> {
    let mut attrs = Attrs::new("envelope");
    let mut points = Vec::new();

    while let Some(child) = reader.child(&mut attrs) {
        if &*child == "controlpoint" {
            let mut point = Attrs::new("controlpoint");
            reader.drain(&mut point);
            points.push((point.f64("t")?, point.f64("val")?));
        } else {
            reader.skip();
        }
    }

    if points.iter().all(|&(_, gain)| gain == 1.0) {
        points.clear();
    }
    Ok(points)
}

/// Reads a `labeltrack` element, whose Start has been consumed.
fn read_labeltrack(reader: &mut Reader) -> Result<LabelTrack> {
    let mut attrs = Attrs::new("labeltrack");
    let mut labels = Vec::new();

    while let Some(child) = reader.child(&mut attrs) {
        if &*child == "label" {
            let mut label = Attrs::new("label");
            reader.drain(&mut label);
            labels.push(Label {
                t: label.f64("t")?,
                t1: label.f64_or("t1", label.f64("t")?)?,
                title: label.text_or("title", "").to_owned(),
            });
        } else {
            reader.skip();
        }
    }

    Ok(LabelTrack {
        name: attrs.text_or("name", "").to_owned(),
        labels,
    })
}

/// A cursor over the event stream that knows how elements nest.
struct Reader<'a> {
    events: &'a [Event],
    at: usize,
}

impl Reader<'_> {
    /// Consumes the next element's Start and returns its name.
    fn open(&mut self) -> Option<Arc<str>> {
        while let Some(event) = self.events.get(self.at) {
            self.at += 1;
            if let Event::Start(name) = event {
                return Some(Arc::clone(name));
            }
        }
        None
    }

    /// Advances to the current element's next child, collecting attributes.
    ///
    /// `None` means the element closed, and its End has been consumed. Late
    /// attributes fold into `attrs` rather than being dropped: the corpus always
    /// puts them first, and folding costs nothing if a file ever does not.
    fn child(&mut self, attrs: &mut Attrs) -> Option<Arc<str>> {
        while let Some(event) = self.events.get(self.at) {
            self.at += 1;
            match event {
                Event::Attr { name, value } => {
                    attrs.by_name.insert(Arc::clone(name), value.clone());
                }
                // Character data between elements. No element in this format
                // carries meaning in it.
                Event::Text(_) => {}
                Event::Start(name) => return Some(Arc::clone(name)),
                Event::End(_) => return None,
            }
        }
        None
    }

    /// Collects a childless element's attributes and consumes its End.
    fn drain(&mut self, attrs: &mut Attrs) {
        while let Some(child) = self.child(attrs) {
            // An element we expected to be a leaf has a child. Skipping it keeps
            // the cursor synchronized, which is the only thing that matters
            // here; the attributes we came for are already collected.
            let _ = child;
            self.skip();
        }
    }

    /// Skips the current element and everything inside it, Start consumed.
    fn skip(&mut self) {
        let mut depth = 1_usize;
        while let Some(event) = self.events.get(self.at) {
            self.at += 1;
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

/// One element's attributes, with the element's name so that a refusal can say
/// where it came from.
struct Attrs {
    element: &'static str,
    by_name: HashMap<Arc<str>, Value>,
}

impl Attrs {
    fn new(element: &'static str) -> Self {
        Self {
            element,
            by_name: HashMap::new(),
        }
    }

    fn missing(&self, attr: &'static str) -> Error {
        Error::MissingAttr {
            element: self.element.to_owned(),
            attr,
        }
    }

    fn bad(&self, attr: &'static str, value: &Value, wanted: &'static str) -> Error {
        Error::BadAttr {
            element: self.element.to_owned(),
            attr,
            found: format!("{value:?}"),
            wanted,
        }
    }

    fn f64_opt(&self, attr: &'static str) -> Result<Option<f64>> {
        match self.by_name.get(attr) {
            None => Ok(None),
            Some(value) => value
                .as_f64()
                .map(Some)
                .ok_or_else(|| self.bad(attr, value, "a number")),
        }
    }

    fn f64(&self, attr: &'static str) -> Result<f64> {
        self.f64_opt(attr)?.ok_or_else(|| self.missing(attr))
    }

    fn f64_or(&self, attr: &'static str, default: f64) -> Result<f64> {
        Ok(self.f64_opt(attr)?.unwrap_or(default))
    }

    fn u64_opt(&self, attr: &'static str) -> Result<Option<u64>> {
        match self.by_name.get(attr) {
            None => Ok(None),
            Some(value) => value
                .as_u64()
                .map(Some)
                .ok_or_else(|| self.bad(attr, value, "a non-negative integer")),
        }
    }

    fn u64(&self, attr: &'static str) -> Result<u64> {
        self.u64_opt(attr)?.ok_or_else(|| self.missing(attr))
    }

    fn u32_opt(&self, attr: &'static str) -> Result<Option<u32>> {
        match self.u64_opt(attr)? {
            None => Ok(None),
            Some(value) => u32::try_from(value).map(Some).map_err(|_| {
                self.bad(
                    attr,
                    self.by_name.get(attr).unwrap_or(&Value::U8(0)),
                    "a 32-bit unsigned integer",
                )
            }),
        }
    }

    fn u32(&self, attr: &'static str) -> Result<u32> {
        self.u32_opt(attr)?.ok_or_else(|| self.missing(attr))
    }

    fn bool_or(&self, attr: &'static str, default: bool) -> Result<bool> {
        Ok(self.u64_opt(attr)?.map_or(default, |value| value != 0))
    }

    fn text_opt(&self, attr: &'static str) -> Result<Option<&str>> {
        match self.by_name.get(attr) {
            None => Ok(None),
            Some(value) => value
                .as_str()
                .map(Some)
                .ok_or_else(|| self.bad(attr, value, "text")),
        }
    }

    fn text(&self, attr: &'static str) -> Result<&str> {
        self.text_opt(attr)?.ok_or_else(|| self.missing(attr))
    }

    /// Text, or a default for an attribute that is absent.
    ///
    /// Infallible on purpose: the only callers are names, which Audacity may
    /// legitimately leave off, and a nameless track is not a reason to refuse a
    /// file. A name of the *wrong type* would be, but no corpus file has one and
    /// inventing an error path for it would be inventing a test for it too.
    fn text_or<'a>(&'a self, attr: &'static str, default: &'a str) -> &'a str {
        self.by_name
            .get(attr)
            .and_then(Value::as_str)
            .unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{Project, SampleFormat};
    use crate::doc::{Event, Value};
    use crate::error::Error;

    fn start(name: &str) -> Event {
        Event::Start(Arc::from(name))
    }

    fn end(name: &str) -> Event {
        Event::End(Arc::from(name))
    }

    fn attr(name: &str, value: Value) -> Event {
        Event::Attr {
            name: Arc::from(name),
            value,
        }
    }

    fn number(name: &str, value: f64) -> Event {
        attr(name, Value::F64(value))
    }

    fn count(name: &str, value: u64) -> Event {
        attr(name, Value::U64(value))
    }

    /// A `wavetrack` with one clip per `(offset, trim_left, trim_right,
    /// num_samples)` tuple, at 192 kHz, which is the rate the corpus numbers in
    /// these tests came from.
    fn track_of(clips: &[(f64, f64, f64, u64)]) -> Vec<Event> {
        track_at(192_000.0, Some(192_000.0), clips)
    }

    /// The same, with the two rates set independently so a test can make them
    /// disagree the way every corpus file does, or leave the track's off.
    fn track_at(
        project_rate: f64,
        track_rate: Option<f64>,
        clips: &[(f64, f64, f64, u64)],
    ) -> Vec<Event> {
        let mut events = vec![
            start("project"),
            number("rate", project_rate),
            start("wavetrack"),
            attr("name", Value::Text("Audio 1".to_owned())),
            attr("sampleformat", Value::U32(SampleFormat::Int24.code())),
        ];
        if let Some(rate) = track_rate {
            events.push(number("rate", rate));
        }
        for &(offset, trim_left, trim_right, num_samples) in clips {
            events.extend([
                start("waveclip"),
                number("offset", offset),
                number("trimLeft", trim_left),
                number("trimRight", trim_right),
                start("sequence"),
                count("numsamples", num_samples),
                count("maxsamples", 262_144),
                attr("sampleformat", Value::U32(SampleFormat::Int24.code())),
                start("waveblock"),
                count("start", 0),
                count("blockid", 1),
                end("waveblock"),
                end("sequence"),
                end("waveclip"),
            ]);
        }
        events.extend([end("wavetrack"), end("project")]);
        events
    }

    #[test]
    fn a_clips_offset_is_its_sequences_origin_not_its_audible_start() {
        // The corpus clip that settles it, and the one before it. Read as an
        // audible start, the trimmed clip begins at 4.314469 s - inside its
        // predecessor, which runs to 8.525766 s. Read as a sequence origin it
        // begins at 9.525766 s.
        let events = track_of(&[
            (8.525_765_625, 0.0, 0.0, 107_341),
            (9.084_833_333_333_334, 0.0, 0.0, 84_659),
            (4.314_468_75, 5.211_296_875, 0.0, 1_299_910),
        ]);
        let project = Project::from_events(&events).expect("a layout that works");
        let track = &project.tracks[0];
        let rate = track.rate;

        // Sorted by audible start, which puts the clip with the smallest
        // `offset` last.
        let offsets: Vec<f64> = track.clips.iter().map(|clip| clip.offset).collect();
        assert_eq!(
            offsets,
            vec![8.525_765_625, 9.084_833_333_333_334, 4.314_468_75],
            "document order and offset order are both wrong orders to play in"
        );

        let trimmed = &track.clips[2];
        assert_eq!(trimmed.first_audible_sample(rate), 1_000_569);
        assert_eq!(trimmed.start(rate), 9.525_765_625);
        assert_eq!(trimmed.audible_samples(rate), 299_341);
        assert_eq!(trimmed.end(rate), 11.084_833_333_333_334);

        // Contiguous, which is the property the corpus has and the reason to
        // believe the reading.
        assert_eq!(
            track.clips[0].end_sample(rate),
            track.clips[1].start_sample(rate)
        );
        assert_eq!(track.clips[1].end_sample(rate), trimmed.start_sample(rate));
        assert_eq!(track.duration(), 11.084_833_333_333_334);
    }

    #[test]
    fn a_trim_is_rounded_to_samples_and_not_truncated() {
        // 8.845239583333333 s at 192 kHz is 1698286.0000000 samples to within
        // 3e-9, and truncating the product loses one of them. One sample is the
        // difference between a contiguous timeline and an overlapping one.
        let events = track_of(&[(0.0, 0.0, 8.845_239_583_333_333, 2_698_855)]);
        let project = Project::from_events(&events).expect("model");
        let clip = &project.tracks[0].clips[0];
        assert_eq!(clip.audible_samples(192_000.0), 1_000_569);
        assert_eq!(clip.end(192_000.0), 5.211_296_875);
    }

    #[test]
    fn overlapping_clips_are_refused_rather_than_laid_out_anyway() {
        // Two untrimmed clips, the second starting a second inside the first.
        let events = track_of(&[(0.0, 0.0, 0.0, 384_000), (1.0, 0.0, 0.0, 384_000)]);
        let error = Project::from_events(&events).expect_err("an impossible timeline");
        match error {
            Error::OverlappingClips {
                track,
                start,
                previous_end,
            } => {
                assert_eq!(track, "Audio 1");
                assert_eq!(start, 1.0);
                assert_eq!(previous_end, 2.0);
            }
            other => panic!("expected an overlap refusal, got {other}"),
        }
    }

    #[test]
    fn a_gap_between_clips_is_kept() {
        // VCW's model has gaps in it - a track's end is not the next track's
        // start - so a gap is content, not an error and not something to close.
        let events = track_of(&[(0.0, 0.0, 0.0, 192_000), (2.0, 0.0, 0.0, 192_000)]);
        let project = Project::from_events(&events).expect("model");
        let track = &project.tracks[0];
        assert_eq!(track.clips[0].end(192_000.0), 1.0);
        assert_eq!(track.clips[1].start(192_000.0), 2.0);
        assert_eq!(track.duration(), 3.0);
    }

    #[test]
    fn the_track_rate_is_used_and_the_project_rate_is_only_recorded() {
        // The rate trap. `project/@rate` says 192000.0 in every corpus file;
        // this track says 48000.0 and it is the one that decides how long the
        // audio is. Trusting the project attribute would make this clip a
        // quarter of its real length and play it at four times speed.
        let events = track_at(192_000.0, Some(48_000.0), &[(0.0, 0.0, 0.0, 48_000)]);
        let project = Project::from_events(&events).expect("model");
        assert_eq!(project.editor_rate_preference, Some(192_000.0));
        assert_eq!(project.rate(), Some(48_000.0));
        assert_eq!(project.tracks[0].duration(), 1.0);
    }

    #[test]
    fn a_track_without_a_rate_is_refused_rather_than_defaulted() {
        // The project's rate is there and is not a fallback. A defaulted rate is
        // the four-times-speed bug with no error to find it by.
        let events = track_at(192_000.0, None, &[(0.0, 0.0, 0.0, 48_000)]);
        assert!(
            matches!(
                Project::from_events(&events),
                Err(Error::MissingAttr { attr: "rate", .. })
            ),
            "a wavetrack with no rate must be refused"
        );
    }

    #[test]
    fn an_all_unity_envelope_is_the_same_thing_as_no_envelope() {
        // A conversion gave 16 clips a single val="1.0" point each. The user
        // drew none of them, and a consumer that saw an envelope would apply a
        // gain curve where there is none.
        let mut events = track_of(&[(0.0, 0.0, 0.0, 192_000)]);
        let at = events
            .iter()
            .position(|e| matches!(e, Event::End(n) if &**n == "waveclip"))
            .expect("the clip closes");
        events.splice(
            at..at,
            [
                start("envelope"),
                count("numpoints", 1),
                start("controlpoint"),
                number("t", 14.056_536_458_333_312),
                number("val", 1.0),
                end("controlpoint"),
                end("envelope"),
            ],
        );
        let project = Project::from_events(&events).expect("model");
        assert!(
            project.tracks[0].clips[0].envelope.is_empty(),
            "a unity point is not an envelope"
        );

        // A point that is not unity is kept, so the emptiness above is a
        // decision about content and not a parser that drops envelopes.
        let mut drawn = events.clone();
        for event in &mut drawn {
            if let Event::Attr { name, value } = event
                && &**name == "val"
            {
                *value = Value::F64(0.5);
            }
        }
        let project = Project::from_events(&drawn).expect("model");
        assert_eq!(
            project.tracks[0].clips[0].envelope,
            vec![(14.056_536_458_333_312, 0.5)]
        );
    }

    #[test]
    fn the_three_sample_formats_are_known_and_a_fourth_is_refused() {
        assert_eq!(
            SampleFormat::from_code(0x0002_0001).ok(),
            Some(SampleFormat::Int16)
        );
        assert_eq!(
            SampleFormat::from_code(0x0004_0001).ok(),
            Some(SampleFormat::Int24)
        );
        assert_eq!(
            SampleFormat::from_code(0x0004_000F).ok(),
            Some(SampleFormat::Float32)
        );
        assert_eq!(
            SampleFormat::Int24.bytes_per_sample(),
            4,
            "24 bits in 4 bytes"
        );
        assert_eq!(SampleFormat::Int16.bytes_per_sample(), 2);

        // There is no 32-bit integer format. A plausible code for one has the
        // right byte count in its high half, which is exactly why the low half
        // has to be checked: inferring the width would read int32 as float32
        // and produce noise at full scale.
        assert!(
            matches!(
                SampleFormat::from_code(0x0004_0002),
                Err(Error::UnknownSampleFormat { found: 0x0004_0002 })
            ),
            "a plausible fourth code must be refused, not inferred"
        );
    }

    #[test]
    fn metadata_is_a_map_because_tag_order_is_not_stable() {
        // An AUP3 to AUP4 conversion reorders `tag` elements with no change of
        // content, so anything that compared them as a sequence would report a
        // difference that is not there.
        let events = vec![
            start("project"),
            start("tags"),
            start("tag"),
            attr("name", Value::Text("GENRE".to_owned())),
            attr("value", Value::Text("Metal".to_owned())),
            end("tag"),
            start("tag"),
            attr("name", Value::Text("ALBUM".to_owned())),
            attr("value", Value::Text("Obsolete (Vinyl)".to_owned())),
            end("tag"),
            end("tags"),
            end("project"),
        ];
        let project = Project::from_events(&events).expect("model");
        assert_eq!(project.tags.get("GENRE").map(String::as_str), Some("Metal"));
        assert_eq!(
            project.tags.get("ALBUM").map(String::as_str),
            Some("Obsolete (Vinyl)")
        );
        assert_eq!(project.tags.len(), 2);
    }

    #[test]
    fn a_labeltrack_is_recognized_by_its_name_and_not_by_its_attributes() {
        // In AUP4 a labeltrack carries wavetrack's whole attribute set - gain,
        // rate, sampleformat and all - so a reader that sniffed attributes would
        // build a silent audio track out of the user's track boundaries.
        let events = vec![
            start("project"),
            start("labeltrack"),
            attr("name", Value::Text("Labels 1".to_owned())),
            number("rate", 192_000.0),
            number("gain", 1.0),
            attr("sampleformat", Value::U32(SampleFormat::Float32.code())),
            count("numlabels", 2),
            start("label"),
            number("t", 9.0),
            number("t1", 972.898),
            attr("title", Value::Text("Quanah Parker".to_owned())),
            end("label"),
            start("label"),
            number("t", 972.898),
            number("t1", 1466.739),
            attr("title", Value::Text("IDLT".to_owned())),
            end("label"),
            end("labeltrack"),
            end("project"),
        ];
        let project = Project::from_events(&events).expect("model");
        assert!(project.tracks.is_empty(), "a labeltrack is not a wavetrack");
        assert_eq!(project.label_tracks.len(), 1);
        let labels = &project.label_tracks[0].labels;
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].title, "Quanah Parker");
        assert_eq!(labels[0].t, 9.0);
        assert_eq!(labels[1].t1, 1466.739);
    }

    #[test]
    fn a_shared_block_is_counted_once_in_the_distinct_set_and_twice_in_the_references() {
        // 532 references to 456 blocks in the corpus, one block used three
        // times. Anything that copies or frees blocks works from the distinct
        // set; anything that reads audio works from the references.
        let events = track_of(&[(0.0, 0.0, 0.0, 192_000), (1.0, 0.0, 0.0, 192_000)]);
        let project = Project::from_events(&events).expect("model");
        assert_eq!(project.block_refs().count(), 2);
        assert_eq!(project.distinct_blocks().len(), 1);
    }
}
