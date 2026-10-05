/*
 *  genres.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Turning what a provider calls a genre into what the catalog calls one (§32).
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

//! Turning what a provider calls a genre into what the catalog calls one (§32).
//!
//! Providers are not tidy. Discogs hands back `Electronic` as a genre and
//! `Dub Techno, Ambient` as styles; MusicBrainz hands back lowercase tags voted on
//! by strangers; and the tags on a 1994 pressing were typed by whoever cataloged
//! it. §32 asks for a normalized genre, and the answer is a lookup table with a
//! pass-through default, ported from VRipr's `assets/genre.dat` - 639 mappings
//! built from a database dump, and the only part of VRipr's metadata code worth
//! keeping verbatim.
//!
//! # The three rules
//!
//! 1. **Exact match wins.** `HH` is `Hip-Hop; Hip Hop`, and nothing else matches
//!    `HH`.
//! 2. **Case-insensitive next.** `indierock` finds `IndieRock`.
//! 3. **Unknown passes through unchanged.** The data file says so in its own
//!    header - "for safety values pass-thru if not defined here" - and it is the
//!    right default: a genre nobody anticipated is still what the record is, and
//!    silently dropping it would lose information the provider had.
//!
//! One key can expand to several genres, which is how `Folk Pop` becomes
//! `Folk Pop; Folk; Pop`: a broad genre for browsing and a specific one for
//! knowing what it is. Output is deduplicated case-insensitively, in first-seen
//! order, because order here is a ranking - the most specific answer first.
//!
//! # What changed in the port
//!
//! VRipr held the map in a `OnceLock<RwLock<GenreState>>` global and reloaded it
//! by mutating that global. [`Genres`] is a plain owned value instead. A global
//! that a custom file can swap under a running catalog means two exports in the
//! same session can disagree about what a genre is, and it cannot be tested
//! without leaking state between tests. A value is passed in, and a caller that
//! wants a custom file constructs a second one.
//!
//! The case-insensitive fallback is also no longer a linear scan of all 639 keys
//! per miss. A second lowercase-keyed index is built once, which matters because
//! genre normalization runs over every track of every release on import.
//!
//! That index also made a latent bug visible. Twenty-two of the table's keys
//! collide when lowercased, and five of those collisions have *different* answers:
//! `HardRock` gives `Hard Rock; Rock` while `Hardrock` gives only `Hard Rock`, and
//! `J-pop`, `Jpop`, `Electro-acoustic` and `Non-music` each have a differently
//! capitalised twin. VRipr resolved them with `HashMap::iter().find()`, so
//! normalizing `HARDROCK` gave one answer or the other depending on hash order
//! within the run. Here the first spelling in file order wins, every time. The
//! ported parity fixture excludes exactly those five folds, because VRipr's answer
//! for them is not a fact to be held to.
//!
//! Seven keys are also *exactly* repeated, four of them with different answers
//! (`J-Pop`, `JPop`, `Reggae-Pop` and `Techno, Experimental, Ambient`), and there
//! the later row wins - which is how a data file appends a correction rather than
//! contradicting itself. 639 rows, 632 keys.

use std::collections::HashMap;
use std::path::Path;

/// The mapping table shipped with VCW, ported from VRipr.
pub const BUILTIN: &str = include_str!("../assets/genre.dat");

/// A genre mapping table.
///
/// Cheap to clone the results of, expensive enough to build that one is made per
/// catalog rather than per track. [`Genres::builtin`] parses 639 rows into 632
/// keys.
#[derive(Debug, Clone)]
pub struct Genres {
    /// Keys exactly as the data file spells them.
    exact: HashMap<String, Vec<String>>,
    /// The same keys lowercased, for rule 2. Built once instead of scanned, and
    /// first-in-file-order-wins where two keys fold together.
    folded: HashMap<String, Vec<String>>,
}

impl Genres {
    /// The table shipped with VCW: 639 rows, 632 keys.
    #[must_use]
    pub fn builtin() -> Self {
        Self::parse(BUILTIN)
    }

    /// A table with no mappings, so everything passes through.
    ///
    /// Useful for a caller who wants the trimming and deduplication without the
    /// opinions, and for proving that the pass-through default is the default.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            exact: HashMap::new(),
            folded: HashMap::new(),
        }
    }

    /// A table parsed from the data-file format.
    ///
    /// One mapping per line, `Key|Target` or `Key|Target;Target;...`. Blank lines
    /// and lines starting with `#` are comments. A line with no `|`, an empty key
    /// or an empty target list is skipped rather than being an error: the file is
    /// data, and one bad row should not cost a user every other mapping in it.
    #[must_use]
    pub fn parse(data: &str) -> Self {
        // Two passes, because both kinds of duplicate key have to resolve the same
        // way every run. An exactly repeated key means what the *last* row says,
        // which is how a file appends a correction. A fold collision resolves to
        // the *first* spelling in file order - and then to whatever that spelling
        // finally means, which is why the fold cannot be built during pass one.
        let mut order: Vec<String> = Vec::new();
        let mut exact: HashMap<String, Vec<String>> = HashMap::new();
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, targets)) = line.split_once('|') else {
                continue;
            };
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            let targets: Vec<String> = split_list(targets);
            if targets.is_empty() {
                continue;
            }
            if exact.insert(key.to_string(), targets).is_none() {
                order.push(key.to_string());
            }
        }
        let mut folded: HashMap<String, Vec<String>> = HashMap::new();
        for key in &order {
            let Some(targets) = exact.get(key) else {
                continue;
            };
            folded
                .entry(key.to_lowercase())
                .or_insert_with(|| targets.clone());
        }
        Self { exact, folded }
    }

    /// A table read from a file, or the built-in one if it cannot be read.
    ///
    /// Falling back rather than failing is deliberate and matches VRipr: a typo in
    /// a settings path should not leave a user with no genre mapping at all. The
    /// caller is told which happened by the `bool`, so a UI can say so.
    #[must_use]
    pub fn from_file_or_builtin(path: &Path) -> (Self, bool) {
        match std::fs::read_to_string(path) {
            Ok(data) => (Self::parse(&data), true),
            Err(_) => (Self::builtin(), false),
        }
    }

    /// How many keys the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.exact.len()
    }

    /// Whether the table maps nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty()
    }

    /// What one genre name maps to, or `None` if the table does not know it.
    ///
    /// Exact match first, then case-insensitive. `None` means rule 3 applies and
    /// the caller should keep the name it has.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&[String]> {
        let name = name.trim();
        if let Some(targets) = self.exact.get(name) {
            return Some(targets);
        }
        self.folded.get(&name.to_lowercase()).map(Vec::as_slice)
    }

    /// Normalizes a semicolon-delimited genre string.
    ///
    /// This is VRipr's `sanitize_genres`, and the input format is semicolon
    /// delimited because that is what the providers' fields concatenate to.
    ///
    /// ```
    /// # use vcw_metadata::Genres;
    /// let genres = Genres::builtin();
    /// assert_eq!(genres.normalize("Folk Pop"), ["Folk Pop", "Folk", "Pop"]);
    /// assert_eq!(genres.normalize("HH; Hip-Hop"), ["Hip-Hop", "Hip Hop"]);
    /// assert_eq!(genres.normalize("Shoegaze Revival"), ["Shoegaze Revival"]);
    /// ```
    #[must_use]
    pub fn normalize(&self, input: &str) -> Vec<String> {
        self.normalize_all(input.split(';'))
    }

    /// Normalizes a list of genre names, which is how a provider hands them over.
    ///
    /// Each name is itself split on `;`, so a provider field that concatenated two
    /// genres into one string still comes out as two.
    #[must_use]
    pub fn normalize_all<I, S>(&self, names: I) -> Vec<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out: Vec<String> = Vec::new();
        for name in names {
            for part in split_list(name.as_ref()) {
                match self.lookup(&part) {
                    Some(targets) => {
                        for target in targets {
                            push_once(&mut out, target);
                        }
                    }
                    None => push_once(&mut out, &part),
                }
            }
        }
        out
    }
}

impl Default for Genres {
    fn default() -> Self {
        Self::builtin()
    }
}

/// Splits a semicolon-delimited list, trimming and dropping empties.
fn split_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Appends unless something equal ignoring case is already there.
fn push_once(out: &mut Vec<String>, value: &str) {
    if !out.iter().any(|seen| seen.eq_ignore_ascii_case(value)) {
        out.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_table_is_the_whole_file() {
        let genres = Genres::builtin();
        assert_eq!(
            genres.len(),
            632,
            "639 rows, 7 of which repeat a key; a change here means the data file changed"
        );
        assert!(!genres.is_empty());
    }

    #[test]
    fn an_abbreviation_expands() {
        let genres = Genres::builtin();
        assert_eq!(genres.normalize("Mn"), ["Minimal"]);
        assert_eq!(genres.normalize("DT"), ["Dub Techno"]);
        assert_eq!(genres.normalize("J"), ["Jazz"]);
    }

    #[test]
    fn one_key_can_mean_several_genres_most_specific_first() {
        let genres = Genres::builtin();
        assert_eq!(
            genres.normalize("Southern Rock"),
            ["Southern Rock", "Rock"],
            "the broad genre comes after the specific one, which is the ranking"
        );
        assert_eq!(genres.normalize("IndieRock"), ["Indie Rock", "Indie"]);
    }

    #[test]
    fn case_does_not_matter_but_the_exact_spelling_wins() {
        let genres = Genres::parse("Rock|Rock\nrock|Lowercase Rock\n");
        assert_eq!(genres.normalize("Rock"), ["Rock"], "rule 1");
        assert_eq!(genres.normalize("rock"), ["Lowercase Rock"], "also rule 1");
        assert_eq!(
            genres.normalize("ROCK"),
            ["Rock"],
            "rule 2, and the first key in file order is the one it found"
        );
    }

    #[test]
    fn a_repeated_key_means_what_the_later_row_says() {
        let genres = Genres::builtin();
        assert_eq!(
            genres.normalize("Reggae-Pop"),
            ["Reggae Pop"],
            "the file says Reggae;Pop;Reggae-Pop at line 100-odd and Reggae Pop later"
        );
        assert_eq!(genres.normalize("J-Pop"), ["J-Pop"]);
        assert_eq!(
            genres.normalize("Techno, Experimental, Ambient"),
            ["Techno", "Experimental", "Ambient"],
            "the later row dropped Electronic from the front"
        );
    }

    #[test]
    fn a_folding_collision_resolves_the_same_way_every_time() {
        // Five real pairs in the shipped table disagree about their answer. The
        // file lists HardRock before Hardrock, so HardRock is what HARDROCK means.
        let genres = Genres::builtin();
        assert_eq!(genres.normalize("HardRock"), ["Hard Rock", "Rock"]);
        assert_eq!(genres.normalize("Hardrock"), ["Hard Rock"]);
        for _ in 0..32 {
            assert_eq!(
                Genres::builtin().normalize("HARDROCK"),
                ["Hard Rock", "Rock"],
                "a fresh table each time, and the same answer each time"
            );
        }
    }

    #[test]
    fn an_unknown_genre_passes_through_unchanged() {
        let genres = Genres::builtin();
        assert_eq!(
            genres.normalize("Hauntological Library Music"),
            ["Hauntological Library Music"],
            "the data file's own header promises this"
        );
        assert!(Genres::empty().normalize("Anything") == ["Anything"]);
    }

    #[test]
    fn duplicates_collapse_across_expansions() {
        let genres = Genres::builtin();
        assert_eq!(
            genres.normalize("Folk Pop; Pop; Folk"),
            ["Folk Pop", "Folk", "Pop"],
            "Pop and Folk arrived from the expansion already"
        );
        assert_eq!(genres.normalize("Jazz; jazz; JAZZ"), ["Jazz"]);
    }

    #[test]
    fn whitespace_and_empties_are_not_genres() {
        let genres = Genres::builtin();
        assert!(genres.normalize("").is_empty());
        assert!(genres.normalize("  ;  ; ").is_empty());
        assert_eq!(genres.normalize("  Mn  "), ["Minimal"]);
    }

    #[test]
    fn a_provider_list_is_normalized_as_one_run() {
        let genres = Genres::builtin();
        assert_eq!(
            genres.normalize_all(["Electronic", "DT", "Mn"]),
            ["Electronic", "Dub Techno", "Minimal"]
        );
        assert_eq!(
            genres.normalize_all(["HH; Jazz"]),
            ["Hip-Hop", "Hip Hop", "Jazz"],
            "a single string holding two genres still comes out as two"
        );
    }

    #[test]
    fn a_broken_row_costs_only_that_row() {
        let genres = Genres::parse(
            "# a comment\n\nGood|Fine\nno pipe here\n|Empty key\nEmptyTargets|\nAlso|Good\n",
        );
        assert_eq!(genres.len(), 2);
        assert_eq!(genres.normalize("Good"), ["Fine"]);
        assert_eq!(genres.normalize("Also"), ["Good"]);
        assert_eq!(
            genres.normalize("EmptyTargets"),
            ["EmptyTargets"],
            "skipped, so it passes through"
        );
    }

    #[test]
    fn a_missing_file_falls_back_to_the_builtin_and_says_so() {
        let (genres, from_file) = Genres::from_file_or_builtin(Path::new("/nonexistent/genre.dat"));
        assert!(!from_file);
        assert_eq!(genres.len(), 632);
    }

    #[test]
    fn a_file_that_exists_is_used() {
        let dir = tempfile::tempdir().expect("a tempdir");
        let path = dir.path().join("genre.dat");
        std::fs::write(&path, "Krautrock|Krautrock;Rock\n").expect("write");
        let (genres, from_file) = Genres::from_file_or_builtin(&path);
        assert!(from_file);
        assert_eq!(genres.len(), 1);
        assert_eq!(genres.normalize("Krautrock"), ["Krautrock", "Rock"]);
    }

    #[test]
    fn lookup_reports_a_miss_rather_than_inventing_an_answer() {
        let genres = Genres::builtin();
        assert!(genres.lookup("Mn").is_some());
        assert!(genres.lookup("mn").is_some(), "rule 2");
        assert!(genres.lookup("Definitely Not A Genre Key").is_none());
    }

    #[test]
    fn the_table_has_no_em_dashes_or_other_typography() {
        assert!(
            BUILTIN.is_ascii(),
            "the data file is ASCII, so a genre cannot arrive with smart punctuation in it"
        );
    }
}
