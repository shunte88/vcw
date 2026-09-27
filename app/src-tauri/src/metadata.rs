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
//! # Why these two are async and the rest are not
//!
//! A search is a network round trip, and §40 boxes it at ten seconds. A
//! synchronous command would hold the main thread for that long, so these run
//! on Tauri's runtime and do the blocking part in
//! [`spawn_blocking`](tauri::async_runtime::spawn_blocking). They return a
//! value rather than emitting events, unlike the export and the detection pass,
//! because a person pressed a key and is looking at the result: a dialog that
//! filled itself in from an event would have to cope with an answer to a
//! question it had already replaced.
//!
//! # Cancellation
//!
//! [`Shell::searching`] holds the in-flight [`Cancel`]. Starting a search
//! cancels whichever one was already running, which is what a person retyping
//! an album title means, and is the reason a token is held at all.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};
use vcw_contract::command::{Search, Selection};
use vcw_contract::view::{Accepted, Candidate};
use vcw_core::identity;
use vcw_metadata::credentials::Credentials;
use vcw_metadata::{Cancel, ProviderId, Query, Setup};
use vcw_project::Project;

use crate::config;
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
#[tauri::command]
pub(crate) async fn search_metadata(
    shell: State<'_, Shell>,
    app: AppHandle,
    search: Search,
) -> Result<Vec<Candidate>, Error> {
    let settings = config::load(&app)?.metadata;
    let (musicbrainz, discogs) = which(&search, settings.musicbrainz, settings.discogs)?;

    let query = query(&search)?;
    let mut setup = Setup::new()
        .online(settings.online)
        .only(musicbrainz, discogs);
    if let Some(directory) = cache(&app) {
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

    let found = tauri::async_runtime::spawn_blocking(move || {
        let credentials = Credentials::from_env();
        let providers = setup.providers(&credentials);
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
    })
    .await
    .map_err(|why| Error::Invalid {
        field: "provider".to_owned(),
        why: format!("the search thread did not finish: {why}"),
    })??;

    Ok(found)
}

/// Fetches a candidate and writes it into the project. §35's `select_release`.
///
/// # Errors
///
/// [`Error::NoProject`] with nothing open, [`Error::Invalid`] for a provider
/// name that is not one of the two, [`Error::Metadata`] if the fetch refused,
/// and [`Error::Project`] if the write fails.
#[tauri::command]
pub(crate) async fn select_release(
    shell: State<'_, Shell>,
    app: AppHandle,
    selection: Selection,
) -> Result<Accepted, Error> {
    let path = shell.project_path()?;
    let settings = config::load(&app)?.metadata;
    let provider = provider_id(&selection.provider)?;

    let mut setup = Setup::new().online(settings.online).only(
        provider == ProviderId::MusicBrainz,
        provider == ProviderId::Discogs,
    );
    if let Some(directory) = cache(&app) {
        setup = setup.with_cache(directory);
    }

    let id = selection.id.clone();
    let found = tauri::async_runtime::spawn_blocking(move || {
        let credentials = Credentials::from_env();
        let providers = setup.providers(&credentials);
        let Some(one) = providers.first() else {
            // Unreachable through `provider_id`, which only returns a provider
            // `Setup` will then build. Said out loud rather than unwrapped,
            // because the two would drift silently if a third provider arrived.
            return Err(vcw_metadata::Error::NothingToSearch { provider });
        };
        one.fetch(&id, &Cancel::new())
    })
    .await
    .map_err(|why| Error::Invalid {
        field: "id".to_owned(),
        why: format!("the fetch thread did not finish: {why}"),
    })??;

    // Opened writable only once the fetch has come back, so a provider timing
    // out does not hold a write lock on the project for ten seconds.
    let mut project = Project::open(&path)?;
    let applied = identity::accept(&mut project, &found)?;
    project.close()?;
    Ok(Accepted::of(&found, &applied))
}

/// Where provider answers are cached (§40).
///
/// `None` if the platform has no cache directory, which leaves the client with
/// its in-process cache rather than failing: a search that cannot be cached is
/// still a search.
fn cache(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_cache_dir()
        .ok()
        .map(|dir| dir.join("providers"))
}

/// Which providers to ask: what the request said, narrowed by what §39 allows.
///
/// A request naming a provider the settings have turned off is honoured rather
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
/// asking a provider for everything it has is not a search - that judgement is
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
            why: "give at least one of an artist, an album, a catalogue number or a barcode"
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
