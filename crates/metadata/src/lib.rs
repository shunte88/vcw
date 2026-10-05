/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Release metadata providers.
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

//! Release metadata providers.
//!
//! Requirements: §28 (providers), §32 (genres), §40 (caching, rate limits, timeouts).
//!
//! The provider trait is the point of this crate: §40 requires the application to be
//! fully usable with networking disabled, so every provider is fixture-backed in test
//! and every call is cancellable and time-boxed in production.
//!
//! # The shape of it
//!
//! ```text
//!   Provider  (discogs::Discogs, musicbrainz::MusicBrainz)
//!      |  builds URLs, parses bodies, knows one service's grammar
//!      v
//!   Client    cache -> rate limit -> retry -> timeout -> cancel
//!      |
//!      v
//!   Transport (Offline | Recorded | Agent)   <- the only thing that can do I/O
//! ```
//!
//! Nothing above the transport knows whether a network exists, and nothing below it
//! knows what a release is. That is what makes §40's "fully usable with networking
//! disabled" a property of the build rather than a flag checked at call sites: the
//! default transport is [`net::Offline`], the real one is behind the `net` feature,
//! and a build without that feature has no code that can open a socket.
//!
//! # Offline is normal
//!
//! An offline provider returns [`Error::Offline`], which is an ordinary answer with
//! an ordinary message. Every part of VCW that works on a *project* - capture,
//! detection, editing, export - works with no provider at all; metadata is
//! enrichment, and the application says so rather than failing.
//!
//! # Credentials (§39)
//!
//! Credentials come from the environment and go out in headers. They are never
//! written to a project file, never part of a URL, never part of a cache key and
//! never in a log line. [`Token`] cannot be serialised and prints as a character
//! count. See [`credentials`].
//!
//! # Getting one release
//!
//! ```
//! use std::sync::Arc;
//! use vcw_metadata::{Cancel, Provider, Query, musicbrainz::MusicBrainz, net::Offline};
//!
//! // The default transport refuses, so this is what the offline path looks like.
//! let provider = MusicBrainz::new(Arc::new(Offline));
//! let query = Query::new().artist("Autechre").album("Amber");
//! match provider.search(&query, &Cancel::new()) {
//!     Ok(candidates) => println!("{} candidates", candidates.len()),
//!     Err(error) => println!("{error}"),
//! }
//! ```

pub mod acoustid;
#[cfg(feature = "net")]
pub mod agent;
pub mod artwork;
pub mod cache;
pub mod client;
pub mod credentials;
pub mod discogs;
pub mod error;
pub mod fixtures;
pub mod genres;
pub mod musicbrainz;
pub mod net;
pub mod policy;
pub mod positions;
pub mod provider;
pub mod query;
pub mod release;
pub mod setup;

pub use acoustid::AcoustId;
#[cfg(feature = "net")]
pub use agent::Agent;
pub use artwork::{Artwork, ArtworkFormat};
pub use cache::{Cache, Disk, Memory, NoCache};
pub use client::{Cancel, Client, Stats};
pub use credentials::{Credentials, Token};
pub use discogs::Discogs;
pub use error::{Error, Result};
pub use genres::Genres;
pub use musicbrainz::MusicBrainz;
pub use net::{Request, Response, Transport, TransportError};
pub use policy::{Clock, Limiter, Retry, SystemClock};
pub use provider::{Provider, search_all};
pub use query::{Criterion, Fingerprint, Query};
pub use release::{
    ArtworkRef, Candidate, Medium, ProviderId, Recording, RecordingRelease, Release, TrackEntry,
};
pub use setup::Setup;

// Re-exported because a caller working with a release works with its sides, and
// should not have to name `vcw-types` to do it.
pub use vcw_types::vinyl::{Face, Numbering, Position, Side};
