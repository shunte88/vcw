/*
 *  migrate_from_cli.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The 'vcw migrate' verb from the outside: a real v1 file, upgraded once.
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

//! `vcw migrate` from the outside, against a project wound back to v1.
//!
//! Two properties, and the second is the one worth the fixture:
//!
//! 1. A dry run predicts, and the real run measures. The plan printed by
//!    `--dry-run` comes from `migrate::pending`; the list printed by the real
//!    run is read back out of `schema_migrations` afterwards. They are produced
//!    by different code from different sources, so holding them to each other
//!    says the prediction was true - see the `a-dry-run-is-a-prediction` note.
//! 2. A dry run writes nothing. `Project::open` *is* the migration, so the verb
//!    has to take a read-only handle to say anything at all, and a regression
//!    here would upgrade somebody's project while telling them it had not.
//!
//! The refusal half is here too: before this change, `vcw export` against a v1
//! project answered `no such table: releases`, which is SQLite's opinion of a
//! database rather than VCW's answer about a project.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

/// Records a second of the simulated source into `path`.
fn record(path: &Path) {
    let out = Command::new(VCW)
        .args([
            "session",
            &path.display().to_string(),
            "--rate",
            "48000",
            "--channels",
            "2",
            "--script",
            "arm,record,sleep 1,stop",
        ])
        .output()
        .expect("run vcw session");
    assert!(
        out.status.success(),
        "recording failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Winds a current project back to the v1 it would have been before WP-13.
///
/// The same shape as `vcw_shell::transport`'s fixture, and it carries the same
/// warning: **every migration after 1 has to be undone here**. v4 and v5 need no
/// line of their own only because both put their columns on tables migration 2
/// creates, which this drops outright. One that lands on `captures`, as v3 did,
/// needs subtracting by name or the fixture is not a v1 project and the
/// migration fails re-applying its own work.
fn wind_back_to_v1(path: &Path) {
    let project = vcw_project::Project::open(path).expect("opening the project");
    project
        .conn()
        .execute_batch(
            "DROP TABLE IF EXISTS tracks;
             DROP TABLE IF EXISTS track_boundaries;
             DROP TABLE IF EXISTS sides;
             DROP TABLE IF EXISTS release_artwork;
             DROP TABLE IF EXISTS releases;
             ALTER TABLE captures DROP COLUMN capture_eq;
             DELETE FROM schema_migrations WHERE version >= 2;
             PRAGMA user_version = 1;",
        )
        .expect("winding the schema back");
    assert_eq!(
        version(path),
        1,
        "the fixture did not land on v1, so it proves nothing about an old project"
    );
    project.close().expect("closing the fixture");
}

/// `PRAGMA user_version`, read without going through the project at all - which
/// is the point, since opening it for writing is what migrates it.
fn version(path: &Path) -> u32 {
    let uri = format!("file:{}?mode=ro", path.display());
    let conn = rusqlite::Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .expect("opening read-only");
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .expect("reading user_version") as u32
}

/// Runs `vcw migrate` with `extra`, and returns stdout and whether it succeeded.
fn migrate(path: &Path, extra: &[&str]) -> (String, bool) {
    let mut args = vec!["migrate", &path.display().to_string()]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<String>>();
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    let out = Command::new(VCW)
        .args(&args)
        .output()
        .expect("run vcw migrate");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    )
}

/// The versions in a `--json` list, whether `pending` or `applied`.
fn versions(value: &Value, key: &str) -> Vec<u64> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} should be a list, got {}", value[key]))
        .iter()
        .map(|step| step["version"].as_u64().expect("a version"))
        .collect()
}

#[test]
fn a_dry_run_predicts_what_the_real_run_then_does() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("old.vcw");
    record(&path);
    wind_back_to_v1(&path);

    let (text, ok) = migrate(&path, &["--dry-run", "--json"]);
    assert!(ok, "the dry run failed:\n{text}");
    let predicted: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(predicted["found"], 1);
    assert_eq!(predicted["already_current"], false);
    assert!(
        predicted["applied"].is_null(),
        "a dry run applied something"
    );
    let plan = versions(&predicted, "pending");
    assert!(!plan.is_empty(), "nothing to do on a v1 project");

    assert_eq!(
        version(&path),
        1,
        "the dry run upgraded the project it was only asked about"
    );

    let (text, ok) = migrate(&path, &["--json"]);
    assert!(ok, "the migration failed:\n{text}");
    let done: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(
        versions(&done, "applied"),
        plan,
        "the dry run predicted {plan:?} and the real run did something else"
    );
    assert_eq!(version(&path), vcw_project::SCHEMA_VERSION);

    let (text, ok) = migrate(&path, &["--json"]);
    assert!(ok, "a second run should succeed and do nothing:\n{text}");
    let again: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(again["already_current"], true);
    assert!(versions(&again, "pending").is_empty());
}

#[test]
fn the_plan_a_dry_run_prints_is_the_plan_the_real_run_prints() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("old.vcw");
    record(&path);
    wind_back_to_v1(&path);

    // The human text, not the JSON: counts are not names, and a person reading
    // `--dry-run` is deciding on the strength of these lines.
    let (predicted, _) = migrate(&path, &["--dry-run"]);
    let (done, _) = migrate(&path, &[]);
    let body = |text: &str| -> Vec<String> {
        text.lines()
            .take_while(|line| !line.starts_with("dry run") && !line.starts_with("applied"))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(
        body(&predicted),
        body(&done),
        "the dry run described a different migration from the one that ran"
    );
    assert!(
        predicted.contains("An older VCW will refuse the upgraded file"),
        "the one-way warning is the whole reason to read a dry run:\n{predicted}"
    );
}

#[test]
fn an_old_project_is_refused_by_name_rather_than_by_sqlite() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("old.vcw");
    record(&path);
    wind_back_to_v1(&path);

    let out = Command::new(VCW)
        .args([
            "export",
            &path.display().to_string(),
            "--into",
            &dir.path().join("out").display().to_string(),
            "--dry-run",
        ])
        .output()
        .expect("run vcw export");
    assert!(!out.status.success(), "export accepted a v1 project");
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("written by an older VCW"),
        "the refusal did not say the project was old:\n{said}"
    );
    assert!(
        said.contains("vcw migrate"),
        "the refusal did not name the verb that fixes it:\n{said}"
    );
    assert!(
        !said.contains("no such table"),
        "SQLite's sentence reached the person:\n{said}"
    );
    assert_eq!(version(&path), 1, "a refused read migrated the project");
}

/// The other half of the refusal: a verb that only reads must not migrate.
///
/// `Project::open` *is* the migration, so until WP-13 was followed up every verb
/// taking a write handle upgraded the file as the price of running - including
/// two that read far more than they write. `vcw release show` was the worse of
/// the two, because `show` is not a read if it changes the schema, and a person
/// who copies a project to be safe and then looks inside the copy has defeated
/// the copy.
#[test]
fn a_verb_that_only_reads_leaves_the_schema_where_it_found_it() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("old.vcw");
    record(&path);
    wind_back_to_v1(&path);

    // `release show` reads the §29 tables, which arrive in v2, so on a v1
    // project it has to refuse rather than quietly build what it wants to read.
    let out = Command::new(VCW)
        .args(["release", &path.display().to_string(), "show"])
        .output()
        .expect("run vcw release show");
    assert!(!out.status.success(), "release show accepted a v1 project");
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("vcw migrate"),
        "the refusal did not name the verb that fixes it:\n{said}"
    );
    assert_eq!(version(&path), 1, "release show migrated the project");

    // `waveform` reads nothing newer than v1, so it draws the old project as it
    // is - and still leaves it alone.
    let out = Command::new(VCW)
        .args([
            "waveform",
            &path.display().to_string(),
            "--pixels",
            "20",
            "--json",
        ])
        .output()
        .expect("run vcw waveform");
    assert!(
        out.status.success(),
        "waveform refused a v1 project it can read:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let drawn: Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json columns");
    assert!(
        drawn["channels"].as_array().is_some_and(|c| !c.is_empty()),
        "waveform drew nothing, so leaving the schema alone proves nothing"
    );
    assert_eq!(version(&path), 1, "waveform migrated the project");

    // And the write that is still a write still upgrades, because asking for one
    // is asking for the other.
    let (text, ok) = migrate(&path, &[]);
    assert!(ok, "the migration failed:\n{text}");
    assert_eq!(version(&path), vcw_project::SCHEMA_VERSION);
    let out = Command::new(VCW)
        .args(["release", &path.display().to_string(), "show"])
        .output()
        .expect("run vcw release show");
    assert!(
        out.status.success(),
        "release show refused a current project:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The question `user_version` cannot answer: what the project actually holds.
///
/// `MIGRATIONS[0]` *is* `SCHEMA_V1`, which stops the create path and the upgrade
/// path drifting - but migration 1 only ever runs against an empty database, so
/// an edit to `SCHEMA_V1` reaches every project made afterwards and no project
/// made before. One such edit happened: two covering indexes were added to the
/// waveform tables on 2026-09-25, eleven days before 0.1.1-alpha. A project
/// written the week before is current by every version number in it, has
/// neither index, and answers with a SQL error when the pyramid is read.
/// Nothing in `schema_migrations` can say so, because every migration it names
/// did run. No released build ever wrote such a file - `/data2/vcw_soak/live.vcw`
/// is a development project and it is what this check was built against - so
/// the test manufactures the shape rather than carrying the file.
#[test]
fn migrate_asks_the_dictionary_and_not_only_the_version_number() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("current.vcw");
    record(&path);

    // A project this build wrote has nothing to report, and says nothing.
    let (text, ok) = migrate(&path, &[]);
    assert!(ok, "{text}");
    assert!(
        !text.contains("dictionary"),
        "a clean project was given a drift paragraph:\n{text}"
    );

    // Now the shape of the real case, reproduced by taking the index away.
    {
        let project = vcw_project::Project::open(&path).expect("opening");
        project
            .conn()
            .execute_batch("DROP INDEX sampleblocks_summary256;")
            .expect("dropping the index");
        project.close().expect("closing");
    }

    let (text, ok) = migrate(&path, &[]);
    assert!(
        ok,
        "drift is a statement about the file, not a failure of the verb:\n{text}"
    );
    assert!(
        text.contains("already current"),
        "the version number is still current and the report has to say so:\n{text}"
    );
    assert!(
        text.contains("sampleblocks_summary256"),
        "the missing index was not named:\n{text}"
    );

    // The statement in `--json` is the remedy, not a description of one.
    let (text, ok) = migrate(&path, &["--json"]);
    assert!(ok, "{text}");
    let said: Value = serde_json::from_str(&text).expect("json");
    let drift = said["drift"].as_array().expect("a drift list");
    assert_eq!(drift.len(), 1, "{drift:?}");
    assert_eq!(drift[0]["how"], "missing");
    assert_eq!(drift[0]["kind"], "index");
    let sql = drift[0]["sql"].as_str().expect("the statement");
    {
        let project = vcw_project::Project::open(&path).expect("opening");
        project.conn().execute_batch(sql).expect("the remedy runs");
        project.close().expect("closing");
    }

    let (text, ok) = migrate(&path, &["--json"]);
    assert!(ok, "{text}");
    let said: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(
        said["drift"].as_array().map(Vec::len),
        Some(0),
        "running the statement the report carried did not clear the drift"
    );
}

/// An older project is not asked, because the answer would be the migration.
#[test]
fn the_dictionary_check_waits_until_the_project_is_current() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("old.vcw");
    record(&path);
    wind_back_to_v1(&path);

    // A v1 project is missing every §29 table, and listing them under a plan
    // that is about to create them would bury anything that mattered.
    let (text, ok) = migrate(&path, &["--dry-run"]);
    assert!(ok, "{text}");
    assert!(
        text.contains("runs once the project is current"),
        "a dry run on an old project should say the check was not made:\n{text}"
    );
    assert!(
        !text.contains("missing     table releases"),
        "the pending migration's own tables were reported as drift:\n{text}"
    );

    let (text, ok) = migrate(&path, &["--json"]);
    assert!(ok, "{text}");
    let said: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(
        said["drift"].as_array().map(Vec::len),
        Some(0),
        "a project migrated to current should have no drift: {}",
        said["drift"]
    );
}
