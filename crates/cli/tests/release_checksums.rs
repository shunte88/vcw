/*
 *  crates/cli/tests/release_checksums.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The release checksum tool writes and checks what WP-19 says it does.
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

//! The release checksum tool writes and checks what WP-19 says it does.
//!
//! WP-19 asks for a "checksum verification tool", and `tools/verify-release.py`
//! is it. The tool runs on a machine where VCW is not installed yet - somebody
//! has downloaded an installer and wants to know whether it arrived intact - so
//! it is Python with nothing but the standard library, and it cannot be tested
//! from inside a crate it has no connection to. It is tested the way
//! `tools/vcw-read.py` is: by being run.
//!
//! The interesting cases are the failures. A verifier that says `ok` is doing
//! the easy half; the half that matters is the tampered file, the download that
//! never finished, and the artifact that is sitting in the directory without
//! being in the list at all.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The tool under test.
fn tool() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate is two levels below the repository root")
        .join("tools/verify-release.py");
    assert!(path.is_file(), "{} is missing", path.display());
    path
}

/// A Python 3 interpreter, or a loud failure.
///
/// Not a skip. The tool exists to be run on somebody else's machine with
/// nothing installed, and a test that turns itself off when the interpreter is
/// absent would report that the tool works on a host where nothing ran it.
fn python() -> Command {
    for (program, args) in [
        ("python3", &[][..]),
        ("python", &[][..]),
        ("py", &["-3"][..]),
    ] {
        let mut probe = Command::new(program);
        probe.args(args).arg("--version");
        if probe
            .output()
            .is_ok_and(|out| out.status.success() && out.stdout.starts_with(b"Python 3"))
        {
            let mut found = Command::new(program);
            found.args(args);
            return found;
        }
    }
    panic!("no Python 3 interpreter found (tried python3, python, py -3)");
}

/// Run the tool. Returns whether it succeeded, and everything it said.
fn run(args: &[&str]) -> (bool, String) {
    let output = python()
        .arg(tool())
        .args(args)
        .output()
        .expect("run verify-release.py");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text)
}

/// A directory of plausible release artifacts.
fn artifacts(dir: &Path) {
    for (name, bytes) in [
        ("VCW_0.1.0_amd64.AppImage", &b"an appimage"[..]),
        ("VCW_0.1.0_amd64.deb", &b"a debian package"[..]),
        ("VCW_0.1.0_arm64.deb", &b"a debian package for the pi"[..]),
        ("VCW_0.1.0_x64_en-US.msi", &b"an installer"[..]),
        ("VCW_0.1.0_aarch64.dmg", &b"a disk image"[..]),
    ] {
        std::fs::write(dir.join(name), bytes).expect("write an artifact");
    }
}

fn sums(dir: &Path) -> String {
    dir.join("SHA256SUMS").display().to_string()
}

#[test]
fn the_tool_writes_a_sums_file_for_every_artifact_and_then_agrees_with_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    artifacts(dir.path());

    let (ok, said) = run(&["--write", &sums(dir.path())]);
    assert!(ok, "--write failed:\n{said}");
    let written = std::fs::read_to_string(dir.path().join("SHA256SUMS")).expect("read SHA256SUMS");
    assert_eq!(written.lines().count(), 5, "wrote:\n{written}");
    for name in [
        "VCW_0.1.0_amd64.AppImage",
        "VCW_0.1.0_amd64.deb",
        "VCW_0.1.0_arm64.deb",
        "VCW_0.1.0_x64_en-US.msi",
        "VCW_0.1.0_aarch64.dmg",
    ] {
        assert!(written.contains(name), "{name} is not in:\n{written}");
    }
    // The coreutils format, so `sha256sum -c SHA256SUMS` reads the same file.
    // Anyone who already has that tool should not need this one.
    for line in written.lines() {
        let (digest, name) = line.split_once("  ").expect("two spaces");
        assert_eq!(digest.len(), 64, "not a sha256: {line}");
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()), "{line}");
        assert!(!name.contains(' '), "{line}");
    }
    // It does not list itself.
    assert!(!written.contains("SHA256SUMS"), "{written}");

    let (ok, said) = run(&[&sums(dir.path())]);
    assert!(ok, "checking what it just wrote failed:\n{said}");
    assert_eq!(said.matches("ok       ").count(), 5, "{said}");
    assert!(said.contains("5 file(s) verified"), "{said}");
    // The honest sentence, which is the reason this is a checksum tool and not
    // a claim about provenance.
    assert!(said.contains("does not say who made them"), "{said}");
}

#[test]
fn a_tampered_artifact_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    artifacts(dir.path());
    assert!(run(&["--write", &sums(dir.path())]).0);

    std::fs::write(
        dir.path().join("VCW_0.1.0_x64_en-US.msi"),
        b"an installer with something extra in it",
    )
    .expect("tamper");

    let (ok, said) = run(&[&sums(dir.path())]);
    assert!(!ok, "a tampered artifact was accepted:\n{said}");
    assert!(said.contains("FAILED   VCW_0.1.0_x64_en-US.msi"), "{said}");
    assert!(said.contains("Do not run these files"), "{said}");
    // And it does not condemn the innocent ones.
    assert_eq!(said.matches("ok       ").count(), 4, "{said}");
}

#[test]
fn a_download_that_never_finished_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    artifacts(dir.path());
    assert!(run(&["--write", &sums(dir.path())]).0);
    std::fs::remove_file(dir.path().join("VCW_0.1.0_aarch64.dmg")).expect("remove");

    let (ok, said) = run(&[&sums(dir.path())]);
    assert!(!ok, "a missing artifact was accepted:\n{said}");
    assert!(said.contains("MISSING  VCW_0.1.0_aarch64.dmg"), "{said}");
}

#[test]
fn an_artifact_nobody_signed_up_for_is_named() {
    // Not a failure: the checksums cover what they cover, and an extra file is
    // usually a half-finished download or one from another release. Saying
    // nothing about it is how somebody ends up running it.
    let dir = tempfile::tempdir().expect("tempdir");
    artifacts(dir.path());
    assert!(run(&["--write", &sums(dir.path())]).0);
    std::fs::write(dir.path().join("VCW_0.0.9_amd64.deb"), b"last month's").expect("write");

    let (ok, said) = run(&[&sums(dir.path())]);
    assert!(ok, "an unlisted file should not fail the check:\n{said}");
    assert!(said.contains("unlisted VCW_0.0.9_amd64.deb"), "{said}");
}

#[test]
fn something_that_is_not_an_artifact_at_all_is_left_alone() {
    // Measured against the real artifacts: pointing the tool at a SHA256SUMS
    // written elsewhere made it report every file in the sums file's directory
    // as unlisted - eighty lines of scratch logs and X11 lock files around two
    // real answers. The sweep now looks only at things shaped like a release.
    let dir = tempfile::tempdir().expect("tempdir");
    artifacts(dir.path());
    assert!(run(&["--write", &sums(dir.path())]).0);
    std::fs::write(dir.path().join("RELEASE-NOTES.md"), b"# 0.1.0").expect("write");
    std::fs::write(dir.path().join("build.log"), b"...").expect("write");

    let (ok, said) = run(&[&sums(dir.path())]);
    assert!(
        ok,
        "a release note beside the artifacts is not a problem:\n{said}"
    );
    assert!(
        !said.contains("unlisted"),
        "nothing here is a stray artifact:\n{said}"
    );
}

#[test]
fn a_digest_can_be_pinned_to_one_given_out_of_band() {
    let dir = tempfile::tempdir().expect("tempdir");
    artifacts(dir.path());
    assert!(run(&["--write", &sums(dir.path())]).0);
    let written = std::fs::read_to_string(dir.path().join("SHA256SUMS")).expect("read");
    let (digest, name) = written
        .lines()
        .next()
        .and_then(|line| line.split_once("  "))
        .expect("a first line");

    // Upper case on purpose: a digest read off a web page or out of an email is
    // whatever case it was written in.
    let (ok, said) = run(&[&sums(dir.path()), "--expect", &digest.to_uppercase()]);
    assert!(ok, "a correct pinned digest was refused:\n{said}");
    assert!(said.contains(name), "{said}");

    let (ok, said) = run(&[&sums(dir.path()), "--expect", &"0".repeat(64)]);
    assert!(!ok, "a digest that matches nothing was accepted:\n{said}");
}

#[test]
fn a_sums_file_it_cannot_read_is_an_error_and_not_a_pass() {
    // The failure mode worth designing against: a verifier that skips the lines
    // it does not understand and then prints a reassuring summary.
    let dir = tempfile::tempdir().expect("tempdir");
    artifacts(dir.path());
    let path = dir.path().join("SHA256SUMS");

    for content in [
        "this is not a checksum file\n",
        "deadbeef  VCW_0.1.0_amd64.deb\n",
        "",
    ] {
        std::fs::write(&path, content).expect("write");
        let (ok, said) = run(&[&sums(dir.path())]);
        assert!(!ok, "accepted {content:?}:\n{said}");
        assert!(!said.contains("verified"), "{said}");
    }

    let (ok, said) = run(&[&dir.path().join("NOT-THERE").display().to_string()]);
    assert!(!ok, "accepted a SHA256SUMS that is not there:\n{said}");
    assert!(said.contains("not there"), "{said}");
}
