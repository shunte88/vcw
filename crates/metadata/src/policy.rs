/*
 *  policy.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Clocks, rate limits and conservative retries (§40).
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

//! Clocks, rate limits and conservative retries (§40).
//!
//! §40 asks for timeouts, cancellation, rate limits, caching and conservative
//! retries. Three of those are policy rather than plumbing, and policy that
//! involves time is usually untestable - which is why the clock is a trait.
//! [`TestClock`] advances virtually, so **no test in this crate sleeps**: the rate
//! limiter's spacing and the retry backoff are asserted as numbers, not waited out.
//!
//! # Rate limits are a promise, not a tuning parameter
//!
//! Discogs allows 60 requests a minute with a token and MusicBrainz asks for one a
//! second, sustained. Both will start refusing a client that ignores them, and
//! §40 treats exceeding a published limit as a defect rather than an optimization
//! opportunity. [`Limiter`] therefore *reserves* a slot before a request is made,
//! so two threads asking at once queue behind each other instead of both deciding
//! that enough time has passed.
//!
//! # Conservative means three things
//!
//! Retry only what could plausibly succeed, back off geometrically, and obey
//! `Retry-After` when the provider sends one - but refuse to wait longer than
//! [`Retry::max_backoff`]. A provider asking for an hour is telling the user
//! something, and the honest response is to say so rather than to hang.

use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::net::{Response, TransportError};

/// A source of time that a test can control.
pub trait Clock: fmt::Debug + Send + Sync {
    /// Milliseconds since an arbitrary fixed point. Monotonic.
    fn now_millis(&self) -> u64;

    /// Waits. A test clock advances instead.
    fn sleep(&self, duration: Duration);
}

/// The real clock.
#[derive(Debug)]
pub struct SystemClock {
    epoch: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl SystemClock {
    /// A clock whose zero is now.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        // Saturating rather than wrapping: 2^64 ms is half a billion years, but a
        // cast that could silently go backwards has no business in a rate limiter.
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// A clock that advances only when something sleeps.
///
/// Test support, compiled always because the integration tests need it too.
#[derive(Debug, Default)]
pub struct TestClock {
    state: Mutex<TestClockState>,
}

#[derive(Debug, Default)]
struct TestClockState {
    now: u64,
    slept: Vec<u64>,
}

impl TestClock {
    /// A clock reading zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves time forward without recording a sleep.
    pub fn advance(&self, duration: Duration) {
        let mut state = self.state.lock().expect("the test clock is not poisoned");
        state.now = state.now.saturating_add(millis(duration));
    }

    /// Every sleep asked of this clock, in order, in milliseconds.
    #[must_use]
    pub fn sleeps(&self) -> Vec<u64> {
        self.state
            .lock()
            .expect("the test clock is not poisoned")
            .slept
            .clone()
    }

    /// The total time slept, in milliseconds.
    #[must_use]
    pub fn slept_total(&self) -> u64 {
        self.sleeps().iter().sum()
    }
}

impl Clock for TestClock {
    fn now_millis(&self) -> u64 {
        self.state
            .lock()
            .expect("the test clock is not poisoned")
            .now
    }

    fn sleep(&self, duration: Duration) {
        let millis = millis(duration);
        let mut state = self.state.lock().expect("the test clock is not poisoned");
        state.now = state.now.saturating_add(millis);
        state.slept.push(millis);
    }
}

/// A `Duration` as whole milliseconds, saturating.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Spaces requests to a provider out to a published rate (§40).
#[derive(Debug)]
pub struct Limiter {
    min_interval: Duration,
    /// When the next request may be issued, in clock milliseconds.
    next_free: Mutex<u64>,
}

impl Limiter {
    /// A limiter allowing one request every `interval`.
    #[must_use]
    pub fn every(interval: Duration) -> Self {
        Self {
            min_interval: interval,
            next_free: Mutex::new(0),
        }
    }

    /// A limiter allowing `requests` per minute, rounded up to whole milliseconds.
    ///
    /// Zero is treated as unlimited rather than as a deadlock.
    #[must_use]
    pub fn per_minute(requests: u32) -> Self {
        if requests == 0 {
            return Self::unlimited();
        }
        let interval = Duration::from_millis(60_000_u64.div_ceil(u64::from(requests)));
        Self::every(interval)
    }

    /// A limiter that never waits. For a transport that is not a network.
    #[must_use]
    pub fn unlimited() -> Self {
        Self::every(Duration::ZERO)
    }

    /// The configured spacing.
    #[must_use]
    pub const fn interval(&self) -> Duration {
        self.min_interval
    }

    /// Claims the next slot and returns how long the caller must wait for it.
    ///
    /// Reserving rather than merely measuring is what makes this correct with more
    /// than one caller: the slot is spoken for the moment it is handed out, so two
    /// threads get two slots one interval apart rather than the same one.
    #[must_use]
    pub fn reserve(&self, now_millis: u64) -> Duration {
        if self.min_interval.is_zero() {
            return Duration::ZERO;
        }
        let mut next_free = self.next_free.lock().expect("the limiter is not poisoned");
        let issue_at = (*next_free).max(now_millis);
        *next_free = issue_at.saturating_add(millis(self.min_interval));
        Duration::from_millis(issue_at.saturating_sub(now_millis))
    }

    /// Gives back a slot that was reserved and then not used.
    ///
    /// A cache hit reserves nothing, but a canceled request has already taken its
    /// place in the queue; handing it back keeps a canceled search from delaying
    /// the next real one.
    pub fn release(&self, reserved_until: u64) {
        let mut next_free = self.next_free.lock().expect("the limiter is not poisoned");
        if *next_free == reserved_until {
            *next_free = reserved_until.saturating_sub(millis(self.min_interval));
        }
    }

    /// When the slot reserved at `now_millis` after waiting `delay` expires.
    #[must_use]
    pub fn reserved_until(&self, now_millis: u64, delay: Duration) -> u64 {
        now_millis
            .saturating_add(millis(delay))
            .saturating_add(millis(self.min_interval))
    }
}

/// What a single attempt produced.
#[derive(Debug)]
pub enum Attempt<'a> {
    /// The provider answered, with whatever status.
    Answered(&'a Response),
    /// The provider did not answer.
    Failed(&'a TransportError),
}

/// How hard to try again, and how long to wait between attempts (§40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    /// Total attempts, including the first. One means no retrying.
    pub attempts: u32,
    /// The wait before the second attempt; doubled for each one after.
    pub initial_backoff: Duration,
    /// The longest VCW will wait between attempts, and the longest it will honor
    /// a `Retry-After` for before giving up instead.
    pub max_backoff: Duration,
}

impl Default for Retry {
    fn default() -> Self {
        Self::conservative()
    }
}

impl Retry {
    /// Three attempts, one second, doubling, capped at eight.
    ///
    /// Three because the failures worth retrying - a dropped connection, a
    /// momentary 503, a rate limit that has just tripped - almost always clear on
    /// the second attempt, and a provider that fails three times is not having a
    /// blip. The whole sequence costs at most three seconds of waiting.
    #[must_use]
    pub const fn conservative() -> Self {
        Self {
            attempts: 3,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(8),
        }
    }

    /// One attempt and no waiting.
    #[must_use]
    pub const fn never() -> Self {
        Self {
            attempts: 1,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
        }
    }

    /// The backoff before attempt number `attempt`, which is one-based.
    ///
    /// Attempt 1 has no backoff because it has not failed yet.
    #[must_use]
    pub fn backoff(&self, attempt: u32) -> Duration {
        if attempt <= 1 {
            return Duration::ZERO;
        }
        let doublings = attempt - 2;
        let scaled = self
            .initial_backoff
            .saturating_mul(2_u32.saturating_pow(doublings.min(16)));
        scaled.min(self.max_backoff)
    }

    /// How long to wait before trying again, or `None` to stop.
    ///
    /// `attempt` is the one-based number of the attempt that just finished.
    #[must_use]
    pub fn after(&self, attempt: u32, outcome: &Attempt<'_>) -> Option<Duration> {
        if attempt >= self.attempts {
            return None;
        }
        let backoff = self.backoff(attempt + 1);
        match outcome {
            Attempt::Answered(response) if response.is_rate_limited() => {
                match response.retry_after() {
                    // The provider named a wait. Obey it, unless it is longer than
                    // VCW is prepared to hang about for, in which case stopping and
                    // saying so is more use than a frozen window.
                    Some(asked) if asked > self.max_backoff => None,
                    Some(asked) => Some(asked.max(backoff)),
                    None => Some(backoff),
                }
            }
            Attempt::Answered(response) if response.is_server_error() => Some(backoff),
            Attempt::Answered(_) => None,
            Attempt::Failed(error) if error.is_transient() => Some(backoff),
            Attempt::Failed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_clock_moves_only_when_something_sleeps() {
        let clock = TestClock::new();
        assert_eq!(clock.now_millis(), 0);
        clock.sleep(Duration::from_millis(250));
        assert_eq!(clock.now_millis(), 250);
        clock.advance(Duration::from_millis(50));
        assert_eq!(clock.now_millis(), 300);
        assert_eq!(clock.sleeps(), [250], "advancing is not sleeping");
        assert_eq!(clock.slept_total(), 250);
    }

    #[test]
    fn a_published_rate_becomes_an_interval() {
        assert_eq!(Limiter::per_minute(60).interval(), Duration::from_secs(1));
        assert_eq!(
            Limiter::per_minute(25).interval(),
            Duration::from_millis(2_400),
            "Discogs without a token"
        );
        assert_eq!(
            Limiter::per_minute(7).interval(),
            Duration::from_millis(8_572),
            "rounded up, so seven requests take just over a minute rather than just under"
        );
        assert_eq!(Limiter::per_minute(0).interval(), Duration::ZERO);
    }

    #[test]
    fn the_first_request_does_not_wait_and_the_second_does() {
        let limiter = Limiter::per_minute(60);
        assert_eq!(limiter.reserve(0), Duration::ZERO);
        assert_eq!(limiter.reserve(0), Duration::from_millis(1_000));
        assert_eq!(
            limiter.reserve(0),
            Duration::from_millis(2_000),
            "three callers at once queue one second apart"
        );
        assert_eq!(
            limiter.reserve(10_000),
            Duration::ZERO,
            "and a caller that arrives later waits for nothing"
        );
    }

    #[test]
    fn an_unused_slot_goes_back() {
        let limiter = Limiter::per_minute(60);
        assert_eq!(limiter.reserve(0), Duration::ZERO);
        let until = limiter.reserved_until(0, Duration::ZERO);
        assert_eq!(until, 1_000);
        limiter.release(until);
        assert_eq!(
            limiter.reserve(0),
            Duration::ZERO,
            "a canceled request does not delay the next one"
        );
    }

    #[test]
    fn backoff_doubles_and_then_stops_doubling() {
        let retry = Retry::conservative();
        assert_eq!(retry.backoff(1), Duration::ZERO);
        assert_eq!(retry.backoff(2), Duration::from_secs(1));
        assert_eq!(retry.backoff(3), Duration::from_secs(2));
        assert_eq!(retry.backoff(4), Duration::from_secs(4));
        assert_eq!(retry.backoff(5), Duration::from_secs(8));
        assert_eq!(
            retry.backoff(50),
            retry.max_backoff,
            "capped, not overflowing"
        );
    }

    #[test]
    fn only_what_could_work_is_retried() {
        let retry = Retry::conservative();
        let five_hundred = Response::status(503, "");
        let four_oh_four = Response::status(404, "");
        let timeout = TransportError::Timeout(10_000);
        let disabled = TransportError::Disabled;

        assert_eq!(
            retry.after(1, &Attempt::Answered(&five_hundred)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(retry.after(1, &Attempt::Answered(&four_oh_four)), None);
        assert_eq!(
            retry.after(1, &Attempt::Failed(&timeout)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            retry.after(1, &Attempt::Failed(&disabled)),
            None,
            "offline is not a blip"
        );
        assert_eq!(
            retry.after(3, &Attempt::Answered(&five_hundred)),
            None,
            "three attempts means three"
        );
        assert_eq!(
            retry.after(1, &Attempt::Answered(&Response::ok("{}"))),
            None
        );
    }

    #[test]
    fn a_retry_after_header_is_obeyed_up_to_a_point() {
        let retry = Retry::conservative();
        let polite = Response::status(429, "").with_header("Retry-After", "3");
        assert_eq!(
            retry.after(1, &Attempt::Answered(&polite)),
            Some(Duration::from_secs(3)),
            "the provider knows better than our backoff"
        );
        let brief = Response::status(429, "").with_header("Retry-After", "0");
        assert_eq!(
            retry.after(1, &Attempt::Answered(&brief)),
            Some(Duration::from_secs(1)),
            "but not when that would mean hammering it"
        );
        let hour = Response::status(429, "").with_header("Retry-After", "3600");
        assert_eq!(
            retry.after(1, &Attempt::Answered(&hour)),
            None,
            "an hour is an answer to give the user, not a wait to sit through"
        );
        let silent = Response::status(429, "");
        assert_eq!(
            retry.after(1, &Attempt::Answered(&silent)),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn retry_can_be_switched_off() {
        let retry = Retry::never();
        let broken = Response::status(503, "");
        assert_eq!(retry.after(1, &Attempt::Answered(&broken)), None);
        assert_eq!(retry.attempts, 1);
    }
}
