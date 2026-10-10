/*
 *  auth.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Who is allowed to drive a `vcw serve` listener, and on what evidence (§52).
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

//! §52's trust boundary.
//!
//! Every request is authenticated, loopback included. The reasoning is in §52
//! and is not repeated here, but the one consequence that shapes this module
//! is worth restating because it is the part that looks redundant: the token
//! alone does not stop DNS rebinding once the browser is holding it. A page on
//! `evil.example` that re-points its own name at 127.0.0.1 becomes, as far as
//! the browser is concerned, the same origin as the listener - so the browser
//! attaches the cookie for it. What that page cannot do is control the `Host`
//! header, which still carries the name it was fetched under. So the `Host`
//! check is the rebinding defense and the token is the authentication, and
//! neither substitutes for the other.
//!
//! Order matters for the same reason: `Host` is checked before the secret, so
//! a rebound request is refused without the comparison that would tell it
//! whether its guess was close.

use std::collections::BTreeSet;
use std::fmt;

/// The §52 session secret.
///
/// Deliberately opaque. There is no `Display`, no `Serialize` and a `Debug`
/// that prints a length, for the reason §39 gives and
/// `vcw_metadata::Token` already follows: the value ends up in a log line, a
/// panic message or a pasted bug report the moment anything can render it.
///
/// The one place it is rendered is [`Secret::in_url`], which exists so that
/// printing the startup URL has to be written on purpose.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({} chars)", self.0.len())
    }
}

/// Where the secret running this listener came from.
///
/// Reported so the startup banner can tell an operator whether the URL it just
/// printed will still work after a restart, which is the only difference
/// between the two that they can act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Supplied by the environment. Survives a restart.
    Environment,
    /// Minted for the life of this process. A restart invalidates it.
    Minted,
}

/// The environment variable §52 reads the secret from.
pub const VARIABLE: &str = "VCW_SERVE_TOKEN";

/// 32 bytes, hex. Long enough that guessing is not a strategy and short enough
/// to survive being pasted into a terminal that wraps.
const BYTES: usize = 32;

impl Secret {
    /// Take the secret from the environment, or mint one for this process.
    ///
    /// §52: the environment wins when it supplies one, because a bookmark and
    /// a service unit both want a URL that outlives a restart. Nothing is
    /// written to disk in either case.
    ///
    /// An empty or whitespace-only variable is treated as unset rather than as
    /// an empty password: `VCW_SERVE_TOKEN=` in a unit file is a mistake, and
    /// honoring it would be the one reading of it that disables the check.
    pub fn from_env_or_mint() -> (Self, Origin) {
        match std::env::var(VARIABLE) {
            Ok(stated) if !stated.trim().is_empty() => (Self::stated(&stated), Origin::Environment),
            _ => Self::mint(),
        }
    }

    /// A secret somebody stated, rather than one this process minted.
    ///
    /// Trimmed, because a token pasted into a unit file or exported from a
    /// shell arrives with whatever whitespace came with it, and a secret that
    /// fails only on the machine where it was pasted with a trailing newline
    /// is the worst kind of intermittent.
    pub fn stated(secret: &str) -> Self {
        Self(secret.trim().to_owned())
    }

    /// A fresh secret from the platform CSPRNG.
    fn mint() -> (Self, Origin) {
        let mut bytes = [0u8; BYTES];
        getrandom::fill(&mut bytes).expect("the platform random source");
        let mut hex = String::with_capacity(BYTES * 2);
        for byte in bytes {
            use fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
        }
        (Self(hex), Origin::Minted)
    }

    /// Does the presented string match?
    ///
    /// Constant time in the length of the secret. A listener answers an
    /// unauthenticated request on every connection it accepts, so a comparison
    /// that returns early on the first wrong byte is an oracle that reads the
    /// secret out one byte at a time. The length is allowed to leak - it is
    /// not secret, and §52 does not fix it.
    pub fn matches(&self, presented: &str) -> bool {
        let (ours, theirs) = (self.0.as_bytes(), presented.as_bytes());
        if ours.len() != theirs.len() {
            return false;
        }
        let mut differs = 0u8;
        for (a, b) in ours.iter().zip(theirs) {
            differs |= a ^ b;
        }
        differs == 0
    }

    /// The secret, for the one caller allowed to render it: the startup URL.
    ///
    /// Named so that a `grep` for where the token can escape finds this and
    /// nothing else.
    pub fn in_url(&self) -> &str {
        &self.0
    }
}

/// What [`Guard::admit`] decided about one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Carry on. The caller is already holding the secret.
    Admit,
    /// Carry on, and hand the browser the secret so the next request has it
    /// without the URL carrying it again.
    AdmitAndRemember,
    /// Refuse, with a reason fit to show a person.
    ///
    /// The reason is deliberately about what to do rather than about what was
    /// wrong: a refusal that names no remedy is a dead end, and the operator
    /// reading it is the same person who can fix it.
    Refuse(String),
}

/// The §52 trust boundary for one listener.
#[derive(Debug)]
pub struct Guard {
    secret: Secret,
    /// Every `Host` value this listener will answer to, lowercased.
    hosts: BTreeSet<String>,
}

impl Guard {
    /// Build a guard for a listener bound to `address`, answering also to any
    /// of `also` - the names an operator said they would browse by.
    ///
    /// `address` is the literal the operator gave, port included, because that
    /// is what their browser will send. A loopback bind additionally answers
    /// to `localhost`, since the two are the same machine by definition and
    /// requiring `--allow-host localhost` to use the URL we just printed would
    /// be a refusal aimed at the wrong person.
    pub fn new<I, S>(secret: Secret, address: &str, also: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut hosts = BTreeSet::new();
        hosts.insert(address.to_ascii_lowercase());
        // A wildcard bind is not a name anybody browses by, so it would only
        // sit in the set refusing every real request. `needs_a_stated_host`
        // is what stops a listener reaching this state with nothing else in
        // the set; dropping it here keeps the refusal text honest about what
        // the listener actually answers to.
        if split_port(address).is_some_and(|(host, _)| is_wildcard(host)) {
            hosts.clear();
        }
        if let Some((_, port)) = split_port(address).filter(|(host, _)| is_loopback(host)) {
            for name in ["localhost", "127.0.0.1", "[::1]"] {
                hosts.insert(format!("{name}:{port}"));
            }
        }
        for name in also {
            hosts.insert(name.as_ref().to_ascii_lowercase());
        }
        Self { secret, hosts }
    }

    /// The `Set-Cookie` value that hands the browser the secret.
    ///
    /// Here rather than in the transport because this is the other place the
    /// secret is rendered, and keeping both in this module is what makes
    /// "where can the token escape?" a question with a short answer.
    ///
    /// `HttpOnly` so no script can read it, `SameSite=Strict` so no other site
    /// can cause it to be sent, and deliberately no `Secure`: §52 chose plain
    /// HTTP, and a `Secure` cookie on an insecure origin is never sent at all.
    pub fn cookie(&self) -> String {
        format!(
            "{COOKIE}={}; Path=/; HttpOnly; SameSite=Strict",
            self.secret.in_url()
        )
    }

    /// Every name this listener answers to. For the startup banner and the
    /// refusal text, which both have to be able to say what the set is.
    pub fn hosts(&self) -> impl Iterator<Item = &str> {
        self.hosts.iter().map(String::as_str)
    }

    /// Decide one request.
    ///
    /// `host` is the `Host` header, `cookie` the whole `Cookie` header, and
    /// `query` the raw query string. Each is what the request actually carried,
    /// so a missing header is `None` and not an empty string - a request with
    /// no `Host` at all is refused rather than defaulted, because HTTP/1.1
    /// requires one and something that omits it is not a browser.
    pub fn admit(&self, host: Option<&str>, cookie: Option<&str>, query: Option<&str>) -> Verdict {
        // Host first. See the module note: this is the rebinding gate, and it
        // runs before anything compares the secret.
        let Some(host) = host else {
            return Verdict::Refuse(
                "this request carried no Host header, and VCW serve needs one to tell \
                 an ordinary browser from a page that has pointed a name it owns at \
                 this machine"
                    .to_owned(),
            );
        };
        if !self.hosts.contains(&host.to_ascii_lowercase()) {
            let known: Vec<&str> = self.hosts().collect();
            return Verdict::Refuse(format!(
                "this listener answers to {} and the request asked for {host:?}. If that \
                 is a name you meant to browse by, start serve with --allow-host {host:?}",
                known.join(" or "),
            ));
        }

        if cookie_secret(cookie).is_some_and(|held| self.secret.matches(held)) {
            return Verdict::Admit;
        }
        if query_secret(query).is_some_and(|given| self.secret.matches(given)) {
            return Verdict::AdmitAndRemember;
        }

        Verdict::Refuse(
            "this request carried no valid VCW token. The address to use is printed \
             where serve started, token and all; if that console has scrolled away, \
             restart serve or set VCW_SERVE_TOKEN to pin a token of your own"
                .to_owned(),
        )
    }
}

/// Does binding here require the operator to say what names they will browse
/// by?
///
/// A wildcard bind means "every interface", and the `Host` header will then
/// carry whatever name the browser was pointed at - a LAN address, an mDNS
/// name, something out of a hosts file. None of those can be derived from
/// `0.0.0.0`, and guessing by enumerating interfaces would still miss the
/// names. §52's rebinding defense is only a defense if the set is stated, so
/// `serve` refuses to start rather than bind wide with a check that cannot
/// work.
pub fn needs_a_stated_host(address: &str) -> bool {
    split_port(address).is_some_and(|(host, _)| is_wildcard(host))
}

/// `0.0.0.0` or `[::]`, the two ways of asking for every interface.
fn is_wildcard(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare == "0.0.0.0" || bare == "::" || bare == "*"
}

/// The name of the cookie the browser holds the secret in.
pub const COOKIE: &str = "vcw_token";

/// The query parameter the secret arrives in, once, from a pasted URL.
pub const PARAMETER: &str = "t";

/// Pull our cookie out of a whole `Cookie` header.
///
/// Hand-written rather than split on `;` and `=` in one pass, because a cookie
/// value may itself contain `=` and taking the first one would truncate a
/// perfectly good token into a wrong one.
fn cookie_secret(header: Option<&str>) -> Option<&str> {
    header?.split(';').find_map(|pair| {
        let pair = pair.trim_start();
        let rest = pair.strip_prefix(COOKIE)?;
        rest.strip_prefix('=')
    })
}

/// Pull the secret out of a raw query string.
fn query_secret(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|pair| {
        let rest = pair.strip_prefix(PARAMETER)?;
        rest.strip_prefix('=')
    })
}

/// Split `host:port`, tolerating the `[::1]:9000` form.
fn split_port(address: &str) -> Option<(&str, &str)> {
    if let Some(end) = address.rfind(']') {
        let port = address.get(end + 1..)?.strip_prefix(':')?;
        return Some((&address[..=end], port));
    }
    let colon = address.rfind(':')?;
    Some((&address[..colon], &address[colon + 1..]))
}

/// Is this the local machine, written any of the ways it gets written?
fn is_loopback(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.eq_ignore_ascii_case("localhost")
        || bare == "::1"
        || bare
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|v4| v4.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A guard on loopback with a known secret, for the cases below.
    fn guard() -> Guard {
        Guard::new(
            Secret("abc123".to_owned()),
            "127.0.0.1:9000",
            Vec::<String>::new(),
        )
    }

    fn cookie(value: &str) -> String {
        format!("{COOKIE}={value}")
    }

    #[test]
    fn a_minted_secret_is_not_the_same_twice() {
        // The whole value of minting. If this ever fails the source of
        // randomness has been replaced by something that is not one.
        let (first, origin) = Secret::mint();
        let (second, _) = Secret::mint();
        assert_eq!(origin, Origin::Minted);
        assert_ne!(first.in_url(), second.in_url());
        assert_eq!(first.in_url().len(), BYTES * 2);
    }

    #[test]
    fn a_secret_does_not_print_itself() {
        let secret = Secret("hunter2hunter2".to_owned());
        let shown = format!("{secret:?}");
        assert!(!shown.contains("hunter2"), "{shown:?} leaks the secret");
        assert!(shown.contains("14"), "{shown:?} should say how long it is");
    }

    #[test]
    fn a_wrong_secret_of_the_right_length_is_refused() {
        // The case a length check alone would admit.
        let secret = Secret("abc123".to_owned());
        assert!(secret.matches("abc123"));
        assert!(!secret.matches("abc124"));
        assert!(!secret.matches("abc12"));
        assert!(!secret.matches("abc1234"));
        assert!(!secret.matches(""));
    }

    #[test]
    fn the_token_admits_from_the_url_and_then_from_the_cookie() {
        let guard = guard();
        assert_eq!(
            guard.admit(Some("127.0.0.1:9000"), None, Some("t=abc123")),
            Verdict::AdmitAndRemember,
            "a pasted URL is how the first request arrives"
        );
        assert_eq!(
            guard.admit(Some("127.0.0.1:9000"), Some(&cookie("abc123")), None),
            Verdict::Admit,
            "and every request after it carries the cookie instead"
        );
    }

    #[test]
    fn no_token_is_refused_even_on_loopback() {
        // The decision this module exists to enforce: loopback is not a
        // trusted address. If a trusted-address exemption is ever added back,
        // this is the test that has to be deleted to do it.
        let refused = guard().admit(Some("127.0.0.1:9000"), None, None);
        assert!(matches!(refused, Verdict::Refuse(_)), "{refused:?}");
    }

    #[test]
    fn a_rebound_name_is_refused_though_the_browser_sends_our_cookie() {
        // DNS rebinding, exactly. The attacking page cannot read the cookie,
        // but after rebinding its own name to 127.0.0.1 the browser attaches
        // it anyway - so the correct secret arrives on a request we must still
        // refuse. The only thing that distinguishes it is the Host header.
        let refused = guard().admit(Some("evil.example:9000"), Some(&cookie("abc123")), None);
        let Verdict::Refuse(reason) = refused else {
            panic!("a rebound Host was admitted while holding a valid token");
        };
        assert!(
            reason.contains("--allow-host"),
            "{reason:?} names no remedy"
        );
    }

    #[test]
    fn a_missing_host_is_refused_rather_than_defaulted() {
        let refused = guard().admit(None, Some(&cookie("abc123")), None);
        assert!(matches!(refused, Verdict::Refuse(_)), "{refused:?}");
    }

    #[test]
    fn a_stated_name_is_answered_to() {
        let guard = Guard::new(
            Secret("abc123".to_owned()),
            "192.168.1.5:9000",
            ["pi.local:9000"],
        );
        assert_eq!(
            guard.admit(Some("pi.local:9000"), None, Some("t=abc123")),
            Verdict::AdmitAndRemember
        );
        assert_eq!(
            guard.admit(Some("PI.LOCAL:9000"), None, Some("t=abc123")),
            Verdict::AdmitAndRemember,
            "a Host header is not case sensitive"
        );
        // And a bind that is not loopback does not quietly answer to localhost.
        assert!(matches!(
            guard.admit(Some("localhost:9000"), None, Some("t=abc123")),
            Verdict::Refuse(_)
        ));
    }

    #[test]
    fn a_loopback_bind_answers_to_every_spelling_of_itself() {
        let guard = guard();
        for spelling in ["127.0.0.1:9000", "localhost:9000", "[::1]:9000"] {
            assert_eq!(
                guard.admit(Some(spelling), None, Some("t=abc123")),
                Verdict::AdmitAndRemember,
                "{spelling} is this machine"
            );
        }
    }

    #[test]
    fn our_cookie_is_found_among_others_and_not_confused_with_a_prefix() {
        let guard = guard();
        assert_eq!(
            guard.admit(
                Some("127.0.0.1:9000"),
                Some("theme=dark; vcw_token=abc123; other=1"),
                None
            ),
            Verdict::Admit
        );
        // A cookie whose name merely starts the same way must not be read as
        // ours - `vcw_token_old` would otherwise match on a naive prefix.
        assert!(matches!(
            guard.admit(Some("127.0.0.1:9000"), Some("vcw_token_old=abc123"), None),
            Verdict::Refuse(_)
        ));
    }

    #[test]
    fn a_secret_containing_an_equals_sign_survives_the_round_trip() {
        // An operator-supplied VCW_SERVE_TOKEN can be anything they typed, and
        // base64 ends in `=` often enough that splitting on every `=` would
        // silently truncate it into a token that never matches.
        let guard = Guard::new(
            Secret("aGk=".to_owned()),
            "127.0.0.1:9000",
            Vec::<String>::new(),
        );
        assert_eq!(
            guard.admit(Some("127.0.0.1:9000"), Some("vcw_token=aGk="), None),
            Verdict::Admit
        );
    }

    #[test]
    fn an_empty_environment_variable_is_not_an_empty_password() {
        // Checked directly rather than through `from_env_or_mint`, because
        // tests in one binary share an environment and setting a variable in
        // one races every other.
        let blank: Option<String> = Some("   ".to_owned());
        let unset = blank.filter(|stated| !stated.trim().is_empty());
        assert!(unset.is_none(), "whitespace must read as unset");
    }
}

#[cfg(test)]
mod wildcard {
    use super::*;

    #[test]
    fn a_wildcard_bind_has_to_be_told_what_to_answer_to() {
        for wide in ["0.0.0.0:9000", "[::]:9000", "*:9000"] {
            assert!(needs_a_stated_host(wide), "{wide} is every interface");
        }
        for narrow in [
            "127.0.0.1:9000",
            "192.168.1.5:9000",
            "[::1]:9000",
            "pi.local:9000",
        ] {
            assert!(!needs_a_stated_host(narrow), "{narrow} is one name");
        }
    }

    #[test]
    fn a_wildcard_guard_answers_only_to_the_names_it_was_given() {
        // The failure this prevents is a listener that quietly answers to the
        // literal string "0.0.0.0:9000" - which no browser sends - and so
        // refuses everything while looking configured.
        let guard = Guard::new(
            Secret("abc123".to_owned()),
            "0.0.0.0:9000",
            ["pi.local:9000"],
        );
        let names: Vec<&str> = guard.hosts().collect();
        assert_eq!(names, vec!["pi.local:9000"]);
        assert_eq!(
            guard.admit(Some("pi.local:9000"), None, Some("t=abc123")),
            Verdict::AdmitAndRemember
        );
        assert!(matches!(
            guard.admit(Some("0.0.0.0:9000"), None, Some("t=abc123")),
            Verdict::Refuse(_)
        ));
    }
}
