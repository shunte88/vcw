/*
 *  migrate.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The 'vcw migrate' verb: bring an older project up to this build's schema (16).
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

//! The `vcw migrate` verb: bring an older project up to this build's schema (§16).
//!
//! The window has always done this silently - opening a project for writing
//! migrates it, so a person with a window never learns the schema moved. The CLI
//! had no equivalent, which left a CLI-only user in the worst position of the
//! three: they got upgraded anyway, by happening to run a verb that takes a write
//! handle, with nothing said either before or after. This is that upgrade, named
//! and asked for.
//!
//! # Why it opens the project twice
//!
//! [`Project::open`] *is* the migration - there is no read-write handle that
//! leaves an old file alone. So the plan is read through a read-only handle
//! first, which is also the only way `--dry-run` can say anything at all.
//!
//! The real run then reports what `schema_migrations` records rather than
//! replaying the plan it printed, so the two halves are a prediction and a
//! measurement and not the same sentence twice. `migrate_from_cli` holds them to
//! each other.
//!
//! # No backup
//!
//! Decided rather than overlooked. A migration cannot half-apply - each step is
//! one transaction with its own bookkeeping - so the risk this would cover is not
//! a torn file but a change of mind: once the project is current, the build that
//! wrote it refuses it as `SchemaTooNew`. A copy is the only way back, and this
//! says so before it writes. It does not make the copy, because a side is well
//! over a gigabyte and the audience this verb exists for - a headless box with no
//! window - is the one most likely to be short of the room for a second one.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vcw_project::{MIGRATIONS, Project, migrate};

/// Everything the verb was asked to do.
pub(crate) struct Args {
    /// Project to upgrade.
    pub(crate) project: PathBuf,
    /// Report what would happen and write nothing.
    pub(crate) dry_run: bool,
    /// Machine-readable output.
    pub(crate) json: bool,
}

/// One migration, either predicted or recorded.
struct Step {
    version: u32,
    description: String,
}

/// Runs the verb.
pub(crate) fn run(args: &Args) -> Result<()> {
    if !args.project.exists() {
        bail!("{} does not exist", args.project.display());
    }

    // Read-only, because opening for writing is what performs the migration.
    // A project from the future is refused here, by `identify`, and says so.
    let (found, planned) = {
        let project = Project::open_read_only(&args.project)
            .with_context(|| format!("opening {}", args.project.display()))?;
        let found = project.schema_version()?;
        let planned: Vec<Step> = migrate::pending(project.conn(), MIGRATIONS)?
            .into_iter()
            .map(|m| Step {
                version: m.version,
                description: m.description.to_owned(),
            })
            .collect();
        project.close()?;
        (found, planned)
    };
    let target = migrate::target_version(MIGRATIONS);

    if planned.is_empty() {
        // Exit 0. "There was nothing to do" is a successful answer to "bring
        // this up to date", and a script that runs this before every session
        // should not have to special-case the ordinary outcome.
        print(args, found, target, &planned, None);
        return Ok(());
    }

    if args.dry_run {
        print(args, found, target, &planned, None);
        return Ok(());
    }

    let project = Project::open(&args.project)
        .with_context(|| format!("upgrading {}", args.project.display()))?;
    let reached = project.schema_version()?;
    let applied = recorded_since(project.conn(), found)?;
    project.close()?;

    if reached != target {
        bail!(
            "{} is at schema {reached} and this build targets {target}",
            args.project.display()
        );
    }
    print(args, found, reached, &planned, Some(&applied));
    Ok(())
}

/// The migrations `schema_migrations` says landed after `found`.
///
/// Read back rather than assumed: this is what the file now claims about itself,
/// which is the thing a later build will act on.
fn recorded_since(conn: &vcw_project::Connection, found: u32) -> Result<Vec<Step>> {
    let mut stmt = conn.prepare(
        "SELECT version, description FROM schema_migrations
         WHERE version > ?1 ORDER BY version",
    )?;
    let rows = stmt.query_map([found], |row| {
        Ok(Step {
            version: row.get(0)?,
            description: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<Step>, _>>()?)
}

/// Says what will happen, or what did.
///
/// `applied` present means the run was real; the body above it is identical
/// either way, which is the point - a dry run is the real run's own sentence.
fn print(args: &Args, found: u32, target: u32, planned: &[Step], applied: Option<&[Step]>) {
    if args.json {
        print_json(args, found, target, planned, applied);
    } else {
        print_human(args, found, target, planned, applied);
    }
}

fn print_human(args: &Args, found: u32, target: u32, planned: &[Step], applied: Option<&[Step]>) {
    let path = args.project.display();
    if planned.is_empty() {
        println!("{path}  schema {found}, already current");
        return;
    }

    println!("{path}  schema {found} -> {target}");
    println!();
    for step in planned {
        println!("  {:<3} {}", step.version, step.description);
    }
    println!();
    println!("This is one way. An older VCW will refuse the upgraded file.");
    println!("Copy it first if you need to go back.");
    println!();
    match applied {
        None => println!("dry run - nothing was written"),
        Some(done) => println!("applied {} - {path} is now schema {target}", versions(done)),
    }
}

fn print_json(args: &Args, found: u32, target: u32, planned: &[Step], applied: Option<&[Step]>) {
    let steps = |set: &[Step]| -> Vec<serde_json::Value> {
        set.iter()
            .map(|s| serde_json::json!({"version": s.version, "description": s.description}))
            .collect()
    };
    let out = serde_json::json!({
        "project": args.project.display().to_string(),
        "found": found,
        "target": target,
        "already_current": planned.is_empty(),
        "pending": steps(planned),
        "applied": applied.map(steps),
        "dry_run": args.dry_run,
    });
    println!("{out}");
}

/// `2, 3, 4, 5`.
fn versions(set: &[Step]) -> String {
    set.iter()
        .map(|s| s.version.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
