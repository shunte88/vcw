/*
 *  library.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The read commands: everything a UI draws, taken from the project rather than cached (§2, §35).
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

//! The read commands: everything a UI draws, taken from the project rather than
//! cached (§2, §35).
//!
//! Each of these opens the project read-only, reads, and closes. That is a
//! deliberate choice and not an oversight.
//!
//! A kept-open handle would be faster, and it would also be a second writer's
//! worth of risk for no gain: the engine holds the project while a capture is
//! running, SQLite's WAL lets a reader in beside it, and a read-only handle
//! cannot corrupt the one thing in the file that cannot be recorded again. The
//! reads are also small - a tracklist is tens of rows - and the one that is not
//! is the waveform, which reads summaries rather than audio precisely so that
//! this stays true.
//!
//! No command here returns audio. §35 forbids PCM crossing the boundary, and a
//! waveform's three arrays are one float per drawn column, which is a few
//! thousand numbers for a screen-width of a two-hour side.

use std::process::Command;

use vcw_contract::command::Zoom;
use vcw_contract::read;
use vcw_contract::view::{About, Boundary, Capture, Device, Release, Side, Track, Waveform};
use vcw_project::{Project, session};

use crate::state::{Error, Shell};

/// Every audio device the host offers, with what each can do (§7).
///
/// Enumeration is a synchronous walk of every host API and takes long enough on
/// some machines to be visible, which is why this is the one read command that
/// does not touch the project: it is also the first one §50 calls, before
/// anything is open.
///
/// # Errors
///
/// Never. A host that will not answer becomes a line in `problems` on the
/// affected device, or in the snapshot's own list if a whole host API failed -
/// which is why the return type is a list and not a `Result` of one: a machine
/// with one broken USB interface should still show the other three.
pub fn devices() -> Vec<Device> {
    vcw_audio::devices::enumerate()
        .devices
        .iter()
        .map(Device::from)
        .collect()
}

/// Which build this is, and what it links (WP-28).
///
/// No project, like [`devices`], and nothing to ask: every field is a
/// compile-time constant of the crate that owns the fact. The notices in
/// particular are derived from `vcw-export`'s cargo features, which is the only
/// place that question can be answered - `cfg!(feature = "mp3")` written here
/// would be asking about *this* crate's features.
pub fn about() -> About {
    About::current()
}

/// Every page this product can send the operator to, by the name a frontend
/// asks for.
///
/// A table and not two commands, because the refusal below has to name what
/// exists: a hand-written "try coffee or shirts" is a sentence that goes stale
/// the day a third page is added.
const PAGES: [(&str, &str); 3] = [
    ("coffee", "https://www.buymeacoffee.com/shunte88"),
    (
        "shirts",
        "https://www.zazzle.com/team_badger_t_shirt-235604841593837420",
    ),
    // Where a person goes to get the token Settings > Metadata asks them for.
    // Here and not in the panel for the reason the other two are: this product
    // hands the webview no addresses.
    (
        "discogs-token",
        "https://www.discogs.com/settings/developers",
    ),
];

/// Every address this build can send the operator to, by the name a frontend
/// asks for: [`PAGES`], the repository, and one per third-party notice.
///
/// Generated rather than listed, because the last two sets are *derived* - the
/// repository from the manifest and the notices from the cargo features - so a
/// hand-written table would be wrong in exactly the builds [`About`] exists to
/// describe. A build without `mp3` must not offer a link to libmp3lame's source
/// and must not refuse one it is still linking.
fn addresses() -> Vec<(String, String)> {
    let about = About::current();
    PAGES
        .iter()
        .map(|(name, url)| ((*name).to_owned(), (*url).to_owned()))
        .chain([("repository".to_owned(), about.repository)])
        .chain(
            about
                .notices
                .into_iter()
                .map(|notice| (notice.component, notice.source)),
        )
        .collect()
}

/// Opens one of `addresses` in the operator's own browser (WP-28).
///
/// The frontend names a page and not an address, which is the whole security
/// story: a command that takes a URL opens whatever the webview asks for, and
/// every address this product can reach is derived here from the manifest and
/// the features it was built with.
///
/// A component's own name is its page name. That reads oddly next to `coffee`
/// and reads exactly right at the call site, where the About dialog has the
/// notice in its hand and nothing else to call it.
///
/// # Errors
///
/// [`Error::Invalid`] if the name is not one of `addresses`, or if the
/// platform's opener could not be started - a headless box, or a desktop with
/// nothing registered for `https`. Starting it is as far as this goes: the
/// opener exits as soon as it has handed the URL on, so what the browser does
/// next is not VCW's to report.
pub fn support(page: String) -> Result<(), Error> {
    let known = addresses();
    let url = known
        .iter()
        .find(|(name, _)| *name == page)
        .map(|(_, url)| url.as_str())
        .ok_or_else(|| Error::Invalid {
            field: "page".to_owned(),
            why: format!("no page called {page:?} - this build offers {}", named()),
        })?;

    // ponytail: not reaped, so a click leaves one zombie until the app exits.
    // `status()` would reap it and block the window while the browser starts -
    // see the module note on sync commands - and a thread per click to call
    // `wait` is more machinery than a short-lived `xdg-open` is worth.
    opener(url).spawn().map(drop).map_err(|why| Error::Invalid {
        field: "page".to_owned(),
        why: format!("no browser could be started for {url}: {why}"),
    })
}

/// The page names, for a refusal that cannot go stale.
fn named() -> String {
    addresses()
        .into_iter()
        .map(|(name, _)| name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The platform's "open this with whatever handles it", built but not run.
///
/// Separate from [`support`] so a test can read the program and its arguments
/// without a browser window opening on whoever ran the gate.
fn opener(url: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        // `start` is a cmd builtin rather than a program, and the first quoted
        // argument it takes is the window title - hence the empty one, or a URL
        // containing a space becomes the title and nothing opens.
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(url);
        command
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    }
}

/// The release, or `null` in a project that has not had one filled in.
///
/// # Errors
///
/// [`Error::NoProject`] if nothing is open, [`Error::Project`] if the file will
/// not read.
pub fn release(shell: &Shell) -> Result<Option<Release>, Error> {
    with_project(shell, read::release)
}

/// Every side, in playing order.
///
/// # Errors
///
/// As [`release`].
pub fn sides(shell: &Shell) -> Result<Vec<Side>, Error> {
    with_project(shell, read::sides)
}

/// Every track, in playing order, with §29's positions already rendered.
///
/// # Errors
///
/// As [`release`].
pub fn tracks(shell: &Shell) -> Result<Vec<Track>, Error> {
    with_project(shell, read::tracks)
}

/// Every capture in the project, oldest first.
///
/// # Errors
///
/// As [`release`].
pub fn captures(shell: &Shell) -> Result<Vec<Capture>, Error> {
    with_project(shell, read::captures)
}

/// Every boundary in the project, promoted into a track or not (§31).
///
/// The track editor's own list. [`tracks`] answers with the pairs that survived
/// §24's promotion policy; this answers with every observation, which on the
/// real side is 270 rows where `tracks` is 6. A person cannot promote by hand
/// what they cannot see, and the policy is deliberately blunt - so the 264 have
/// to be reachable.
///
/// # Errors
///
/// As [`release`].
pub fn boundaries(shell: &Shell) -> Result<Vec<Boundary>, Error> {
    with_project(shell, read::boundaries)
}

/// One channel of one capture, drawn to a given width (§17, §20).
///
/// The columns come from the stored summaries, so this is a few hundred rows of
/// index rather than a decode - which is what makes it safe to call on every
/// zoom and pan. The reader picks the summary level from the frames-per-pixel
/// the request works out to; a frontend does not choose it and should not.
///
/// # Errors
///
/// As [`release`], plus [`Error::Invalid`] if `pixels` is zero - a canvas no
/// pixels wide is a frontend bug worth naming rather than an empty answer.
pub fn waveform(shell: &Shell, zoom: Zoom) -> Result<Waveform, Error> {
    if zoom.pixels == 0 {
        return Err(Error::Invalid {
            field: "pixels".to_owned(),
            why: "a waveform needs at least one column".to_owned(),
        });
    }
    let path = shell.project_path()?;
    let project = Project::open_read_only(&path)?;
    let shape = vcw_project::waveform::Shape::of(project.conn(), zoom.capture_id)?;
    let end = zoom.end_frame.unwrap_or(shape.frames).min(shape.frames);
    let start = zoom.start_frame.min(end);
    let request = vcw_signal::waveform::Request::new(start, end, zoom.pixels);
    let drawn =
        vcw_project::waveform::read(project.conn(), zoom.capture_id, zoom.channel, &request)?;
    // The rate, so the view can carry the window in seconds as well as in
    // frames. The frontend needs both - it draws in frames and seeks in
    // seconds - and dividing one by the other in TypeScript would put a unit
    // conversion on the wrong side of the boundary, where `vcw --json` cannot
    // reach it.
    let rate = session::load(project.conn(), zoom.capture_id)?
        .ok_or_else(|| Error::Invalid {
            field: "captureId".to_owned(),
            why: format!("there is no capture {} in this project", zoom.capture_id),
        })?
        .info
        .rate;
    project.close()?;
    Ok(Waveform::of(zoom.capture_id, &drawn, rate))
}

/// Opens the project read-only, runs a reader, closes it.
///
/// The close is not decoration: `SQLITE_OPEN_READONLY` still creates `-wal` and
/// `-shm` sidecars, and an explicit close is what deletes them. A handle
/// dropped without one leaves them behind, which looks like a crash to the next
/// recovery check.
fn with_project<T, F>(shell: &Shell, reader: F) -> Result<T, Error>
where
    F: FnOnce(&vcw_project::Connection) -> vcw_project::Result<T>,
{
    let path = shell.project_path()?;
    let project = Project::open_read_only(&path)?;
    let read = reader(project.conn());
    project.close()?;
    Ok(read?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The point of the table: whatever the webview sends, the URL that reaches
    /// the platform's opener is one of ours.
    ///
    /// Over [`addresses`] and not [`PAGES`], because the two sets that are
    /// *derived* are the two that could carry something nobody checked.
    #[test]
    fn every_page_offered_is_a_page_of_the_authors() {
        for (name, url) in addresses() {
            let url = url.as_str();
            let args: Vec<String> = opener(url)
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();

            assert!(
                url.starts_with("https://"),
                "{name} is not an https address: {url}"
            );
            assert!(
                args.contains(&url.to_owned()),
                "the opener must be handed {url}, got {args:?}"
            );
            assert_eq!(args.len(), expected_args(), "no argument but the URL");
        }
    }

    /// A name nobody offers is refused, and the refusal lists the names that
    /// are offered rather than a sentence somebody has to remember to edit.
    #[test]
    fn a_page_nobody_offers_is_refused_and_the_refusal_names_the_ones_that_exist() {
        let refused = support("merch".to_owned()).expect_err("no page is called merch");
        let said = refused.to_string();

        for (name, _) in addresses() {
            assert!(said.contains(&name), "{said:?} does not mention {name}");
        }
    }

    /// The About dialog's links, which are the reason the table is generated:
    /// it has a notice in its hand and asks for that component by name.
    ///
    /// A build without `mp3` links no libmp3lame and must offer no page for it,
    /// which is the half of this that a default build cannot fail - the gate's
    /// `features` leg builds the one where it can.
    #[test]
    fn the_about_dialog_can_reach_everything_it_lists() {
        let about = About::current();
        let offered: Vec<String> = addresses().into_iter().map(|(name, _)| name).collect();

        assert!(
            offered.iter().any(|name| name == "repository"),
            "the logo has nowhere to go: {offered:?}"
        );
        for notice in &about.notices {
            assert!(
                offered.contains(&notice.component),
                "{} is listed and cannot be opened: {offered:?}",
                notice.component
            );
        }
        assert_eq!(
            offered.len(),
            PAGES.len() + 1 + about.notices.len(),
            "the table offers something the dialog does not list: {offered:?}"
        );
    }

    /// How many arguments the platform's opener is given, the URL included.
    const fn expected_args() -> usize {
        if cfg!(target_os = "windows") { 4 } else { 1 }
    }
}
