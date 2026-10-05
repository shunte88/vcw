/*
 *  doc.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The Audacity document grammar: a name dictionary and a flat record stream.
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
//! The Audacity document grammar: a name dictionary and a flat record stream.
//!
//! Clean-room. Every rule here was derived from the bytes of generated files and
//! from S5's published description of them; no Audacity source was read. See
//! `docs/spikes/S5-audacity-format.md`, which this module is the Rust port of,
//! and `spikes/aup-format-probe/probe.py`, which is the oracle it is diffed
//! against.
//!
//! ```text
//! dict := 00 04                                  two-byte prologue, invariant
//!         ( 0F id:u16 nbytes:u16 utf32le[nbytes] )*
//!
//! doc  := record*
//!    01 id:u16                                   start element
//!    02 id:u16                                   end element
//!    03 id:u16 nbytes:u32 utf32le[nbytes]        attribute, string
//!    04 id:u16 value:i32                         attribute, 32-bit signed
//!    05 id:u16 value:u8                          attribute, byte / bool
//!    06 id:u16 value:u32                         attribute, 32-bit
//!    07 id:u16 value:u64                         attribute, 64-bit
//!    08 id:u16 value:u32                         attribute, 32-bit
//!    0A id:u16 value:f64 digits:i32              attribute, double + precision hint
//!    0C nbytes:u32 utf32le[nbytes]               character data
//!    0F id:u16 nbytes:u16 utf32le[nbytes]        dictionary entry, inline
//!    10 id:u16 nbytes:u32 bytes[nbytes]          attribute, binary blob  (AUP4)
//! ```
//!
//! Two properties of that table do the work, and both are why this parser
//! refuses rather than recovers:
//!
//! - **Every record is self-delimiting, and no tag is skippable.** A reader that
//!   guesses a width for a tag it does not know desynchronizes and then produces
//!   plausible garbage. Tags `0x00`, `0x09`, `0x0B`, `0x0D` and `0x0E` occur in
//!   neither version and are refused by offset and name. That property is what
//!   made AUP4's delta cheap to find: the AUP3 grammar did not mis-parse the new
//!   files, it stopped dead at `tag 0x10 at 1237 (name 'data')`.
//! - **Every length is in bytes and every string is UTF-32LE**, so a length read
//!   as a character count under-reads by four. The one exception is `0x10`,
//!   whose length is a real byte count because its payload is not text - which
//!   makes it the single place a reader who has internalised the rule will
//!   over-correct.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{Error, Result};

/// Dictionary entry, and also a record that may appear inline in the document.
const TAG_DICT_ENTRY: u8 = 0x0F;
/// The two bytes every dictionary begins with, in file order.
const DICT_PROLOGUE: [u8; 2] = [0x00, 0x04];

/// An attribute's value, kept in the width the file used.
///
/// Not widened to one numeric type, because the width is evidence: `numsamples`
/// is a `u64` and `groupId`'s no-group sentinel is `u32::MAX`, and a model that
/// had already flattened both would have to guess which it was looking at.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `0x03`. UTF-32LE in the file, UTF-8 here.
    Text(String),
    /// `0x04`.
    I32(i32),
    /// `0x05`. A byte, used for booleans.
    U8(u8),
    /// `0x06` and `0x08`, which carry identical payloads and are used
    /// interchangeably for the same attribute - `sampleformat` appears under
    /// both within one corpus. Presumably distinct C++ overloads upstream; a
    /// reader must treat them alike.
    U32(u32),
    /// `0x07`.
    U64(u64),
    /// `0x0A`. The record also carries a precision hint for formatting, which is
    /// dropped: it says how Audacity would print the number, not what it is.
    F64(f64),
    /// `0x10`, AUP4 only. Kept whole rather than decoded; the only observed use
    /// is a PNG screenshot of the editor window.
    Blob(Vec<u8>),
}

impl Value {
    /// The value as an unsigned integer, whatever width it was stored in.
    ///
    /// `None` for text, blobs, negatives and non-integral doubles, so a caller
    /// reading `numsamples` cannot silently accept `rate`.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            Self::I32(v) => u64::try_from(v).ok(),
            Self::U8(v) => Some(u64::from(v)),
            Self::U32(v) => Some(u64::from(v)),
            Self::U64(v) => Some(v),
            // Audacity writes some integral quantities as doubles. Accepted only
            // when the double really is one.
            Self::F64(v) if v >= 0.0 && v.fract() == 0.0 && v <= 9_007_199_254_740_992.0 =>
            {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "guarded above: non-negative, integral, and within 2^53"
                )]
                Some(v as u64)
            }
            _ => None,
        }
    }

    /// The value as a double, whatever width it was stored in.
    ///
    /// Integers widen because Audacity is inconsistent about which numeric
    /// record it uses for the same attribute across versions.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::F64(v) => Some(v),
            Self::I32(v) => Some(f64::from(v)),
            Self::U8(v) => Some(f64::from(v)),
            Self::U32(v) => Some(f64::from(v)),
            #[expect(
                clippy::cast_precision_loss,
                reason = "a u64 attribute past 2^53 is a frame count, never a timing"
            )]
            Self::U64(v) => Some(v as f64),
            _ => None,
        }
    }

    /// The value as text, for attributes that really are text.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(v) => Some(v),
            _ => None,
        }
    }
}

/// One record from the document stream.
///
/// Names are `Arc<str>` rather than borrowed, because the document may extend
/// the dictionary while it is being read - the `0x0F` record appears in both
/// grammars - and a borrow of a map that is still growing does not compile.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `0x01`. An element opens.
    Start(Arc<str>),
    /// `0x02`. An element closes.
    End(Arc<str>),
    /// One attribute of the element that is currently open.
    Attr {
        /// The attribute's name, resolved through the file's own dictionary.
        name: Arc<str>,
        /// Its value, in the width the file used.
        value: Value,
    },
    /// `0x0C`. Character data between elements.
    Text(String),
}

/// A file's name table.
///
/// **Per-file, never a format constant.** The corpus carries 59, 61 and 81
/// entries, and the ids assigned to a given name differ between files. Resolve
/// through the file's own dictionary every time; never cache an id across files
/// and never hard-code one.
#[derive(Clone, Debug, Default)]
pub struct Dict {
    names: HashMap<u16, Arc<str>>,
}

impl Dict {
    /// The name for an id, if this file defines one.
    #[must_use]
    pub fn get(&self, id: u16) -> Option<&Arc<str>> {
        self.names.get(&id)
    }

    /// How many names this file defines.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the dictionary is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// Parses `project.dict`.
///
/// # Errors
///
/// [`Error::BadDictPrologue`] if the two-byte prologue is missing,
/// [`Error::BadDictTag`] for any entry tag other than `0x0F`, and
/// [`Error::DictTruncated`] if the blob ends mid-entry. Consumes every byte or
/// fails; a dictionary with trailing bytes is not a dictionary we understand.
pub fn parse_dict(blob: &[u8]) -> Result<Dict> {
    if blob.len() < DICT_PROLOGUE.len() {
        return Err(Error::DictTruncated {
            offset: 0,
            length: blob.len(),
        });
    }
    let prologue = [blob[0], blob[1]];
    if prologue != DICT_PROLOGUE {
        return Err(Error::BadDictPrologue { found: prologue });
    }

    let mut names = HashMap::new();
    let mut at = DICT_PROLOGUE.len();
    while at < blob.len() {
        let start = at;
        // Tag, id and length: five bytes before any payload.
        if blob.len() - at < 5 {
            return Err(Error::DictTruncated {
                offset: start,
                length: blob.len(),
            });
        }
        if blob[at] != TAG_DICT_ENTRY {
            return Err(Error::BadDictTag {
                tag: blob[at],
                offset: start,
            });
        }
        let id = u16::from_le_bytes([blob[at + 1], blob[at + 2]]);
        let nbytes = usize::from(u16::from_le_bytes([blob[at + 3], blob[at + 4]]));
        at += 5;
        let end = at.checked_add(nbytes).ok_or(Error::DictTruncated {
            offset: start,
            length: blob.len(),
        })?;
        if end > blob.len() {
            return Err(Error::DictTruncated {
                offset: start,
                length: blob.len(),
            });
        }
        names.insert(id, Arc::from(utf32(&blob[at..end], at)?.as_str()));
        at = end;
    }

    Ok(Dict { names })
}

/// The record shapes the format defines, which is the whole of it.
///
/// A tag with no arm here is not a tag we skip, it is a tag we refuse: every
/// record is self-delimiting and none is skippable, so a reader that guesses a
/// width desynchronizes and then produces plausible garbage. `0x00`, `0x09`,
/// `0x0B`, `0x0D` and `0x0E` occur in neither generation of the format, and if
/// a third generation introduces one we want to be told, by offset and name,
/// exactly as AUP4's `0x10` told us.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// `0x01`, an element opening.
    Start,
    /// `0x02`, an element closing.
    End,
    /// `0x03`, a string attribute.
    Text,
    /// `0x04`.
    I32,
    /// `0x05`.
    U8,
    /// `0x06` and `0x08`. See [`Value::U32`].
    U32,
    /// `0x07`.
    U64,
    /// `0x0A`, a double followed by a precision hint.
    F64,
    /// `0x0C`, character data. The one record with no name id.
    CharData,
    /// `0x0F`, a dictionary entry, which defines a name rather than using one.
    DictEntry,
    /// `0x10`, a binary blob. AUP4 only.
    Blob,
}

impl Shape {
    /// The shape of a record, or `None` for a tag the format never defines.
    const fn of(tag: u8) -> Option<Self> {
        Some(match tag {
            0x01 => Self::Start,
            0x02 => Self::End,
            0x03 => Self::Text,
            0x04 => Self::I32,
            0x05 => Self::U8,
            0x06 | 0x08 => Self::U32,
            0x07 => Self::U64,
            0x0A => Self::F64,
            0x0C => Self::CharData,
            TAG_DICT_ENTRY => Self::DictEntry,
            0x10 => Self::Blob,
            _ => return None,
        })
    }
}

/// One record, and the bytes it occupied.
///
/// The span is what makes [`crate::fixture`] possible: records are
/// self-delimiting, so shrinking a project is deleting byte ranges rather than
/// re-encoding a document, and every byte that survives is still a byte
/// Audacity wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// What the record says.
    pub event: Event,
    /// Where it was: `blob[span]` is the whole record including its tag.
    pub span: std::ops::Range<usize>,
}

/// Parses `project.doc` against a dictionary, consuming every byte.
///
/// The dictionary is taken by value and returned alongside the events because
/// the document may add to it: `0x0F` is a legal record in the stream as well as
/// in the dictionary blob.
///
/// # Errors
///
/// See [`parse_doc_spans`], which this drops the spans from.
pub fn parse_doc(blob: &[u8], dict: Dict) -> Result<(Vec<Event>, Dict)> {
    let (records, dict) = parse_doc_spans(blob, dict)?;
    Ok((
        records.into_iter().map(|record| record.event).collect(),
        dict,
    ))
}

/// Parses `project.doc`, keeping each record's byte range.
///
/// # Errors
///
/// [`Error::UnknownTag`] for a tag with no rule, [`Error::Truncated`] for a
/// record claiming more bytes than remain, [`Error::UnknownName`] for an id the
/// dictionary does not define, and [`Error::MismatchedElement`] or
/// [`Error::UnclosedElement`] if the element structure does not close.
///
/// The element checks are stricter than the oracle, which does not track
/// nesting. They are cheap and they are the only thing that would notice a
/// desynchronization that happened to land on a plausible tag.
pub fn parse_doc_spans(blob: &[u8], mut dict: Dict) -> Result<(Vec<Record>, Dict)> {
    let mut events = Vec::new();
    let mut open: Vec<Arc<str>> = Vec::new();
    let mut cursor = Cursor::new(blob);

    while !cursor.done() {
        let tag = cursor.begin_record();
        let shape = Shape::of(tag).ok_or_else(|| Error::UnknownTag {
            tag,
            offset: cursor.record_start(),
            // The record's own name, when it has one and it resolves. This is
            // what made AUP4's delta a one-line diagnosis rather than a hunt:
            // `tag 0x10 at 1237 (name 'data')`.
            name: cursor.peek_name(&dict).map(|n| n.to_string()),
        })?;

        match shape {
            Shape::CharData => {
                let n = cursor.u32()? as usize;
                let text = cursor.utf32(n)?;
                push(&mut events, &cursor, Event::Text(text));
            }
            // A dictionary entry in the middle of the document.
            //
            // Unattested in all 30 corpus projects, so the widths here are the
            // only ones the format gives tag 0x0F - the u16 length it uses in
            // the dictionary blob - rather than invented. Writing this arm
            // found a defect in the oracle, whose own copy of it advanced six
            // bytes past the header instead of four: both had never executed,
            // and full byte consumption cannot catch a branch that never runs.
            Shape::DictEntry => {
                let id = cursor.u16()?;
                let n = usize::from(cursor.u16()?);
                let name: Arc<str> = Arc::from(cursor.utf32(n)?.as_str());
                dict.names.insert(id, name);
            }
            Shape::Start => {
                let name = cursor.name(&dict, tag)?;
                open.push(Arc::clone(&name));
                push(&mut events, &cursor, Event::Start(name));
            }
            Shape::End => {
                let name = cursor.name(&dict, tag)?;
                let expected = open.pop().ok_or_else(|| Error::MismatchedElement {
                    found: name.to_string(),
                    expected: "nothing".to_owned(),
                    offset: cursor.record_start(),
                })?;
                if expected != name {
                    return Err(Error::MismatchedElement {
                        found: name.to_string(),
                        expected: expected.to_string(),
                        offset: cursor.record_start(),
                    });
                }
                push(&mut events, &cursor, Event::End(name));
            }
            Shape::Text => {
                let name = cursor.name(&dict, tag)?;
                let n = cursor.u32()? as usize;
                let value = Value::Text(cursor.utf32(n)?);
                push(&mut events, &cursor, Event::Attr { name, value });
            }
            Shape::I32 => {
                let name = cursor.name(&dict, tag)?;
                let value = Value::I32(cursor.i32()?);
                push(&mut events, &cursor, Event::Attr { name, value });
            }
            Shape::U8 => {
                let name = cursor.name(&dict, tag)?;
                let value = Value::U8(cursor.u8()?);
                push(&mut events, &cursor, Event::Attr { name, value });
            }
            Shape::U32 => {
                let name = cursor.name(&dict, tag)?;
                let value = Value::U32(cursor.u32()?);
                push(&mut events, &cursor, Event::Attr { name, value });
            }
            Shape::U64 => {
                let name = cursor.name(&dict, tag)?;
                let value = Value::U64(cursor.u64()?);
                push(&mut events, &cursor, Event::Attr { name, value });
            }
            Shape::F64 => {
                let name = cursor.name(&dict, tag)?;
                let value = Value::F64(cursor.f64()?);
                // The precision hint, read and dropped. It says how Audacity
                // would print the number, not what the number is, and
                // 0xFFFFFFFF means "default". Read rather than skipped so the
                // record's width is stated in one place.
                let _digits = cursor.i32()?;
                push(&mut events, &cursor, Event::Attr { name, value });
            }
            Shape::Blob => {
                let name = cursor.name(&dict, tag)?;
                // The one length in the format that is already a byte count.
                let n = cursor.u32()? as usize;
                let value = Value::Blob(cursor.take(n)?.to_vec());
                push(&mut events, &cursor, Event::Attr { name, value });
            }
        }
    }

    if let Some(innermost) = open.last() {
        return Err(Error::UnclosedElement {
            depth: open.len(),
            innermost: innermost.to_string(),
        });
    }

    Ok((events, dict))
}

/// Pushes a record with the span of the record the cursor just finished.
fn push(into: &mut Vec<Record>, cursor: &Cursor, event: Event) {
    into.push(Record {
        event,
        span: cursor.record_span(),
    });
}

/// A byte reader that knows which record it is inside, so it can say where a
/// truncation happened without every call site repeating the offset.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    tag: u8,
    start: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            at: 0,
            tag: 0,
            start: 0,
        }
    }

    const fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    const fn record_start(&self) -> usize {
        self.start
    }

    /// The bytes the record now finished occupied, tag included.
    const fn record_span(&self) -> std::ops::Range<usize> {
        self.start..self.at
    }

    /// Consumes the tag byte and remembers where this record began.
    ///
    /// Infallible because [`Self::done`] was false, so there is a byte here.
    fn begin_record(&mut self) -> u8 {
        self.start = self.at;
        self.tag = self.bytes[self.at];
        self.at += 1;
        self.tag
    }

    /// The name this record refers to, without consuming it.
    ///
    /// Only for error messages, so every way of not having one - too few bytes
    /// left, an id this file never defined, a record that carries no id at all -
    /// collapses to `None` rather than to a second error.
    fn peek_name(&self, dict: &Dict) -> Option<Arc<str>> {
        let b = self.bytes.get(self.at..self.at + 2)?;
        dict.names
            .get(&u16::from_le_bytes([b[0], b[1]]))
            .map(Arc::clone)
    }

    /// Consumes the record's name id and resolves it through the file's own
    /// dictionary, which is how every record but char data and a dictionary
    /// entry begins.
    fn name(&mut self, dict: &Dict, tag: u8) -> Result<Arc<str>> {
        let id = self.u16()?;
        dict.names
            .get(&id)
            .map(Arc::clone)
            .ok_or(Error::UnknownName {
                tag,
                offset: self.start,
                id,
            })
    }

    /// `n` bytes, or the truncation error naming this record.
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(Error::Truncated {
            tag: self.tag,
            offset: self.start,
            claimed: n,
            available: self.bytes.len() - self.at,
        })?;
        if end > self.bytes.len() {
            return Err(Error::Truncated {
                tag: self.tag,
                offset: self.start,
                claimed: n,
                available: self.bytes.len() - self.at,
            });
        }
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Result<i32> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }

    /// `n` *bytes* of UTF-32LE. The length is never a character count.
    fn utf32(&mut self, n: usize) -> Result<String> {
        let start = self.at;
        utf32(self.take(n)?, start)
    }
}

/// Decodes UTF-32LE, refusing anything that is not a scalar value.
///
/// `offset` is only for the error message, which is why this is a free function:
/// the dictionary parser needs it too and does not have a cursor.
fn utf32(bytes: &[u8], offset: usize) -> Result<String> {
    if !bytes.len().is_multiple_of(4) {
        return Err(Error::BadUtf32Length {
            offset,
            bytes: bytes.len(),
        });
    }
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|unit| {
            let value = u32::from_le_bytes(*unit);
            char::from_u32(value).ok_or(Error::BadUtf32Char {
                offset,
                unit: value,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Dict, Event, Value, parse_dict, parse_doc};
    use crate::error::Error;

    /// Builds a document blob the way Audacity does, so the tests exercise the
    /// grammar rather than a convenience of the parser's own.
    ///
    /// Deliberately not a general encoder. It takes byte-level arguments -
    /// including the tag - so that a test can write a record the format does not
    /// allow, which is half of what needs testing here.
    #[derive(Default)]
    struct Writer(Vec<u8>);

    impl Writer {
        fn utf32(text: &str) -> Vec<u8> {
            text.chars()
                .flat_map(|c| u32::from(c).to_le_bytes())
                .collect()
        }

        /// A dictionary entry, `0x0F id:u16 nbytes:u16 utf32`.
        fn entry(mut self, id: u16, name: &str) -> Self {
            let text = Self::utf32(name);
            self.0.push(0x0F);
            self.0.extend(id.to_le_bytes());
            self.0.extend(
                u16::try_from(text.len())
                    .expect("a short name")
                    .to_le_bytes(),
            );
            self.0.extend(text);
            self
        }

        /// A record with a name id and no payload.
        fn named(mut self, tag: u8, id: u16) -> Self {
            self.0.push(tag);
            self.0.extend(id.to_le_bytes());
            self
        }

        /// A record with a name id and raw payload bytes.
        fn attr(mut self, tag: u8, id: u16, payload: &[u8]) -> Self {
            self.0.push(tag);
            self.0.extend(id.to_le_bytes());
            self.0.extend(payload);
            self
        }

        /// A string attribute, with the length in bytes as the format says.
        fn text(self, id: u16, value: &str) -> Self {
            let text = Self::utf32(value);
            let mut payload = u32::try_from(text.len())
                .expect("a short string")
                .to_le_bytes()
                .to_vec();
            payload.extend(text);
            self.attr(0x03, id, &payload)
        }

        fn bytes(self) -> Vec<u8> {
            self.0
        }
    }

    /// `00 04` and two names, which is the smallest useful dictionary.
    fn dict() -> Dict {
        let mut blob = vec![0x00, 0x04];
        blob.extend(
            Writer::default()
                .entry(1, "project")
                .entry(2, "rate")
                .bytes(),
        );
        parse_dict(&blob).expect("a well-formed dictionary")
    }

    #[test]
    fn a_dictionary_maps_ids_to_names_and_the_prologue_is_required() {
        let parsed = dict();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed.get(1).map(|n| &**n), Some("project"));
        assert_eq!(parsed.get(2).map(|n| &**n), Some("rate"));
        assert_eq!(parsed.get(3), None, "an id no entry defined");

        let mut wrong = vec![0x04, 0x00];
        wrong.extend(Writer::default().entry(1, "project").bytes());
        assert!(
            matches!(
                parse_dict(&wrong),
                Err(Error::BadDictPrologue {
                    found: [0x04, 0x00]
                })
            ),
            "the prologue is 00 04 in file order and a swapped pair is not it"
        );
    }

    #[test]
    fn a_dictionary_name_length_is_bytes_and_not_characters() {
        // "rate" is four characters and sixteen bytes. A parser that read the
        // length as characters would take four bytes, decode one character, and
        // then find the rest of the name where the next tag should be.
        let mut blob = vec![0x00, 0x04];
        blob.extend(Writer::default().entry(7, "rate").bytes());
        assert_eq!(blob.len(), 2 + 5 + 16);
        let parsed = parse_dict(&blob).expect("parse");
        assert_eq!(parsed.get(7).map(|n| &**n), Some("rate"));

        // The same entry with the length written as a character count.
        let mut bad = blob.clone();
        bad[5] = 4;
        bad[6] = 0;
        let error = parse_dict(&bad).expect_err("a character count must not parse");
        assert!(
            matches!(
                error,
                Error::BadDictTag { .. } | Error::DictTruncated { .. }
            ),
            "expected a refusal rather than a silent mis-read, got {error}"
        );
    }

    #[test]
    fn the_five_never_observed_tags_are_refused_by_offset_and_name() {
        // A start element, then a record with a tag the format does not define.
        // The refusal has to name the offset and the attribute: that is exactly
        // how AUP4's new 0x10 record was identified rather than mis-parsed.
        for tag in [0x00_u8, 0x09, 0x0B, 0x0D, 0x0E] {
            let blob = Writer::default()
                .named(0x01, 1)
                .attr(tag, 2, &[0x00, 0x00, 0x00, 0x00])
                .bytes();
            let error = parse_doc(&blob, dict()).expect_err("an undefined tag must be refused");
            match error {
                Error::UnknownTag {
                    tag: found,
                    offset,
                    name,
                } => {
                    assert_eq!(found, tag);
                    assert_eq!(offset, 3, "the record began right after the 3-byte start");
                    assert_eq!(name.as_deref(), Some("rate"));
                }
                other => panic!("tag 0x{tag:02X} gave {other} rather than an unknown-tag refusal"),
            }
        }
    }

    #[test]
    fn a_record_that_claims_more_bytes_than_it_has_is_refused() {
        // A string attribute claiming 4 KiB inside a blob that holds nine bytes.
        let blob = Writer::default()
            .attr(0x03, 2, &4096_u32.to_le_bytes())
            .bytes();
        let error = parse_doc(&blob, dict()).expect_err("a truncated record must be refused");
        assert!(
            matches!(
                error,
                Error::Truncated {
                    tag: 0x03,
                    offset: 0,
                    claimed: 4096,
                    available: 0
                }
            ),
            "got {error}"
        );
    }

    #[test]
    fn the_two_interchangeable_u32_tags_are_read_identically() {
        // 0x06 and 0x08 carry the same payload and Audacity uses both for
        // `sampleformat`. A reader that handled only one would refuse half the
        // corpus, and one that gave them different meanings would be worse.
        let float32 = 0x0004_000F_u32;
        for tag in [0x06_u8, 0x08] {
            let blob = Writer::default()
                .attr(tag, 2, &float32.to_le_bytes())
                .bytes();
            let (events, _) = parse_doc(&blob, dict()).expect("parse");
            assert_eq!(
                events,
                vec![Event::Attr {
                    name: dict().get(2).cloned().expect("rate"),
                    value: Value::U32(float32),
                }],
                "tag 0x{tag:02X}"
            );
        }
    }

    #[test]
    fn a_double_carries_a_precision_hint_that_is_read_and_dropped() {
        // Twelve bytes, not eight: the hint is part of the record's width, so a
        // reader that treats 0x0A as eight bytes desynchronizes by four.
        let mut payload = 48_000.0_f64.to_le_bytes().to_vec();
        payload.extend((-1_i32).to_le_bytes());
        let blob = Writer::default()
            .attr(0x0A, 2, &payload)
            .named(0x01, 1)
            .named(0x02, 1)
            .bytes();
        let (events, _) = parse_doc(&blob, dict()).expect("parse");
        assert_eq!(
            events.len(),
            3,
            "the hint must not be mistaken for a record"
        );
        assert_eq!(
            events[0],
            Event::Attr {
                name: dict().get(2).cloned().expect("rate"),
                value: Value::F64(48_000.0),
            }
        );
    }

    #[test]
    fn a_blob_length_is_a_byte_count_and_needs_no_correction() {
        // The one length in the format that is not a UTF-32 byte count. A reader
        // that had internalised the times-four rule and applied it here would
        // over-read by a factor of four on the only record that carries a PNG.
        let png = [0x89, b'P', b'N', b'G', 0x0D];
        let mut payload = u32::try_from(png.len())
            .expect("five")
            .to_le_bytes()
            .to_vec();
        payload.extend(png);
        let blob = Writer::default().attr(0x10, 2, &payload).bytes();
        let (events, _) = parse_doc(&blob, dict()).expect("parse");
        assert_eq!(
            events,
            vec![Event::Attr {
                name: dict().get(2).cloned().expect("rate"),
                value: Value::Blob(png.to_vec()),
            }]
        );
    }

    #[test]
    fn a_string_length_is_a_byte_count() {
        let blob = Writer::default().text(2, "48000.0").bytes();
        let (events, _) = parse_doc(&blob, dict()).expect("parse");
        assert_eq!(
            events,
            vec![Event::Attr {
                name: dict().get(2).cloned().expect("rate"),
                value: Value::Text("48000.0".to_owned()),
            }]
        );

        // The same record with the length written as a character count. Seven
        // instead of twenty-eight, which leaves 21 bytes where the next tag
        // should be.
        let mut bad = blob.clone();
        bad[3..7].copy_from_slice(&7_u32.to_le_bytes());
        assert!(
            parse_doc(&bad, dict()).is_err(),
            "a character count must not parse as a byte count"
        );
    }

    #[test]
    fn an_unresolvable_name_id_is_refused() {
        let blob = Writer::default().named(0x01, 99).bytes();
        assert!(
            matches!(
                parse_doc(&blob, dict()),
                Err(Error::UnknownName {
                    tag: 0x01,
                    offset: 0,
                    id: 99
                })
            ),
            "the id is the name, so an unresolvable one has no meaning to fall back on"
        );
    }

    #[test]
    fn elements_have_to_close_and_have_to_close_in_order() {
        let unclosed = Writer::default().named(0x01, 1).bytes();
        assert!(matches!(
            parse_doc(&unclosed, dict()),
            Err(Error::UnclosedElement { depth: 1, .. })
        ));

        // 'rate' closing while 'project' is open. The document never does this,
        // which is the point: it is the check that notices a desynchronization
        // that happened to land on a plausible tag.
        let crossed = Writer::default().named(0x01, 1).named(0x02, 2).bytes();
        assert!(
            matches!(
                parse_doc(&crossed, dict()),
                Err(Error::MismatchedElement { .. })
            ),
            "a mismatched close must be refused"
        );

        let closed_twice = Writer::default()
            .named(0x01, 1)
            .named(0x02, 1)
            .named(0x02, 1)
            .bytes();
        assert!(matches!(
            parse_doc(&closed_twice, dict()),
            Err(Error::MismatchedElement { .. })
        ));
    }

    #[test]
    fn character_data_carries_no_name_id() {
        // 0x0C is the one record with a length and no id. Reading an id first
        // would consume two bytes of the length.
        let text = Writer::utf32("hello");
        let mut blob = vec![0x0C];
        blob.extend(u32::try_from(text.len()).expect("twenty").to_le_bytes());
        blob.extend(text);
        let (events, _) = parse_doc(&blob, dict()).expect("parse");
        assert_eq!(events, vec![Event::Text("hello".to_owned())]);
    }

    #[test]
    fn an_inline_dictionary_entry_extends_the_dictionary_rather_than_being_refused() {
        // Unattested in all 30 corpus projects, so this is the arm that says
        // what we do if one ever turns up: define the name and carry on. The
        // record that follows uses it, which is the only way to tell whether the
        // widths were right - the alternative is a desynchronized stream.
        let blob = Writer::default()
            .entry(50, "labeltrack")
            .named(0x01, 50)
            .named(0x02, 50)
            .bytes();
        let (events, extended) = parse_doc(&blob, dict()).expect("parse");
        assert_eq!(extended.len(), 3, "the document added a name");
        assert_eq!(extended.get(50).map(|n| &**n), Some("labeltrack"));
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0],
            Event::Start(extended.get(50).cloned().expect("it"))
        );
    }

    #[test]
    fn a_utf32_unit_that_is_not_a_character_is_refused() {
        // 0xD800 is a surrogate, which is a valid u32 and not a scalar value.
        // Decoded blindly it would be a replacement character, and the name it
        // sat in would then resolve to nothing.
        let mut blob = vec![0x00, 0x04, 0x0F, 0x01, 0x00, 0x04, 0x00];
        blob.extend(0x0000_D800_u32.to_le_bytes());
        assert!(matches!(
            parse_dict(&blob),
            Err(Error::BadUtf32Char {
                unit: 0x0000_D800,
                ..
            })
        ));

        // And a length that is not a whole number of units.
        let ragged = vec![0x00, 0x04, 0x0F, 0x01, 0x00, 0x03, 0x00, b'a', b'b', b'c'];
        assert!(matches!(
            parse_dict(&ragged),
            Err(Error::BadUtf32Length { bytes: 3, .. })
        ));
    }

    #[test]
    fn every_byte_of_the_document_is_accounted_for() {
        // Trailing bytes are the signature of a width mistake somewhere earlier,
        // so the parser has no "stop when it stops making sense" path: it reads
        // to the end of the blob or it fails.
        let blob = Writer::default()
            .named(0x01, 1)
            .attr(0x04, 2, &48_000_i32.to_le_bytes())
            .named(0x02, 1)
            .bytes();
        let (events, _) = parse_doc(&blob, dict()).expect("parse");
        assert_eq!(events.len(), 3);

        let mut extra = blob.clone();
        extra.push(0x04);
        assert!(
            parse_doc(&extra, dict()).is_err(),
            "a stray byte after the last record must be refused"
        );
    }
}
