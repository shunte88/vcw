/*
 *  naming.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The naming templates: tokens in, a relative path out (§33).
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

//! The naming templates: tokens in, a relative path out (§33).
//!
//! Ported from VRipr's `apply_path_template`, tokens and all, because the point
//! of a template is that the library someone built with the old tool keeps the
//! shape it had. Two things are deliberately different, and both are noted where
//! they happen: VCW knows the vinyl topology, so `{side}`, `{position}` and
//! `{disc}` are new tokens, and [`collapse_brackets`] fixes an original that only
//! ever collapsed the first group it looked at.
//!
//! Nothing here touches a database. A [`Values`] is a flat set of strings, which
//! is what makes the substitution rules testable without a project - and the
//! rules are where the surprises are.

use std::path::{Path, PathBuf};

/// The default template: one folder per release, `01 - Title` inside it.
///
/// Track number first because a directory listing sorted by name is then in
/// playing order, which is the whole reason people put it there.
pub const DEFAULT_TEMPLATE: &str = "{album_artist}/{album}/{tracknum} - {title}";

/// Every token a template may use.
///
/// `side`, `position` and `disc` are VCW's; the rest are VRipr's, spelled the
/// same way so an old template still expands.
pub const TOKENS: &[&str] = &[
    "title",
    "artist",
    "album",
    "album_artist",
    "genre",
    "year",
    "tracknum",
    "composer",
    "country",
    "country_iso",
    "catalog",
    "label",
    "discogs_id",
    "side",
    "position",
    "disc",
];

/// What a template is expanded against.
///
/// Strings rather than the project's own types on purpose: this is the boundary
/// where a release, a track and a side become text, and the one place that
/// conversion happens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values {
    /// Track title.
    pub title: String,
    /// Track artist, already resolved against the release's.
    pub artist: String,
    /// Release title.
    pub album: String,
    /// Release artist.
    pub album_artist: String,
    /// Genres, `'; '`-separated as they are stored.
    pub genre: String,
    /// Release year, or empty.
    pub year: String,
    /// Track number, as typed into the template (zero-padded on expansion).
    pub tracknum: String,
    /// Composer, already resolved against the release's.
    pub composer: String,
    /// Country of the pressing, as the provider spelled it.
    pub country: String,
    /// Catalogue number.
    pub catalog: String,
    /// Record label.
    pub label: String,
    /// Discogs release id.
    pub discogs_id: String,
    /// Side letter: `A`, `B`, `C`.
    pub side: String,
    /// Position on the record: `A1`, or `1` under numeric numbering.
    pub position: String,
    /// Disc number, one-based.
    pub disc: String,
}

impl Values {
    /// Every field made safe for a single path segment.
    ///
    /// Used by [`path_for`] before substitution. [`expand`] deliberately does
    /// not do this: its output is text - a comment, a log line, a label in the
    /// interface - where a slash is just a slash.
    #[must_use]
    pub fn sanitised(&self) -> Self {
        // A blank field must stay blank. `sanitise` answers "Unknown" for an
        // empty segment, which is right for a whole segment and wrong for a
        // token, because `[{year}]` can only collapse while the year is empty.
        let clean = |value: &String| {
            if value.trim().is_empty() {
                String::new()
            } else {
                sanitise(value)
            }
        };
        Self {
            title: clean(&self.title),
            artist: clean(&self.artist),
            album: clean(&self.album),
            album_artist: clean(&self.album_artist),
            genre: clean(&self.genre),
            year: clean(&self.year),
            tracknum: clean(&self.tracknum),
            composer: clean(&self.composer),
            country: clean(&self.country),
            catalog: clean(&self.catalog),
            label: clean(&self.label),
            discogs_id: clean(&self.discogs_id),
            side: clean(&self.side),
            position: clean(&self.position),
            disc: clean(&self.disc),
        }
    }
}

/// An unknown token, and what was probably meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown {
    /// The token as written, without braces.
    pub token: String,
    /// The closest supported token, where one is close enough to name.
    pub suggestion: Option<String>,
}

impl std::fmt::Display for Unknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.suggestion {
            Some(s) => write!(f, "{{{}}} (did you mean {{{s}}}?)", self.token),
            None => write!(f, "{{{}}}", self.token),
        }
    }
}

/// Finds every token a template uses that does not exist.
///
/// An unclosed brace is not an error. A template is typed by a person into a
/// settings field, and complaining about `{tit` while they are still typing it is
/// the kind of help nobody wants; the expansion leaves it alone as literal text.
#[must_use]
pub fn validate(template: &str) -> Vec<Unknown> {
    let mut unknown = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('}') else { break };
        let token = &rest[..close];
        rest = &rest[close + 1..];
        if !token.is_empty() && !TOKENS.contains(&token) {
            unknown.push(Unknown {
                token: token.to_owned(),
                suggestion: suggest(token),
            });
        }
    }
    unknown
}

/// The closest supported token to something unrecognised.
///
/// The alias table first, because the common cases are not spelling mistakes but
/// other tools' names for the same thing, and edit distance would sooner map
/// `track` to `title` than to `tracknum`.
#[must_use]
pub fn suggest(unknown: &str) -> Option<String> {
    const ALIASES: &[(&str, &str)] = &[
        ("track", "tracknum"),
        ("track_number", "tracknum"),
        ("track_no", "tracknum"),
        ("trackno", "tracknum"),
        ("track_num", "tracknum"),
        ("number", "tracknum"),
        ("num", "tracknum"),
        ("album_name", "album"),
        ("album_title", "album"),
        ("record", "album"),
        ("country_code", "country_iso"),
        ("iso", "country_iso"),
        ("iso_country", "country_iso"),
        ("catno", "catalog"),
        ("cat_no", "catalog"),
        ("catalogue", "catalog"),
        ("catalognumber", "catalog"),
        ("organization", "label"),
        ("publisher", "label"),
        ("date", "year"),
        ("released", "year"),
        ("style", "genre"),
        ("styles", "genre"),
        ("albumartist", "album_artist"),
        ("band", "album_artist"),
        ("performer", "artist"),
        ("writer", "composer"),
        ("face", "side"),
        ("disk", "disc"),
        ("discogs", "discogs_id"),
        ("discogs_release_id", "discogs_id"),
    ];
    let lower = unknown.to_lowercase();
    if let Some((_, to)) = ALIASES.iter().find(|(from, _)| *from == lower) {
        return Some((*to).to_owned());
    }
    // Edit distance, but only when it is close enough to be a typo rather than a
    // different word: a third of the token's length, at least one and at most
    // three, so `titel` finds `title` and `bitrate` finds nothing.
    //
    // `titel` is the reason the distance counts a transposition as one edit and
    // not two. Plain Levenshtein puts it two from `title`, which a budget of one
    // rejects - and swapped adjacent letters are the typo people actually make.
    let budget = (lower.chars().count() / 3).clamp(1, 3);
    TOKENS
        .iter()
        .map(|token| (token, distance(&lower, token)))
        .filter(|&(_, d)| d <= budget)
        .min_by_key(|&(_, d)| d)
        .map(|(token, _)| (*token).to_owned())
}

/// Damerau-Levenshtein distance, restricted to adjacent transpositions.
///
/// Three rows rather than a matrix: the transposition case needs the row before
/// the previous one, which is the only reason this is not the two-row
/// Levenshtein it started as.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut before: Vec<usize> = vec![0; b.len() + 1];
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, &ac) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, &bc) in b.iter().enumerate() {
            let substitute = previous[j] + usize::from(ac != bc);
            let mut best = substitute.min(previous[j + 1] + 1).min(current[j] + 1);
            if i > 0 && j > 0 && ac == b[j - 1] && a[i - 1] == bc {
                best = best.min(before[j - 1] + 1);
            }
            current[j + 1] = best;
        }
        std::mem::swap(&mut before, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// Expands a template into text, with empty bracket groups removed.
///
/// The track number is zero-padded to two digits, because a template that says
/// `{tracknum}` wants `01` and nobody writes `{tracknum:02}`. A number that is
/// not a number - an alpha position typed into the wrong field - is passed
/// through as it stands.
///
/// A `{title}` with nothing behind it becomes [`UNTITLED`] unless it sits
/// inside a bracket group, where the group's own rule wins and it stays blank.
/// The tag is not touched: [`crate::splitter`] builds the tags from the record
/// separately, so an untitled track keeps an empty title tag and gains the word
/// only in its file name.
#[must_use]
pub fn expand(template: &str, values: &Values) -> String {
    let tracknum = if values.tracknum.is_empty() {
        "00".to_owned()
    } else {
        values
            .tracknum
            .parse::<u32>()
            .map_or_else(|_| values.tracknum.clone(), |n| format!("{n:02}"))
    };

    // A title nobody has filled in yet gets a word, the way an empty
    // `{tracknum}` gets `00` just above. Without it the default template left
    // the separator standing with nothing after it - `A2 -.flac`, four of them
    // on a two-sided rip, which is what a real 192 kHz capture of an unnamed
    // side produced. Done before the substitution rather than after, because
    // afterwards there is no way to tell a title that is absent from a title
    // that is genuinely blank.
    let named;
    let template = if values.title.trim().is_empty() {
        named = name_the_untitled(template);
        named.as_str()
    } else {
        template
    };

    let mut out = template.to_owned();
    for (token, value) in [
        ("{title}", values.title.as_str()),
        ("{artist}", values.artist.as_str()),
        ("{album}", values.album.as_str()),
        ("{album_artist}", values.album_artist.as_str()),
        ("{genre}", values.genre.as_str()),
        ("{year}", values.year.as_str()),
        ("{tracknum}", tracknum.as_str()),
        ("{composer}", values.composer.as_str()),
        ("{country}", values.country.as_str()),
        ("{country_iso}", country_iso(&values.country)),
        ("{catalog}", values.catalog.as_str()),
        ("{label}", values.label.as_str()),
        ("{discogs_id}", values.discogs_id.as_str()),
        ("{side}", values.side.as_str()),
        ("{position}", values.position.as_str()),
        ("{disc}", values.disc.as_str()),
    ] {
        if out.contains(token) {
            out = out.replace(token, value);
        }
    }
    collapse_brackets(&out)
}

/// What an unnamed track is called.
pub const UNTITLED: &str = "Untitled";

/// Puts [`UNTITLED`] where `{title}` stands outside any bracket group.
///
/// Outside, because `[...]` is already the way a template says *only if there
/// is one* - see [`collapse_brackets`]. Someone who wrote
/// `{tracknum}[ - {title}]` to work around the dangling separator asked for the
/// whole group to vanish, and substituting a word into it would quietly take
/// that back and start writing `A2 - Untitled` where they had arranged for
/// `A2`. So the group keeps the old behaviour exactly, and an unbracketed
/// `{title}` - which is what the default template has - gets the word.
fn name_the_untitled(template: &str) -> String {
    let mut out = String::with_capacity(template.len() + UNTITLED.len());
    let mut depth = 0usize;
    let mut rest = template;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("{title}") {
            out.push_str(if depth == 0 { UNTITLED } else { "{title}" });
            rest = tail;
            continue;
        }
        let Some(ch) = rest.chars().next() else { break };
        match ch {
            '[' => depth += 1,
            // Saturating, because a template with a stray `]` is a typo and not
            // a reason to panic; `collapse_brackets` leaves it alone too.
            ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// Expands a template into a relative path.
///
/// Segments are split on `/` whatever the platform is, because a template is
/// stored in settings and copied between machines. The values are sanitised
/// *before* they go into the template rather than after, which is what keeps the
/// two kinds of slash apart: the operator's `/` in `{album}/{title}` is a
/// directory, and a provider's `/` in `AC/DC Medley` is a character in a name.
/// Sanitising the joined-up string cannot tell them apart, and one folder called
/// `AC` is nobody's intention.
///
/// The segments are then sanitised again, which is not redundant: it catches the
/// hazards the operator typed into the template itself.
///
/// An expansion that comes out empty - every token blank - falls back to the
/// track number, since a file still has to be called something.
#[must_use]
pub fn path_for(template: &str, values: &Values) -> PathBuf {
    let expanded = expand(template, &values.sanitised());
    let mut path = PathBuf::new();
    for segment in expanded.split('/') {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        path.push(sanitise(trimmed));
    }
    if path.as_os_str().is_empty() {
        let number = if values.tracknum.is_empty() {
            "00"
        } else {
            &values.tracknum
        };
        path.push(format!("{number} - Unknown"));
    }
    path
}

/// Makes one path segment safe on every platform VCW ships to.
///
/// VRipr replaced the nine characters Windows forbids. This adds three things
/// that only show up on a real library: control characters, which a tag from a
/// badly encoded release really does contain; trailing dots and spaces, which
/// Windows accepts in an API call and then cannot open; and the reserved device
/// names, where `AUX` is a file nobody can create. A track called `AUX` is
/// unlikely and `Aux` is a French word, so the check is on the stem and
/// case-insensitive.
#[must_use]
pub fn sanitise(segment: &str) -> String {
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if segment.is_empty() {
        return "Unknown".to_owned();
    }
    let mut out: String = segment
        .chars()
        .map(|c| match c {
            '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\\' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    if out.is_empty() {
        return "Unknown".to_owned();
    }
    let stem = out.split('.').next().unwrap_or(&out).to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.push('_');
    }
    out
}

/// Removes `[...]` groups whose contents came out empty.
///
/// This is how a template says *only if there is one*: `[{year}] {title}` gives
/// `1988 Desire Lines` on a release with a year and `Desire Lines` on one
/// without.
///
/// **Divergence from VRipr, deliberately.** The original found the first `[` in
/// the string each time round its loop and stopped as soon as that one turned out
/// to be non-empty, so `[{year}] - [{catalog}]` with a year and no catalogue
/// number left a bare `[]` in the file name. This scans the whole string and
/// repeats until nothing changes, so nested and later groups both go. Templates
/// are not a parity corpus - nobody's library depends on VCW reproducing a
/// stray bracket - so this one is fixed rather than reproduced.
#[must_use]
pub fn collapse_brackets(text: &str) -> String {
    let mut out = text.to_owned();
    loop {
        let mut changed = false;
        let mut result = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(open) = rest.find('[') {
            let (before, from_open) = rest.split_at(open);
            result.push_str(before);
            let inside = &from_open[1..];
            let Some(close) = inside.find(']') else {
                // An unclosed bracket is literal text, like an unclosed brace.
                result.push_str(from_open);
                rest = "";
                break;
            };
            if inside[..close].trim().is_empty() {
                changed = true;
            } else {
                result.push('[');
                result.push_str(&inside[..close]);
                result.push(']');
            }
            rest = &inside[close + 1..];
        }
        result.push_str(rest);
        out = result;
        if !changed {
            break;
        }
    }
    out
}

/// Maps a provider's country name to its ISO 3166-1 alpha-2 code.
///
/// VRipr's table, unchanged. Anything not in it is returned as it stands, which
/// covers the two cases that matter: a name already given as a code, and a
/// country nobody has typed yet.
#[must_use]
pub fn country_iso(country: &str) -> &str {
    match country.trim() {
        "UK" => "GB",
        "Germany" => "DE",
        "France" => "FR",
        "Japan" => "JP",
        "Italy" => "IT",
        "Netherlands" => "NL",
        "Australia" => "AU",
        "Canada" => "CA",
        "Spain" => "ES",
        "Brazil" => "BR",
        "Belgium" => "BE",
        "Sweden" => "SE",
        "Norway" => "NO",
        "Denmark" => "DK",
        "Finland" => "FI",
        "Switzerland" => "CH",
        "Austria" => "AT",
        "New Zealand" => "NZ",
        "South Africa" => "ZA",
        "Mexico" => "MX",
        "Argentina" => "AR",
        "Portugal" => "PT",
        "Greece" => "GR",
        "Poland" => "PL",
        "Czech Republic" => "CZ",
        "Hungary" => "HU",
        "Romania" => "RO",
        "Bulgaria" => "BG",
        "Russia" => "RU",
        "Yugoslavia" => "YU",
        "India" => "IN",
        "South Korea" => "KR",
        "Taiwan" => "TW",
        "Hong Kong" => "HK",
        "Israel" => "IL",
        "Turkey" => "TR",
        "Venezuela" => "VE",
        "Colombia" => "CO",
        "Chile" => "CL",
        "Uruguay" => "UY",
        "Ireland" => "IE",
        "Iceland" => "IS",
        other => other,
    }
}

/// Whether a path stays inside the directory it is joined to.
///
/// A template is a person's own text, so this is not a security boundary - but a
/// title read off a provider is *not* their text, and `..` in one would write
/// outside the folder they chose. [`sanitise`] leaves dots alone deliberately,
/// because `Vol. 2` and `Mr. Bungle` are ordinary names, so the check happens
/// here instead.
#[must_use]
pub fn stays_within(relative: &Path) -> bool {
    !relative.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Values {
        Values {
            title: "Desire Lines".into(),
            artist: "Lush".into(),
            album: "Split".into(),
            album_artist: "Lush".into(),
            genre: "Shoegaze; Dream Pop".into(),
            year: "1994".into(),
            tracknum: "3".into(),
            composer: "Emma Anderson".into(),
            country: "UK".into(),
            catalog: "CAD 4014".into(),
            label: "4AD".into(),
            discogs_id: "372951".into(),
            side: "A".into(),
            position: "A3".into(),
            disc: "1".into(),
        }
    }

    #[test]
    fn the_default_template_gives_a_sortable_listing() {
        let path = path_for(DEFAULT_TEMPLATE, &values());
        assert_eq!(path, PathBuf::from("Lush/Split/03 - Desire Lines"));
    }

    #[test]
    fn a_track_number_is_padded_to_two_digits() {
        // Because a directory listing is sorted as text, and `10` sorts before
        // `3`. Nobody writes `{tracknum:02}` in a settings field.
        let mut v = values();
        v.tracknum = "7".into();
        assert_eq!(expand("{tracknum}", &v), "07");
        v.tracknum = "12".into();
        assert_eq!(expand("{tracknum}", &v), "12");
        v.tracknum = String::new();
        assert_eq!(expand("{tracknum}", &v), "00");
        // An alpha position typed into the wrong field is passed through rather
        // than turned into 00, which would lose it silently.
        v.tracknum = "A3".into();
        assert_eq!(expand("{tracknum}", &v), "A3");
    }

    #[test]
    fn every_token_expands() {
        // The catch-all: a token in TOKENS that expand() forgot would silently
        // survive into the file name as literal braces.
        let v = values();
        for token in TOKENS {
            let expanded = expand(&format!("{{{token}}}"), &v);
            assert!(
                !expanded.contains('{'),
                "token {token} is listed but not substituted"
            );
            assert!(!expanded.is_empty(), "token {token} expanded to nothing");
        }
    }

    #[test]
    fn the_vinyl_tokens_are_ours_to_add() {
        let v = values();
        assert_eq!(
            expand("{disc}{side} {position} {title}", &v),
            "1A A3 Desire Lines"
        );
    }

    #[test]
    fn a_track_with_no_title_is_called_untitled() {
        let mut v = values();
        v.title = String::new();
        // The default template, which is where this actually bit: `A2 -.flac`,
        // a file name ending in a separator with nothing after it.
        assert_eq!(
            path_for(DEFAULT_TEMPLATE, &v),
            PathBuf::from("Lush/Split/03 - Untitled")
        );
        // Whitespace counts as none. A title of one space came out of a real
        // provider row, and it sanitises to nothing a moment later anyway.
        v.title = "   ".into();
        assert_eq!(expand("{tracknum} - {title}", &v), "03 - Untitled");
    }

    #[test]
    fn a_title_that_is_set_is_left_alone() {
        let v = values();
        assert_eq!(expand("{title}", &v), "Desire Lines");
        assert!(!expand(DEFAULT_TEMPLATE, &v).contains(UNTITLED));
    }

    #[test]
    fn a_bracketed_title_still_disappears_when_there_is_none() {
        let mut v = values();
        v.title = String::new();
        // `[...]` is the existing way to say *only if there is one*, and
        // someone who wrote this asked for the title and its brackets to go
        // together. Substituting a word inside the group would take that back.
        assert_eq!(expand("{tracknum}[{title}]", &v), "03");
        // Both halves of what the user guide says about `[...]`: the group
        // goes when it is blank, and keeps its brackets when it is not - which
        // is why there is no template that drops the separator only on the
        // untitled tracks. Pinned here so the guide cannot go stale.
        assert_eq!(expand("{tracknum}[ {title}]", &v), "03");
        v.title = "Desire Lines".into();
        assert_eq!(expand("{tracknum}[ {title}]", &v), "03[ Desire Lines]");
        v.title = String::new();
        // A collapse takes the group and nothing around it, which is the rule
        // `[{year}] {title}` already demonstrates - so the space stays.
        assert_eq!(expand("{tracknum} [{title}]", &v), "03 ");
        // Outside the group it is named, in the same template, so the two
        // rules do not fight: whichever `{title}` you wrote is what you get.
        assert_eq!(
            expand("{tracknum} - {title}[ ({year})]", &v),
            "03 - Untitled[ (1994)]"
        );
        // A group only collapses when it trims to nothing, which is the rule
        // `collapse_brackets` already had - so punctuation inside one keeps the
        // group alive and the title stays blank there. Pinned because it is the
        // reason the bracket form was never a workaround for the dangling
        // separator: `[ - {title}]` leaves the dash behind.
        assert_eq!(expand("{tracknum}[ - {title}]", &v), "03[ - ]");
    }

    #[test]
    fn a_stray_closing_bracket_does_not_panic_or_swallow_the_title() {
        let mut v = values();
        v.title = String::new();
        // A typo in a template someone is still editing. The depth count
        // saturates rather than wrapping, so `{title}` is still at depth zero.
        assert_eq!(expand("]{tracknum} - {title}", &v), "]03 - Untitled");
    }

    #[test]
    fn a_slash_in_a_title_does_not_become_a_directory() {
        // The case that makes per-segment sanitising the rule rather than a
        // detail: this title is real, and one folder called `AC` is not what
        // anyone meant.
        let mut v = values();
        v.title = "AC/DC Medley".into();
        assert_eq!(
            path_for("{album}/{title}", &v),
            PathBuf::from("Split/AC_DC Medley")
        );
    }

    #[test]
    fn windows_only_hazards_are_handled_on_every_platform() {
        // Deliberately not conditional on the platform: the file is exported on
        // Linux and read on Windows more often than not.
        assert_eq!(sanitise("Where Is My Mind?"), "Where Is My Mind_");
        assert_eq!(sanitise("Vol. 2 "), "Vol. 2");
        assert_eq!(sanitise("Trailing..."), "Trailing");
        assert_eq!(sanitise("aux"), "aux_", "a reserved device name");
        assert_eq!(sanitise("aux.wav"), "aux.wav_");
        assert_eq!(sanitise("Auxiliary"), "Auxiliary", "only the whole stem");
        assert_eq!(
            sanitise("Mr. Bungle"),
            "Mr. Bungle",
            "an interior dot is fine"
        );
        assert_eq!(sanitise("bell\u{7}er"), "bell_er", "a control character");
        assert_eq!(sanitise(""), "Unknown");
        assert_eq!(sanitise("   "), "Unknown", "blank after trimming");
    }

    #[test]
    fn empty_bracket_groups_go_and_full_ones_stay() {
        let mut v = values();
        assert_eq!(expand("[{year}] {title}", &v), "[1994] Desire Lines");
        v.year = String::new();
        assert_eq!(expand("[{year}] {title}", &v), " Desire Lines");
    }

    #[test]
    fn a_later_empty_group_goes_too() {
        // The VRipr divergence, pinned. The original stopped at the first group
        // it found if that one was non-empty, so this left `[]` in the name.
        let mut v = values();
        v.catalog = String::new();
        assert_eq!(
            expand("[{year}] - [{catalog}]", &v),
            "[1994] - ",
            "the second group should have gone"
        );
        v.year = String::new();
        assert_eq!(expand("[{year}][{catalog}]", &v), "", "both of them");
    }

    #[test]
    fn an_unclosed_bracket_or_brace_is_literal_text() {
        // Someone is typing. Refusing a half-written template is not help.
        let v = values();
        assert_eq!(expand("[{year} {title}", &v), "[1994 Desire Lines");
        assert_eq!(expand("{tit {title}", &v), "{tit Desire Lines");
        assert!(validate("{tit").is_empty());
    }

    #[test]
    fn unknown_tokens_are_reported_with_a_suggestion() {
        let found = validate("{album}/{tracknum} - {titel}");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].token, "titel");
        assert_eq!(found[0].suggestion.as_deref(), Some("title"));
        assert_eq!(found[0].to_string(), "{titel} (did you mean {title}?)");
    }

    #[test]
    fn the_alias_table_beats_edit_distance() {
        // `track` is one edit from `title` and five from `tracknum`, so distance
        // alone gives the wrong answer for the commonest alias of all.
        assert_eq!(suggest("track").as_deref(), Some("tracknum"));
        assert_eq!(suggest("catno").as_deref(), Some("catalog"));
        assert_eq!(suggest("albumartist").as_deref(), Some("album_artist"));
        assert_eq!(suggest("disk").as_deref(), Some("disc"));
    }

    #[test]
    fn a_word_that_is_not_a_typo_gets_no_suggestion() {
        assert_eq!(suggest("bitrate"), None);
        assert_eq!(suggest("samplerate"), None);
        assert_eq!(suggest("engineer"), None);
    }

    #[test]
    fn a_country_name_becomes_a_code_and_a_code_stays_one() {
        let mut v = values();
        assert_eq!(expand("{country_iso}", &v), "GB", "Discogs writes UK");
        v.country = "Germany".into();
        assert_eq!(expand("{country_iso}", &v), "DE");
        v.country = "US".into();
        assert_eq!(expand("{country_iso}", &v), "US");
        v.country = "Atlantis".into();
        assert_eq!(expand("{country_iso}", &v), "Atlantis", "passed through");
    }

    #[test]
    fn a_template_of_nothing_still_names_a_file() {
        let v = Values {
            tracknum: "2".into(),
            ..Values::default()
        };
        // `{title}` is the one token that is never blank now - it becomes
        // `Untitled` - so a template whose only other token is empty names the
        // file after the placeholder rather than falling through to `Unknown`.
        // Two untitled tracks under this template therefore collide, which
        // `splitter::plan` refuses by name; the default template does not,
        // because it leads with the position.
        assert_eq!(path_for("{album}/{title}", &v), PathBuf::from("Untitled"));
        // The fallback still has a job: a template with no tokens at all, or
        // one whose tokens are all blank and none of them the title.
        assert_eq!(path_for("{album}", &v), PathBuf::from("2 - Unknown"));
        assert_eq!(
            path_for("", &Values::default()),
            PathBuf::from("00 - Unknown")
        );
    }

    #[test]
    fn a_title_cannot_climb_out_of_the_output_directory() {
        // Not the operator's template - a title that came off a provider. The
        // sanitiser leaves dots alone on purpose, so this is checked instead.
        let mut v = values();
        v.title = "../../etc/passwd".into();
        let path = path_for("{album}/{title}", &v);
        assert_eq!(path, PathBuf::from("Split/.._.._etc_passwd"));
        assert!(stays_within(&path));
        assert!(!stays_within(Path::new("../elsewhere/track.wav")));
        assert!(!stays_within(Path::new("/etc/passwd")));
    }
}
