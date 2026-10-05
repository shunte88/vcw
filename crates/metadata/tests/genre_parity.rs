/*
 *  genre_parity.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The genre port answers what VRipr answered, row for row.
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

//! The genre port answers what VRipr answered, row for row.
//!
//! `crates/metadata/src/genres.rs` is a port of VRipr's `sanitize_genres`, and a
//! port that is nearly right is worse than no port: a catalog built with VRipr
//! and a catalog built with VCW would disagree about what a record is. So the
//! reference is VRipr's own output, not my reading of VRipr's code.
//!
//! `tests/fixtures/vripr_genres.jsonl` was produced out of tree by
//! `/data2/vcw-scratch/genreparity`, a crate holding a byte-verbatim copy of
//! `/data2/vripr/src/metadata/genre.rs` and of `assets/genre.dat`. Nothing in this
//! repository carries a second implementation of the algorithm; only its answers.
//! Regenerate with `cargo run --release < inputs.txt > vripr_genres.jsonl`.
//!
//! The input set is every key in the table, each also lowercased and uppercased,
//! plus a handful of composites. Five case-folds are deliberately absent: see the
//! module docs for `genres` for why VRipr's answer to `HARDROCK` is not a fact.

use vcw_metadata::Genres;

/// One recorded answer.
struct Case {
    input: String,
    output: Vec<String>,
}

/// Reads the fixture. Hand-parsed because the shape is fixed and two fields do not
/// justify a serde model in a test.
fn cases() -> Vec<Case> {
    let raw = include_str!("fixtures/vripr_genres.jsonl");
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("bad fixture line: {e}"));
            Case {
                input: value["input"].as_str().expect("an input").to_string(),
                output: value["output"]
                    .as_array()
                    .expect("an output array")
                    .iter()
                    .map(|g| g.as_str().expect("a genre").to_string())
                    .collect(),
            }
        })
        .collect()
}

#[test]
fn every_recorded_answer_is_reproduced_exactly() {
    let genres = Genres::builtin();
    let cases = cases();
    assert!(
        cases.len() > 1_800,
        "the fixture should cover every key in the table, got {}",
        cases.len()
    );

    let mut differences = Vec::new();
    for case in &cases {
        let ours = genres.normalize(&case.input);
        if ours != case.output {
            differences.push(format!(
                "{:?}: VRipr {:?}, VCW {:?}",
                case.input, case.output, ours
            ));
        }
    }
    assert!(
        differences.is_empty(),
        "{} of {} answers differ:\n{}",
        differences.len(),
        cases.len(),
        differences
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_fixture_is_not_trivially_satisfiable() {
    // A port that returned its input unchanged would be wrong, and a fixture that
    // could not tell the difference would be worthless. Most rows must actually
    // transform something.
    let cases = cases();
    let changed = cases
        .iter()
        .filter(|case| {
            let passthrough: Vec<String> = case
                .input
                .split(';')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect();
            passthrough != case.output
        })
        .count();
    assert!(
        changed * 2 > cases.len(),
        "only {changed} of {} rows change their input; the fixture is not exercising the table",
        cases.len()
    );
}

#[test]
fn the_answers_do_not_move_between_two_tables_built_from_the_same_data() {
    // The port resolves the table's duplicate and case-folding keys by file order
    // rather than by hash order, which is the one behavior VRipr could not
    // promise. Two independently built tables must agree on all of it.
    let first = Genres::builtin();
    let second = Genres::builtin();
    for case in cases() {
        assert_eq!(
            first.normalize(&case.input),
            second.normalize(&case.input),
            "unstable answer for {:?}",
            case.input
        );
    }
}
