/*
 *  fixtures.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A transport that answers from a table, so tests need no network (§40).
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

//! A transport that answers from a table, so tests need no network (§40).
//!
//! WP-12's exit criterion is fixture-backed offline tests, and this is the piece
//! that makes them possible: [`Recorded`] is a [`Transport`] whose answers are
//! written down in advance. Every provider test in this crate and in the command
//! line's integration tests runs through it, which means the parsers are tested
//! against real captured payloads (see `tests/fixtures/README.md`) without any test
//! depending on a service being up, on a credential existing, or on a rate limit.
//!
//! It is compiled unconditionally rather than behind `cfg(test)`, because the
//! integration tests are separate crates and cannot see another crate's test-only
//! items. That also makes it usable from the command line's own tests.
//!
//! # Answers are a queue, and the last one repeats
//!
//! A URL can be given several answers, consumed in order. That is how a retry is
//! tested: queue a 429 and then a 200 and assert that the second attempt is the one
//! that counted. Once the queue is down to its last answer that answer repeats
//! forever, so a test that accidentally asks twice gets a stable result rather than
//! a confusing "no fixture" error on the second call.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::net::{Request, Response, Transport, TransportError};

/// A transport that replays prepared answers.
#[derive(Debug, Default)]
pub struct Recorded {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Answers for an exact URL, consumed front to back.
    exact: HashMap<String, Vec<Response>>,
    /// Answers for any URL containing a substring, tried in insertion order.
    matching: Vec<(String, Vec<Response>)>,
    /// Every request made, in order.
    seen: Vec<Request>,
}

impl Recorded {
    /// A transport with no answers. Every request fails as unreachable.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an answer for an exact URL.
    #[must_use]
    pub fn answering(self, url: impl Into<String>, response: Response) -> Self {
        {
            let mut state = self.state.lock().expect("the fixture is not poisoned");
            state.exact.entry(url.into()).or_default().push(response);
        }
        self
    }

    /// Queues a JSON body for an exact URL.
    #[must_use]
    pub fn json(self, url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        let response = Response::ok(body).with_header("Content-Type", "application/json");
        self.answering(url, response)
    }

    /// Queues an answer for any URL containing `fragment`.
    ///
    /// For tests that care what came back and not about how the URL was spelled.
    /// Exact matches are tried first, so a specific answer always beats a general
    /// one however they were registered.
    #[must_use]
    pub fn matching(self, fragment: impl Into<String>, response: Response) -> Self {
        {
            let mut state = self.state.lock().expect("the fixture is not poisoned");
            let fragment = fragment.into();
            match state.matching.iter_mut().find(|(f, _)| *f == fragment) {
                Some((_, queue)) => queue.push(response),
                None => state.matching.push((fragment, vec![response])),
            }
        }
        self
    }

    /// Queues a JSON body for any URL containing `fragment`.
    #[must_use]
    pub fn json_matching(self, fragment: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        let response = Response::ok(body).with_header("Content-Type", "application/json");
        self.matching(fragment, response)
    }

    /// Every request made so far, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        self.state
            .lock()
            .expect("the fixture is not poisoned")
            .seen
            .clone()
    }

    /// How many requests were made.
    #[must_use]
    pub fn calls(&self) -> usize {
        self.state
            .lock()
            .expect("the fixture is not poisoned")
            .seen
            .len()
    }

    /// The URL of the `index`th request, if it happened.
    #[must_use]
    pub fn url(&self, index: usize) -> Option<String> {
        self.requests().get(index).map(|r| r.url.clone())
    }

    /// Whether any request was made to a URL containing `fragment`.
    #[must_use]
    pub fn asked_about(&self, fragment: &str) -> bool {
        self.requests().iter().any(|r| r.url.contains(fragment))
    }

    /// Forgets the recorded requests, keeping the queued answers.
    pub fn clear_requests(&self) {
        self.state
            .lock()
            .expect("the fixture is not poisoned")
            .seen
            .clear();
    }
}

/// Takes the next answer from a queue, leaving the last one in place.
fn next(queue: &mut Vec<Response>) -> Response {
    if queue.len() > 1 {
        queue.remove(0)
    } else {
        queue.first().cloned().expect("a non-empty queue")
    }
}

impl Transport for Recorded {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        let mut state = self.state.lock().expect("the fixture is not poisoned");
        state.seen.push(request.clone());
        if let Some(queue) = state.exact.get_mut(&request.url) {
            return Ok(next(queue));
        }
        let url = request.url.clone();
        if let Some((_, queue)) = state
            .matching
            .iter_mut()
            .find(|(fragment, _)| url.contains(fragment.as_str()))
        {
            return Ok(next(queue));
        }
        // Unreachable rather than a 404: a missing fixture means the test asked a
        // question nobody prepared an answer for, and a 404 would let a provider
        // quietly report "no such release" for what is really a test defect.
        Err(TransportError::Unreachable(format!(
            "no fixture for {}",
            request.url
        )))
    }

    fn name(&self) -> &'static str {
        "recorded"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exact_url_is_answered_and_recorded() {
        let transport = Recorded::new().json("https://example.invalid/a", "{\"ok\":true}");
        let response = transport
            .send(&Request::get("https://example.invalid/a"))
            .expect("an answer");
        assert!(response.is_success());
        assert_eq!(response.body, b"{\"ok\":true}");
        assert_eq!(response.header("content-type"), Some("application/json"));
        assert_eq!(transport.calls(), 1);
        assert_eq!(
            transport.url(0).as_deref(),
            Some("https://example.invalid/a")
        );
        assert!(transport.asked_about("/a"));
    }

    #[test]
    fn a_queue_is_consumed_in_order_and_then_repeats() {
        let transport = Recorded::new()
            .answering("https://example.invalid/a", Response::status(429, "slow"))
            .answering("https://example.invalid/a", Response::ok("first"))
            .answering("https://example.invalid/a", Response::ok("second"));
        let request = Request::get("https://example.invalid/a");
        assert_eq!(transport.send(&request).expect("a").status, 429);
        assert_eq!(transport.send(&request).expect("b").body, b"first");
        assert_eq!(transport.send(&request).expect("c").body, b"second");
        assert_eq!(
            transport.send(&request).expect("d").body,
            b"second",
            "the last answer repeats rather than running out"
        );
        assert_eq!(transport.calls(), 4);
    }

    #[test]
    fn an_exact_answer_beats_a_fragment_however_they_were_registered() {
        let transport = Recorded::new()
            .json_matching("/release", "general")
            .json("https://example.invalid/release/1", "specific");
        assert_eq!(
            transport
                .send(&Request::get("https://example.invalid/release/1"))
                .expect("an answer")
                .body,
            b"specific"
        );
        assert_eq!(
            transport
                .send(&Request::get("https://example.invalid/release/2"))
                .expect("an answer")
                .body,
            b"general"
        );
    }

    #[test]
    fn a_missing_fixture_is_a_test_defect_and_says_so() {
        let transport = Recorded::new();
        let error = transport
            .send(&Request::get("https://example.invalid/nothing"))
            .expect_err("no fixture");
        assert_eq!(
            error,
            TransportError::Unreachable(
                "no fixture for https://example.invalid/nothing".to_owned()
            )
        );
    }

    #[test]
    fn requests_can_be_forgotten_without_losing_the_answers() {
        let transport = Recorded::new().json_matching("/a", "body");
        let request = Request::get("https://example.invalid/a");
        transport.send(&request).expect("an answer");
        transport.clear_requests();
        assert_eq!(transport.calls(), 0);
        assert!(
            transport.send(&request).is_ok(),
            "the answer is still there"
        );
    }
}
