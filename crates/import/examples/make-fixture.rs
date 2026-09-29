/*
 *  make-fixture.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Making a committable test fixture out of one of the user's real projects.
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
//! Making a committable test fixture out of one of the user's real projects.
//!
//! ```text
//! cargo run -p vcw-import --example make-fixture -- <source.aup3> <fixture.aup3>
//! ```
//!
//! An example rather than a subcommand: this is a development tool that reads
//! files no user of VCW has, and it has no place on the product's command line.
//! It is in the tree, and compiled by `cargo clippy --all-targets`, because the
//! three fixtures under `crates/import/tests/fixtures/` have to be reproducible
//! from the corpus rather than being bytes nobody can account for.
//!
//! What it keeps and what it rewrites is [`vcw_import::fixture`]'s business, and
//! that module's documentation is the honest list. The short version: the
//! document is Audacity's, the audio is zeros.
//!
//! The three committed fixtures were made with the default [`Shrink`] from:
//!
//! ```text
//! simples_test.aup3                 -> clips.aup3      splits, shared blocks, int24
//! simples_test.aup4                 -> clips.aup4      the AUP4 delta, a 0x10 blob
//! Gasp_Stardonas (Transverse).aup3  -> rate-trap.aup3  48 kHz float32, 64 KiB pages
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use vcw_import::fixture::{Shrink, shrink};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let (Some(source), Some(destination), None) = (args.next(), args.next(), args.next()) else {
        eprintln!(
            "usage: make-fixture <source.aup3|source.aup4> <destination>\n\
             \n\
             The source is opened read-only and is never written to. The \
             destination must not exist."
        );
        return ExitCode::FAILURE;
    };

    match shrink(&source, &destination, &Shrink::default()) {
        Ok(report) => {
            println!("{}", destination.display());
            println!(
                "  blocks kept      {} ({} rows deleted)",
                report.blocks_kept, report.blocks_removed
            );
            println!("  waveblocks cut   {}", report.refs_removed);
            println!(
                "  document         {} -> {} bytes",
                report.doc_bytes.0, report.doc_bytes.1
            );
            println!("  history rows cut {}", report.history_removed);
            println!("  file             {} bytes", report.file_bytes);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", source.display());
            ExitCode::FAILURE
        }
    }
}
