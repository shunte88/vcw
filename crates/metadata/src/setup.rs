/*
 *  setup.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Building the provider set a run should ask, in one place (§28, §39, §40).
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

//! Building the provider set a run should ask, in one place (§28, §39, §40).
//!
//! Every caller that wants to ask Discogs or MusicBrainz something needs the
//! same six decisions made: which transport, which rate limit, how long to
//! wait, what to put in the user agent, where to cache, and which genre table
//! to fold through. None of those is a question about the *caller* - they are
//! this crate's own wiring - and a limit or a user agent written down in two
//! places is one that will eventually disagree.
//!
//! So the CLI and the Tauri shell both construct their providers here. What
//! they legitimately differ on is the timeout, which is why it is a field: a
//! person who typed a command at a terminal is waiting on purpose, and a dialog
//! in a window is not.
//!
//! # §40 is structural here
//!
//! [`Setup::online`] false means [`net::Offline`], and so does a build with the
//! `net` feature turned off, whatever the field says. That is the difference
//! between a switch and a guarantee: the feature removes the code that could
//! open a socket, and this module has no `cfg` branch that can smuggle it back.
//!
//! # Credentials are not held here
//!
//! [`Setup`] carries no token. They are read from the environment at the call
//! site and passed in, so nothing long-lived in a UI is holding a secret and
//! [`Setup`] stays `Clone` and printable. See [`crate::credentials`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::cache::Disk;
use crate::client::{Client, default_limiter};
use crate::credentials::Credentials;
use crate::discogs::Discogs;
use crate::genres::Genres;
use crate::musicbrainz::MusicBrainz;
use crate::net::{self, Transport, user_agent};
use crate::policy::Retry;
use crate::provider::Provider;
use crate::release::ProviderId;

/// How long a caller that does not say waits for a provider.
///
/// Ten seconds: long enough for a cold MusicBrainz search behind a rate limit,
/// short enough that a person who pressed a key has not concluded the
/// application is broken.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Which providers to build, and how.
///
/// The defaults are §40's: offline, no cache on disk, both services enabled so
/// that turning the network on is the only decision left to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// Whether a lookup may go out at all (§40).
    pub online: bool,
    /// Whether to include MusicBrainz (§27).
    pub musicbrainz: bool,
    /// Whether to include Discogs (§26).
    pub discogs: bool,
    /// Where to cache responses, or `None` to keep them in the process.
    pub cache: Option<PathBuf>,
    /// How long to wait for one request.
    pub timeout: Duration,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            online: false,
            musicbrainz: true,
            discogs: true,
            cache: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl Setup {
    /// The defaults: offline, both providers, no disk cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allows the network (§40).
    #[must_use]
    pub const fn online(mut self, online: bool) -> Self {
        self.online = online;
        self
    }

    /// Caches responses under a directory.
    #[must_use]
    pub fn with_cache(mut self, directory: impl Into<PathBuf>) -> Self {
        self.cache = Some(directory.into());
        self
    }

    /// Waits this long for one request instead of [`DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Asks only the named providers.
    #[must_use]
    pub const fn only(mut self, musicbrainz: bool, discogs: bool) -> Self {
        self.musicbrainz = musicbrainz;
        self.discogs = discogs;
        self
    }

    /// The providers this setup asks for, in the order to ask them.
    ///
    /// MusicBrainz first, because it is the one that needs no credential: a
    /// search that comes back from it costs nothing to have tried, and a
    /// Discogs search without a token is a refusal.
    ///
    /// An empty vector is a legitimate result - both services turned off is a
    /// person saying they do not want lookups - and
    /// [`search_all`](crate::provider::search_all) answers it with
    /// [`crate::Error::NothingToSearch`] rather than an empty candidate list.
    #[must_use]
    pub fn providers(&self, credentials: &Credentials) -> Vec<Box<dyn Provider>> {
        let genres = Genres::builtin();
        let mut providers: Vec<Box<dyn Provider>> = Vec::new();
        if self.musicbrainz {
            providers.push(Box::new(
                MusicBrainz::new(self.transport())
                    .with_client(self.client(ProviderId::MusicBrainz, credentials))
                    .with_genres(genres.clone()),
            ));
        }
        if self.discogs {
            providers.push(Box::new(
                Discogs::new(self.transport())
                    .with_client(self.client(ProviderId::Discogs, credentials))
                    .with_genres(genres)
                    .with_token(credentials.discogs().cloned()),
            ));
        }
        providers
    }

    /// The transport: the real one, or the one that refuses.
    ///
    /// The `cfg` is the whole of §40's strongest form. With the `net` feature
    /// off there is no `Agent` to construct, so `online: true` is honoured as
    /// far as it can be - which is not at all - and the build contains no code
    /// that can reach a network. A caller that needs to know asks
    /// [`Provider::is_offline`].
    fn transport(&self) -> Arc<dyn Transport> {
        #[cfg(feature = "net")]
        if self.online {
            return Arc::new(crate::agent::Agent::new());
        }
        Arc::new(net::Offline)
    }

    /// A client for one provider, with that provider's own published rate.
    fn client(&self, provider: ProviderId, credentials: &Credentials) -> Client {
        let mut client = Client::new(provider, self.transport())
            // From the library rather than restated here: a limit written down
            // twice is a limit that will disagree.
            .with_limiter(default_limiter(provider))
            .with_retry(Retry::conservative())
            .with_timeout(self.timeout)
            .with_user_agent(user_agent(credentials.contact()));
        if let Some(directory) = &self.cache {
            client = client.with_cache(Arc::new(Disk::new(directory)));
        }
        client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_offline_with_both_providers() {
        // §40's default, and the one worth asserting: a caller that forgets to
        // set anything does not reach the network.
        let setup = Setup::new();
        assert!(!setup.online);
        let providers = setup.providers(&Credentials::none());
        assert_eq!(providers.len(), 2);
        assert!(
            providers.iter().all(|p| p.is_offline()),
            "a default setup built a provider that thinks it can reach something"
        );
    }

    #[test]
    fn musicbrainz_is_asked_first() {
        // Not cosmetic: MusicBrainz needs no credential, so asking it first
        // means a machine with no Discogs token still gets an answer before
        // anything can refuse.
        let providers = Setup::new().providers(&Credentials::none());
        assert_eq!(providers[0].id(), ProviderId::MusicBrainz);
        assert_eq!(providers[1].id(), ProviderId::Discogs);
    }

    #[test]
    fn turning_a_provider_off_leaves_it_out() {
        let only_discogs = Setup::new()
            .only(false, true)
            .providers(&Credentials::none());
        assert_eq!(only_discogs.len(), 1);
        assert_eq!(only_discogs[0].id(), ProviderId::Discogs);

        // And both off is an empty set rather than a panic or a silent default,
        // because "do not look anything up" is a setting a person may hold.
        assert!(
            Setup::new()
                .only(false, false)
                .providers(&Credentials::none())
                .is_empty()
        );
    }

    #[test]
    fn a_setup_carries_no_credential() {
        // The reason `providers` takes them rather than holding them: a `Setup`
        // is long-lived in a UI, and `Debug` on it must be safe to log (§39).
        let text = format!("{:?}", Setup::new().with_cache("/tmp/vcw").online(true));
        assert!(text.contains("online: true"), "{text}");
        assert!(!text.to_lowercase().contains("token"), "{text}");
    }
}
