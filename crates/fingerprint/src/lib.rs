/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Acoustic fingerprinting and AcoustID lookup.
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

//! Acoustic fingerprinting: bytes in, a fingerprint out (§25).
//!
//! Requirements: §25. The lookup that §26 and §27 ask for is next door, in
//! `vcw_metadata::acoustid`.
//!
//! [`chromaprint`] is built (WP-21): bytes off a capture in, a fingerprint out, and
//! nothing else - no project, no signal analysis, no network. What a *region* is lives
//! in `vcw_core::fingerprinting`, because deciding where a track starts is the
//! detector's job and this crate has no business knowing one exists.
//!
//! # Where the AcoustID lookup went, and why it is not here
//!
//! WP-22's plan put it in this crate. It landed in `vcw-metadata` instead, because a
//! lookup is a provider request before it is anything to do with audio: it needs the
//! rate limiter, the retry policy, the credential rule, the user agent, the recorded
//! fixtures and the offline transport that §40 is built out of, and every one of
//! those already exists there. Putting it here would have meant this crate depending
//! on all of it to add one `POST` - and `vcw_metadata::query::Fingerprint` was
//! already the type that carries the evidence, for exactly this reason.
//!
//! So the seam is: this crate answers "what does this audio sound like", with no
//! network in it at all, and `vcw-metadata` answers "what does the world call that".
//! `vcw-identify` stays separate from both, which is §27.
//!
//! S4 settled how fingerprinting attaches to capture: it runs off the capture stream
//! at the capture rate with plain narrowing to `i16`. Rate, gain and narrowing all
//! proved free, so there is no staging file and no pre-decimation.

pub mod chromaprint;
