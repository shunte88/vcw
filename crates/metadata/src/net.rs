/*
 *  net.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The wire: one request shape, one response shape, one trait (§40).
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

//! The wire: one request shape, one response shape, one trait (§40).
//!
//! Everything in this crate that could reach the network goes through [`Transport`],
//! and [`Transport`] can do exactly one thing: fetch a URL and hand back bytes. That
//! narrowness is the whole design. It means the providers, the rate limiter, the
//! retry policy and the cache are all testable with no sockets, and it means
//! §40's "fully usable offline" is a choice of transport rather than a flag
//! threaded through every call site.
//!
//! Three transports exist:
//!
//! - [`Offline`], which refuses everything. What the application uses when the user
//!   has switched networking off, and the default when nothing else is configured.
//! - [`crate::fixtures::Recorded`], which answers from a table. What the tests use.
//! - `crate::agent::Agent`, a real HTTP client, compiled only with the `net`
//!   feature so that a build without it *cannot* reach the network.
//!
//! # Secrets and `Debug`
//!
//! A request carries an `Authorization` header and a `Debug` impl is one `tracing`
//! call away from a log file, so [`Request`]'s `Debug` redacts header values that
//! look like credentials. The URL is printed whole, which is safe because §39's
//! rule is enforced upstream: credentials go in headers and never into a URL.

use std::fmt;
use std::time::Duration;

/// How long a request may take before it is abandoned (§40).
///
/// Ten seconds. Long enough for a slow provider on a slow connection, short
/// enough that a person does not conclude the application has hung.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// One HTTP header.
#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    /// The field name, as sent.
    pub name: String,
    /// The field value.
    pub value: String,
}

impl Header {
    /// A header from a name and a value.
    #[must_use]
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    /// Whether this header's value must never be printed.
    #[must_use]
    pub fn is_sensitive(&self) -> bool {
        let lower = self.name.to_ascii_lowercase();
        lower == "authorization" || lower.ends_with("-key") || lower.ends_with("-token")
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_sensitive() {
            write!(f, "{}: <redacted>", self.name)
        } else {
            write!(f, "{}: {}", self.name, self.value)
        }
    }
}

/// A request: a URL, and optionally a body to send with it.
///
/// There is no method field, because the body is the method. A request with no
/// body is a GET and a request with one is a POST, which is the only distinction
/// either provider needs: both are read-only, so a POST here never modifies
/// anything at the far end - it exists because an AcoustID fingerprint does not
/// fit in a URL. A real vinyl side measures ~28 base64 characters per second of
/// audio, so a five-minute track is 8.5 KB and a ten-minute one 16.9 KB, both at
/// or past the request-line limit a default HTTP server enforces.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// The absolute URL, credentials excluded by §39's rule.
    pub url: String,
    /// Headers to send, including the user agent §40 requires.
    pub headers: Vec<Header>,
    /// The time box for this request.
    pub timeout: Duration,
    /// The body, if this is a POST. A `Content-Type` header goes with it.
    ///
    /// This is where a credential lives on a POST - AcoustID's `client` parameter
    /// is a form field, not a query parameter - so it is never printed. See the
    /// [`fmt::Debug`] impl.
    pub body: Option<Vec<u8>>,
}

impl Request {
    /// A request for a URL, with the default timeout and no headers.
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
            body: None,
        }
    }

    /// A form POST to a URL, with the default timeout.
    ///
    /// The content type is set here rather than left to the caller, because the
    /// only body this crate sends is a form: `a=1&b=2`, each value through
    /// [`encode`]. A provider that wanted JSON would need its own constructor, and
    /// neither of ours does.
    #[must_use]
    pub fn post_form(url: impl Into<String>, form: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: vec![Header::new(
                "Content-Type",
                "application/x-www-form-urlencoded",
            )],
            timeout: DEFAULT_TIMEOUT,
            body: Some(form.into().into_bytes()),
        }
    }

    /// Adds a header.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push(Header::new(name, value));
        self
    }

    /// Sets the time box.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The value of a header, matched case-insensitively.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("url", &self.url)
            .field("headers", &self.headers)
            .field("timeout_ms", &self.timeout.as_millis())
            // The length, never the content: a form body is where a POST carries
            // its credential (§39), and the one thing a diagnostic needs to know
            // about it is whether it was there and how big it was.
            .field("body_bytes", &self.body.as_ref().map_or(0, Vec::len))
            .finish()
    }
}

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// The HTTP status.
    pub status: u16,
    /// Response headers.
    pub headers: Vec<Header>,
    /// The body, undecoded.
    pub body: Vec<u8>,
}

impl Response {
    /// A 200 with a body and no headers, which is most of what tests need.
    #[must_use]
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// A response with a status and a body.
    #[must_use]
    pub fn status(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// Adds a header.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push(Header::new(name, value));
        self
    }

    /// The value of a header, matched case-insensitively.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    /// Whether the status is 2xx.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// Whether the provider is asking VCW to slow down.
    #[must_use]
    pub const fn is_rate_limited(&self) -> bool {
        self.status == 429
    }

    /// Whether the provider is broken rather than unhappy with the request.
    #[must_use]
    pub const fn is_server_error(&self) -> bool {
        self.status >= 500
    }

    /// Whether the credential was missing, wrong or exhausted.
    #[must_use]
    pub const fn is_unauthorised(&self) -> bool {
        self.status == 401 || self.status == 403
    }

    /// How long the provider asked VCW to wait, if it said.
    ///
    /// Only the delta-seconds form is understood. The HTTP-date form is legal and
    /// neither provider uses it; an unparseable value is treated as absent, which
    /// falls back to the retry policy's own backoff rather than failing.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        let raw = self.header("retry-after")?.trim();
        raw.parse::<u64>().ok().map(Duration::from_secs)
    }

    /// The first `limit` characters of the body as text, for an error message.
    ///
    /// Lossy, trimmed and length-capped, with `...` if anything was dropped: this
    /// only ever ends up in a message a person reads, and a provider's HTML error
    /// page should not fill a terminal.
    #[must_use]
    pub fn preview(&self, limit: usize) -> String {
        let text = String::from_utf8_lossy(&self.body);
        let trimmed = text.trim();
        let mut out: String = trimmed.chars().take(limit).collect();
        if trimmed.chars().count() > limit {
            out.push_str("...");
        }
        out
    }
}

/// Why a request produced no response at all.
///
/// Distinct from an error *status*, which is a response. The difference decides
/// whether retrying is sensible and what the user is told.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// Networking is switched off in this build or by this configuration.
    #[error("networking is disabled")]
    Disabled,
    /// The time box expired.
    #[error("timed out after {0} ms")]
    Timeout(u64),
    /// DNS, connection or TLS failure: nothing was exchanged.
    #[error("unreachable: {0}")]
    Unreachable(String),
    /// Something was exchanged and it was not HTTP VCW could follow.
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl TransportError {
    /// Whether asking again later could plausibly work.
    #[must_use]
    pub const fn is_transient(&self) -> bool {
        matches!(self, Self::Timeout(_) | Self::Unreachable(_))
    }
}

/// Anything that can make a request.
///
/// `Send + Sync` because a provider is shared across the worker threads that use
/// it; `Debug` because a client that cannot say which transport it is holding is
/// no use in a diagnostic bundle (§42).
pub trait Transport: fmt::Debug + Send + Sync {
    /// Makes a request, or explains why it could not.
    ///
    /// One method and not two, because [`Request::body`] already says which verb
    /// this is. An implementation that forgot to look at it would send an
    /// 8 KB fingerprint as a query string, which is the failure the body exists
    /// to avoid, so every implementation is tested on a body.
    fn send(&self, request: &Request) -> Result<Response, TransportError>;

    /// A short name for logs and diagnostics: `offline`, `recorded`, `http`.
    fn name(&self) -> &'static str;
}

/// Percent-encodes a query-string value.
///
/// RFC 3986 unreserved characters pass through and everything else becomes `%XX`,
/// including the space - `%20` rather than `+`. VRipr used `+`, which is the form
/// encoding and is only correct in a form body; a `+` in a catalogue number
/// searched for literally would come back as a space.
///
/// ```
/// # use vcw_metadata::net::encode;
/// assert_eq!(encode("Simon & Garfunkel"), "Simon%20%26%20Garfunkel");
/// assert_eq!(encode("WARPLP25"), "WARPLP25");
/// assert_eq!(encode("12\" Vinyl"), "12%22%20Vinyl");
/// ```
#[must_use]
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The transport that refuses everything.
///
/// §40's offline mode, and the default. Refusing is not an error state: a caller
/// receives [`crate::Error::Offline`] and carries on, because everything the
/// application does with a *project* works without a provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct Offline;

impl Transport for Offline {
    fn send(&self, _request: &Request) -> Result<Response, TransportError> {
        Err(TransportError::Disabled)
    }

    fn name(&self) -> &'static str {
        "offline"
    }
}

/// The application's version, as sent in the user agent.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where VCW lives, as sent in the user agent when no contact is configured.
pub const HOMEPAGE: &str = "https://github.com/shunte88/vcw";

/// The user agent VCW identifies itself with (§40).
///
/// Both providers require an application to identify itself, and MusicBrainz will
/// serve a 403 to a client that does not. The contact is whatever the user
/// configured; with none, the project's homepage stands in, which is what the
/// MusicBrainz guidance asks for when there is no individual to name.
#[must_use]
pub fn user_agent(contact: Option<&str>) -> String {
    let contact = contact.map_or(HOMEPAGE, |c| c);
    format!("VCW/{VERSION} ( {contact} )")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_header_does_not_print_itself() {
        let request = Request::get("https://api.discogs.com/database/search?q=pole")
            .with_header("Authorization", "Discogs token=sekrit")
            .with_header("User-Agent", user_agent(None));
        let debug = format!("{request:?}");
        assert!(!debug.contains("sekrit"), "{debug}");
        assert!(debug.contains("Authorization: <redacted>"), "{debug}");
        assert!(
            debug.contains("User-Agent: VCW/"),
            "the user agent is meant to be seen: {debug}"
        );
        assert!(
            debug.contains("q=pole"),
            "and the URL is printable, because no credential is ever in one"
        );
    }

    #[test]
    fn header_names_that_look_like_secrets_are_redacted_too() {
        for name in [
            "Authorization",
            "authorization",
            "X-Api-Key",
            "X-Auth-Token",
        ] {
            let header = Header::new(name, "sekrit");
            assert!(header.is_sensitive(), "{name}");
            assert!(!format!("{header:?}").contains("sekrit"), "{name}");
        }
        assert!(!Header::new("User-Agent", "VCW/0.1.0").is_sensitive());
    }

    #[test]
    fn a_status_is_classified_once_and_in_one_place() {
        assert!(Response::ok("{}").is_success());
        assert!(Response::status(429, "slow down").is_rate_limited());
        assert!(Response::status(503, "").is_server_error());
        assert!(Response::status(401, "").is_unauthorised());
        assert!(Response::status(403, "").is_unauthorised());
        assert!(!Response::status(404, "").is_server_error());
    }

    #[test]
    fn retry_after_is_read_when_it_is_a_number_and_ignored_otherwise() {
        let asked = Response::status(429, "").with_header("Retry-After", " 5 ");
        assert_eq!(asked.retry_after(), Some(Duration::from_secs(5)));
        let dated =
            Response::status(429, "").with_header("Retry-After", "Wed, 21 Oct 2026 07:28:00 GMT");
        assert_eq!(
            dated.retry_after(),
            None,
            "the date form falls back to our own backoff rather than failing"
        );
        assert_eq!(Response::status(429, "").retry_after(), None);
    }

    #[test]
    fn an_error_body_is_previewed_not_dumped() {
        let long = Response::status(500, "x".repeat(500));
        let preview = long.preview(40);
        assert_eq!(preview.chars().count(), 43, "40 characters and an ellipsis");
        assert!(preview.ends_with("..."));
        assert_eq!(
            Response::status(404, "  not found\n").preview(40),
            "not found"
        );
    }

    #[test]
    fn a_form_post_carries_the_body_and_says_what_it_is() {
        let request = Request::post_form("https://api.acoustid.org/v2/lookup", "a=1&b=2");
        assert_eq!(
            request.header("content-type"),
            Some("application/x-www-form-urlencoded"),
            "matched case-insensitively, as a header is"
        );
        assert_eq!(request.body.as_deref(), Some(&b"a=1&b=2"[..]));
        assert_eq!(request.timeout, DEFAULT_TIMEOUT);
        assert!(
            Request::get("https://musicbrainz.org/ws/2/release")
                .body
                .is_none(),
            "a GET has no body, which is how a transport tells them apart"
        );
    }

    #[test]
    fn a_post_body_does_not_print_itself() {
        // It is where a credential lives on a POST: AcoustID's `client` parameter
        // is a form field (§39), and a `Debug` is one `tracing` call from a log.
        let shown = format!(
            "{:?}",
            Request::post_form("https://api.acoustid.org/v2/lookup", "client=secretkey")
        );
        assert!(!shown.contains("secretkey"), "{shown}");
        assert!(shown.contains("body_bytes: 16"), "{shown}");
    }

    #[test]
    fn the_offline_transport_refuses_everything() {
        let offline = Offline;
        assert_eq!(offline.name(), "offline");
        assert_eq!(
            offline.send(&Request::get("https://musicbrainz.org/ws/2/release")),
            Err(TransportError::Disabled)
        );
        assert_eq!(
            offline.send(&Request::post_form(
                "https://api.acoustid.org/v2/lookup",
                "client=k"
            )),
            Err(TransportError::Disabled),
            "a body is not a way around §40"
        );
        assert!(
            !TransportError::Disabled.is_transient(),
            "waiting does not switch the network on"
        );
        assert!(TransportError::Timeout(10_000).is_transient());
    }

    #[test]
    fn the_user_agent_identifies_the_application_and_a_way_to_complain() {
        let default = user_agent(None);
        assert_eq!(default, format!("VCW/{VERSION} ( {HOMEPAGE} )"));
        assert_eq!(
            user_agent(Some("someone@example.invalid")),
            format!("VCW/{VERSION} ( someone@example.invalid )")
        );
    }
}
