/*
 *  error.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What can go wrong talking to a metadata provider.
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

//! What can go wrong talking to a metadata provider.
//!
//! The variants are deliberately about *what the user should do*, not about which
//! library failed. A missing token, a rate limit that did not clear and a release
//! that does not exist are three different conversations to have with a person, so
//! they are three variants; which HTTP status or socket error produced them is
//! detail carried inside.
//!
//! [`Error::Offline`] is the one worth singling out. §40 requires the application
//! to stay fully usable with networking disabled, which means "disabled" has to be
//! an ordinary answer rather than an exception: every code path that would reach
//! the network returns this, and no caller is entitled to treat it as a bug.

use crate::release::ProviderId;

/// The result of a metadata operation.
pub type Result<T> = std::result::Result<T, Error>;

/// A metadata provider operation that did not produce an answer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Networking is switched off, so the request was never made.
    #[error("networking is disabled, so {provider} was not contacted")]
    Offline {
        /// The provider that would have been asked.
        provider: ProviderId,
    },

    /// The provider requires a credential and none is configured (§39).
    ///
    /// The message names the environment variable because that is the only place
    /// §39 allows a credential to come from, so naming it is the whole of the
    /// help a caller can give.
    #[error("{provider} needs a credential: set {variable}")]
    MissingCredential {
        /// The provider that asked for one.
        provider: ProviderId,
        /// The environment variable that would supply it.
        variable: &'static str,
    },

    /// The credential was present and the provider rejected it.
    #[error("{provider} rejected the credential")]
    Rejected {
        /// The provider that rejected it.
        provider: ProviderId,
    },

    /// The search carried no criterion this provider can use.
    ///
    /// Covers both an empty query and one whose every criterion the provider
    /// ignores, because the consequence is the same: asking anyway would return
    /// the provider's whole database, which is not a search.
    #[error("nothing to search: {provider} was given no criteria it can use")]
    NothingToSearch {
        /// The provider that was asked.
        provider: ProviderId,
    },

    /// The caller canceled the operation.
    #[error("canceled")]
    Canceled,

    /// The request did not complete within its time box.
    #[error("{provider} did not answer within {millis} ms")]
    Timeout {
        /// The provider that did not answer.
        provider: ProviderId,
        /// The time box that expired.
        millis: u64,
    },

    /// The provider rate-limited the request and retries did not clear it.
    #[error("{provider} is rate-limiting VCW and did not clear after {attempts} attempt(s)")]
    RateLimited {
        /// The provider doing the limiting.
        provider: ProviderId,
        /// How many attempts were made in total.
        attempts: u32,
    },

    /// The provider answered with an error status.
    #[error("{provider} returned HTTP {status}: {message}")]
    Http {
        /// The provider that answered.
        provider: ProviderId,
        /// The status it answered with.
        status: u16,
        /// Whatever it said about why, trimmed and truncated.
        message: String,
    },

    /// The provider answered 200 and refused the request in the body.
    ///
    /// AcoustID does this and it is not an edge case: an unreadable fingerprint, a
    /// missing parameter and an exhausted quota all come back as HTTP 200 with
    /// `{"status": "error"}` and a message. A caller that trusted the status code
    /// would read a refusal as "no match found", which is the one wrong answer -
    /// it would tell the user their record is unknown when the truth is that VCW
    /// asked the question badly.
    ///
    /// Not [`Self::Malformed`], because the response was perfectly readable, and
    /// not [`Self::Http`], because 200 is not what went wrong.
    #[error("{provider} could not process the request: {message}")]
    Refused {
        /// The provider that refused.
        provider: ProviderId,
        /// The provider's own explanation, trimmed and truncated.
        message: String,
    },

    /// The provider answered, but not with something VCW can read.
    #[error("{provider} sent a response VCW could not read: {detail}")]
    Malformed {
        /// The provider that answered.
        provider: ProviderId,
        /// What was wrong with it.
        detail: String,
    },

    /// The network itself could not be reached.
    #[error("{provider} could not be reached: {detail}")]
    Unreachable {
        /// The provider that could not be reached.
        provider: ProviderId,
        /// The transport's account of why.
        detail: String,
    },

    /// A download was not the kind of thing it was supposed to be.
    #[error("{url} is not usable artwork: {detail}")]
    NotArtwork {
        /// Where it came from.
        url: String,
        /// Why it was refused.
        detail: String,
    },

    /// The on-disk cache could not be read or written.
    ///
    /// Never fatal to an operation: a cache that cannot be used is skipped, and
    /// this is what the caller is told if it asked the cache a direct question.
    #[error("the metadata cache at {path} could not be used: {source}")]
    Cache {
        /// The directory involved.
        path: String,
        /// The underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    /// Whether waiting and asking again could plausibly give a different answer.
    ///
    /// Used to decide what a caller may offer the user: "try again" is sensible
    /// after a timeout and dishonest after a rejected token.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Timeout { .. } | Self::RateLimited { .. } | Self::Unreachable { .. } => true,
            Self::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_credential_is_not_worth_retrying_and_a_timeout_is() {
        assert!(
            Error::Timeout {
                provider: ProviderId::Discogs,
                millis: 10_000
            }
            .is_transient()
        );
        assert!(
            Error::Http {
                provider: ProviderId::MusicBrainz,
                status: 503,
                message: "service unavailable".into()
            }
            .is_transient()
        );
        assert!(
            !Error::Rejected {
                provider: ProviderId::Discogs
            }
            .is_transient()
        );
        assert!(
            !Error::Http {
                provider: ProviderId::Discogs,
                status: 404,
                message: "not found".into()
            }
            .is_transient(),
            "a release that does not exist will not start existing"
        );
        assert!(
            !Error::Offline {
                provider: ProviderId::Discogs
            }
            .is_transient(),
            "offline is a setting, not a failure to wait out"
        );
    }

    #[test]
    fn the_message_says_which_provider_and_what_to_do() {
        let offline = Error::Offline {
            provider: ProviderId::MusicBrainz,
        };
        assert_eq!(
            offline.to_string(),
            "networking is disabled, so MusicBrainz was not contacted"
        );
        let missing = Error::MissingCredential {
            provider: ProviderId::Discogs,
            variable: crate::credentials::DISCOGS_TOKEN_VAR,
        };
        assert_eq!(
            missing.to_string(),
            "Discogs needs a credential: set VCW_DISCOGS_TOKEN"
        );
    }
}
