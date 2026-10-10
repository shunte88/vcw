/*
 *  crates/cli/tests/the_user_guide_names_real_commands.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Every command line in the user guide is one the CLI accepts (§42, §50).
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

//! Every command line in the user guide is one the CLI accepts (§42, §50).
//!
//! A user guide is a set of claims about what happens when somebody types
//! something, and nothing in a Markdown file stops a flag from being renamed
//! underneath it. The first draft of `docs/USER-GUIDE.md` documented
//! `vcw export --out`, `vcw tracks --split 3 --at N` and `vcw metadata release
//! <id>`, and the real CLI has `--into`, a `split` subcommand taking two
//! positionals, and `fetch`. All three read perfectly well and all three were
//! wrong.
//!
//! So the guide is checked against the binary's own `--help`, which is the one
//! description of the CLI that cannot drift from it. The check is structural -
//! subcommand names, flag names, and whether a documented line stopped short of
//! a required subcommand - and deliberately not an execution: running every
//! documented line would reach a metadata provider over the network and open an
//! audio device, and a test that needs either is a test that fails for reasons
//! that have nothing to do with the guide.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// What one `--help` page says about itself.
struct Help {
    commands: BTreeSet<String>,
    options: BTreeSet<String>,
    wants_subcommand: bool,
}

fn guide() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate is two levels below the repository root")
        .join("docs/USER-GUIDE.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// `--help` for one path through the verbs.
fn help(path: &[String]) -> Help {
    let output = Command::new(env!("CARGO_BIN_EXE_vcw"))
        .args(path)
        .arg("--help")
        .output()
        .expect("run the CLI");
    assert!(
        output.status.success(),
        "`vcw {} --help` failed: {}",
        path.join(" "),
        String::from_utf8_lossy(&output.stderr),
    );
    let text = String::from_utf8(output.stdout).expect("help is utf-8");

    let mut commands = BTreeSet::new();
    let mut options = BTreeSet::new();
    let mut wants_subcommand = false;
    let mut in_commands = false;
    for line in text.lines() {
        if let Some(usage) = line.strip_prefix("Usage:") {
            wants_subcommand = usage.contains("<COMMAND>");
            continue;
        }
        if line.starts_with("Commands:") {
            in_commands = true;
            continue;
        }
        if line.trim().is_empty() || !line.starts_with(' ') {
            in_commands = false;
        }
        let trimmed = line.trim_start();
        if in_commands {
            if let Some(name) = trimmed.split_whitespace().next() {
                // `help` is clap's own and is not worth documenting.
                if name != "help" {
                    commands.insert(name.to_owned());
                }
            }
            continue;
        }
        // Options are listed one per line, long form first or after the short.
        // A bare `--` is clap's own separator and not a flag; a two-character
        // `-x` is.
        for word in trimmed.split([' ', ',', '<']) {
            let flag = word.starts_with("--") && word.len() > 2
                || word.starts_with('-') && word.len() == 2;
            if flag {
                options.insert(word.to_owned());
            }
        }
    }
    Help {
        commands,
        options,
        wants_subcommand,
    }
}

/// A documented line, split into words, honoring double quotes.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    for character in line.chars() {
        match character {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// Every `vcw ...` line in the guide's fenced blocks, as arguments.
///
/// A line's trailing `# comment` is dropped, an `ENV=value` prefix is ignored,
/// and a line with no bare `vcw` word in it - `export VCW_CONTACT=...` - is not
/// a command and is skipped.
fn documented() -> Vec<(String, Vec<String>)> {
    let text = guide();
    let mut lines = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            continue;
        }
        let code = line.split_once(" #").map_or(line, |(before, _)| before);
        let parts = words(code);
        let Some(at) = parts.iter().position(|word| word == "vcw") else {
            continue;
        };
        lines.push((line.trim().to_owned(), parts[at + 1..].to_vec()));
    }
    assert!(
        lines.len() > 20,
        "only {} command lines found in the guide",
        lines.len(),
    );
    lines
}

#[test]
fn every_command_line_in_the_user_guide_is_one_the_cli_accepts() {
    let mut checked = 0usize;
    for (line, args) in documented() {
        let truncated = args.iter().any(|word| word == "...");
        let mut path: Vec<String> = Vec::new();
        let mut page = help(&path);
        let mut flags: Vec<String> = Vec::new();

        let mut skip_value = false;
        for word in &args {
            if word == "..." {
                continue;
            }
            if let Some(flag) = word.strip_prefix("--") {
                let name = format!("--{}", flag.split('=').next().unwrap_or(flag));
                flags.push(name);
                skip_value = !word.contains('=');
                continue;
            }
            if word.starts_with('-') && word.len() == 2 {
                flags.push(word.clone());
                skip_value = true;
                continue;
            }
            if skip_value {
                // The value of the flag just seen, not a subcommand.
                skip_value = false;
                continue;
            }
            if page.commands.contains(word) {
                path.push(word.clone());
                page = help(&path);
                // A flag written before the subcommand is still the parent's,
                // so what has been collected stays collected.
            }
            // Anything else is a positional: a project, an id, a frame number.
        }

        for flag in &flags {
            assert!(
                page.options.contains(flag)
                    || help(&path[..path.len().saturating_sub(1)])
                        .options
                        .contains(flag),
                "the guide documents `{line}`, and `{flag}` is not a flag of \
                 `vcw {}`. Fix the guide, not this test.",
                path.join(" "),
            );
        }
        assert!(
            !(page.wants_subcommand && !truncated),
            "the guide documents `{line}`, and `vcw {}` needs one of {:?}. \
             Fix the guide, not this test.",
            path.join(" "),
            page.commands,
        );
        checked += 1;
    }
    assert!(checked > 20, "only {checked} lines checked");
}

#[test]
fn the_guide_documents_every_verb_a_person_needs() {
    // The other direction. A guide can be perfectly accurate about the six
    // verbs it happens to mention while leaving a person unable to find the one
    // they need, and §50's workflow is the list of what they need. `capture`,
    // `play`, `waveform` and `soak` are deliberately absent: the first two are
    // what `session` and the window do better, and the last two are
    // instruments rather than workflow.
    let text = guide();
    for verb in [
        "doctor", "devices", "formats", "session", "recover", "import", "detect", "tracks",
        "metadata", "release", "export", "bundle", "serve",
    ] {
        assert!(
            text.contains(&format!("vcw {verb}")),
            "the user guide never mentions `vcw {verb}`",
        );
    }
}

#[test]
fn every_document_links_to_something_that_is_there() {
    // The guide's last section is a table of links to the other documents and
    // to the two worked examples, which is the part of a document that rots
    // first and the part nobody re-reads. Relative links only: an external URL
    // is not this test's business and checking one would need the network.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate is two levels below the repository root")
        .to_owned();

    let mut checked = 0usize;
    for name in ["USER-GUIDE.md", "PROJECT-API.md", "SCHEMA.md"] {
        let path = root.join("docs").join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
        for (at, _) in text.match_indices("](") {
            let target = text[at + 2..]
                .split(')')
                .next()
                .expect("a closing parenthesis")
                .split('#')
                .next()
                .expect("a link target");
            if target.is_empty() || target.contains("://") || target.starts_with("mailto:") {
                continue;
            }
            let resolved = path.parent().expect("docs/").join(target);
            assert!(
                resolved.exists(),
                "{name} links to `{target}`, which is not there",
            );
            checked += 1;
        }
    }
    assert!(checked > 5, "only {checked} relative links checked");
}
