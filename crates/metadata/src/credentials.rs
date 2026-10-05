/*
 *  credentials.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Provider credentials, which never touch a project file (§39).
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

//! Provider credentials, which never touch a project file (§39).
//!
//! §39 says credentials shall not be stored in project files. That is easy to obey
//! by accident today and easy to break by accident later, when someone adds
//! `#[derive(Serialize)]` to a settings struct that happens to hold a token. So the
//! rule is enforced by the type rather than by discipline: [`Token`] does not
//! implement `Serialize`, `Display` or a revealing `Debug`, and the only way to see
//! what is inside it is to call [`Token::expose`], which is easy to search for.
//!
//! # Where a token comes from
//!
//! The environment, or a settings store outside the project. [`Credentials::from_env`]
//! is the whole of it today; when §39's settings work lands it gains a second source
//! and nothing else about this module changes.
//!
//! # Why the token is a header and not a query parameter
//!
//! VRipr put the Discogs token in the URL, which every provider still accepts. VCW
//! sends it as an `Authorization` header instead, because a URL is not private: it
//! is the cache key, it is what a log line prints, and it is what a person pastes
//! into a bug report. Keeping the credential out of it means the cache and the logs
//! cannot leak something they never saw.

use std::fmt;

/// A provider credential.
///
/// Deliberately hard to leak: no `Display`, no `Serialize`, and a `Debug` that
/// prints the length rather than the value.
///
/// ```
/// use vcw_metadata::credentials::Token;
///
/// let token = Token::new("sekrit").expect("a non-empty token");
/// assert_eq!(token.expose(), "sekrit");
/// assert_eq!(format!("{token:?}"), "Token(6 characters, redacted)");
/// ```
///
/// A token cannot be serialized, which is what keeps §39 true by construction:
///
/// ```compile_fail
/// use vcw_metadata::credentials::Token;
///
/// fn stored_in_a_settings_file<T: serde::Serialize>(_value: &T) {}
///
/// let token = Token::new("sekrit").expect("a non-empty token");
/// // Token does not implement Serialize, so this does not compile.
/// stored_in_a_settings_file(&token);
/// ```
///
/// The same call with something that *is* serializable compiles, which is what
/// makes the failure above evidence about `Token` rather than about the snippet:
///
/// ```
/// fn stored_in_a_settings_file<T: serde::Serialize>(_value: &T) {}
///
/// stored_in_a_settings_file(&String::from("not a credential"));
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// A token from whatever the settings or the environment gave us.
    ///
    /// `None` for anything empty or whitespace, because an empty credential is
    /// the same situation as a missing one and callers should only have to
    /// handle it once.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(Self(trimmed.to_owned()))
        }
    }

    /// The credential itself. Every caller is a place to audit.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// How long it is, which is all anything other than a request may know.
    #[must_use]
    pub fn characters(&self) -> usize {
        self.0.chars().count()
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Token({} characters, redacted)", self.characters())
    }
}

/// The name of the environment variable holding the Discogs personal token.
pub const DISCOGS_TOKEN_VAR: &str = "VCW_DISCOGS_TOKEN";

/// The name of the environment variable holding the AcoustID client key.
pub const ACOUSTID_KEY_VAR: &str = "VCW_ACOUSTID_KEY";

/// The name of the environment variable holding the contact for the user agent.
pub const CONTACT_VAR: &str = "VCW_CONTACT";

/// What VCW has been given to identify itself with.
///
/// Cloneable and cheap, so a provider can hold one; not serializable, so a
/// settings file cannot hold one by accident.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credentials {
    discogs: Option<Token>,
    acoustid: Option<Token>,
    contact: Option<String>,
}

impl Credentials {
    /// No credentials at all. MusicBrainz still works; Discogs does not.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Whatever the environment offers.
    ///
    /// Absent and empty variables are both simply absent. Reading the environment
    /// is the only I/O in this crate that is not a request.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            discogs: std::env::var(DISCOGS_TOKEN_VAR).ok().and_then(Token::new),
            acoustid: std::env::var(ACOUSTID_KEY_VAR).ok().and_then(Token::new),
            contact: std::env::var(CONTACT_VAR)
                .ok()
                .map(|c| c.trim().to_owned())
                .filter(|c| !c.is_empty()),
        }
    }

    /// The same credentials with a Discogs token.
    #[must_use]
    pub fn with_discogs(mut self, token: Token) -> Self {
        self.discogs = Some(token);
        self
    }

    /// The same credentials with an AcoustID key.
    #[must_use]
    pub fn with_acoustid(mut self, token: Token) -> Self {
        self.acoustid = Some(token);
        self
    }

    /// The same credentials with a contact address for the user agent.
    ///
    /// MusicBrainz asks that an application identify itself and say how to get in
    /// touch if it misbehaves (§40). A contact is not a credential - it is public
    /// by design - but it arrives from the same settings, so it lives here.
    #[must_use]
    pub fn with_contact(mut self, contact: impl Into<String>) -> Self {
        let contact = contact.into();
        let trimmed = contact.trim();
        self.contact = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        };
        self
    }

    /// The Discogs token, if there is one.
    #[must_use]
    pub fn discogs(&self) -> Option<&Token> {
        self.discogs.as_ref()
    }

    /// The AcoustID key, if there is one.
    #[must_use]
    pub fn acoustid(&self) -> Option<&Token> {
        self.acoustid.as_ref()
    }

    /// The contact address, if one was configured.
    #[must_use]
    pub fn contact(&self) -> Option<&str> {
        self.contact.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_credential_is_the_same_as_a_missing_one() {
        assert_eq!(Token::new(""), None);
        assert_eq!(Token::new("   \t\n "), None);
        assert_eq!(
            Token::new("  abc  ").map(|t| t.expose().to_owned()),
            Some("abc".to_owned()),
            "and a pasted token keeps none of the whitespace around it"
        );
    }

    #[test]
    fn a_token_does_not_print_itself() {
        let token = Token::new("a-real-looking-token").expect("a token");
        let debug = format!("{token:?}");
        assert!(
            !debug.contains("a-real-looking-token"),
            "Debug leaked the token: {debug}"
        );
        assert_eq!(debug, "Token(20 characters, redacted)");
    }

    #[test]
    fn credentials_do_not_print_themselves_either() {
        let credentials = Credentials::none()
            .with_discogs(Token::new("discogs-secret").expect("a token"))
            .with_acoustid(Token::new("acoustid-secret").expect("a token"))
            .with_contact("someone@example.invalid");
        let debug = format!("{credentials:?}");
        assert!(!debug.contains("discogs-secret"), "{debug}");
        assert!(!debug.contains("acoustid-secret"), "{debug}");
        assert!(
            debug.contains("someone@example.invalid"),
            "a contact is public by design, and seeing it in a log is the point"
        );
    }

    #[test]
    fn a_blank_contact_is_no_contact() {
        let credentials = Credentials::none().with_contact("   ");
        assert_eq!(credentials.contact(), None);
        assert_eq!(
            Credentials::none()
                .with_contact(" me@example.invalid ")
                .contact(),
            Some("me@example.invalid")
        );
    }

    #[test]
    fn nothing_configured_is_a_usable_state() {
        let credentials = Credentials::none();
        assert_eq!(credentials.discogs(), None);
        assert_eq!(credentials.acoustid(), None);
        assert_eq!(credentials.contact(), None);
    }
}
