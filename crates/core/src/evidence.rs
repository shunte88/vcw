/*
 *  evidence.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What a project already knows about the record, as resolver evidence (§23, §26).
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

//! What a project already knows about the record, as resolver evidence (§23, §26).
//!
//! [`vcw_identify`] takes an [`Observed`] and never reads a database; [`vcw_project`]
//! holds the rows and has never heard of a resolver. This is the one function that
//! turns the second into the first, and it lives here for the same reason
//! [`crate::identity`] does: `vcw-core` is the only crate that depends on both.
//!
//! # Two sources, and the line between them
//!
//! What a person typed at setup is [`Source::Stated`]: artist, album, catalog
//! number, label, year. What the project measured is [`Source::Signal`]: how many
//! tracks there are, and how long each captured side runs. §26 turns on that line,
//! because a stated fact is the one a candidate can *contradict*.
//!
//! A release somebody has already accepted is the awkward case. Its text came back
//! from a provider rather than off a sleeve, so reading it as stated lets that
//! provider's release vouch for itself at a person's weight. Nothing here can tell
//! the two apart from the row alone, so nothing here tries: [`Known::confirmed`]
//! carries the flag out and the caller says so. See `vcw identify`, which prints a
//! line about it rather than quietly scoring a circle.
//!
//! # A side's length, when there is one to have
//!
//! A side agrees with a candidate's side lengths or it does not, and the comparison
//! only means anything if the length is the side's rather than the capture's. So the
//! tracks win where there are tracks: their total is what the record's face holds.
//! Where a side has no tracks yet the capture's own length stands in, but only if
//! that capture belongs to this side alone - two faces in one pass is the case
//! `sides.capture_id` exists to allow, and half of that capture is not a side.
//!
//! # The audio a lookup would ask about
//!
//! [`heard`] is the other half: not what the project knows, but what it could be
//! asked. It offers the *first track* of the first captured side rather than the
//! side, because AcoustID filters on the declared duration exactly and a whole
//! side is not a recording anybody submitted. It is here rather than in a caller
//! because both doors onto the walk - `vcw identify` and the window - need the
//! same track chosen the same way.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use vcw_identify::evidence::{Fact, Observed, Source};
use vcw_metadata::query::Fingerprint;
use vcw_project::error::Result;
use vcw_project::pcm::Layout;
use vcw_project::{Connection, Project, release, session, side, track};
use vcw_types::Span;

/// What a project had to say, and the two things a caller has to know about it.
#[derive(Debug, Clone)]
pub struct Known {
    /// The evidence, ready for `vcw_identify::walk`.
    pub observed: Observed,
    /// Whether a release has already been accepted into this project (§26).
    ///
    /// When it has, the text in [`Known::observed`] is a provider's rather than a
    /// person's, and a walk over it is partly circular.
    pub confirmed: bool,
    /// Sides that hold a capture, which is what an audio lookup can be run over.
    pub captured: Vec<(vcw_types::Side, i64)>,
}

/// Reads everything a project knows about the record it is of.
///
/// Empty strings are left out rather than observed as empty: a project with no
/// catalog number typed has no catalog fact, which is what makes
/// `next_lookup` skip the catalog lookup instead of searching for "".
///
/// # Errors
///
/// If a row will not read.
pub fn observe(conn: &Connection) -> Result<Known> {
    let mut observed = Observed::new();
    let found = release::load(conn)?;
    let confirmed = found.as_ref().is_some_and(|record| record.confirmed);
    if let Some(record) = &found {
        for (text, fact) in [
            (&record.album_artist, Fact::Artist as fn(String) -> Fact),
            (&record.album, Fact::Album),
            (&record.catalog, Fact::Catalog),
            (&record.label, Fact::Label),
        ] {
            if !text.trim().is_empty() {
                observed.add(Source::Stated, fact(text.clone()));
            }
        }
        if let Some(year) = record.year {
            observed.add(Source::Stated, Fact::Year(year));
        }
    }

    let tracks = track::listing(conn)?;
    if !tracks.is_empty() {
        observed.add(Source::Signal, Fact::Count(tracks.len()));
    }

    let mut captured = Vec::new();
    for face in side::list(conn)? {
        if let Some(capture) = face.capture {
            captured.push((face.side, capture));
        }
        if let Some(length) = side_length(conn, &face)? {
            observed.add(Source::Signal, Fact::Side(length));
        }
    }

    Ok(Known {
        observed,
        confirmed,
        captured,
    })
}

/// How long one face of the record is, where the project can say.
///
/// The tracks where there are tracks, the capture where the capture is this side's
/// alone, and nothing otherwise - a figure that might be two faces is worse than no
/// figure, because a candidate would then *disagree* with it.
fn side_length(conn: &Connection, face: &side::Record) -> Result<Option<Duration>> {
    let Some(capture) = face.capture else {
        return Ok(None);
    };
    let Some(record) = session::load(conn, capture)? else {
        return Ok(None);
    };
    let tracks = track::tracks_of(conn, face.id)?;
    if !tracks.is_empty() {
        let rate = record.info.rate;
        let seconds: f64 = tracks.iter().map(|one| one.seconds(rate)).sum();
        return Ok(Some(Duration::from_secs_f64(seconds.max(0.0))));
    }
    if side::for_capture(conn, capture)?.len() != 1 {
        return Ok(None);
    }
    // The committed frames rather than the session row's own duration, so a
    // capture that was recovered rather than closed cleanly still measures.
    let layout = Layout::of(conn, capture)?;
    let hz = f64::from(layout.rate.hz());
    if hz <= 0.0 || layout.frames == 0 {
        return Ok(None);
    }
    Ok(Some(Duration::from_secs_f64(layout.frames as f64 / hz)))
}

/// The track an audio lookup would ask about, and how to fingerprint it.
///
/// Held as a struct rather than built inline because `walk::Audio` borrows the
/// closure, and a temporary closure would not outlive the call.
pub struct Heard {
    /// Where it is, for the report.
    pub what: String,
    /// Why the audio could not be fingerprinted, once something has tried.
    ///
    /// The walk's contract for a closure that returns `None` is "there was nothing
    /// to ask with", and it records no step for it - correct, and silent. A person
    /// who has a capture and no AcoustID lookup is owed the reason, so the closure
    /// leaves it here. A two-second track is the common one: chromaprint needs
    /// several seconds of audio before it has a fingerprint at all.
    pub trouble: Rc<RefCell<Option<String>>>,
    /// Called at most once, and only if the walk reaches the audio lookup.
    pub fingerprint: Box<dyn Fn() -> Option<Fingerprint>>,
}

/// The first track of the first captured side, or nothing to ask with.
///
/// Takes a path rather than an open project because the closure outlives the call
/// and reopens the file itself: a resolver walk may never reach the audio lookup,
/// and holding a connection open for a question that is usually not asked would
/// keep a reader on the database for the length of a network round trip.
#[must_use]
pub fn heard(path: &Path, known: &Known) -> Option<Heard> {
    let project = Project::open_read_only(path).ok()?;
    let (side, capture) = *known.captured.first()?;
    let layout = Layout::of(project.conn(), capture).ok()?;
    let face = side::load(project.conn(), side).ok()??;
    let first = track::tracks_of(project.conn(), face.id)
        .ok()?
        .into_iter()
        .next()?;
    project.close().ok()?;

    let span = Span::new(first.start, first.end.min(layout.frames));
    if span.end <= span.start {
        return None;
    }
    let seconds = first.seconds(layout.rate);
    let owned = path.to_path_buf();
    let trouble: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let said = Rc::clone(&trouble);
    Some(Heard {
        what: format!("{side}1, {seconds:.0} s"),
        trouble,
        fingerprint: Box::new(move || {
            let note = |why: String| {
                *said.borrow_mut() = Some(why);
                None::<Fingerprint>
            };
            let project = match Project::open_read_only(&owned) {
                Ok(project) => project,
                Err(error) => return note(error.to_string()),
            };
            match crate::fingerprinting::of_span(&project, capture, span) {
                // The track's own length, which is the recording's. Declaring the
                // window's instead is the mistake `vcw fingerprint --duration`
                // exists to let a person avoid; here there is no window to get
                // wrong.
                Ok(code) => Some(Fingerprint::new(
                    &code.encoded,
                    seconds.round().max(0.0) as u32,
                )),
                Err(error) => note(error.to_string()),
            }
        }),
    })
}
