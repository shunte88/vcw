/*
 *  cache.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Provider responses, kept so the same question is not asked twice (§40).
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

//! Provider responses, kept so the same question is not asked twice (§40).
//!
//! A cache here is politeness before it is speed. Both providers publish a rate
//! limit, both are free, and both are answering questions about records pressed
//! decades ago: asking twice in a minute is rude, and asking twice in a session is
//! just slow. §40 asks for caching and this is it.
//!
//! # Two implementations, for two different kinds of time
//!
//! [`Memory`] expires entries against a [`Clock`], which a test can control.
//! [`Disk`] cannot: its entries outlive the process, so a monotonic clock started
//! at launch means nothing to them. It uses the file's modification time instead,
//! which is wall-clock, survives a restart, and is what every other cache on the
//! machine uses.
//!
//! # A key is a URL, and a URL holds no credential
//!
//! Cache keys are built from the provider and the request URL, never from the
//! headers - so the Discogs token cannot reach the cache, because §39's rule put
//! it in an `Authorization` header rather than a query parameter. Two users
//! sharing a cache directory would share answers, which is correct, and would not
//! share credentials, which is the point.
//!
//! # Collisions cannot serve the wrong answer
//!
//! A disk entry is named after a CRC-32 of its key, which is 32 bits and therefore
//! collides. The key itself is stored in the file and checked on read, so a
//! collision is a miss - one wasted request - rather than a release served under
//! the wrong catalog number.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use crate::policy::Clock;
use crate::release::ProviderId;

/// How long a cached provider answer is used for.
///
/// A day. Releases barely change, but tracklists do get corrected, and a person
/// who has just fixed a typo on Discogs and comes back to VCW should see it
/// without being told to clear a cache they do not know exists.
pub const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// The file extension used for disk cache entries.
pub const ENTRY_EXTENSION: &str = "vcwcache";

/// The magic first line of a disk cache entry.
const MAGIC: &str = "vcw-cache-1";

/// A cache key for one request.
///
/// Built from the provider and the URL only, deliberately: see the module docs.
#[must_use]
pub fn key(provider: ProviderId, url: &str) -> String {
    format!("{}:{}", provider.as_str(), url)
}

/// Somewhere to keep provider answers.
pub trait Cache: std::fmt::Debug + Send + Sync {
    /// The cached body for a key, if there is one and it has not expired.
    fn get(&self, key: &str) -> Option<Vec<u8>>;

    /// Stores a body. Failures are not the caller's problem and are not reported.
    fn put(&self, key: &str, value: &[u8]);

    /// A short name for logs and diagnostics.
    fn name(&self) -> &'static str;
}

/// The cache that remembers nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCache;

impl Cache for NoCache {
    fn get(&self, _key: &str) -> Option<Vec<u8>> {
        None
    }

    fn put(&self, _key: &str, _value: &[u8]) {}

    fn name(&self) -> &'static str {
        "none"
    }
}

/// An in-process cache, expiring against an injected clock.
#[derive(Debug)]
pub struct Memory {
    clock: Arc<dyn Clock>,
    ttl: Duration,
    entries: RwLock<HashMap<String, (u64, Vec<u8>)>>,
}

impl Memory {
    /// A memory cache with the default TTL.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self::with_ttl(clock, DEFAULT_TTL)
    }

    /// A memory cache with a chosen TTL. Zero means every entry is stale at once.
    #[must_use]
    pub fn with_ttl(clock: Arc<dyn Clock>, ttl: Duration) -> Self {
        Self {
            clock,
            ttl,
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// How many entries are held, expired ones included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }

    /// Whether anything is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drops expired entries and returns how many went.
    pub fn purge(&self) -> usize {
        let now = self.clock.now_millis();
        let ttl = millis(self.ttl);
        let mut entries = match self.entries.write() {
            Ok(entries) => entries,
            Err(_) => return 0,
        };
        let before = entries.len();
        entries.retain(|_, (stored, _)| now.saturating_sub(*stored) < ttl);
        before - entries.len()
    }
}

impl Cache for Memory {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        let now = self.clock.now_millis();
        let entries = self.entries.read().ok()?;
        let (stored, body) = entries.get(key)?;
        if now.saturating_sub(*stored) < millis(self.ttl) {
            Some(body.clone())
        } else {
            None
        }
    }

    fn put(&self, key: &str, value: &[u8]) {
        let now = self.clock.now_millis();
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(key.to_owned(), (now, value.to_vec()));
        }
    }

    fn name(&self) -> &'static str {
        "memory"
    }
}

/// A cache in a directory, surviving restarts.
///
/// Not in the project file. A project is a recording and its edits; a provider's
/// answer is neither, it is a copy of someone else's data that may be stale, and
/// §39 keeps provider configuration out of projects for the same reason.
#[derive(Debug, Clone)]
pub struct Disk {
    dir: PathBuf,
    ttl: Duration,
}

impl Disk {
    /// A disk cache in `dir`, with the default TTL. The directory is created lazily.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_ttl(dir, DEFAULT_TTL)
    }

    /// A disk cache with a chosen TTL.
    #[must_use]
    pub fn with_ttl(dir: impl Into<PathBuf>, ttl: Duration) -> Self {
        Self {
            dir: dir.into(),
            ttl,
        }
    }

    /// Where the entries live.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.dir
    }

    /// The path an entry would have.
    #[must_use]
    pub fn path_for(&self, key: &str) -> PathBuf {
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(key.as_bytes());
        self.dir
            .join(format!("{:08x}.{ENTRY_EXTENSION}", hasher.finalize()))
    }

    /// Reads an entry, reporting why it could not be used.
    ///
    /// `Ok(None)` covers every ordinary miss: absent, expired, or a CRC collision
    /// with a different key. `Err` is for a directory that cannot be read at all,
    /// which is worth telling someone about.
    pub fn load(&self, key: &str) -> Result<Option<Vec<u8>>, io::Error> {
        let path = self.path_for(key);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let age = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .unwrap_or(Duration::ZERO);
        if age >= self.ttl {
            return Ok(None);
        }
        Ok(parse(&bytes).and_then(|(stored_key, body)| (stored_key == key).then_some(body)))
    }

    /// Writes an entry, reporting failure.
    ///
    /// Written to a temporary file and renamed, so a cache read never sees half an
    /// entry and a killed process leaves at most a stray `.part`.
    pub fn store(&self, key: &str, value: &[u8]) -> Result<(), io::Error> {
        fs::create_dir_all(&self.dir)?;
        let path = self.path_for(key);
        let part = path.with_extension(format!("{ENTRY_EXTENSION}.part"));
        let mut bytes = format!("{MAGIC}\n{key}\n").into_bytes();
        bytes.extend_from_slice(value);
        fs::write(&part, &bytes)?;
        fs::rename(&part, &path)
    }

    /// Deletes expired entries and returns how many went.
    ///
    /// Nothing calls this on a timer. It is what a "clear cached metadata" action
    /// in the settings does, and what a diagnostic bundle reports the size of.
    pub fn purge(&self) -> Result<usize, io::Error> {
        let mut removed = 0;
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some(ENTRY_EXTENSION) {
                continue;
            }
            let expired = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age >= self.ttl);
            if expired && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// How many bytes the cache occupies, entries only.
    pub fn size_bytes(&self) -> Result<u64, io::Error> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        Ok(entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some(ENTRY_EXTENSION))
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum())
    }
}

impl Cache for Disk {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.load(key).ok().flatten()
    }

    fn put(&self, key: &str, value: &[u8]) {
        // A cache that cannot be written is a cache miss next time, which is a
        // performance problem and not a correctness one. The operation the user
        // asked for has already succeeded by this point.
        let _ = self.store(key, value);
    }

    fn name(&self) -> &'static str {
        "disk"
    }
}

/// Splits an entry into its key and its body, or `None` if it is not an entry.
fn parse(bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    let first = bytes.iter().position(|b| *b == b'\n')?;
    if &bytes[..first] != MAGIC.as_bytes() {
        return None;
    }
    let rest = &bytes[first + 1..];
    let second = rest.iter().position(|b| *b == b'\n')?;
    let key = String::from_utf8(rest[..second].to_vec()).ok()?;
    Some((key, rest[second + 1..].to_vec()))
}

/// A `Duration` as whole milliseconds, saturating.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::TestClock;

    fn memory(ttl: Duration) -> (Arc<TestClock>, Memory) {
        let clock = Arc::new(TestClock::new());
        let cache = Memory::with_ttl(clock.clone(), ttl);
        (clock, cache)
    }

    #[test]
    fn a_key_is_the_provider_and_the_url() {
        assert_eq!(
            key(ProviderId::Discogs, "https://api.discogs.com/releases/1"),
            "discogs:https://api.discogs.com/releases/1"
        );
        assert_ne!(
            key(ProviderId::Discogs, "https://x/1"),
            key(ProviderId::MusicBrainz, "https://x/1"),
            "two providers answering the same URL are two answers"
        );
    }

    #[test]
    fn a_memory_entry_expires_on_the_clock_it_was_given() {
        let (clock, cache) = memory(Duration::from_secs(60));
        cache.put("k", b"body");
        assert_eq!(cache.get("k").as_deref(), Some(&b"body"[..]));
        clock.advance(Duration::from_secs(59));
        assert!(cache.get("k").is_some(), "still fresh");
        clock.advance(Duration::from_secs(1));
        assert_eq!(cache.get("k"), None, "stale at exactly the TTL");
        assert_eq!(cache.len(), 1, "stale but still occupying space");
        assert_eq!(cache.purge(), 1);
        assert!(cache.is_empty());
    }

    #[test]
    fn a_zero_ttl_cache_never_hits() {
        let (_clock, cache) = memory(Duration::ZERO);
        cache.put("k", b"body");
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn nothing_is_remembered_by_nocache() {
        let cache = NoCache;
        cache.put("k", b"body");
        assert_eq!(cache.get("k"), None);
        assert_eq!(cache.name(), "none");
    }

    #[test]
    fn a_disk_entry_survives_being_forgotten_in_memory() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let cache = Disk::new(dir.path());
        cache.put("discogs:https://api.discogs.com/releases/1", b"{\"id\":1}");
        let reopened = Disk::new(dir.path());
        assert_eq!(
            reopened
                .get("discogs:https://api.discogs.com/releases/1")
                .as_deref(),
            Some(&b"{\"id\":1}"[..])
        );
        assert!(reopened.size_bytes().expect("a size") > 0);
    }

    #[test]
    fn a_crc_collision_is_a_miss_and_not_a_wrong_answer() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let cache = Disk::new(dir.path());
        cache
            .store("the-real-key", b"the real body")
            .expect("stored");
        // Forge a collision: write the same file with a different key inside it.
        let path = cache.path_for("the-real-key");
        let mut forged = format!("{MAGIC}\nsomeone-elses-key\n").into_bytes();
        forged.extend_from_slice(b"someone else's body");
        fs::write(&path, forged).expect("written");
        assert_eq!(
            cache.get("the-real-key"),
            None,
            "the key in the file did not match, so it is a miss"
        );
    }

    #[test]
    fn a_file_that_is_not_an_entry_is_a_miss() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let cache = Disk::new(dir.path());
        let path = cache.path_for("k");
        fs::create_dir_all(dir.path()).expect("the directory");
        fs::write(&path, b"not a cache entry at all").expect("written");
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn an_expired_disk_entry_is_a_miss_and_can_be_purged() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let cache = Disk::with_ttl(dir.path(), Duration::ZERO);
        cache.store("k", b"body").expect("stored");
        assert_eq!(cache.get("k"), None, "a zero TTL expires immediately");
        assert_eq!(cache.purge().expect("purged"), 1);
        assert_eq!(cache.size_bytes().expect("a size"), 0);
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let cache = Disk::new(dir.path().join("not-created-yet"));
        assert_eq!(cache.load("k").expect("no error"), None);
        assert_eq!(cache.purge().expect("no error"), 0);
        assert_eq!(cache.size_bytes().expect("no error"), 0);
    }

    #[test]
    fn a_disk_cache_that_cannot_be_written_is_silent_through_the_trait() {
        // A file where the directory should be: portable, and the same situation
        // as a permissions failure from the cache's point of view.
        let dir = tempfile::tempdir().expect("a temporary directory");
        let blocked = dir.path().join("in-the-way");
        fs::write(&blocked, b"not a directory").expect("written");
        let cache = Disk::new(&blocked);
        assert!(
            cache.store("k", b"body").is_err(),
            "the inherent call says so"
        );
        cache.put("k", b"body");
        assert_eq!(cache.get("k"), None, "and the trait call simply misses");
    }
}
