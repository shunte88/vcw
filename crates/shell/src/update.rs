/*
 *  update.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Whether a newer VCW has been published (WP-28).
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

//! Whether a newer VCW has been published (WP-28).
//!
//! One GET to the forge's releases endpoint, read for a tag, compared with the
//! version this binary was built as. That is the whole of it, and the things it
//! deliberately does not do are the design:
//!
//! - **It does not update anything.** On Linux the copy on disk belongs to
//!   `dpkg` or to whoever unpacked it, on Windows to an installer, on macOS to a
//!   signed bundle. A program that writes over itself behind any of those leaves
//!   an installation its own package manager can no longer describe.
//! - **It does not fail.** [`check`] answers [`Update`] with `checked: false`
//!   where it could not find out - offline, a refusal, a body it could not read -
//!   because "I do not know whether you are current" is not news and a window
//!   with nothing to say should say nothing. The one thing it returns an error
//!   for is a settings file that will not parse, which is [`config::load`]'s
//!   error and is already how every other command behaves.
//! - **It does not ask unless the operator allows a network.** §40's switch is
//!   the one that decides, the same switch the providers obey, because a release
//!   check is a request to a stranger's server exactly like a lookup is. An
//!   offline build has no HTTP client compiled in at all.
//!
//! # The comparison
//!
//! A tag is `v0.3.4-alpha` and a version is `0.3.4`, so the two are compared on
//! their numeric parts: strip a leading `v`, cut at the first `-`, read up to
//! three dot-separated numbers. Anything that does not parse is not newer, which
//! is the safe direction - a tag nobody can read has no business raising a
//! banner.

use vcw_contract::view::{About, Update};
use vcw_metadata::net::{Request, Transport, user_agent};
use vcw_metadata::setup::Setup;

use crate::config;
use crate::host::Hosted;
use crate::state::Error;

/// How long the check waits before giving up.
///
/// Three seconds, against the ten a provider lookup gets. Nobody is waiting on
/// this answer: it runs once when the window opens, and a slow forge should
/// delay nothing a person can see.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// The page name [`check`] hands back for a build that is behind, before the
/// tag is appended: `release:v0.3.5-alpha`.
///
/// A tag and not a bare name because there is one release page per tag and a
/// build cannot enumerate the tags that will exist after it shipped;
/// `crate::library::support` is where that one exception is handled, and
/// `tests` below holds the two ends to each other.
pub const PAGE: &str = crate::library::RELEASE_PAGE;

/// Whether a newer release has been published.
///
/// # Errors
///
/// [`Error::Invalid`] only if the settings file will not parse. Everything a
/// network can do to a request comes back as `checked: false`.
pub fn check(host: &Hosted) -> Result<Update, Error> {
    let settings = config::load(host)?;
    let about = About::current();
    let transport = Setup::new().online(settings.metadata.online).transport();
    Ok(decide(
        &about,
        latest(
            transport.as_ref(),
            &about,
            settings.metadata.contact.as_deref(),
        ),
    ))
}

/// The newest published tag, or `None` where nothing could be learned.
///
/// Split from [`check`] so a test can hand it a recorded transport, and taking
/// `About` rather than a URL so the host it asks is derived from the same
/// manifest field the link is.
///
/// # Why not `releases/latest`
///
/// Because it answers 404 here. GitHub's idea of "latest" excludes anything
/// flagged as a pre-release, and every VCW release so far is one - the whole
/// 0.x line is `-alpha`. The obvious endpoint therefore reported, for a year
/// of releases, that there were none. The list endpoint is ordered newest
/// first and includes pre-releases, so this reads the first row of it that is
/// not a draft.
fn latest(transport: &dyn Transport, about: &About, contact: Option<&str>) -> Option<String> {
    let request = Request::get(endpoint(&about.repository)?)
        // GitHub answers 403 to a request with no user agent. The same one the
        // providers send, for the same reason: a service refusing VCW should be
        // able to see it is VCW.
        .with_header("User-Agent", user_agent(contact))
        .with_header("Accept", "application/vnd.github+json")
        .with_timeout(TIMEOUT);

    let response = transport.send(&request).ok()?;
    if !response.is_success() {
        tracing::debug!(status = response.status, "the release check was refused");
        return None;
    }
    let body: serde_json::Value = serde_json::from_slice(&response.body).ok()?;
    body.as_array()?
        .iter()
        // A draft is visible to whoever can edit the repository and to nobody
        // else, so this is for the author running their own build: a release
        // being prepared is not one anybody can download.
        .filter(|release| release.get("draft").and_then(serde_json::Value::as_bool) != Some(true))
        .find_map(|release| {
            let tag = release.get("tag_name")?.as_str()?.trim();
            (!tag.is_empty()).then(|| tag.to_owned())
        })
}

/// The API address for a repository page, or `None` for a forge this does not
/// know how to ask.
///
/// Only GitHub, because only GitHub is where VCW is published. A fork living
/// somewhere else gets `checked: false` rather than a request to a host that
/// would not understand it.
fn endpoint(repository: &str) -> Option<String> {
    let path = repository
        .trim_end_matches('/')
        .strip_prefix("https://github.com/")?;
    // Ten, because the answer wanted is the first row and a page of thirty is
    // thirty releases' worth of asset metadata to download for it.
    (path.split('/').count() == 2)
        .then(|| format!("https://api.github.com/repos/{path}/releases?per_page=10"))
}

/// What to report, given this build and whatever the forge said.
fn decide(about: &About, tag: Option<String>) -> Update {
    let Some(latest) = tag else {
        return Update {
            current: about.version.clone(),
            ..Update::default()
        };
    };
    let newer = numbers(&latest) > numbers(&about.version);
    let page = if newer {
        format!("{PAGE}{latest}")
    } else {
        String::new()
    };
    Update {
        current: about.version.clone(),
        latest,
        newer,
        checked: true,
        page,
    }
}

/// The numeric part of a version or a tag, as three numbers.
///
/// ponytail: numbers only, so `v0.3.4-beta` does not read as newer than
/// `v0.3.4-alpha`. VCW tags one pre-release per version, so there is nothing
/// for a pre-release ordering to separate; the upgrade is a semver crate, and
/// it is not here because it would be a dependency to compare three integers.
fn numbers(version: &str) -> [u32; 3] {
    let mut out = [0; 3];
    let core = version
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()
        .unwrap_or_default();
    for (slot, part) in out.iter_mut().zip(core.split('.')) {
        *slot = part.parse().unwrap_or_default();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcw_metadata::fixtures::Recorded;

    fn answering(body: &str) -> Recorded {
        let about = About::current();
        let url = endpoint(&about.repository).expect("VCW is published on GitHub");
        Recorded::new().json(url, body.as_bytes().to_vec())
    }

    #[test]
    fn the_page_it_names_is_one_the_build_can_open() {
        // The two halves of a link live in different files - the address in
        // `library`, the name here - and a banner naming a page that does not
        // exist is a refusal where a download should be. `support` is the one
        // that resolves it, and it is not called: it spawns a browser, and the
        // gate runs as somebody.
        let about = About::current();
        let page = decide(&about, Some("v9.9.9-alpha".to_owned())).page;
        let tag = page
            .strip_prefix(crate::library::RELEASE_PAGE)
            .unwrap_or_else(|| panic!("{page:?} is not a release page name"));
        assert_eq!(tag, "v9.9.9-alpha", "the tag has to reach the address");
    }

    #[test]
    fn a_later_tag_is_newer_and_an_equal_one_is_not() {
        let about = About::current();
        let mine = numbers(&about.version);
        let later = format!("v{}.{}.{}-alpha", mine[0], mine[1], mine[2] + 1);

        let behind = decide(&about, Some(later.clone()));
        assert!(behind.newer, "{later} is ahead of {}", about.version);
        assert_eq!(behind.page, format!("{PAGE}{later}"));
        assert!(behind.checked);

        let current = decide(&about, Some(format!("v{}-alpha", about.version)));
        assert!(!current.newer, "the tag of this build is not ahead of it");
        assert!(current.checked, "a build that is current was still checked");
        assert!(
            current.page.is_empty(),
            "a current build offered a download link"
        );
    }

    #[test]
    fn nothing_learned_is_not_the_same_as_being_current() {
        let answer = decide(&About::current(), None);
        assert!(!answer.checked);
        assert!(!answer.newer);
        assert!(answer.latest.is_empty());
        assert_eq!(answer.current, About::current().version);
    }

    #[test]
    fn an_offline_build_asks_nobody() {
        // §40 taken literally: the refusing transport is what `Setup` hands back
        // when the network is off, and the check has to survive it.
        let transport = Setup::new().transport();
        let about = About::current();
        assert_eq!(latest(transport.as_ref(), &about, None), None);
    }

    #[test]
    fn the_first_tag_in_the_list_is_the_answer() {
        // Newest first, which is the list endpoint's own order and the reason
        // it is usable as a stand-in for a `latest` that excludes everything
        // this project publishes.
        let transport = answering(
            r#"[{"tag_name": "v9.9.9-alpha", "draft": false},
                {"tag_name": "v9.9.8-alpha", "draft": false}]"#,
        );
        let about = About::current();
        assert_eq!(
            latest(&transport, &about, None).as_deref(),
            Some("v9.9.9-alpha")
        );
    }

    #[test]
    fn a_release_still_being_written_is_not_published() {
        // Only the author sees a draft, and only because they are signed in as
        // the person preparing it. Being told to update to something nobody
        // else can download is the bug this skips.
        let transport = answering(
            r#"[{"tag_name": "v9.9.9-alpha", "draft": true},
                {"tag_name": "v9.9.8-alpha", "draft": false}]"#,
        );
        let about = About::current();
        assert_eq!(
            latest(&transport, &about, None).as_deref(),
            Some("v9.9.8-alpha")
        );
    }

    #[test]
    fn a_body_with_no_tag_is_not_an_answer() {
        // The 404 `releases/latest` used to give, and an empty list, which is
        // what a repository with no release at all answers.
        let about = About::current();
        assert_eq!(
            latest(&answering(r#"{"message": "Not Found"}"#), &about, None),
            None
        );
        assert_eq!(latest(&answering("[]"), &about, None), None);
    }

    #[test]
    fn a_forge_this_cannot_ask_is_not_asked() {
        assert_eq!(endpoint("https://example.invalid/shunte88/vcw"), None);
        assert_eq!(endpoint("https://github.com/shunte88"), None);
        assert_eq!(
            endpoint("https://github.com/shunte88/vcw/").as_deref(),
            Some("https://api.github.com/repos/shunte88/vcw/releases?per_page=10")
        );
    }
}
