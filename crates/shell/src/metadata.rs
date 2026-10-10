/*
 *  metadata.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  §28's two commands: asking a provider, and accepting what it said.
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

//! §28's two commands: asking a provider, and accepting what it said.
//!
//! The two verbs §35 names that WP-15 declared and refused. What it was waiting
//! for was a decision about where the provider client, the disk cache and the
//! §39 credential live, and this module is the answer:
//!
//! - **The client is built per call, not held.** A provider is cheap to
//!   construct - [`Setup::providers`] is six field assignments and a `Genres`
//!   clone - and building it fresh means a settings change takes effect on the
//!   next search rather than on the next launch. A long-lived client would be a
//!   copy of §39 in the shell, which is the thing §2 exists to prevent.
//! - **The cache is on disk, under Tauri's cache directory.** §40 wants the
//!   same question not asked twice, and a cache in the process would be empty
//!   every time the window opens. One directory for the application rather than
//!   one per project: two projects of the same pressing should share the answer.
//! - **The credential is read from the environment at the call site and never
//!   stored.** [`Credentials::from_env`] on each call, passed to `providers`,
//!   dropped when the command returns. Nothing in [`Shell`] holds a token, and
//!   the only command that reports on credentials is
//!   [`crate::config::credentials`], which reports a length.
//!
//! # The contact is a setting, and the settings file is where a person put it
//!
//! §40 asks the application to identify itself and MusicBrainz rate limits
//! harder, and sometimes refuses outright, when it does not. A contact address
//! is public by design, so unlike a token it is allowed in the settings file -
//! and [`crate::config`] has had a field for it since WP-16. Nothing read it:
//! the user agent was built from [`Credentials::from_env`] alone, so typing an
//! address into the settings panel changed nothing and the only way to be
//! identified was an environment variable nobody is told about.
//!
//! `credentials` is where the two meet. The settings file wins where it has
//! an address, because it is the one a person can see and change from inside
//! the window; `VCW_CONTACT` is the fallback, and is what the CLI runs on.
//!
//! # Why these two take their time, and whose problem that is
//!
//! A search is a network round trip, and §40 boxes it at ten seconds. Both
//! functions here block for that long, and getting the waiting off whatever
//! thread must stay answerable belongs to the host: `app/src-tauri` wraps each
//! of them in `spawn_blocking` because a synchronous `#[tauri::command]` runs
//! on the main thread and would freeze the window, and §52's listener is
//! already a thread per connection and wraps nothing.
//!
//! They return a value rather than emitting events, unlike the export and the
//! detection pass, because a person pressed a key and is looking at the
//! result: a dialog that filled itself in from an event would have to cope
//! with an answer to a question it had already replaced.
//!
//! # The cover comes back with the release
//!
//! §28 says a release has artwork and the project has had somewhere to put it
//! since WP-05, but nothing fetched it: `vcw release artwork` took a file off
//! disk and the window had no path at all, so accepting a release gave you the
//! title, the label and the catalog number and left the cover behind.
//! [`select_release`] now downloads the front cover on the same thread as the
//! release fetch and stores it in the project, where it is part of the
//! document and gets backed up with it.
//!
//! A cover that will not download is not an error. The release is right either
//! way, and losing a confirmed identification because an image host returned a
//! 404 would be the tail wagging the dog. [`Accepted::artwork`] reports zero
//! bytes and the panel says so.
//!
//! # Cancellation
//!
//! [`Shell::searching`] holds the in-flight [`Cancel`]. Starting a search
//! cancels whichever one was already running, which is what a person retyping
//! an album title means, and is the reason a token is held at all.

use std::path::PathBuf;

use vcw_contract::command::{Search, Selection};
use vcw_contract::settings::Metadata;
use vcw_contract::view::{Accepted, Candidate};
use vcw_core::identity;
use vcw_metadata::credentials::Credentials;
use vcw_metadata::{Cancel, ProviderId, Query, Setup, artwork};
use vcw_project::{Project, release};

use crate::config;
use crate::host::Hosted;
use crate::state::{Error, Shell};

/// Asks the configured providers about this record. §35's `search_metadata`.
///
/// Every provider that answered contributes, and a provider that refused is not
/// fatal: a machine with no Discogs token still gets MusicBrainz's answer. A
/// search in which *every* provider refused comes back as the first refusal,
/// because an empty list would read as "this record is not in any database".
///
/// # Errors
///
/// [`Error::Invalid`] for a provider name that is not one of the two, or a
/// query with nothing in it, and [`Error::Metadata`] if every provider refused.
pub fn search_metadata(
    shell: &Shell,
    host: &Hosted,
    search: Search,
) -> Result<Vec<Candidate>, Error> {
    let settings = config::load(host)?.metadata;
    let (musicbrainz, discogs) = which(&search, settings.musicbrainz, settings.discogs)?;

    let query = query(&search)?;
    let identified = credentials(&settings);
    let mut setup = Setup::new()
        .online(settings.online)
        .only(musicbrainz, discogs);
    if let Some(directory) = cache(host) {
        setup = setup.with_cache(directory);
    }

    // Replaces whatever was running. Held rather than dropped, so a search
    // still in flight when the next one starts stops paying for a network
    // round trip nobody is waiting for.
    let cancel = Cancel::new();
    if let Some(previous) = shell
        .searching
        .lock()
        .expect("the search mutex")
        .replace(cancel.clone())
    {
        previous.cancel();
    }

    // A plain blocking call. It was a `spawn_blocking` while this lived in the
    // shell, because a synchronous `#[tauri::command]` runs on the main thread
    // and a provider round trip would freeze the window; that is the host's
    // problem rather than this function's, and `app/src-tauri` still does it
    // on the way in. §52's listener is a thread per connection and needs
    // nothing.
    let hunt = || -> Result<Vec<Candidate>, vcw_metadata::Error> {
        let providers = setup.providers(&identified);
        let mut candidates = Vec::new();
        let mut refusals = Vec::new();
        for provider in &providers {
            match provider.search(&query, &cancel) {
                Ok(hits) => candidates.extend(
                    hits.iter()
                        .map(|hit| Candidate::found(name(provider.id()), hit)),
                ),
                Err(error) => refusals.push(error),
            }
        }
        if candidates.is_empty()
            && let Some(first) = refusals.into_iter().next()
        {
            return Err(first);
        }
        Ok(candidates)
    };

    Ok(hunt()?)
}

/// Fetches a candidate and writes it into the project. §35's `select_release`.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] for a provider
/// name that is not one of the two, [`Error::Metadata`] if the fetch refused,
/// and [`Error::Project`] if the write fails.
pub fn select_release(
    shell: &Shell,
    host: &Hosted,
    selection: Selection,
) -> Result<Accepted, Error> {
    let path = shell.project_path()?;
    let settings = config::load(host)?.metadata;
    let provider = provider_id(&selection.provider)?;

    let mut setup = Setup::new().online(settings.online).only(
        provider == ProviderId::MusicBrainz,
        provider == ProviderId::Discogs,
    );
    if let Some(directory) = cache(host) {
        setup = setup.with_cache(directory);
    }

    let id = selection.id.clone();
    let identified = credentials(&settings);
    type Fetched = (vcw_metadata::Release, Option<artwork::Artwork>);
    let fetch = || -> Result<Fetched, vcw_metadata::Error> {
        let providers = setup.providers(&identified);
        let Some(one) = providers.first() else {
            // Unreachable through `provider_id`, which only returns a provider
            // `Setup` will then build. Said out loud rather than unwrapped,
            // because the two would drift silently if a third provider arrived.
            return Err(vcw_metadata::Error::NothingToSearch { provider });
        };
        let cancel = Cancel::new();
        let found = one.fetch(&id, &cancel)?;
        // Through the provider's own client, so the download is counted
        // against the rate limit the release fetch just used and goes out
        // under the same user agent. Discarded on failure for the reason in
        // the module header.
        let cover = artwork::fetch_front(
            &setup.client(provider, &identified),
            &found.artwork,
            &cancel,
        )
        .ok()
        .flatten();
        Ok((found, cover))
    };
    let (found, cover) = fetch()?;

    // Opened writable only once the fetch has come back, so a provider timing
    // out does not hold a write lock on the project for ten seconds.
    let mut project = Project::open(&path)?;
    let applied = identity::accept(&mut project, &found)?;
    let stored = match &cover {
        Some(image) => {
            release::put_artwork(
                &mut project,
                release::Artwork::FRONT,
                image.format.mime(),
                &image.bytes,
                Some(&image.url),
            )?;
            image.len()
        }
        None => 0,
    };
    project.close()?;
    Ok(Accepted::of(&found, &applied).with_artwork(stored))
}

/// The credentials a provider call runs with: §39's environment, plus the
/// contact §39's settings file is allowed to hold.
///
/// Only the contact. A token in a settings file is the thing §39 forbids and
/// [`vcw_metadata::credentials::Credentials`] is not serializable so that one
/// cannot arrive there by accident; an address is public by design - it is
/// sent in a header to every provider on every request - and so it is a
/// setting like any other.
fn credentials(settings: &Metadata) -> Credentials {
    let environment = Credentials::from_env();
    match settings.contact.as_deref().map(str::trim) {
        Some(contact) if !contact.is_empty() => environment.with_contact(contact),
        _ => environment,
    }
}

/// Where provider answers are cached (§40).
///
/// `None` if the platform has no cache directory, which leaves the client with
/// its in-process cache rather than failing: a search that cannot be cached is
/// still a search.
fn cache(host: &Hosted) -> Option<PathBuf> {
    host.cache_dir().map(|dir| dir.join("providers"))
}

/// Which providers to ask: what the request said, narrowed by what §39 allows.
///
/// A request naming a provider the settings have turned off is honored rather
/// than refused. §39 is a default and a request is specific, and a person who
/// clicked "ask Discogs" has said what they want more recently than the
/// settings panel did.
fn which(search: &Search, musicbrainz: bool, discogs: bool) -> Result<(bool, bool), Error> {
    match search.provider.as_deref() {
        None => {
            if !(musicbrainz || discogs) {
                return Err(Error::Invalid {
                    field: "provider".to_owned(),
                    why: "both providers are turned off in settings - turn one on, or name one"
                        .to_owned(),
                });
            }
            Ok((musicbrainz, discogs))
        }
        Some(named) => {
            let id = provider_id(named)?;
            Ok((id == ProviderId::MusicBrainz, id == ProviderId::Discogs))
        }
    }
}

/// The provider a name means.
fn provider_id(name: &str) -> Result<ProviderId, Error> {
    match name.trim().to_ascii_lowercase().as_str() {
        "musicbrainz" | "mb" => Ok(ProviderId::MusicBrainz),
        "discogs" => Ok(ProviderId::Discogs),
        other => Err(Error::Invalid {
            field: "provider".to_owned(),
            why: format!("{other:?} is not a provider - discogs or musicbrainz"),
        }),
    }
}

/// The wire spelling of a provider, which is what a [`Selection`] sends back.
const fn name(id: ProviderId) -> &'static str {
    match id {
        ProviderId::MusicBrainz => "musicbrainz",
        ProviderId::Discogs => "discogs",
        ProviderId::AcoustId => "acoustid",
    }
}

/// The query a request means.
///
/// §28's "which combinations work" is the provider's knowledge, so nothing here
/// decides whether a query is a *good* one. An empty query is refused, because
/// asking a provider for everything it has is not a search - that judgment is
/// [`Query::is_empty`]'s, not this function's.
fn query(search: &Search) -> Result<Query, Error> {
    let mut query = Query::new().vinyl_only(true);
    if let Some(artist) = &search.artist {
        query = query.artist(artist);
    }
    if let Some(album) = &search.album {
        query = query.album(album);
    }
    if let Some(catalog) = &search.catalog {
        query = query.catalog(catalog);
    }
    if let Some(barcode) = &search.barcode {
        query = query.barcode(barcode);
    }
    if query.is_empty() {
        return Err(Error::Invalid {
            field: "artist".to_owned(),
            why: "give at least one of an artist, an album, a catalog number or a barcode"
                .to_owned(),
        });
    }
    Ok(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_provider_overrides_the_settings() {
        // The rule worth a test, because the other reading is defensible and
        // wrong: a person who clicked "ask Discogs" while Discogs is off in
        // settings gets Discogs, not a refusal explaining their own settings.
        let search = Search {
            provider: Some("discogs".to_owned()),
            ..Search::default()
        };
        assert_eq!(which(&search, true, false).expect("which"), (false, true));
    }

    #[test]
    fn the_settings_contact_reaches_the_user_agent() {
        // The defect this exists to keep fixed: the field was in the settings
        // panel, the panel saved it, and every request still went out saying
        // only where VCW lives. A test on `credentials` rather than on the
        // command, because the command needs a window.
        let settings = Metadata {
            contact: Some("  someone@example.invalid  ".to_owned()),
            ..Metadata::default()
        };
        let agent = vcw_metadata::net::user_agent(credentials(&settings).contact());
        assert!(
            agent.contains("someone@example.invalid"),
            "the settings contact should identify us: {agent}"
        );
        assert!(
            !agent.contains("  someone"),
            "and should be trimmed on the way: {agent}"
        );
    }

    #[test]
    fn a_blank_settings_contact_leaves_the_environment_alone() {
        // Blank rather than absent, which is what an input a person typed into
        // and then emptied leaves behind. Overriding with it would make the
        // panel able to *unset* `VCW_CONTACT`, which is not what clearing a
        // field means.
        for blank in [None, Some(String::new()), Some("   ".to_owned())] {
            let settings = Metadata {
                contact: blank.clone(),
                ..Metadata::default()
            };
            assert_eq!(
                credentials(&settings).contact(),
                Credentials::from_env().contact(),
                "a blank contact should change nothing: {blank:?}"
            );
        }
    }

    #[test]
    fn asking_nobody_is_refused_rather_than_answered_emptily() {
        let search = Search::default();
        let refused = which(&search, false, false).expect_err("nothing to ask");
        assert_eq!(refused.field(), Some("provider".to_owned()));
    }

    #[test]
    fn a_provider_nobody_has_is_named_in_the_refusal() {
        let refused = provider_id("allmusic").expect_err("not a provider");
        assert!(
            refused.to_string().contains("allmusic"),
            "{refused} does not quote what was sent"
        );
    }

    #[test]
    fn an_empty_query_is_refused_before_a_socket_is_opened() {
        let refused = query(&Search::default()).expect_err("nothing to search for");
        assert_eq!(refused.code(), "invalid-argument");
    }

    #[test]
    fn a_query_carries_every_criterion_the_request_gave() {
        let search = Search {
            artist: Some("Pole".to_owned()),
            catalog: Some("CHAIN 2".to_owned()),
            ..Search::default()
        };
        let query = query(&search).expect("a query");
        assert!(!query.is_empty());
        // Vinyl only, always: this is a vinyl capture workstation and a CD
        // pressing's tracklist has different positions.
        assert!(format!("{query:?}").contains("vinyl"), "{query:?}");
    }

    /// The provider names round-trip, which is what a [`Selection`] relies on.
    #[test]
    fn a_provider_name_survives_the_round_trip() {
        for id in [ProviderId::MusicBrainz, ProviderId::Discogs] {
            assert_eq!(provider_id(name(id)).expect("round trip"), id);
        }
    }
}
