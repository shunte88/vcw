/*
 *  crates/cli/tests/every_file_the_build_needs_is_tracked.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A file the build or a test needs is a file git must keep.
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

//! A file the build or a test needs is a file git must keep.
//!
//! This test exists because `.gitignore` carried `/tools`, written when that
//! directory held VRipr training material, long after the material moved out.
//! By WP-19 the directory held three files the repo depends on: `vcw-read.py`,
//! the independent reader that *is* WP-18's exit criterion; `verify-release.py`,
//! which the release job runs twice; and `make-icons.sh`. All three were
//! invisible to `git status`, so a clone would have failed
//! `third_party_spec.rs` and the release job would have failed on a missing
//! script - and nothing in the fifteen-leg gate could have said so, because the
//! gate runs against the working tree, where the files are present.
//!
//! The check is "exists and is not ignored" rather than "is tracked", because a
//! file that is neither ignored nor tracked is simply new: `git add -A` will
//! take it, and demanding tracked-ness would fail every legitimate new file
//! before its first commit. Ignored-and-needed is the defect; untracked-and-new
//! is a Tuesday.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository root, from this crate's manifest.
fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root is two levels above crates/cli")
}

/// Extensions worth checking. A path-shaped word without one of these is prose.
const EXTENSIONS: &[&str] = &[
    ".py", ".sh", ".rs", ".md", ".toml", ".json", ".yml", ".yaml", ".ts", ".tsx", ".svg", ".png",
    ".ico", ".icns", ".html", ".css",
];

/// Every path-shaped word in a body of text: contains a slash, ends in a known
/// extension. Quotes, brackets and trailing punctuation are stripped, so this
/// finds paths in YAML `run:` lines and in Rust string literals alike.
fn paths_in(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for word in text.split(|c: char| c.is_whitespace() || "\"'`(),;:=[]{}<>|".contains(c)) {
        let word = word.trim_end_matches(['.', '*', '?', '!']);
        let word = word.strip_prefix("./").unwrap_or(word);
        if word.contains('/') && EXTENSIONS.iter().any(|ext| word.ends_with(ext)) {
            found.insert(word.to_owned());
        }
    }
    found
}

/// Build output a workflow legitimately creates for itself. A path CI names may
/// be ignored only if the rule that hides it is one of these: `dist/` is made by
/// the frontend build, `target/` by cargo. Any other rule hiding a file the
/// build needs is the bug this file was written for.
const BUILD_OUTPUT: &[&str] = &[
    "/target",
    "/spikes/target",
    "/app/target",
    "**/dist/",
    "**/src-tauri/gen/schemas/",
    "node_modules/",
    "*.tsbuildinfo",
];

/// The `.gitignore` pattern that hides this path, or `None` if git would keep
/// it. Asks git, so the answer is the one that matters, and asks for the rule
/// rather than a yes, so a build artefact can be told from a lost file.
fn ignore_rule(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo())
        .args(["check-ignore", "-v", "--no-index"])
        .arg(path)
        .output()
        .expect("git is needed to ask git what it ignores");
    match output.status.code() {
        Some(0) => {
            // `<source>:<line>:<pattern>\t<path>`
            let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let pattern = line.split('\t').next().unwrap_or_default();
            let pattern = pattern.rsplit(':').next().unwrap_or_default();
            // `check-ignore -v` exits 0 for the last *matching* rule, which may
            // be a negation - `!**/tests/fixtures/**` means git keeps the file.
            (!pattern.starts_with('!')).then(|| pattern.to_owned())
        }
        Some(1) => None,
        other => panic!(
            "git check-ignore failed for {}: exit {other:?} {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

/// The reason a path is a problem, or `None` if it is fine to be hidden.
fn hidden_for_no_good_reason(path: &Path) -> Option<String> {
    let rule = ignore_rule(path)?;
    (!BUILD_OUTPUT.contains(&rule.as_str())).then_some(rule)
}

/// Every `.rs` file under a crate's `src` or `tests`, in both workspaces.
fn rust_sources() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let roots = ["crates", "app/src-tauri/src", "app/src-tauri/tests"];
    for root in roots {
        collect(&repo().join(root), &mut found);
    }
    found
}

fn collect(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect(&path, into);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            into.push(path);
        }
    }
}

/// The directory holding the `Cargo.toml` a source file belongs to, which is
/// what `CARGO_MANIFEST_DIR` will be when that file runs.
fn crate_root(file: &Path) -> Option<PathBuf> {
    let mut dir = file.parent()?;
    loop {
        if dir.join("Cargo.toml").is_file() {
            return Some(dir.to_owned());
        }
        dir = dir.parent()?;
    }
}

#[test]
fn every_path_the_ci_workflow_names_is_a_file_git_keeps() {
    let workflows = repo().join(".github/workflows");
    let mut checked = 0;
    let mut hidden = Vec::new();
    for entry in std::fs::read_dir(&workflows)
        .expect("the workflows directory")
        .flatten()
    {
        let text = std::fs::read_to_string(entry.path()).expect("a workflow file");
        for candidate in paths_in(&text) {
            // Only paths that are here now: a workflow also names artifacts it
            // will create, and a missing one of those is not this test's bug.
            if !repo().join(&candidate).exists() {
                continue;
            }
            checked += 1;
            if let Some(rule) = hidden_for_no_good_reason(Path::new(&candidate)) {
                hidden.push(format!(
                    "{} names {candidate}, hidden by `{rule}`",
                    entry.file_name().to_string_lossy()
                ));
            }
        }
    }
    assert!(
        checked > 0,
        "no workflow named a path that exists; the scan found nothing to check"
    );
    assert!(
        hidden.is_empty(),
        "CI runs files git would not clone:\n  {}",
        hidden.join("\n  ")
    );
}

#[test]
fn every_path_a_rust_source_reaches_for_is_a_file_git_keeps() {
    let mut checked = 0;
    let mut hidden = Vec::new();
    for file in rust_sources() {
        let text = std::fs::read_to_string(&file).expect("a rust source file");
        let root = crate_root(&file).expect("every source file is inside a crate");
        for candidate in paths_in(&text) {
            // A literal is resolved the way the code will resolve it: against
            // the crate's manifest directory, or against the repository root.
            let resolved = [root.join(&candidate), repo().join(&candidate)]
                .into_iter()
                .find(|path| path.exists());
            let Some(resolved) = resolved else { continue };
            let resolved = resolved
                .canonicalize()
                .expect("an existing path canonicalises");
            // A header comment may cite a file in another project. Git cannot
            // answer for a path outside the repository and should not be asked.
            if !resolved.starts_with(repo()) {
                continue;
            }
            checked += 1;
            if let Some(rule) = hidden_for_no_good_reason(&resolved) {
                hidden.push(format!(
                    "{} reaches for {candidate}, hidden by `{rule}`",
                    file.strip_prefix(repo()).unwrap_or(&file).display()
                ));
            }
        }
    }
    assert!(
        checked > 0,
        "no source file named a path that exists; the scan found nothing"
    );
    assert!(
        hidden.is_empty(),
        "tests read files git would not clone:\n  {}",
        hidden.join("\n  ")
    );
}

#[test]
fn git_really_does_report_an_ignored_file_as_ignored() {
    // Without this, both tests above pass on a machine where `check-ignore`
    // answers 1 for everything - a gate that cannot say no.
    assert_eq!(
        ignore_rule(Path::new("a-scratch-capture.wav")).as_deref(),
        Some("*.wav"),
        "*.wav is ignored and git should say which rule does it"
    );
    assert_eq!(
        ignore_rule(Path::new("README.md")),
        None,
        "README.md is not ignored"
    );
    assert!(
        hidden_for_no_good_reason(Path::new("app/ui/dist/index.html")).is_none(),
        "build output is allowed to be hidden"
    );
}
