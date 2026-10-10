/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The VCW frontend over HTTP, for a turntable on a machine with no screen (§52).
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

//! §52's remote interface: the existing frontend, offered over HTTP.
//!
//! A turntable attached to a headless machine is a first-class deployment.
//! The desktop shell needs 135 linked packages and 259 MB of WebKit and GTK;
//! the command line needs ALSA and libc. On the host this exists for, the
//! second is available and the first is not.
//!
//! What this crate is *not* is a second application. §2 put every decision
//! behind one module in the frontend - `app/ui/src/api.ts`, 37 functions over
//! `invoke` and one subscription - precisely so that a second transport could
//! be added without a second implementation of anything. This crate is that
//! transport and nothing else.
//!
//! The trust boundary is in [`auth`] and is the part worth reading first.

pub mod auth;
pub mod backend;
pub mod server;
