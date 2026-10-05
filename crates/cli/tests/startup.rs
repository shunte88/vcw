/*
 *  startup.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The binary starts inside the smallest main-thread stack any Tier 1 platform gives it.
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

//! The binary starts inside the smallest main-thread stack any Tier 1 platform
//! gives it.
//!
//! Windows reserves **1 MiB** for a process's main thread where Linux and macOS
//! give 8 MiB, and the size is baked into the executable header rather than
//! requested at run time. A debug build of this CLI wanted between 1.0 and
//! 1.5 MiB before it had parsed a single argument, because clap's derive builds
//! every `Command` and `Arg` as a local of one unoptimized function - so on
//! Windows every `vcw` invocation aborted with
//! `thread 'main' has overflowed its stack`, and with it every integration test
//! in this crate, since `cargo test` builds the binary in debug. CI found it;
//! nothing here could, because the dev box has eight times the headroom.
//!
//! `main` now runs the CLI on a thread with a stack it asks for, and a thread
//! stack is mmapped rather than taken from the executable header or from
//! `RLIMIT_STACK`. That is what makes the ceiling reproducible here: `ulimit -s`
//! lowers the main thread's stack to Windows' size on a machine that is not
//! Windows.
//!
//! On Windows itself this file does nothing, and it does not need to: there the
//! rest of the suite is the check, because every test in it spawns this binary.

#![cfg(unix)]

use std::process::Command;

const VCW: &str = env!("CARGO_BIN_EXE_vcw");

/// Windows' main-thread reserve, in the KiB `ulimit -s` speaks.
const WINDOWS_STACK_KIB: u32 = 1024;

/// Runs `args` with the main thread's stack lowered to `kib`.
///
/// Through `sh` because `setrlimit` is the only way to do this and reaching it
/// from Rust means a `libc` dependency, which D5's pure-Rust position would pay
/// for in every build to serve one test. `ulimit` is a POSIX shell builtin and
/// `exec` means the limit belongs to the binary rather than to a child of the
/// shell.
fn run_with_stack(kib: u32, args: &[&str]) -> std::process::Output {
    let quoted = args
        .iter()
        .map(|a| format!(" '{a}'"))
        .collect::<Vec<_>>()
        .concat();
    Command::new("sh")
        .arg("-c")
        .arg(format!("ulimit -s {kib} && exec '{VCW}'{quoted}"))
        .output()
        .expect("run sh")
}

/// The check that stops the two below from being vacuous.
///
/// If the shell cannot lower the limit - a hard limit already below it, a `sh`
/// whose `ulimit` is a no-op - then `run_with_stack` runs the binary at whatever
/// the machine's default is, both tests pass on 8 MiB of headroom, and the
/// regression they exist for walks straight past them. A gate that cannot fail
/// has to say so rather than report a pass.
#[test]
fn the_shell_really_does_lower_the_stack() {
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("ulimit -s {WINDOWS_STACK_KIB} && ulimit -s"))
        .output()
        .expect("run sh");
    assert!(out.status.success(), "sh could not lower the stack limit");
    let reported = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    assert_eq!(
        reported,
        WINDOWS_STACK_KIB.to_string(),
        "sh reported a stack of {reported} KiB after being asked for \
         {WINDOWS_STACK_KIB}, so the two tests below are not measuring anything"
    );
}

/// Argument parsing is where the overflow was, so this is the whole of it.
#[test]
fn the_binary_parses_its_arguments_in_a_windows_sized_stack() {
    for args in [&["--version"][..], &["--help"][..], &["soak", "--help"][..]] {
        let out = run_with_stack(WINDOWS_STACK_KIB, args);
        assert!(
            out.status.success(),
            "vcw {} aborted in {WINDOWS_STACK_KIB} KiB of stack, which is what \
             Windows gives the main thread:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

/// And a command that does work, not just one that prints and exits.
///
/// `doctor` is the cheapest subcommand that reaches the audio and project crates
/// without needing a device, a file or a network.
#[test]
fn a_real_subcommand_runs_in_a_windows_sized_stack() {
    let out = run_with_stack(WINDOWS_STACK_KIB, &["doctor"]);
    assert!(
        out.status.success(),
        "vcw doctor aborted in {WINDOWS_STACK_KIB} KiB of stack:\n{}",
        String::from_utf8_lossy(&out.stderr),
    );

    // WP-28: the CLI is redistributed in the same package as the window and
    // links the same encoders, so it carries the same notice obligation. The
    // list itself is `vcw_export::notices`' business and tested there in all
    // four feature combinations; what this asserts is that `doctor` prints it
    // at all. It said nothing for three work packages.
    let said = String::from_utf8_lossy(&out.stdout);
    let notices = vcw_export::notices::notices();
    assert!(
        !notices.is_empty(),
        "FLAC is unconditional, so this cannot be empty"
    );
    for notice in &notices {
        // Against the library's own answer rather than against a literal:
        // `cfg!(feature = "mp3")` written in this file would be asking about
        // `vcw-cli`'s features, and `vcw-cli` has none. A hard-coded "LGPL-3.0"
        // here would be a check that cannot fail for the wrong reason.
        assert!(
            said.contains(notice.component),
            "vcw doctor does not name {}:\n{said}",
            notice.component
        );
        assert!(
            said.contains(notice.license),
            "vcw doctor does not say {} is {}:\n{said}",
            notice.component,
            notice.license
        );
    }
    assert!(
        said.contains(env!("CARGO_PKG_LICENSE")),
        "vcw doctor does not say what VCW itself is licensed under:\n{said}"
    );
    if notices.iter().any(|notice| notice.copyleft) {
        assert!(
            said.contains("relink"),
            "this build links weak copyleft code and `doctor` makes no relink \
             offer:\n{said}"
        );
    }
}
