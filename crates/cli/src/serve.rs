/*
 *  serve.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  `vcw serve`: the whole frontend over HTTP, for a turntable with no screen.
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
//! `vcw serve`: the whole frontend over HTTP, for a turntable with no screen (§52).
//!
//! The deployment this exists for is a headless machine with a sound card -
//! the piCorePlayer box the first outside report came from - where the desktop
//! shell cannot run because there is no desktop and no WebKit to draw one
//! with. The same `dist` the window loads is served over a socket, the same
//! command bodies answer it, and the browser is somebody's laptop.
//!
//! What this file adds to `vcw-serve` is the three decisions an operator makes
//! and the sentence that tells them what they just agreed to:
//!
//! - **where the frontend is.** Not baked into this binary: `include_dir!`
//!   would make a Rust build depend on an npm build, and the four CI targets
//!   do not run one. `--root`, `VCW_SERVE_ROOT`, or two places beside the
//!   executable where a package would put it.
//! - **what to bind.** Loopback by default. A wildcard bind is refused without
//!   `--allow-host`, because §52's `Host` check is the DNS-rebinding defense
//!   and a wildcard bind has no address to check against.
//! - **the secret.** `VCW_SERVE_TOKEN` if the operator set one, otherwise
//!   minted for the life of the process. Either way it is printed once, in a
//!   URL, and written nowhere.
//!
//! The banner says out loud that this is plain HTTP. §52 requires it to: a
//! person who can see the sentence can decide to put the connection inside
//! something, and a person who cannot see it will assume the padlock they are
//! used to.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use vcw_serve::auth::{Guard, Origin, Secret, VARIABLE, needs_a_stated_host};
use vcw_serve::backend::Served;
use vcw_serve::server::Listener;

/// What `vcw serve` was asked for.
#[derive(Debug)]
pub(crate) struct Args {
    /// Address to bind, `host:port`.
    pub(crate) address: String,
    /// Where the built frontend is, if the operator said.
    pub(crate) root: Option<PathBuf>,
    /// Extra names this listener answers to, for a wildcard bind.
    pub(crate) allow_host: Vec<String>,
    /// The directory the path browser is confined to, if the operator said.
    pub(crate) files: Option<PathBuf>,
}

/// Serves until the process ends.
pub(crate) fn run(args: Args) -> Result<()> {
    if needs_a_stated_host(&args.address) && args.allow_host.is_empty() {
        bail!(
            "{} is a wildcard bind and nothing said which name to answer to.\n\
             \n\
             §52 checks the `Host` header of every request against the address \
             this listener was asked for, which is what stops a page on another \
             site from driving your turntable through your own browser. A \
             wildcard has no address to check, so the names have to be stated: \
             --allow-host vcw.local:7437 --allow-host 192.168.1.42:7437\n\
             \n\
             Binding one address instead is simpler and is what most setups \
             want: --address 192.168.1.42:7437",
            args.address
        );
    }

    let root = root(args.root)?;
    let (secret, origin) = Secret::from_env_or_mint();
    let url = format!("http://{}/?t={}", printable(&args.address), secret.in_url());
    let guard = Guard::new(secret, &args.address, args.allow_host.iter().cloned());

    let files = files(args.files)?;
    let served = Served::new(files.clone());
    served.pump();
    let listener = Listener::bind(&args.address, guard, &root, Arc::new(served))
        .with_context(|| format!("serving {} on {}", root.display(), args.address))?;

    banner(&listener.address(), &url, origin, &root, files.as_deref());
    listener.serve()
}

/// What to print once, before the first request.
///
/// On stdout rather than through `tracing`, and unconditionally: the URL is
/// the only way in and a log level is not something to discover it behind.
fn banner(
    bound: &str,
    url: &str,
    origin: Origin,
    root: &std::path::Path,
    files: Option<&std::path::Path>,
) {
    println!("VCW is serving {} on http://{bound}", root.display());
    println!();
    println!("  Open this, once, from the machine you want to drive it from:");
    println!();
    println!("    {url}");
    println!();
    match origin {
        Origin::Environment => println!("  The token came from {VARIABLE}."),
        Origin::Minted => println!(
            "  The token was minted for this run and is written nowhere. \
             Restarting VCW mints another one.\n  Set {VARIABLE} to choose \
             your own."
        ),
    }
    println!(
        "  Your browser keeps it in a cookie after the first request, so the \
         URL is needed once."
    );
    println!();
    println!(
        "  This is plain HTTP and nothing on this connection is encrypted - \
         not the token,\n  not what you record, not what you type. On a home \
         network behind a router that is\n  usually what people want. Across \
         anything else, put it inside something: an SSH\n  tunnel is one \
         command, `ssh -L {bound}:{bound} <this-machine>`, and then the \
         address\n  to open is the same one on your own laptop."
    );
    println!();
    match files {
        Some(at) => println!(
            "  Export's directory browser is rooted at {} and cannot see above it.",
            at.display()
        ),
        None => println!(
            "  There is no directory browser: type the export path as this machine \
             names it,\n  or start again with --files <dir> to browse under one."
        ),
    }
    println!();
    println!("  Ctrl-C to stop.");
}

/// The directory §52's path browser is confined to.
///
/// What the operator said, canonicalized so that the fence is a resolved path
/// and the banner prints the directory that will actually be listed. Nothing
/// by default: the alternative - rooting it at the home directory because
/// that is convenient - is a browser over the operator's whole account on a
/// machine reached from somewhere else, chosen for them. §52 says "stated
/// when it starts", and an unstated root is no browser.
fn files(stated: Option<PathBuf>) -> Result<Option<PathBuf>> {
    let Some(at) = stated else { return Ok(None) };
    let at = at
        .canonicalize()
        .with_context(|| format!("--files {}", at.display()))?;
    if !at.is_dir() {
        bail!("--files {} is not a directory", at.display());
    }
    Ok(Some(at))
}

/// Where the built frontend is.
///
/// In order: what the operator said, `VCW_SERVE_ROOT`, then the two places a
/// package puts it relative to this executable. A directory with no
/// `index.html` in it is refused here rather than at the first request,
/// because "I started it and the page is blank" is a worse report than a
/// sentence naming the directory that was looked in.
fn root(stated: Option<PathBuf>) -> Result<PathBuf> {
    let mut tried = Vec::new();
    let mut candidates = Vec::new();
    if let Some(root) = stated {
        candidates.push(root);
    } else {
        if let Some(root) = std::env::var_os("VCW_SERVE_ROOT") {
            candidates.push(PathBuf::from(root));
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push(dir.join("ui"));
            candidates.push(dir.join("../share/vcw/ui"));
        }
    }
    for candidate in candidates {
        if candidate.join("index.html").is_file() {
            return Ok(candidate);
        }
        tried.push(candidate.display().to_string());
    }
    bail!(
        "the VCW frontend is not where I looked for it. Tried:\n  {}\n\n\
         Point me at a directory holding index.html with --root, or set \
         VCW_SERVE_ROOT. A build of your own is `npm run build` in app/ui, \
         which writes app/ui/dist.",
        tried.join("\n  ")
    );
}

/// The address as a person would type it into a browser.
///
/// `0.0.0.0` is a bind, not a destination: printing it in a URL gives a person
/// something that does not work on Windows and works by accident elsewhere.
fn printable(address: &str) -> String {
    match address.rsplit_once(':') {
        Some((host, port)) if host.is_empty() || host == "0.0.0.0" || host == "[::]" => {
            format!("<this-machine>:{port}")
        }
        _ => address.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wildcard_bind_is_refused_until_a_name_is_stated() {
        let refused = run(Args {
            address: "0.0.0.0:0".to_owned(),
            root: None,
            allow_host: Vec::new(),
            files: None,
        })
        .expect_err("a wildcard with no name");
        let said = refused.to_string();
        assert!(said.contains("--allow-host"), "{said}");
    }

    /// An unstated `--files` is no browser, and a stated one has to be real.
    ///
    /// The `None` half is the one worth a test. Defaulting it to the home
    /// directory would have been one line and would have meant that every
    /// `vcw serve` started without thinking about it offered a directory tree
    /// of the operator's whole account to whoever reached the port. §52 fences
    /// the browser at a directory "stated when it starts"; unstated is not a
    /// smaller fence, it is no browser.
    #[test]
    fn a_path_browser_needs_a_directory_stated_for_it() {
        assert_eq!(files(None).expect("no browser is a fine answer"), None);

        let there = tempfile::tempdir().expect("a directory");
        assert_eq!(
            files(Some(there.path().to_owned())).expect("a real directory"),
            Some(there.path().canonicalize().expect("it exists"))
        );

        let not_a_directory = there.path().join("sleeve.jpg");
        std::fs::write(&not_a_directory, b"not a directory").expect("a file");
        let refused = files(Some(not_a_directory)).expect_err("a file is not a root");
        assert!(refused.to_string().contains("sleeve.jpg"), "{refused}");
    }

    #[test]
    fn a_directory_with_no_frontend_in_it_says_which_one() {
        let empty = tempfile::tempdir().expect("a directory");
        let refused = root(Some(empty.path().to_owned())).expect_err("nothing there");
        assert!(
            refused
                .to_string()
                .contains(&empty.path().display().to_string()),
            "{refused}"
        );
    }

    #[test]
    fn a_frontend_that_is_there_is_found() {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::write(dir.path().join("index.html"), "<!doctype html>").expect("a file");
        assert_eq!(
            root(Some(dir.path().to_owned())).expect("found"),
            dir.path()
        );
    }

    #[test]
    fn a_wildcard_is_not_printed_as_somewhere_to_go() {
        assert_eq!(printable("0.0.0.0:7437"), "<this-machine>:7437");
        assert_eq!(printable("127.0.0.1:7437"), "127.0.0.1:7437");
        assert_eq!(printable("vcw.local:7437"), "vcw.local:7437");
    }
}
