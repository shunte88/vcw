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

//! Acoustic fingerprinting and AcoustID lookup.
//!
//! Requirements: §25 (fingerprinting), §26, §27 (lookup).
//!
//! [`chromaprint`] is built (WP-21): bytes off a capture in, a fingerprint out, and
//! nothing else - no project, no signal analysis, no network. What a *region* is lives
//! in `vcw_core::fingerprinting`, because deciding where a track starts is the
//! detector's job and this crate has no business knowing one exists. [`acoustid`] is
//! WP-22, and `vcw-identify` stays separate from both of them, which is §27.
//!
//! S4 settled how fingerprinting attaches to capture: it runs off the capture stream
//! at the capture rate with plain narrowing to `i16`. Rate, gain and narrowing all
//! proved free, so there is no staging file and no pre-decimation.

pub mod acoustid;
pub mod chromaprint;
