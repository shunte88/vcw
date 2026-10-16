/*
 *  identify.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The `vcw identify` verb: drive the resolver's escalation walk over a project (§26, §27).
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

//! The `vcw identify` verb: drive the resolver's escalation walk over a project
//! (§26, §27).
//!
//! `vcw-identify` has had the whole of the decision in it since WP-23 - evidence,
//! candidates, confidence, the resolver and the walk around them - and nothing
//! called it. This is the caller. §4.5 asks for the workflow to be drivable with no
//! UI, and identification is the part of it that had no door at all.
//!
//! # What it costs, and why that is the headline
//!
//! Every lookup is a request to somebody else's service, so the report leads on the
//! lookups rather than on the answer: which were made, what each contributed, and
//! which were never reached. A record whose catalog number is on the sleeve costs
//! one request and the output says so on one line. That is §26's escalation being
//! visible rather than claimed.
//!
//! A lookup VCW had no provider for - no `VCW_DISCOGS_TOKEN`, `--provider
//! musicbrainz` - is not printed as a failure, because it was not made.
//!
//! # The audio lookup asks about a track
//!
//! [`Lookup::IdentifyAudio`] is last and usually unreached. When it is reached it
//! fingerprints the *first track* of the first captured side rather than the side,
//! because AcoustID filters on the declared duration exactly and a whole side is not
//! a recording anybody has submitted. A project with no tracks laid out therefore
//! has nothing to ask with, and the step records that it could not be made rather
//! than making it badly. `vcw tracks <project> detect --write` first.
//!
//! # Reading and writing are separate on purpose
//!
//! The verb reads. `--apply` is the only thing here that writes, it only acts on a
//! resolved answer, and it refuses a shortlist: choosing between two pressings a
//! hair apart is the one decision §26 reserves for a person, and a flag that picked
//! the top row would be making it by being set.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use vcw_core::evidence::{self, Heard, Known};
use vcw_core::identity;
use vcw_identify::candidate::Claim;
use vcw_identify::evidence::{Fact, Item, Source};
use vcw_identify::resolver::{Doubt, Lookup, Outcome, Ranked};
use vcw_identify::walk::{self, Audio, Step};
use vcw_metadata::credentials::Credentials;
use vcw_metadata::release::ProviderId;
use vcw_metadata::{Cancel, Provider, Setup};
use vcw_project::Project;

use crate::metadata::Which;

/// How long a headless run will wait for a provider.
///
/// The same twenty seconds `vcw metadata` and `vcw fingerprint` wait, for the same
/// reason: a person who typed a command is waiting on purpose.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to identify.
    pub(crate) project: PathBuf,
    /// Which providers to ask.
    pub(crate) which: Which,
    /// Refuse to use the network.
    pub(crate) offline: bool,
    /// Where to cache responses. Omit for no cache.
    pub(crate) cache: Option<PathBuf>,
    /// Do not reach the AcoustID lookup even if the walk gets that far.
    pub(crate) no_audio: bool,
    /// Write a resolved release into the project (§26).
    pub(crate) apply: bool,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }
    let known = read(&args.project)?;
    if known.observed.is_empty() {
        bail!(
            "{} says nothing about the record: type an artist, a title or a catalog number first, with `vcw release <project> set`",
            args.project.display()
        );
    }

    let credentials = Credentials::from_env();
    let mut setup = Setup::new()
        .online(!args.offline)
        .with_timeout(TIMEOUT)
        .only(
            matches!(args.which, Which::Both | Which::Musicbrainz),
            matches!(args.which, Which::Both | Which::Discogs),
        );
    if let Some(directory) = &args.cache {
        setup = setup.with_cache(directory);
    }
    let owned = setup.providers(&credentials);
    let borrowed: Vec<&dyn Provider> = owned.iter().map(AsRef::as_ref).collect();

    let acoustid = setup.acoustid(&credentials);
    let source = (!args.no_audio)
        .then(|| evidence::heard(&args.project, &known))
        .flatten();
    let heard = source.as_ref().map(|found| Audio {
        acoustid: &acoustid,
        fingerprint: found.fingerprint.as_ref(),
    });

    let walked = walk::walk(&borrowed, heard.as_ref(), &known.observed, &Cancel::new());

    if args.json {
        print_json(&known, &walked);
    } else {
        print_report(args, &known, source.as_ref(), &walked);
    }

    if args.apply {
        return apply(args, &setup, &credentials, &walked.outcome);
    }
    match &walked.outcome {
        Outcome::Nothing => bail!("no candidate the evidence supports"),
        _ => Ok(()),
    }
}

/// Reads the project, read-only, for the reason analysis always is.
fn read(path: &PathBuf) -> Result<Known> {
    let project =
        Project::open_read_only(path).with_context(|| format!("opening {}", path.display()))?;
    let known = evidence::observe(project.conn())?;
    project.close()?;
    Ok(known)
}

/// Fetches a resolved release and writes it into the project (§26).
fn apply(args: &Args, setup: &Setup, credentials: &Credentials, outcome: &Outcome) -> Result<()> {
    let chosen = match outcome {
        Outcome::Resolved { chosen } => chosen,
        Outcome::Ask { shortlist, because } => bail!(
            "--apply needs one answer and this is a question ({}): {} candidates. Choose one with `vcw metadata fetch` and `vcw release <project> set`",
            why(*because),
            shortlist.len()
        ),
        Outcome::Nothing => {
            bail!("--apply has nothing to apply: no candidate the evidence supports")
        }
    };
    let claim = &chosen.assessed.claim;
    let (Some(provider), Some(id)) = (claim.provider, claim.ids.first()) else {
        bail!("the resolved candidate does not say which service it came from");
    };

    let one = setup
        .clone()
        .only(
            provider == ProviderId::MusicBrainz,
            provider == ProviderId::Discogs,
        )
        .providers(credentials)
        .into_iter()
        .next()
        .with_context(|| format!("{} is not built into this binary", provider.display_name()))?;
    let found = one.fetch(id, &Cancel::new())?;

    let mut project = Project::open(&args.project)?;
    let applied = identity::accept(&mut project, &found)?;
    project.close()?;

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "applied": {
                    "provider": provider.as_str(),
                    "id": id,
                    "retitled": applied.tracks.len(),
                    "left_alone": applied.confirmed.len(),
                    "unmatched": applied.unmatched,
                    "unnamed": applied.unnamed,
                    "relaid": applied.relaid,
                }
            })
        );
    } else {
        println!("  applied    {} from {}", id, provider.display_name());
        println!(
            "             {} track names written, {} left alone as confirmed",
            applied.tracks.len(),
            applied.confirmed.len()
        );
        for (label, rows) in [
            ("unmatched", &applied.unmatched),
            ("unnamed", &applied.unnamed),
            ("relaid", &applied.relaid),
        ] {
            if !rows.is_empty() {
                println!("             {label} {}", rows.join(", "));
            }
        }
    }
    Ok(())
}

/// One line of what the project already knew.
fn summarize(known: &Known) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in known.observed.items() {
        parts.push(match &item.fact {
            Fact::Artist(text) => format!("artist {text}"),
            Fact::Album(text) => format!("album {text}"),
            Fact::Catalog(text) => format!("catalog {text}"),
            Fact::Label(text) => format!("label {text}"),
            Fact::Year(year) => format!("year {year}"),
            Fact::Count(count) => format!("{count} tracks"),
            Fact::Side(length) => format!("a side of {:.0} s", length.as_secs_f64()),
            Fact::Recording { id, .. } => format!("recording {id}"),
        });
    }
    parts.join(", ")
}

/// Why the resolver asked rather than answered, as a person reads it.
fn why(because: Doubt) -> &'static str {
    match because {
        Doubt::NotEnoughEvidence => "not enough evidence for any one of them",
        Doubt::TooClose => "two of them are too close to separate",
        Doubt::ContradictsAPerson => "the best one disputes what was typed at setup",
    }
}

/// A candidate on one line.
fn line(ranked: &Ranked) -> String {
    let claim = &ranked.assessed.claim;
    let mut text = format!("{} - {}", claim.artist, claim.album);
    if let Some(year) = claim.year {
        text.push_str(&format!(" ({year})"));
    }
    if !claim.label.is_empty() {
        text.push_str(&format!(" [{}]", claim.label));
    }
    if !claim.catalog.is_empty() {
        text.push(' ');
        text.push_str(&claim.catalog);
    }
    // A shortlist is nearly always several pressings of one record, so the fields
    // that agreed are the same on every line and the ones that tell them apart are
    // the ones nothing in the project could ever contradict.
    for aside in [&claim.country, &claim.format] {
        if !aside.is_empty() {
            text.push_str(&format!(", {aside}"));
        }
    }
    text
}

/// Where to go back to for the full release.
fn whose(claim: &Claim) -> String {
    match (claim.provider, claim.ids.first()) {
        (Some(provider), Some(id)) => format!("{} {id}", provider.as_str()),
        (None, Some(id)) => id.clone(),
        _ => String::from("no identifier"),
    }
}

/// What each step cost and contributed.
fn print_steps(steps: &[Step], known: &Known) {
    if steps.is_empty() {
        // Two different reasons, and the first version of this said the first
        // one for both. A project with tracks and no text reaches here with
        // Discogs and MusicBrainz both configured and willing: a track count
        // narrows a list of candidates and cannot start one.
        let text = known.observed.items().iter().any(|item| {
            matches!(
                item.fact,
                Fact::Artist(_) | Fact::Album(_) | Fact::Catalog(_)
            )
        });
        if text {
            println!("  lookups    none were possible: no provider for any of them");
        } else {
            println!(
                "  lookups    none were possible: every lookup needs a catalog number, or an\n\
                 \x20            artist and a title. `vcw release <project> set` is where those go."
            );
        }
        return;
    }
    for (index, step) in steps.iter().enumerate() {
        let cost = match &step.failed {
            Some(error) => format!("did not answer: {error}"),
            None => format!(
                "{} new candidate{}",
                step.found,
                if step.found == 1 { "" } else { "s" }
            ),
        };
        println!(
            "  lookup {:<3} {:<22} {cost}",
            index + 1,
            step.lookup.as_str()
        );
    }
    let unreached: Vec<&str> = Lookup::ORDER
        .iter()
        .filter(|lookup| !steps.iter().any(|step| step.lookup == **lookup))
        .map(|lookup| lookup.as_str())
        .collect();
    if !unreached.is_empty() {
        println!("  not asked  {}", unreached.join(", "));
    }
}

/// Prints the walk the way a person reads one.
fn print_report(args: &Args, known: &Known, source: Option<&Heard>, walked: &walk::Walked) {
    println!("  project    {}", args.project.display());
    println!("  known      {}", summarize(known));
    if known.confirmed {
        println!(
            "  note       a release has already been accepted here, so the text above came\n\
             \x20            back from a provider rather than off the sleeve. It will agree with\n\
             \x20            itself."
        );
    }
    match source {
        Some(found) => println!("  audio      {} if the walk gets that far", found.what),
        None if known.captured.is_empty() => println!("  audio      nothing captured"),
        None => println!("  audio      no track laid out to ask about"),
    }
    let recognized = walked.observed.items().len() - known.observed.items().len();
    if recognized > 0 {
        println!("  heard      {recognized} recording(s) AcoustID recognized");
    }
    print_steps(&walked.steps, known);
    if let Some(why) = source.and_then(|found| found.trouble.borrow().clone()) {
        println!("  no audio   the walk reached AcoustID and could not ask: {why}");
    }
    println!();
    match &walked.outcome {
        Outcome::Resolved { chosen } => {
            println!("  resolved   {}", line(chosen));
            println!("             {}", whose(&chosen.assessed.claim));
            println!("             confidence {:.2}", chosen.confidence.value());
        }
        Outcome::Ask { shortlist, because } => {
            println!("  a question {}", why(*because));
            for (index, ranked) in shortlist.iter().enumerate() {
                println!(
                    "  {:>8}.  {:.2}  {}",
                    index + 1,
                    ranked.confidence.value(),
                    line(ranked)
                );
                println!("             {}", whose(&ranked.assessed.claim));
            }
        }
        Outcome::Nothing if walked.steps.is_empty() => {
            println!("  nothing    nothing was asked, so this is not an answer about the record.");
        }
        Outcome::Nothing => {
            let held: usize = walked.steps.iter().map(|step| step.found).sum();
            println!(
                "  nothing    {held} candidate{} came back and none is well enough supported.\n\
                 \x20            Every fact above is scored against every one of them, so a track\n\
                 \x20            count or a side length the record does not have will refuse the\n\
                 \x20            right pressing. `vcw release <project> show` is what was weighed.",
                if held == 1 { "" } else { "s" }
            );
        }
    }
}

/// The same walk, for a script.
fn print_json(known: &Known, walked: &walk::Walked) {
    let candidate = |ranked: &Ranked| {
        let claim = &ranked.assessed.claim;
        serde_json::json!({
            "provider": claim.provider.map(ProviderId::as_str),
            "id": claim.ids.first(),
            "artist": claim.artist,
            "album": claim.album,
            "catalog": claim.catalog,
            "label": claim.label,
            "year": claim.year,
            "country": claim.country,
            "format": claim.format,
            "tracks": claim.tracks,
            "confidence": ranked.confidence.value(),
        })
    };
    let (outcome, resolved, shortlist, because) = match &walked.outcome {
        Outcome::Resolved { chosen } => ("resolved", Some(candidate(chosen)), Vec::new(), None),
        Outcome::Ask { shortlist, because } => (
            "ask",
            None,
            shortlist.iter().map(candidate).collect(),
            Some(why(*because)),
        ),
        Outcome::Nothing => ("nothing", None, Vec::new(), None),
    };
    let value = serde_json::json!({
        "confirmed": known.confirmed,
        "known": known.observed.items().iter().map(stated).collect::<Vec<_>>(),
        "steps": walked.steps.iter().map(|step| serde_json::json!({
            "lookup": step.lookup.as_str(),
            "found": step.found,
            "failed": step.failed.as_ref().map(ToString::to_string),
        })).collect::<Vec<_>>(),
        "outcome": outcome,
        "resolved": resolved,
        "shortlist": shortlist,
        "because": because,
        "held": walked.steps.iter().map(|step| step.found).sum::<usize>(),
    });
    println!("{value}");
}

/// One known fact, as JSON.
fn stated(item: &Item) -> serde_json::Value {
    serde_json::json!({
        "source": match item.source {
            Source::Stated => "stated",
            Source::Signal => "signal",
            Source::Discogs => "discogs",
            Source::MusicBrainz => "musicbrainz",
            Source::AcoustId => "acoustid",
        },
        "fact": format!("{:?}", item.fact),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vcw_identify::candidate::Assessed;
    use vcw_identify::confidence::weigh;

    fn pressing(country: &str, format: &str) -> Ranked {
        let claim = Claim {
            provider: Some(ProviderId::Discogs),
            ids: vec!["1865584".into()],
            artist: "Ultravox".into(),
            album: "Vienna".into(),
            catalog: "CHR 1296".into(),
            label: "Chrysalis".into(),
            year: Some(1980),
            country: country.into(),
            format: format.into(),
            ..Claim::default()
        };
        let assessed = Assessed {
            claim,
            agreements: Vec::new(),
        };
        Ranked {
            confidence: weigh(&assessed),
            assessed,
        }
    }

    /// A shortlist is nearly always one record pressed several ways, so every
    /// line agrees on everything the resolver weighed. If the line prints only
    /// what was weighed, a person is asked to choose between identical strings.
    #[test]
    fn two_pressings_of_one_record_read_differently() {
        let spain = line(&pressing("Spain", "Vinyl, LP, Album"));
        let sweden = line(&pressing("Scandinavia", "Vinyl, LP, Album, Stereo"));
        assert_ne!(spain, sweden);
        assert!(spain.contains("Spain"), "{spain}");
        assert!(sweden.contains("Stereo"), "{sweden}");
    }

    /// What a provider did not say is left out rather than printed empty.
    #[test]
    fn a_silent_provider_leaves_no_gap() {
        let bare = line(&pressing("", ""));
        assert_eq!(bare, "Ultravox - Vienna (1980) [Chrysalis] CHR 1296");
    }

    #[test]
    fn a_claim_nobody_attributed_still_names_its_id() {
        let mut hand = pressing("UK", "");
        hand.assessed.claim.provider = None;
        assert_eq!(whose(&hand.assessed.claim), "1865584");
        assert_eq!(whose(&pressing("UK", "").assessed.claim), "discogs 1865584");
    }
}
