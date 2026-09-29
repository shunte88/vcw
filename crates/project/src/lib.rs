/*
 *  lib.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `.vcw` project: a single self-contained SQLite file.
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

//! The `.vcw` project: a single self-contained SQLite file.
//!
//! Requirements: §12 (format), §13, §14 (block capture and transactions), §15
//! (recovery), §16 (versioning), §29, §31, §49.
//!
//! D1 fixes the shape: the schema is a deliberate *superset* of Audacity's AUP4 -
//! `sampleblocks` column-for-column identical, same summary pyramids, blocks never
//! updated once written - plus the tables Audacity has no equivalent for. The file
//! self-identifies through SQLite's `application_id` and `user_version`, so the
//! reader never has to trust the extension.
//!
//! Superset, not clone: §8 requires 32-bit integer capture and Audacity has no
//! sample-format code for it (S5). Compatibility is architectural and one-way -
//! we import `.aup3` and `.aup4`, we do not write them.
//!
//! ```no_run
//! # fn main() -> Result<(), vcw_project::Error> {
//! use vcw_project::{validate, Options, Project};
//!
//! let project = Project::create("side-a.vcw")?;
//! assert!(validate(&project, Options::default())?.is_clean());
//! project.close()?;
//! # Ok(()) }
//! ```

pub mod disc;
pub mod doc;
pub mod error;
pub mod meta;
pub mod migrate;
pub mod pcm;
pub mod persistence;
pub mod recovery;
pub mod release;
pub mod schema;
pub mod session;
pub mod side;
pub mod sqlite;
pub mod track;
pub mod validate;
pub mod waveform;

pub use error::{Error, Result};
// Re-exported because the project layer's own surface already speaks it:
// `Session::advance` and `pcm::Reader::open` both take one, so a caller that
// cannot name the type cannot call them.
pub use migrate::{MIGRATIONS, Migration};
pub use pcm::Layout;
pub use persistence::{Checkpoint, Writer};
pub use recovery::{Assessment, Plan, Recovered, Sidecars, recover, recover_all, survey};
pub use rusqlite::Connection;
pub use schema::{APPLICATION_ID, EXTENSION, FORMAT_VERSION, SCHEMA_VERSION};
pub use session::Session;
pub use sqlite::{Access, PAGE_CACHE_KIB, Project, block_checksum};
pub use validate::{Finding, Options, Report, integrity_check, validate};

/// Unix seconds, for the `*_at` columns.
///
/// Saturating rather than panicking on a clock before the epoch: a wrong timestamp
/// is a nuisance, and refusing to commit a captured block over it is not.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
