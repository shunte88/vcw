/*
 *  agent.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The only code in VCW that opens a socket (§40).
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

//! The only code in VCW that opens a socket (§40).
//!
//! Behind the `net` feature, and that is the whole design. §40 requires the
//! application to be fully usable with networking disabled, and the strongest form
//! of that promise is a build in which the code that could make a request does not
//! exist. `cargo check -p vcw-metadata --no-default-features` is a gate leg for
//! exactly this reason: if a call site ever reaches past [`Transport`], that build
//! stops compiling.
//!
//! # Blocking, deliberately
//!
//! VCW has no async runtime (ADR-0003) and is not acquiring one for two REST APIs.
//! `ureq` is blocking, which suits a design where a search runs on a worker thread
//! and the thing that has to stay responsive is a device callback that this crate
//! cannot reach anyway.
//!
//! # rustls, deliberately
//!
//! No OpenSSL on the build machine, no `native-tls` on Windows, no system
//! certificate store to be missing on a Pi. `webpki-roots` is already in the
//! license allowlist in `deny.toml`.
//!
//! # What this file does not do
//!
//! No retries, no rate limiting, no caching, no cancellation. Those are
//! [`crate::Client`]'s, which is why they are testable without a network: an agent
//! that also retried would mean the retry policy could only be tested against a
//! real server.

use std::time::Duration;

use crate::net::{Request, Response, Transport, TransportError};

/// The largest body the agent will read into memory.
///
/// A metadata response is tens of kilobytes and a cover is capped at eight
/// megabytes by [`crate::artwork::MAX_BYTES`], so sixteen is generous. The point
/// is that a misbehaving or hostile server cannot make VCW allocate without bound.
pub const MAX_BODY: usize = 16 * 1024 * 1024;

/// A blocking HTTP transport.
#[derive(Debug, Clone, Copy, Default)]
pub struct Agent;

impl Agent {
    /// A transport that will really make requests.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Transport for Agent {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        let agent = ureq::Agent::config_builder()
            // Both halves matter: a server that accepts a connection and then says
            // nothing would otherwise hold a worker thread forever.
            .timeout_connect(Some(request.timeout))
            .timeout_global(Some(request.timeout))
            // A 4xx or 5xx is a response, not an error, and the body is the half of
            // it worth reading. ureq's default turns the status into
            // `Error::StatusCode(u16)` and drops everything else, which cost every
            // provider its error message: AcoustID answers a bad key with
            // `400 {"error": {"code": 4, "message": "invalid API key"}}` and VCW
            // reported `AcoustID returned HTTP 400: ` with nothing after the colon.
            // The status reaching the client is what lets it decide 429 is worth
            // retrying; the body is what lets a person know what to fix.
            .http_status_as_error(false)
            .build()
            .new_agent();

        tracing::debug!(
            url = %without_query(&request.url),
            timeout_ms = millis(request.timeout),
            body_bytes = request.body.as_ref().map_or(0, Vec::len),
            "provider request"
        );

        // The header loop twice over, because ureq's builder is a different type
        // with a body than without one and the two cannot share a variable. Six
        // duplicated lines beats a generic helper for two call sites.
        let outcome = match &request.body {
            Some(body) => {
                let mut call = agent.post(&request.url);
                for header in &request.headers {
                    call = call.header(&header.name, &header.value);
                }
                call.send(body.as_slice())
            }
            None => {
                let mut call = agent.get(&request.url);
                for header in &request.headers {
                    call = call.header(&header.name, &header.value);
                }
                call.call()
            }
        };

        let mut response = match outcome {
            Ok(response) => response,
            // Still handled, though `http_status_as_error(false)` means it should
            // not arrive: it is one config call away, and a body-less status is a
            // better answer than a transport error that loses the status too.
            Err(ureq::Error::StatusCode(status)) => {
                tracing::warn!(
                    url = %without_query(&request.url),
                    status,
                    "the provider answered with an error status"
                );
                return Ok(Response::status(status, Vec::new()));
            }
            Err(ureq::Error::Timeout(_)) => {
                tracing::warn!(
                    url = %without_query(&request.url),
                    timeout_ms = millis(request.timeout),
                    "the provider did not answer in time"
                );
                return Err(TransportError::Timeout(millis(request.timeout)));
            }
            Err(error) => {
                tracing::warn!(
                    url = %without_query(&request.url),
                    error = %error,
                    "the provider request failed"
                );
                return Err(classify(&error));
            }
        };

        let status = response.status().as_u16();
        let mut out = Response::status(status, Vec::new());
        for (name, value) in response.headers() {
            if let Ok(value) = value.to_str() {
                out = out.with_header(name.as_str(), value);
            }
        }
        out.body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY as u64)
            .read_to_vec()
            .map_err(|error| match error {
                ureq::Error::Timeout(_) => TransportError::Timeout(millis(request.timeout)),
                other => classify(&other),
            })?;
        tracing::debug!(
            url = %without_query(&request.url),
            status,
            bytes = out.body.len(),
            "provider response"
        );
        Ok(out)
    }

    fn name(&self) -> &'static str {
        "http"
    }
}

/// A URL with its query string removed, for logging.
///
/// §39 keeps credentials out of the URL and into a header, and nothing here ever
/// logs a header. This is the belt to that braces: a provider that one day wants
/// its key as `?token=` would otherwise put it in every log line, and a log is a
/// file people paste into bug reports. The path is what identifies the request;
/// the query is what it asked for, and this crate's own cache keys already record
/// that where it matters.
fn without_query(url: &str) -> &str {
    url.split_once('?').map_or(url, |(before, _)| before)
}

/// Sorts a `ureq` failure into the three kinds a caller can act on.
///
/// The distinction that matters is transient against permanent: a DNS failure on a
/// laptop that just woke up is worth retrying, and a malformed URL never will be.
fn classify(error: &ureq::Error) -> TransportError {
    use ureq::Error as U;
    match error {
        U::Timeout(_) => TransportError::Timeout(0),
        // Permanent: the request itself is wrong, or the server said something
        // that is not HTTP. Asking again produces the same answer.
        U::Http(detail) => TransportError::Protocol(format!("http: {detail}")),
        U::BadUri(uri) => TransportError::Protocol(format!("bad url: {uri}")),
        U::Protocol(detail) => TransportError::Protocol(detail.to_string()),
        U::BodyExceedsLimit(limit) => {
            TransportError::Protocol(format!("body exceeds {limit} bytes"))
        }
        U::LargeResponseHeader(size, limit) => {
            TransportError::Protocol(format!("response header of {size} bytes exceeds {limit}"))
        }
        U::RequireHttpsOnly(url) => {
            TransportError::Protocol(format!("plain http is refused: {url}"))
        }
        U::TlsRequired => TransportError::Protocol("tls is required".into()),
        U::InvalidProxyUrl => TransportError::Protocol("the proxy url is not a url".into()),
        // A status reaching here would be a bug in `get`, which handles it, but
        // reporting it as an HTTP answer is still the truthful thing to do.
        U::StatusCode(status) => TransportError::Protocol(format!("unhandled status {status}")),
        // Everything else is the network being the network: a name that does not
        // resolve yet, a refused connection, a laptop that just woke up.
        other => TransportError::Unreachable(other.to_string()),
    }
}

/// A duration as whole milliseconds, saturating.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_names_itself_and_is_not_the_offline_one() {
        assert_eq!(Agent::new().name(), "http");
        assert_ne!(Agent::new().name(), "offline");
    }

    #[test]
    fn a_url_that_is_not_a_url_is_a_protocol_error_and_not_a_hang() {
        // No network is touched: there is nothing here to resolve.
        let error = Agent::new()
            .send(&Request::get("not a url at all").with_timeout(Duration::from_millis(50)))
            .expect_err("no response");
        assert!(matches!(error, TransportError::Protocol(_)), "{error:?}");
        assert!(!error.is_transient(), "asking again will not help");
    }

    #[test]
    fn an_unresolvable_host_is_transient_rather_than_permanent() {
        // `.invalid` is reserved by RFC 2606 and cannot resolve, so this fails
        // locally in the resolver rather than reaching anything.
        let error = Agent::new()
            .send(
                &Request::get("https://vcw.invalid/release/1")
                    .with_timeout(Duration::from_millis(500)),
            )
            .expect_err("no response");
        assert!(
            error.is_transient(),
            "a name that will not resolve today might tomorrow: {error:?}"
        );
    }

    #[test]
    fn the_body_limit_is_smaller_than_anything_worth_reading_twice() {
        const { assert!(MAX_BODY > crate::artwork::MAX_BYTES) };
        const { assert!(MAX_BODY <= 32 * 1024 * 1024) };
    }
}
