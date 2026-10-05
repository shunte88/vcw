/*
 *  logging.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Installing the log subscriber: the one place logging policy is decided (§42).
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
//! Installing the log subscriber: the one place logging policy is decided (§42).
//!
//! The libraries take `tracing` as a facade and nothing more. None of them
//! installs a subscriber, because a library that configures logging has taken a
//! decision belonging to whoever runs it - and a library that logs by default is
//! a library that writes to somebody else's stderr. So the binaries decide, and
//! for the CLI the decision is here.
//!
//! # Off unless asked
//!
//! Default is `warn`, which in practice means silence: nothing in the tree logs
//! at `warn` unless something is actually wrong. That matters because the verbs
//! are meant to pipe - `vcw bundle | jq`, `vcw metadata --json` - and a tool
//! that narrates itself onto stderr is a tool whose output people redirect to
//! `/dev/null` along with the error they needed.
//!
//! Two ways to turn it up, and the environment wins because the whole point is
//! to be able to say "run that again with `VCW_LOG=vcw_audio=trace`" to
//! somebody who is not going to learn the flag:
//!
//! ```text
//! vcw --log debug capture ...        # everything at debug
//! VCW_LOG=vcw_audio=trace vcw ...    # one crate, loudly
//! VCW_LOG=warn,vcw_project=debug ... # a filter per target
//! ```
//!
//! Logs go to **stderr**, never stdout. Several verbs emit JSON on stdout and a
//! log line in the middle of it would make the document unparseable.

use std::io::IsTerminal;

use tracing_subscriber::EnvFilter;

/// The environment variable that overrides the flag.
///
/// Not `RUST_LOG`. `RUST_LOG` is set globally on plenty of development machines,
/// and inheriting somebody's Rust-wide debug level would turn a quiet tool loud
/// for reasons that have nothing to do with VCW.
pub(crate) const FILTER_VAR: &str = "VCW_LOG";

/// Installs the subscriber for this process.
///
/// Called once, before anything else runs. Failure to install is ignored on
/// purpose: a subscriber already in place means something has arranged its own
/// logging, and a tool that refused to run because it could not log would be
/// worse than a tool that does not log.
pub(crate) fn install(level: Option<&str>) {
    let filter = match std::env::var(FILTER_VAR) {
        Ok(text) if !text.trim().is_empty() => EnvFilter::builder().parse_lossy(text),
        _ => EnvFilter::builder().parse_lossy(level.unwrap_or("warn")),
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        // Color only when a person is looking. A redirected log full of escape
        // codes is a log somebody has to strip before reading.
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(true)
        .try_init();
}
