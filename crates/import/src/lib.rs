/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Reading Audacity .aup3 and .aup4 projects into VCW.
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
//! Reading Audacity .aup3 and .aup4 projects into VCW.
//!
//! Requirements: §12 (import an existing Audacity project), and R13, the risk
//! that owns this crate.
//!
//! # Clean room
//!
//! Audacity is GPL and VCW is MIT, so no Audacity source was read to write any
//! of this. The grammar was derived in spike S5 by observing the bytes of 30
//! projects the user generated with the shipped application, and this crate is
//! the port of `docs/spikes/S5-audacity-format.md`. Keep it that way: the
//! oracle for a disagreement is another file from the corpus, never upstream
//! code.
//!
//! # Shape
//!
//! An Audacity project is a SQLite database, so the reading is in three layers:
//!
//! 1. [`sniff`] - is this an Audacity project, and which generation. The
//!    `application_id` says the former and `user_version` alone says the latter;
//!    the extension is not evidence.
//! 2. [`doc`] - the document, which is a name dictionary plus a flat stream of
//!    self-delimiting records, not XML. Every byte is consumed or the file is
//!    refused.
//! 3. The audio, which lives in `sampleblocks` as mono blocks of 262144 samples
//!    and is read on demand rather than up front: the smallest project in the
//!    corpus is 271 MB.
//!
//! # The trap that matters most
//!
//! `project/@rate` is an editor preference. It reads `192000.0` in all 30 corpus
//! files, including the 22 rips that were captured at 48 kHz. The authoritative
//! rate is `wavetrack/@rate`, per track. A reader that trusts the project
//! attribute plays most of this corpus at four times speed and its output is
//! wrong in a way no assertion about block counts would notice.

pub mod audit;
pub mod doc;
pub mod error;
pub mod fixture;
pub mod land;
pub mod model;
pub mod read;
pub mod sniff;
pub mod timeline;

pub use audit::{Audit, audit};
pub use doc::{Dict, Event, Record, Value};
pub use error::{Error, Result};
pub use fixture::{Shrink, Shrunk, shrink};
pub use land::{Landed, Options, Source, land, land_from};
pub use model::{BlockRef, Clip, Label, LabelTrack, Project, SampleFormat, Track};
pub use read::{Document, Survey, survey};
pub use sniff::{Sniffed, Version};
pub use timeline::Timeline;
