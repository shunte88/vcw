/*
 *  client.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  One request, with a time box, a rate limit, a cache and a way out (§40).
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

//! One request, with a time box, a rate limit, a cache and a way out (§40).
//!
//! [`Client`] is where §40's five requirements meet: it caches, it rate-limits, it
//! retries conservatively, it hands the transport a timeout, and it checks a
//! [`Cancel`] token often enough that a user who closes a dialog is not waiting on
//! a provider. A provider module holds one and never talks to a transport itself.
//!
//! # Order of operations, and why it is that order
//!
//! Cache, then rate limit, then request. A cached answer costs no slot: the limit
//! exists to be polite to a service, and reading something already downloaded does
//! not involve the service at all. Getting this backwards would make a warm cache
//! *slower* than a cold one.
//!
//! # Cancellation without a runtime
//!
//! VCW has no async runtime and this crate does not introduce one. Cancellation is
//! therefore cooperative and honest about its granularity: the token is checked
//! before each attempt and every [`CANCEL_SLICE`] of any backoff or rate-limit
//! wait, so waiting is interruptible immediately, while a request already on the
//! wire is bounded by its timeout rather than by the token. That is the true
//! behaviour and the reason the timeout set by [`Client::with_timeout`] defaults to
//! something a person will sit through.
//!
//! # No network on the audio thread
//!
//! §40 forbids it and nothing here enforces it, because nothing here can: a
//! `Client` is `Send + Sync` and an engine that called one from a device callback
//! would be as wrong as one that called SQLite from it. What keeps it true is
//! ADR-0003's layering - `vcw-audio` does not depend on this crate, and cannot.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use crate::cache::{self, Cache, NoCache};
use crate::error::{Error, Result};
use crate::net::{
    DEFAULT_TIMEOUT, Header, Request, Response, Transport, TransportError, user_agent,
};
use crate::policy::{Attempt, Clock, Limiter, Retry, SystemClock};
use crate::release::ProviderId;

/// The longest a wait goes uninterrupted before the cancel token is checked.
pub const CANCEL_SLICE: Duration = Duration::from_millis(50);

/// A token a caller keeps in order to stop waiting.
///
/// Clone it: every clone refers to the same flag, so a UI can cancel a search from
/// a different thread than the one waiting on it. Cancelling is one-way, because a
/// user who changed their mind twice is better served by starting a new search than
/// by a token that un-cancels under the worker.
#[derive(Debug, Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
}

impl Cancel {
    /// A token that has not been cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels every operation holding this token or a clone of it.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Whether it has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// `Err(Error::Cancelled)` if it has been cancelled, for the `?` operator.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// What a client has done, for diagnostics and for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Requests actually handed to the transport, retries included.
    pub requests: u64,
    /// Answers served from the cache.
    pub cache_hits: u64,
    /// Bodies written to the cache.
    pub cache_writes: u64,
    /// Attempts made after a first attempt failed.
    pub retries: u64,
    /// Total time spent waiting on the rate limiter, in milliseconds.
    pub limited_millis: u64,
    /// Total time spent in retry backoff, in milliseconds.
    pub backoff_millis: u64,
}

#[derive(Debug, Default)]
struct Counters {
    requests: AtomicU64,
    cache_hits: AtomicU64,
    cache_writes: AtomicU64,
    retries: AtomicU64,
    limited_millis: AtomicU64,
    backoff_millis: AtomicU64,
}

impl Counters {
    fn snapshot(&self) -> Stats {
        Stats {
            requests: self.requests.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_writes: self.cache_writes.load(Ordering::Relaxed),
            retries: self.retries.load(Ordering::Relaxed),
            limited_millis: self.limited_millis.load(Ordering::Relaxed),
            backoff_millis: self.backoff_millis.load(Ordering::Relaxed),
        }
    }
}

/// The request path for one provider.
#[derive(Debug)]
pub struct Client {
    provider: ProviderId,
    transport: Arc<dyn Transport>,
    clock: Arc<dyn Clock>,
    cache: Arc<dyn Cache>,
    limiter: Limiter,
    retry: Retry,
    timeout: Duration,
    user_agent: String,
    counters: Counters,
}

impl Client {
    /// A client for `provider` over `transport`.
    ///
    /// The defaults are the cautious ones: the provider's published rate limit,
    /// [`Retry::conservative`], the system clock, no cache, and the user agent
    /// [`user_agent`] builds with no contact.
    #[must_use]
    pub fn new(provider: ProviderId, transport: Arc<dyn Transport>) -> Self {
        Self {
            provider,
            transport,
            clock: Arc::new(SystemClock::new()),
            cache: Arc::new(NoCache),
            limiter: default_limiter(provider),
            retry: Retry::conservative(),
            timeout: DEFAULT_TIMEOUT,
            user_agent: user_agent(None),
            counters: Counters::default(),
        }
    }

    /// Replaces the clock. Test support, and the reason no test here sleeps.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Replaces the cache.
    #[must_use]
    pub fn with_cache(mut self, cache: Arc<dyn Cache>) -> Self {
        self.cache = cache;
        self
    }

    /// Replaces the rate limiter.
    #[must_use]
    pub fn with_limiter(mut self, limiter: Limiter) -> Self {
        self.limiter = limiter;
        self
    }

    /// Replaces the retry policy.
    #[must_use]
    pub fn with_retry(mut self, retry: Retry) -> Self {
        self.retry = retry;
        self
    }

    /// Replaces the per-request time box.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the user agent, which is how §40's identification requirement is met.
    #[must_use]
    pub fn with_user_agent(mut self, agent: impl Into<String>) -> Self {
        self.user_agent = agent.into();
        self
    }

    /// Which provider this client speaks to.
    #[must_use]
    pub const fn provider(&self) -> ProviderId {
        self.provider
    }

    /// The transport's name, for diagnostics.
    #[must_use]
    pub fn transport_name(&self) -> &'static str {
        self.transport.name()
    }

    /// Whether this client can reach anything at all.
    ///
    /// A caller uses it to grey out a "search" button rather than to offer one that
    /// is certain to fail.
    #[must_use]
    pub fn is_offline(&self) -> bool {
        self.transport.name() == "offline"
    }

    /// What this client has done.
    #[must_use]
    pub fn stats(&self) -> Stats {
        self.counters.snapshot()
    }

    /// Fetches a URL, through the cache, and returns the body.
    ///
    /// `extra` carries anything provider-specific, which in practice means the
    /// `Authorization` header. It is *not* part of the cache key: see
    /// [`crate::cache`] for why that is deliberate rather than an oversight.
    pub fn body(&self, url: &str, extra: &[Header], cancel: &Cancel) -> Result<Vec<u8>> {
        cancel.check()?;
        let key = cache::key(self.provider, url);
        if let Some(cached) = self.cache.get(&key) {
            self.counters.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(cached);
        }

        let mut request = Request::get(url)
            .with_header("User-Agent", self.user_agent.clone())
            .with_timeout(self.timeout);
        request.headers.extend_from_slice(extra);

        let response = self.send(&request, cancel)?;
        self.cache.put(&key, &response.body);
        self.counters.cache_writes.fetch_add(1, Ordering::Relaxed);
        Ok(response.body)
    }

    /// Fetches a URL, ignoring the cache in both directions.
    ///
    /// For a body that has no business being kept: artwork goes through its own
    /// path, and a "refresh from provider" action should mean it.
    pub fn uncached(&self, url: &str, extra: &[Header], cancel: &Cancel) -> Result<Vec<u8>> {
        cancel.check()?;
        let mut request = Request::get(url)
            .with_header("User-Agent", self.user_agent.clone())
            .with_timeout(self.timeout);
        request.headers.extend_from_slice(extra);
        Ok(self.send(&request, cancel)?.body)
    }

    /// Sends a form POST and hands back the body.
    ///
    /// Uncached in both directions, unlike [`Self::body`]. A cache key is the URL,
    /// and every AcoustID lookup is the *same* URL - `/v2/lookup` - so what
    /// distinguishes two of them is the form, and the form carries the API key
    /// (§39), which must not end up in a key written to disk. A cached POST
    /// therefore needs a tag from the caller naming what it asked; worth adding
    /// when a repeated lookup costs more than the plumbing, and not before.
    ///
    /// The rate limiter, the retry policy and the counters all apply: this goes
    /// through the same send path as a GET.
    pub fn post_form(
        &self,
        url: &str,
        form: &str,
        extra: &[Header],
        cancel: &Cancel,
    ) -> Result<Vec<u8>> {
        cancel.check()?;
        let mut request = Request::post_form(url, form)
            .with_header("User-Agent", self.user_agent.clone())
            .with_timeout(self.timeout);
        request.headers.extend_from_slice(extra);
        Ok(self.send(&request, cancel)?.body)
    }

    /// Makes a request, waiting for a slot and retrying what is worth retrying.
    fn send(&self, request: &Request, cancel: &Cancel) -> Result<Response> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            cancel.check()?;
            self.wait_for_a_slot(cancel)?;

            self.counters.requests.fetch_add(1, Ordering::Relaxed);
            let outcome = self.transport.send(request);

            let (again, error) = match &outcome {
                Ok(response) if response.is_success() => return Ok(response.clone()),
                Ok(response) => (
                    self.retry.after(attempt, &Attempt::Answered(response)),
                    self.classify(response, attempt),
                ),
                Err(transport) => (
                    self.retry.after(attempt, &Attempt::Failed(transport)),
                    self.explain(transport),
                ),
            };

            match again {
                Some(backoff) => {
                    self.counters.retries.fetch_add(1, Ordering::Relaxed);
                    self.counters
                        .backoff_millis
                        .fetch_add(as_millis(backoff), Ordering::Relaxed);
                    self.nap(backoff, cancel)?;
                }
                None => return Err(error),
            }
        }
    }

    /// Waits for the rate limiter to allow a request.
    fn wait_for_a_slot(&self, cancel: &Cancel) -> Result<()> {
        let now = self.clock.now_millis();
        let delay = self.limiter.reserve(now);
        if delay.is_zero() {
            return Ok(());
        }
        self.counters
            .limited_millis
            .fetch_add(as_millis(delay), Ordering::Relaxed);
        match self.nap(delay, cancel) {
            Ok(()) => Ok(()),
            Err(error) => {
                // Cancelled while queueing: hand the slot back so the next search
                // does not pay for a request that never happened.
                self.limiter
                    .release(self.limiter.reserved_until(now, delay));
                Err(error)
            }
        }
    }

    /// Sleeps, in slices, so cancellation does not have to wait for the whole wait.
    fn nap(&self, duration: Duration, cancel: &Cancel) -> Result<()> {
        let mut left = duration;
        while !left.is_zero() {
            cancel.check()?;
            let slice = left.min(CANCEL_SLICE);
            self.clock.sleep(slice);
            left -= slice;
        }
        cancel.check()
    }

    /// Turns an error status into the error a person should see.
    fn classify(&self, response: &Response, attempts: u32) -> Error {
        let provider = self.provider;
        if response.is_unauthorised() {
            return Error::Rejected { provider };
        }
        if response.is_rate_limited() {
            return Error::RateLimited { provider, attempts };
        }
        Error::Http {
            provider,
            status: response.status,
            message: response.preview(200),
        }
    }

    /// Turns a transport failure into the error a person should see.
    fn explain(&self, error: &TransportError) -> Error {
        let provider = self.provider;
        match error {
            TransportError::Disabled => Error::Offline { provider },
            TransportError::Timeout(millis) => Error::Timeout {
                provider,
                millis: *millis,
            },
            TransportError::Unreachable(detail) => Error::Unreachable {
                provider,
                detail: detail.clone(),
            },
            TransportError::Protocol(detail) => Error::Malformed {
                provider,
                detail: detail.clone(),
            },
        }
    }
}

/// The published rate limit for a provider (§40).
///
/// Discogs allows 60 requests a minute to an authenticated client and 25 to an
/// anonymous one; the authenticated figure is used because a client without a token
/// cannot search at all. MusicBrainz asks for one request a second, sustained, and
/// says so in its own documentation rather than enforcing it silently.
#[must_use]
pub fn default_limiter(provider: ProviderId) -> Limiter {
    match provider {
        ProviderId::Discogs => Limiter::per_minute(60),
        ProviderId::MusicBrainz => Limiter::every(Duration::from_secs(1)),
        ProviderId::AcoustId => Limiter::every(Duration::from_secs(1)),
    }
}

/// A `Duration` as whole milliseconds, saturating.
fn as_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::Memory;
    use crate::fixtures::Recorded;
    use crate::net::Offline;
    use crate::policy::TestClock;

    const URL: &str = "https://api.discogs.com/releases/1";

    fn client(transport: Arc<dyn Transport>) -> (Arc<TestClock>, Client) {
        let clock = Arc::new(TestClock::new());
        let client = Client::new(ProviderId::Discogs, transport)
            .with_clock(clock.clone())
            .with_limiter(Limiter::unlimited());
        (clock, client)
    }

    #[test]
    fn a_body_comes_back_and_the_user_agent_went_out() {
        let transport = Arc::new(Recorded::new().json(URL, "{\"id\":1}"));
        let (_clock, client) = client(transport.clone());
        let body = client.body(URL, &[], &Cancel::new()).expect("a body");
        assert_eq!(body, b"{\"id\":1}");
        let sent = transport.requests().remove(0);
        assert!(
            sent.header("user-agent")
                .is_some_and(|ua| ua.starts_with("VCW/")),
            "every provider requires identification"
        );
        assert_eq!(sent.timeout, DEFAULT_TIMEOUT, "and a time box");
        assert_eq!(client.stats().requests, 1);
    }

    #[test]
    fn a_cached_answer_costs_no_request_and_no_rate_limit_slot() {
        let transport = Arc::new(Recorded::new().json(URL, "{\"id\":1}"));
        let clock = Arc::new(TestClock::new());
        let cache = Arc::new(Memory::new(clock.clone()));
        let client = Client::new(ProviderId::Discogs, transport.clone())
            .with_clock(clock.clone())
            .with_cache(cache)
            .with_limiter(Limiter::per_minute(60));
        let cancel = Cancel::new();

        assert_eq!(
            client.body(URL, &[], &cancel).expect("first"),
            b"{\"id\":1}"
        );
        assert_eq!(
            client.body(URL, &[], &cancel).expect("second"),
            b"{\"id\":1}"
        );
        assert_eq!(
            transport.calls(),
            1,
            "the second answer came from the cache"
        );
        let stats = client.stats();
        assert_eq!(
            (stats.requests, stats.cache_hits, stats.cache_writes),
            (1, 1, 1)
        );
        assert_eq!(
            clock.slept_total(),
            0,
            "a cache hit does not wait for a rate limit slot"
        );
    }

    #[test]
    fn the_credential_is_sent_and_is_not_part_of_the_cache_key() {
        let transport = Arc::new(Recorded::new().json(URL, "{\"id\":1}"));
        let clock = Arc::new(TestClock::new());
        let cache = Arc::new(Memory::new(clock.clone()));
        let client = Client::new(ProviderId::Discogs, transport.clone())
            .with_clock(clock)
            .with_cache(cache)
            .with_limiter(Limiter::unlimited());
        let cancel = Cancel::new();
        let auth = [Header::new("Authorization", "Discogs token=sekrit")];

        client.body(URL, &auth, &cancel).expect("first");
        client.body(URL, &[], &cancel).expect("second");
        assert_eq!(
            transport.calls(),
            1,
            "the same URL is the same answer whoever asked for it"
        );
        assert_eq!(
            transport.requests()[0].header("authorization"),
            Some("Discogs token=sekrit"),
            "and the credential did go out"
        );
    }

    #[test]
    fn requests_are_spaced_out_by_the_published_rate() {
        let transport = Arc::new(Recorded::new().json_matching("/releases/", "{}"));
        let clock = Arc::new(TestClock::new());
        let client = Client::new(ProviderId::MusicBrainz, transport)
            .with_clock(clock.clone())
            .with_limiter(Limiter::every(Duration::from_secs(1)));
        let cancel = Cancel::new();
        for id in 1..=3 {
            client
                .body(&format!("https://x/releases/{id}"), &[], &cancel)
                .expect("a body");
        }
        assert_eq!(
            clock.slept_total(),
            2_000,
            "three requests at one a second means two seconds of waiting"
        );
        assert_eq!(client.stats().limited_millis, 2_000);
    }

    #[test]
    fn a_rate_limited_answer_is_retried_and_then_succeeds() {
        let transport = Arc::new(
            Recorded::new()
                .answering(
                    URL,
                    Response::status(429, "slow down").with_header("Retry-After", "2"),
                )
                .answering(URL, Response::ok("{\"id\":1}")),
        );
        let (clock, client) = client(transport.clone());
        let body = client.body(URL, &[], &Cancel::new()).expect("a body");
        assert_eq!(body, b"{\"id\":1}");
        assert_eq!(transport.calls(), 2);
        assert_eq!(
            clock.slept_total(),
            2_000,
            "it waited exactly as long as the provider asked"
        );
        let stats = client.stats();
        assert_eq!(
            (stats.requests, stats.retries, stats.backoff_millis),
            (2, 1, 2_000)
        );
    }

    #[test]
    fn a_rate_limit_that_never_clears_is_reported_as_one() {
        let transport = Arc::new(Recorded::new().answering(URL, Response::status(429, "no")));
        let (_clock, client) = client(transport.clone());
        let error = client.body(URL, &[], &Cancel::new()).expect_err("no body");
        assert!(
            matches!(
                error,
                Error::RateLimited {
                    provider: ProviderId::Discogs,
                    attempts: 3
                }
            ),
            "{error:?}"
        );
        assert_eq!(transport.calls(), 3, "three attempts, as configured");
        assert!(error.is_transient());
    }

    #[test]
    fn a_rejected_credential_is_not_retried() {
        let transport = Arc::new(Recorded::new().answering(URL, Response::status(401, "nope")));
        let (_clock, client) = client(transport.clone());
        let error = client.body(URL, &[], &Cancel::new()).expect_err("no body");
        assert!(matches!(error, Error::Rejected { .. }), "{error:?}");
        assert_eq!(transport.calls(), 1, "asking again would be pointless");
    }

    #[test]
    fn a_missing_release_is_reported_with_its_status_and_not_retried() {
        let transport = Arc::new(
            Recorded::new().answering(URL, Response::status(404, "  <html>not found</html>  ")),
        );
        let (_clock, client) = client(transport.clone());
        let error = client.body(URL, &[], &Cancel::new()).expect_err("no body");
        match error {
            Error::Http {
                status, message, ..
            } => {
                assert_eq!(status, 404);
                assert_eq!(message, "<html>not found</html>");
            }
            other => panic!("wrong error: {other:?}"),
        }
        assert_eq!(transport.calls(), 1);
    }

    #[test]
    fn the_offline_transport_produces_an_offline_error_and_no_retries() {
        let client = Client::new(ProviderId::MusicBrainz, Arc::new(Offline))
            .with_clock(Arc::new(TestClock::new()));
        assert!(client.is_offline());
        let error = client.body(URL, &[], &Cancel::new()).expect_err("no body");
        assert!(
            matches!(
                error,
                Error::Offline {
                    provider: ProviderId::MusicBrainz
                }
            ),
            "{error:?}"
        );
        assert_eq!(client.stats().retries, 0);
    }

    #[test]
    fn cancelling_before_the_call_makes_no_request_at_all() {
        let transport = Arc::new(Recorded::new().json(URL, "{}"));
        let (_clock, client) = client(transport.clone());
        let cancel = Cancel::new();
        cancel.cancel();
        assert!(matches!(
            client.body(URL, &[], &cancel),
            Err(Error::Cancelled)
        ));
        assert_eq!(transport.calls(), 0);
    }

    #[test]
    fn cancelling_during_a_wait_gives_the_slot_back() {
        let transport = Arc::new(Recorded::new().json_matching("/releases/", "{}"));
        let clock = Arc::new(TestClock::new());
        let client = Client::new(ProviderId::MusicBrainz, transport.clone())
            .with_clock(clock.clone())
            .with_limiter(Limiter::every(Duration::from_secs(30)));
        let cancel = Cancel::new();

        client
            .body("https://x/releases/1", &[], &cancel)
            .expect("the first");
        cancel.cancel();
        assert!(
            matches!(
                client.body("https://x/releases/2", &[], &cancel),
                Err(Error::Cancelled)
            ),
            "the second was cancelled while queueing"
        );
        assert_eq!(transport.calls(), 1);

        let fresh = Cancel::new();
        client
            .body("https://x/releases/3", &[], &fresh)
            .expect("the third");
        assert_eq!(
            clock.slept_total(),
            30_000,
            "one interval of waiting, not two: the cancelled request handed its slot back"
        );
    }

    #[test]
    fn a_long_wait_is_interruptible_in_slices() {
        let transport = Arc::new(Recorded::new().json_matching("/releases/", "{}"));
        let clock = Arc::new(TestClock::new());
        let client = Client::new(ProviderId::MusicBrainz, transport)
            .with_clock(clock.clone())
            .with_limiter(Limiter::every(Duration::from_secs(1)));
        let cancel = Cancel::new();
        client
            .body("https://x/releases/1", &[], &cancel)
            .expect("the first");
        client
            .body("https://x/releases/2", &[], &cancel)
            .expect("the second");
        let slices = clock.sleeps();
        assert_eq!(slices.iter().sum::<u64>(), 1_000);
        assert!(
            slices.iter().all(|s| *s <= as_millis(CANCEL_SLICE)),
            "no single sleep outlasts the cancellation check: {slices:?}"
        );
    }

    #[test]
    fn an_uncached_fetch_neither_reads_nor_writes_the_cache() {
        let transport = Arc::new(Recorded::new().json(URL, "bytes"));
        let clock = Arc::new(TestClock::new());
        let cache = Arc::new(Memory::new(clock.clone()));
        let client = Client::new(ProviderId::Discogs, transport.clone())
            .with_clock(clock)
            .with_cache(cache.clone())
            .with_limiter(Limiter::unlimited());
        let cancel = Cancel::new();
        client.uncached(URL, &[], &cancel).expect("first");
        client.uncached(URL, &[], &cancel).expect("second");
        assert_eq!(transport.calls(), 2);
        assert!(cache.is_empty());
        let stats = client.stats();
        assert_eq!((stats.cache_hits, stats.cache_writes), (0, 0));
    }

    #[test]
    fn the_published_limits_are_the_defaults() {
        assert_eq!(
            default_limiter(ProviderId::Discogs).interval(),
            Duration::from_secs(1),
            "60 a minute"
        );
        assert_eq!(
            default_limiter(ProviderId::MusicBrainz).interval(),
            Duration::from_secs(1)
        );
    }
}
