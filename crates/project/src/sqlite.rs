/*
 *  sqlite.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Creating, opening and closing a `.vcw` project.
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

//! Creating, opening and closing a `.vcw` project.
//!
//! `rusqlite` with the `bundled` feature (D2, [ADR-0002]): synchronous and
//! predictable, no async runtime anywhere near the writer thread, and one SQLite
//! build across every platform instead of whatever the OS shipped. §15 makes
//! recovery a correctness requirement, and recovery behaviour depends on WAL
//! semantics that vary by SQLite version.
//!
//! [ADR-0002]: https://github.com/shunte88/vcw/blob/main/docs/adr/0002-sqlite-binding.md

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use vcw_types::StorageFormat;

use crate::error::{Error, Result};
use crate::meta;
use crate::migrate::{self, MIGRATIONS};
use crate::schema::{APPLICATION_ID, FORMAT_VERSION, PAGE_SIZE, SCHEMA_VERSION};

/// Whether a project is open for writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Read and write. Migrations may run on open.
    ReadWrite,
    /// Read only. Opened with `mode=ro`, never `immutable=1` - the latter makes
    /// SQLite ignore the `-wal` sidecar and silently serve a stale database, which
    /// for a project being recovered is exactly the wrong answer (S5, trap 18).
    ReadOnly,
}

/// An open `.vcw` project.
#[derive(Debug)]
pub struct Project {
    conn: Connection,
    path: PathBuf,
    access: Access,
}

impl Project {
    /// Creates a new project. Fails if the path already exists.
    ///
    /// Refusing to overwrite is not caution for its own sake: a `.vcw` file is
    /// hours of a record that may not be playable again.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} already exists", path.display()),
            )));
        }

        let mut conn = Connection::open(&path)?;
        // page_size must be set before the first table exists, so it belongs here
        // and nowhere else.
        conn.pragma_update(None, "page_size", PAGE_SIZE)?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
        apply_connection_pragmas(&conn)?;

        migrate::apply(&mut conn, MIGRATIONS)?;

        let now = crate::now().to_string();
        let format = FORMAT_VERSION.to_string();
        meta::set(&conn, meta::CREATED_FORMAT_VERSION, &format)?;
        meta::set(&conn, meta::CREATED_BY, migrate::APPLIED_BY)?;
        meta::set(&conn, meta::CREATED_AT, &now)?;
        meta::set(&conn, meta::FORMAT_VERSION, &format)?;
        meta::set(&conn, meta::LAST_WRITTEN_BY, migrate::APPLIED_BY)?;
        meta::set(&conn, meta::LAST_WRITTEN_AT, &now)?;

        Ok(Self {
            conn,
            path,
            access: Access::ReadWrite,
        })
    }

    /// Opens an existing project for writing, migrating it if it is older.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut conn = Connection::open(&path)?;
        apply_connection_pragmas(&conn)?;
        identify(&conn, &path)?;

        migrate::apply(&mut conn, MIGRATIONS)?;
        meta::set(&conn, meta::FORMAT_VERSION, &FORMAT_VERSION.to_string())?;
        meta::set(&conn, meta::LAST_WRITTEN_BY, migrate::APPLIED_BY)?;
        meta::set(&conn, meta::LAST_WRITTEN_AT, &crate::now().to_string())?;

        Ok(Self {
            conn,
            path,
            access: Access::ReadWrite,
        })
    }

    /// Opens an existing project read-only, without migrating it.
    ///
    /// Uses a `mode=ro` URI so a populated `-wal` is honoured.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let uri = format!("file:{}?mode=ro", path.display());
        let conn = Connection::open_with_flags(
            &uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
        conn.pragma_update(None, "foreign_keys", true)?;
        identify(&conn, &path)?;
        Ok(Self {
            conn,
            path,
            access: Access::ReadOnly,
        })
    }

    /// The underlying connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// The underlying connection, mutably - needed for transactions.
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Where the project lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether this handle can write.
    pub fn access(&self) -> Access {
        self.access
    }

    /// The schema version in the file.
    pub fn schema_version(&self) -> Result<u32> {
        Ok(migrate::current_version(&self.conn)?)
    }

    /// Refuses a project older than this build, for a reader that needs v2.
    ///
    /// The §29 tables - `releases`, `sides`, `tracks` - do not exist in v1, so a
    /// reader that queries them against a v1 file gets `no such table` from
    /// SQLite. That is a true statement about a database and a useless one to put
    /// in front of a person, so the readers that need those tables ask this first
    /// and get [`Error::SchemaNeedsUpgrade`], which says what to do about it.
    ///
    /// Deliberately not called by [`Project::open_read_only`]: a v1 project is a
    /// real project, and everything about a capture - the layout, the blocks, the
    /// waveform - reads out of it unchanged. Refusing at the door would break the
    /// readers that are perfectly happy.
    ///
    /// # Errors
    ///
    /// [`Error::SchemaNeedsUpgrade`] if the file is older than
    /// [`SCHEMA_VERSION`], or the underlying pragma read if it fails.
    pub fn require_current_schema(&self) -> Result<()> {
        let found = self.schema_version()?;
        if found < SCHEMA_VERSION {
            return Err(Error::SchemaNeedsUpgrade {
                path: self.path.clone(),
                found,
                supported: SCHEMA_VERSION,
            });
        }
        Ok(())
    }

    /// The project-format version last written to the file (§16).
    pub fn format_version(&self) -> Result<Option<u32>> {
        Ok(meta::get(&self.conn, meta::FORMAT_VERSION)?.and_then(|v| v.parse().ok()))
    }

    /// Closes the project cleanly: checkpoint the WAL and leave a consistent file.
    ///
    /// §15 requires this of a clean shutdown. Consuming `self` makes "closed
    /// without checkpointing" something you have to do deliberately by dropping.
    pub fn close(self) -> Result<()> {
        if self.access == Access::ReadWrite {
            self.conn
                .pragma_update(None, "wal_checkpoint", "TRUNCATE")
                .or_else(|_| self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)"))?;
        }
        self.conn.close().map_err(|(_, e)| Error::Sqlite(e))?;
        Ok(())
    }
}

/// Pragmas applied to every connection to a project.
///
/// D3, provisional from S2: WAL, `synchronous=FULL`. The combination was chosen
/// for recovery granularity rather than throughput, which S2 found to be a
/// non-issue at 24/192.
fn apply_connection_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    conn.pragma_update(None, "cache_size", -32_000i64)?;
    Ok(())
}

/// Confirms the file is a VCW project this build can read.
///
/// Dispatches on `application_id` and `user_version`, never on the extension. S5
/// is the reason that matters: AUP3 and AUP4 share an `application_id` and only
/// `user_version` separates them, so a reader that trusts the name of the file is
/// already guessing.
fn identify(conn: &Connection, path: &Path) -> Result<()> {
    let app_id: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let app_id = app_id as u32;
    if app_id != APPLICATION_ID {
        return Err(Error::NotAProject {
            path: path.to_path_buf(),
            found: app_id,
        });
    }

    let version = migrate::current_version(conn)?;
    if version > SCHEMA_VERSION {
        return Err(Error::SchemaTooNew {
            path: path.to_path_buf(),
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(())
}

/// The SQLite library version this binary is linked against.
///
/// Bundled, so it is a property of the build rather than of the machine - which is
/// the point of bundling it, and worth printing when a project file misbehaves.
pub fn runtime_version() -> &'static str {
    rusqlite::version()
}

/// The checksum stored on every block, over the raw sample bytes.
///
/// CRC-32. Not a cryptographic hash: this detects storage corruption and truncated
/// writes, which is what §15's diagnostics need. It is deliberately cheap enough to
/// compute on the writer thread without competing with capture.
pub fn block_checksum(samples: &[u8]) -> u32 {
    crc32fast::hash(samples)
}

/// Bytes a block of `frames` frames occupies in one channel's blob.
pub fn block_bytes(format: StorageFormat, frames: u64) -> u64 {
    frames * format.bytes_per_sample() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_is_bundled_and_recent() {
        let v = runtime_version();
        let major: u32 = v
            .split('.')
            .next()
            .and_then(|s| s.parse().ok())
            .expect("version");
        assert!(major >= 3, "unexpected SQLite version {v}");
    }

    #[test]
    fn application_id_is_not_audacitys() {
        assert_ne!(APPLICATION_ID, 0x4155_4459);
        assert_eq!(&APPLICATION_ID.to_be_bytes(), b"VCW\0");
    }

    #[test]
    fn a_block_is_frames_times_the_stored_width() {
        assert_eq!(block_bytes(StorageFormat::Int24Packed, 1000), 3000);
        assert_eq!(block_bytes(StorageFormat::Int24Padded, 1000), 4000);
    }
}
