/*
 *  eq_from_cli.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  `vcw eq` prints the catalog, its citations, and a curve of your own.
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

//! `vcw eq` prints the catalog, its citations, and a curve of your own.
//!
//! The arithmetic is `vcw-eq`'s to prove and its conformance suite proves it.
//! What is only reachable from here is the wiring: that the verb exists, that
//! `--curves` actually merges a user file over the built-in catalog rather than
//! replacing or ignoring it, and that a file without a citation is refused.
//! §51 requires both the citation and the user-defined curve, and a requirement
//! nothing exercises is a requirement nobody has.

use std::process::Command;

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(VCW).args(args).output().expect("run the CLI");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn list_names_every_curve_the_catalog_ships() {
    let (ok, stdout, stderr) = run(&["eq", "list"]);
    assert!(ok, "vcw eq list failed: {stderr}");
    for id in vcw_eq::Catalog::builtin().ids() {
        assert!(stdout.contains(id), "{id} is missing from the listing");
    }
}

#[test]
fn show_prints_the_source_and_the_measured_error() {
    let (ok, stdout, stderr) = run(&["eq", "show", "riaa"]);
    assert!(ok, "vcw eq show failed: {stderr}");
    // §51: the parameters carry a citation, and the response is measured
    // against it. Both have to reach the person reading the terminal.
    assert!(stdout.contains("IEC 98"), "no citation in:\n{stdout}");
    assert!(stdout.contains("+19.27 dB"), "no response in:\n{stdout}");
    assert!(
        stdout.contains("budget of 0.5"),
        "no tolerance in:\n{stdout}"
    );
}

#[test]
fn an_unknown_curve_is_refused_by_name_and_offers_the_others() {
    let (ok, _, stderr) = run(&["eq", "show", "rias"]);
    assert!(!ok, "a misspelled curve id succeeded");
    assert!(stderr.contains("rias"), "the typo is not named: {stderr}");
    assert!(stderr.contains("riaa"), "no alternatives offered: {stderr}");
}

#[test]
fn a_curve_of_your_own_is_added_and_one_of_ours_is_replaced() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("mine.toml");
    std::fs::write(
        &file,
        "[[curve]]\n\
         id = \"riaa\"\n\
         name = \"RIAA as my preamp builds it\"\n\
         shelf_us = 3180.0\n\
         turnover_us = 318.0\n\
         treble_us = 75.0\n\
         citation = \"measured on my own bench, 2026\"\n\
         \n\
         [[curve]]\n\
         id = \"house\"\n\
         name = \"House curve\"\n\
         turnover_us = 500.0\n\
         citation = \"invented for this test\"\n",
    )
    .expect("write the catalog");
    let path = file.to_str().expect("utf-8 path");

    let (ok, stdout, stderr) = run(&["eq", "list", "--curves", path]);
    assert!(ok, "vcw eq list --curves failed: {stderr}");
    assert!(stdout.contains("House curve"), "the new curve is missing");
    assert!(
        stdout.contains("RIAA as my preamp builds it"),
        "the replaced curve still reads as ours",
    );
    assert!(
        stdout.contains("Decca FFRR"),
        "an override file wiped the built-in catalog instead of merging",
    );
}

#[test]
fn a_curve_with_no_citation_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("nameless.toml");
    std::fs::write(&file, "[[curve]]\nid = \"x\"\nname = \"X\"\n").expect("write");
    let (ok, _, stderr) = run(&["eq", "list", "--curves", file.to_str().expect("utf-8")]);
    assert!(!ok, "a curve with no source was accepted");
    assert!(stderr.contains("citation"), "unhelpful refusal: {stderr}");
}
