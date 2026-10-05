/*
 *  sniff.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Deciding what an Audacity file is before reading a byte of it.
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
//! Deciding what an Audacity file is before reading a byte of it.
//!
//! Three facts settled by S5 make this its own module rather than a line at the
//! top of the parser:
//!
//! - **`application_id` is `"AUDY"` for AUP3 and AUP4 alike**, so the magic
//!   identifies the format and not the version.
//! - **The extension is whatever the user typed.** A `.aup3` that Audacity 4 has
//!   converted in place is not a thing, but a renamed file is.
//! - **`user_version` is the only version there is**, and it is a packed dotted
//!   quad: `0x03070000` for 3.7.0.0 and `0x04000001` for 4.0.0.1.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{Error, Result};

/// `"AUDY"` big-endian, which is what Audacity writes into `application_id`.
///
/// Note it is not `"AUD3"`: the magic carries no version. It is also unchanged
/// by the AUP4 conversion.
pub const AUDACITY_APPLICATION_ID: u32 = 0x4155_4459;

/// Which Audacity document generation a file carries.
///
/// The two differ in the document, not in the audio: S5 proved the `sampleblocks`
/// layer converts byte-identically, so this distinction never reaches the sample
/// reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// Audacity 3.x. No `project_history`, no `0x10` record, no `waveblock/@length`.
    Aup3,
    /// Audacity 4.x. Adds `project_history`, the `0x10` binary record and
    /// `waveblock/@length`.
    Aup4,
}

impl Version {
    /// The `(major, minor)` pairs this reader knows.
    ///
    /// Matched on major and minor only, so a 3.7.1 or a 4.0.2 is read rather
    /// than refused: the patch digits moved between the corpus AUP3s
    /// (`3.7.0.0`) and their conversions (`4.0.0.1`) without the document
    /// grammar changing at all. A new *minor* is refused, because that is the
    /// axis the one known format change traveled on.
    fn of(user_version: u32) -> Option<Self> {
        match (user_version >> 24, (user_version >> 16) & 0xFF) {
            (3, 7) => Some(Self::Aup3),
            (4, 0) => Some(Self::Aup4),
            _ => None,
        }
    }

    /// How the generation is spelled where it is recorded or printed.
    ///
    /// The document generation, not the extension: a file called `.aup3` that
    /// carries `user_version` `0x04000001` is an AUP4, and this says so.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Aup3 => "aup3",
            Self::Aup4 => "aup4",
        }
    }

    /// Whether this generation declares `waveblock/@length`.
    ///
    /// AUP4 only, and it matched `sampleblocks` in all 5,664 corpus cases, so it
    /// is worth validating where it is. Its absence means AUP3, not corruption.
    #[must_use]
    pub const fn declares_block_lengths(self) -> bool {
        matches!(self, Self::Aup4)
    }
}

/// What a file says about itself.
#[derive(Clone, Debug)]
pub struct Sniffed {
    /// The document generation, decided by `user_version` alone.
    pub version: Version,
    /// The raw `user_version`, kept so a report can print the exact quad rather
    /// than the family this reader sorted it into.
    pub user_version: u32,
    /// SQLite's page size. 4096 and 65536 both occur in the corpus and the AUP4
    /// conversion preserves whichever it found, so nothing may assume either.
    pub page_size: u32,
}

/// Opens an Audacity project read-only and says what it is.
///
/// # Why `SQLITE_OPEN_READ_ONLY` and not a `mode=ro` URI
///
/// They mean the same thing to SQLite, and the flag cannot be mangled. A URI is
/// percent-decoded and terminated by `?` or `#`, so `file:{path}?mode=ro` is
/// wrong for any filename containing one of those - and the paths here are the
/// user's own album names, not ours. What matters is the half of the decision S5
/// actually argued: **not `immutable=1`**, which tells SQLite to ignore the
/// `-wal` sidecar and would silently read a project with uncheckpointed content
/// as a stale database.
///
/// # Errors
///
/// [`Error::NotAudacity`] if `application_id` is not `"AUDY"`, and
/// [`Error::UnknownVersion`] if `user_version` is a generation this reader does
/// not know. Neither is guessed at.
pub fn open(path: impl AsRef<Path>) -> Result<(Connection, Sniffed)> {
    let path: PathBuf = path.as_ref().to_path_buf();
    let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let sniffed = identify(&conn, &path)?;
    Ok((conn, sniffed))
}

/// The half of [`open`] that works on a connection somebody else opened.
///
/// Separate so a test can point it at a database it built in memory, and so the
/// eventual `vcw import` can sniff a file it has already opened for other
/// reasons without opening it twice.
///
/// # Errors
///
/// As [`open`].
pub fn identify(conn: &Connection, path: &Path) -> Result<Sniffed> {
    let application_id: u32 = pragma(conn, "application_id")?;
    if application_id != AUDACITY_APPLICATION_ID {
        return Err(Error::NotAudacity {
            path: path.to_path_buf(),
            found: application_id,
        });
    }

    let user_version: u32 = pragma(conn, "user_version")?;
    let version = Version::of(user_version).ok_or(Error::UnknownVersion {
        path: path.to_path_buf(),
        found: user_version,
    })?;

    Ok(Sniffed {
        version,
        user_version,
        page_size: pragma(conn, "page_size")?,
    })
}

/// Reads one integer pragma.
///
/// As `u32` because all three of these are: SQLite stores `application_id` and
/// `user_version` as signed 32-bit, and `0x41554459` read as `i32` is negative.
fn pragma(conn: &Connection, name: &str) -> Result<u32> {
    // Interpolated rather than bound because a pragma name cannot be a
    // parameter, and `name` is one of three literals in this file.
    let value: i64 = conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))?;
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "all three pragmas are 32-bit, and application_id's top bit is set"
    )]
    Ok(value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an in-memory database carrying the pragmas we care about.
    fn with(application_id: u32, user_version: u32) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory");
        conn.pragma_update(None, "application_id", i64::from(application_id) as i32)
            .expect("application_id");
        conn.pragma_update(None, "user_version", user_version as i32)
            .expect("user_version");
        conn
    }

    #[test]
    fn the_two_known_generations_are_told_apart_by_user_version_alone() {
        let aup3 = with(AUDACITY_APPLICATION_ID, 0x0307_0000);
        let aup4 = with(AUDACITY_APPLICATION_ID, 0x0400_0001);
        assert_eq!(
            identify(&aup3, Path::new("a.aup3")).expect("aup3").version,
            Version::Aup3
        );
        assert_eq!(
            identify(&aup4, Path::new("b.aup4")).expect("aup4").version,
            Version::Aup4
        );
    }

    /// The trap, asserted rather than commented: the same magic, and an
    /// extension that says the opposite of the truth.
    #[test]
    fn neither_the_magic_nor_the_extension_decides_the_version() {
        let renamed = with(AUDACITY_APPLICATION_ID, 0x0400_0001);
        let sniffed = identify(&renamed, Path::new("lying.aup3")).expect("identify");
        assert_eq!(sniffed.version, Version::Aup4);
        assert!(sniffed.version.declares_block_lengths());
    }

    #[test]
    fn a_patch_release_is_read_and_a_new_minor_is_refused() {
        let patched = with(AUDACITY_APPLICATION_ID, 0x0307_0203);
        assert_eq!(
            identify(&patched, Path::new("a.aup3"))
                .expect("3.7.2.3")
                .version,
            Version::Aup3
        );

        let future = with(AUDACITY_APPLICATION_ID, 0x0401_0000);
        let err = identify(&future, Path::new("a.aup4")).expect_err("4.1 is unknown");
        let text = err.to_string();
        assert!(text.contains("4.1.0.0"), "{text}");
        assert!(text.contains("0x04010000"), "{text}");
    }

    #[test]
    fn a_file_that_is_not_audacity_is_refused_by_its_application_id() {
        // What a VCW project carries, which is the mistake most likely to be made.
        let ours = with(0x5643_5700, 1);
        let err = identify(&ours, Path::new("mine.vcw")).expect_err("not audacity");
        assert!(err.to_string().contains("0x56435700"), "{err}");
    }
}
