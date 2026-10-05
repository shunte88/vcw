/*
 *  metadata.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `vcw metadata` verb: search providers and read a release, with no UI (§4.5, §28).
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

//! The `vcw metadata` verb: search providers and read a release, with no UI
//! (§4.5, §28).
//!
//! §4.5 asks for the whole workflow to be drivable headless, and this is the part
//! of it that talks to the world. It is also the honest demonstration of §40's
//! offline promise: `--offline` is not a simulation, it is the same
//! [`Offline`](vcw_metadata::net::Offline) transport the application defaults
//! to, and the verb still runs, still prints, and still exits zero on the
//! subcommands that need nobody's permission.
//!
//! Credentials come from the environment and nowhere else (§39):
//! `VCW_DISCOGS_TOKEN` and, optionally, `VCW_CONTACT` for the user agent both
//! providers ask callers to identify themselves with. `vcw metadata credentials`
//! says which are set without printing any of them.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use vcw_metadata::credentials::Credentials;
use vcw_metadata::genres::Genres;
use vcw_metadata::net::user_agent;
use vcw_metadata::{Cancel, Candidate, Criterion, Provider, ProviderId, Query, Release, Setup};

/// Which providers to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Which {
    /// Both, MusicBrainz first because it needs no credential.
    Both,
    /// MusicBrainz only.
    Musicbrainz,
    /// Discogs only.
    Discogs,
}

/// What the verb was asked to do.
pub(crate) enum Args {
    /// Search for candidates.
    Search(Box<SearchArgs>),
    /// Fetch one release in full.
    Fetch(FetchArgs),
    /// Normalize genre names through the §32 table.
    Genres {
        /// The names to normalize, semicolon-delimited or one per argument.
        names: Vec<String>,
        /// A replacement mapping table.
        table: Option<PathBuf>,
        /// Machine-readable output.
        json: bool,
    },
    /// Say which credentials are configured, without printing them.
    Credentials,
}

/// Everything a search takes.
pub(crate) struct SearchArgs {
    /// §28's criteria, every one optional.
    pub(crate) artist: Option<String>,
    /// The release title.
    pub(crate) album: Option<String>,
    /// The label's catalog number, which usually identifies one pressing.
    pub(crate) catalog: Option<String>,
    /// The barcode on the sleeve.
    pub(crate) barcode: Option<String>,
    /// The record label.
    pub(crate) label: Option<String>,
    /// Year of release.
    pub(crate) year: Option<u32>,
    /// Country of release.
    pub(crate) country: Option<String>,
    /// Include formats that are not vinyl.
    pub(crate) all_formats: bool,
    /// How many results to ask for.
    pub(crate) limit: usize,
    /// Which providers to ask.
    pub(crate) which: Which,
    /// Refuse to use the network.
    pub(crate) offline: bool,
    /// Where to cache responses. Omit for no cache.
    pub(crate) cache: Option<PathBuf>,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Everything a fetch takes.
pub(crate) struct FetchArgs {
    /// The provider's own identifier: an MBID, or a Discogs release number.
    pub(crate) id: String,
    /// Which provider it belongs to.
    pub(crate) which: Which,
    /// Refuse to use the network.
    pub(crate) offline: bool,
    /// Where to cache responses.
    pub(crate) cache: Option<PathBuf>,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// How long a headless run will wait for a provider.
///
/// Longer than the library default, because a person at a terminal who typed a
/// command is waiting on purpose, whereas a dialog in a UI is not.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Runs the verb.
pub(crate) fn run(args: Args) -> Result<()> {
    match args {
        Args::Search(search) => run_search(&search),
        Args::Fetch(fetch) => run_fetch(&fetch),
        Args::Genres { names, table, json } => run_genres(&names, table.as_deref(), json),
        Args::Credentials => {
            report_credentials(&Credentials::from_env());
            Ok(())
        }
    }
}

/// The providers a run should ask, in the order it should ask them.
///
/// The construction itself - transport, rate limit, retry, user agent, cache,
/// genre table - is [`vcw_metadata::Setup`]'s, because the Tauri shell needs
/// exactly the same six decisions and a rate limit written down twice is a rate
/// limit that will disagree. What is decided here is only what this caller
/// legitimately differs on: [`TIMEOUT`], which is longer than the library's
/// because a person who typed a command is waiting on purpose.
fn providers(
    which: Which,
    offline: bool,
    cache: Option<&PathBuf>,
    credentials: &Credentials,
) -> Vec<Box<dyn Provider>> {
    let mut setup = Setup::new().online(!offline).with_timeout(TIMEOUT).only(
        matches!(which, Which::Both | Which::Musicbrainz),
        matches!(which, Which::Both | Which::Discogs),
    );
    if let Some(directory) = cache {
        setup = setup.with_cache(directory);
    }
    setup.providers(credentials)
}

/// Searches, and prints what came back and what did not.
fn run_search(args: &SearchArgs) -> Result<()> {
    let credentials = Credentials::from_env();
    let mut query = Query::new().vinyl_only(!args.all_formats).limit(args.limit);
    if let Some(artist) = &args.artist {
        query = query.artist(artist);
    }
    if let Some(album) = &args.album {
        query = query.album(album);
    }
    if let Some(catalog) = &args.catalog {
        query = query.catalog(catalog);
    }
    if let Some(barcode) = &args.barcode {
        query = query.barcode(barcode);
    }
    if let Some(label) = &args.label {
        query = query.label(label);
    }
    if let Some(year) = args.year {
        query = query.year(year);
    }
    if let Some(country) = &args.country {
        query = query.country(country);
    }
    if query.is_empty() {
        bail!(
            "nothing to search for: give at least one of --artist, --album, --catalog, --barcode, --label, --year or --country"
        );
    }

    let owned = providers(args.which, args.offline, args.cache.as_ref(), &credentials);
    let borrowed: Vec<&dyn Provider> = owned.iter().map(AsRef::as_ref).collect();

    // Deliberately not `search_all`'s collapse-to-one-error behavior. That is
    // right for a UI, which shows a dialog; here a script asked a question and
    // deserves the whole answer, including which provider refused and why. The
    // report is printed either way and the exit code is what carries the failure.
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut failures: Vec<(ProviderId, vcw_metadata::Error)> = Vec::new();
    let cancel = Cancel::new();
    for provider in &borrowed {
        match provider.search(&query, &cancel) {
            Ok(found) => candidates.extend(found),
            Err(error) => failures.push((provider.id(), error)),
        }
    }

    if args.json {
        let value = serde_json::json!({
            "criteria": query.criteria().into_iter().map(Criterion::as_str).collect::<Vec<_>>(),
            "vinyl_only": query.vinyl_only,
            "candidates": candidates,
            "failures": failures
                .iter()
                .map(|(provider, error)| serde_json::json!({
                    "provider": provider.as_str(),
                    "error": error.to_string(),
                }))
                .collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        print_search(&query, &candidates, &failures, &borrowed);
    }

    if candidates.is_empty() && !failures.is_empty() {
        bail!(
            "no provider answered: {}",
            failures
                .iter()
                .map(|(provider, error)| format!("{provider} {error}"))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(())
}

/// Prints a search the way a person reads one.
fn print_search(
    query: &Query,
    candidates: &[Candidate],
    failures: &[(ProviderId, vcw_metadata::Error)],
    providers: &[&dyn Provider],
) {
    let criteria: Vec<&str> = query
        .criteria()
        .into_iter()
        .map(Criterion::as_str)
        .collect();
    println!(
        "searched on {}{}",
        criteria.join(", "),
        if query.vinyl_only { ", vinyl only" } else { "" }
    );

    // What each provider could not act on. Saying so is the point: a result list
    // that quietly answered a different question is worse than a short one.
    for provider in providers {
        let ignored = query.ignored_by(provider.understands());
        if !ignored.is_empty() {
            println!(
                "  {} ignored {}",
                provider.id().display_name(),
                ignored
                    .into_iter()
                    .map(Criterion::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    for (provider, error) in failures {
        println!("  {} did not answer: {error}", provider.display_name());
    }

    if candidates.is_empty() {
        println!("no candidates");
        return;
    }
    println!();
    for candidate in candidates {
        println!("{}", candidate.summary());
        println!(
            "  id {}{}",
            candidate.id,
            candidate
                .tracks
                .map(|n| format!("  {n} tracks"))
                .unwrap_or_default()
        );
        if let Some(url) = &candidate.url {
            println!("  {url}");
        }
    }
    println!(
        "\n{} candidate{}",
        candidates.len(),
        if candidates.len() == 1 { "" } else { "s" }
    );
}

/// Fetches one release and prints its tracklist by side.
fn run_fetch(args: &FetchArgs) -> Result<()> {
    let credentials = Credentials::from_env();
    let which = match args.which {
        // An MBID is a UUID and a Discogs id is a number, so the right provider
        // is usually inferable - but guessing wrong costs a request and a
        // confusing error, so only the unambiguous shape is inferred.
        Which::Both if looks_like_an_mbid(&args.id) => Which::Musicbrainz,
        Which::Both if args.id.chars().all(|c| c.is_ascii_digit()) => Which::Discogs,
        Which::Both => bail!(
            "cannot tell which provider {} belongs to: pass --provider musicbrainz or --provider discogs",
            args.id
        ),
        chosen => chosen,
    };
    let owned = providers(which, args.offline, args.cache.as_ref(), &credentials);
    let provider = owned.first().expect("one provider was chosen");
    let release = provider.fetch(&args.id, &Cancel::new())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&release)?);
        return Ok(());
    }
    print_release(&release);
    Ok(())
}

/// Whether an identifier is shaped like a MusicBrainz id.
fn looks_like_an_mbid(id: &str) -> bool {
    id.len() == 36
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && id.matches('-').count() == 4
}

/// Prints a release the way the sleeve reads.
fn print_release(release: &Release) {
    println!("{} - {}", release.album_artist, release.album);
    let mut details: Vec<String> = Vec::new();
    if let Some(year) = release.year {
        details.push(year.to_string());
    }
    for (label, value) in [
        ("label", &release.label),
        ("catalog", &release.catalog),
        ("country", &release.country),
    ] {
        if !value.is_empty() {
            details.push(format!("{label} {value}"));
        }
    }
    if let Some(barcode) = &release.barcode {
        details.push(format!("barcode {barcode}"));
    }
    if !details.is_empty() {
        println!("{}", details.join("  "));
    }
    if !release.genres.is_empty() {
        println!("genres: {}", release.genres.join(", "));
    }
    for reference in &release.artwork {
        println!(
            "artwork{}: {}",
            if reference.primary { " (front)" } else { "" },
            reference.url
        );
    }

    for medium in &release.media {
        println!(
            "\nrecord {}{}",
            medium.position,
            if medium.format.is_empty() {
                String::new()
            } else {
                format!("  {}", medium.format)
            }
        );
        for track in &medium.tracks {
            let position = track
                .resolved
                .map(|resolved| resolved.alpha())
                .unwrap_or_else(|| format!("({})", track.position));
            let length = track
                .seconds()
                .map(|seconds| format!("  {:.0}:{:02.0}", (seconds / 60.0).floor(), seconds % 60.0))
                .unwrap_or_default();
            println!("  {position:>5}  {}{length}", track.title);
        }
    }

    // Side totals, because a side that will not fit on a record is the first
    // thing a person checks against what they actually captured.
    let sides = release.sides();
    if !sides.is_empty() {
        println!();
        for side in sides {
            match release.side_seconds(side) {
                Some(seconds) => println!(
                    "side {}: {} tracks, {:.0}:{:02.0}",
                    side.letter(),
                    release.side_tracks(side).len(),
                    (seconds / 60.0).floor(),
                    seconds % 60.0
                ),
                None => println!(
                    "side {}: {} tracks, running time unknown",
                    side.letter(),
                    release.side_tracks(side).len()
                ),
            }
        }
    }
}

/// Normalizes genre names, which needs no network at all (§32).
fn run_genres(names: &[String], table: Option<&std::path::Path>, json: bool) -> Result<()> {
    let (genres, from_file) = match table {
        Some(path) => {
            let (genres, loaded) = Genres::from_file_or_builtin(path);
            if !loaded {
                bail!("could not read the genre table at {}", path.display());
            }
            (genres, true)
        }
        None => (Genres::builtin(), false),
    };
    let normalized = genres.normalize_all(names);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "table": if from_file { "file" } else { "builtin" },
                "keys": genres.len(),
                "input": names,
                "genres": normalized,
            }))?
        );
        return Ok(());
    }
    println!(
        "{} mappings from the {} table",
        genres.len(),
        if from_file { "given" } else { "built-in" }
    );
    if normalized.is_empty() {
        println!("nothing to normalize");
    } else {
        println!("{}", normalized.join("; "));
    }
    Ok(())
}

/// Says what is configured, and never what it is (§39).
fn report_credentials(credentials: &Credentials) {
    println!("credentials come from the environment and are never stored (§39)");
    for (variable, state) in [
        (
            vcw_metadata::credentials::DISCOGS_TOKEN_VAR,
            credentials.discogs().map(|token| token.characters()),
        ),
        (
            vcw_metadata::credentials::ACOUSTID_KEY_VAR,
            credentials.acoustid().map(|token| token.characters()),
        ),
    ] {
        match state {
            Some(characters) => println!("  {variable}  set, {characters} characters"),
            None => println!("  {variable}  not set"),
        }
    }
    match credentials.contact() {
        Some(contact) => println!("  {}  {contact}", vcw_metadata::credentials::CONTACT_VAR),
        None => println!(
            "  {}  not set; providers ask callers to identify themselves",
            vcw_metadata::credentials::CONTACT_VAR
        ),
    }
    println!("\nuser agent: {}", user_agent(credentials.contact()));
    println!(
        "discogs search {} a token",
        if credentials.discogs().is_some() {
            "has"
        } else {
            "needs"
        }
    );
}
