/*
 *  app/src-tauri/tests/the_bundle_ships_what_a_user_needs.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The bundle configuration says what WP-19 promises, and every file it names is there.
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

//! The bundle configuration says what WP-19 promises, and every file it names
//! is there.
//!
//! Three of these checks are here because the first two bundle builds of the
//! project got them wrong and nothing failed: `beforeBuildCommand` pointed at a
//! directory that does not exist (the command runs with `app/` as its working
//! directory, not `app/src-tauri/`), the identifier ended in `.app` which
//! collides with the macOS bundle extension, and `deb.depends` repeated the two
//! packages the bundler adds by itself, which put each of them in the control
//! file twice. A bundle build takes four minutes and is not in the gate, so the
//! configuration needs a reader that is.

use std::path::{Path, PathBuf};

use serde_json::Value;

fn src_tauri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn config() -> Value {
    let text = std::fs::read_to_string(src_tauri().join("tauri.conf.json"))
        .expect("tauri.conf.json is beside the manifest");
    serde_json::from_str(&text).expect("tauri.conf.json is JSON")
}

/// A path as the bundler resolves it: relative to `tauri.conf.json`.
fn beside_config(relative: &str) -> PathBuf {
    src_tauri().join(relative)
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|item| item.as_str().expect("a string").to_owned())
        .collect()
}

#[test]
fn every_file_the_bundle_names_is_where_it_says() {
    let config = config();
    let bundle = &config["bundle"];

    // The icons are also checked by `generate_context!`, which refuses to
    // compile against a path that is not there - proved by mutation, where the
    // missing icon came back as a proc-macro panic rather than a red test. The
    // loop stays because it names the file in a sentence, and because nothing
    // guards `licenseFile` or `deb.files` at all.
    for icon in strings(&bundle["icon"]) {
        let path = beside_config(&icon);
        assert!(
            path.is_file(),
            "bundle.icon names {icon}, which is not there"
        );
    }

    let license = beside_config(bundle["licenseFile"].as_str().expect("a licenseFile"));
    assert!(license.is_file(), "bundle.licenseFile points at nothing");

    for (installed, source) in bundle["linux"]["deb"]["files"]
        .as_object()
        .expect("deb.files is an object")
        .iter()
    {
        let path = beside_config(source.as_str().expect("a source path"));
        assert!(
            path.is_file(),
            "deb.files would install {installed} from {source}, which is not there"
        );
    }

    // The frontend build command runs with `app/` as its working directory, so
    // its `--dir` is relative to that and not to this crate.
    let before = config["build"]["beforeBuildCommand"]
        .as_str()
        .expect("a build command");
    let dir = before
        .split("--dir ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next());
    let dir = dir.expect("the build command passes --dir");
    let crate_dir = src_tauri();
    let app = crate_dir
        .parent()
        .expect("app/ is the parent of src-tauri/");
    assert!(
        app.join(dir).join("package.json").is_file(),
        "beforeBuildCommand builds {dir}, which has no package.json relative to app/"
    );
}

#[test]
fn the_package_carries_the_command_line_as_well_as_the_window() {
    // A package that installs only the window makes most of USER-GUIDE.md
    // unusable: every workflow in it is a `vcw ...` line, and `vcw bundle` is
    // what a person is asked for when something breaks.
    let config = config();
    let sidecars = strings(&config["bundle"]["externalBin"]);
    assert!(
        sidecars.contains(&"binaries/vcw".to_owned()),
        "bundle.externalBin does not ship the CLI: {sidecars:?}"
    );

    // The bundler looks for `<externalBin>-<triple>`, so the staging script has
    // to agree with the configuration about the name and the directory.
    let script = std::fs::read_to_string(src_tauri().join("../../tools/stage-cli.sh"))
        .expect("tools/stage-cli.sh is in the repository");
    assert!(
        script.contains("app/src-tauri/binaries/vcw-$triple$suffix"),
        "stage-cli.sh does not write the name bundle.externalBin asks for"
    );
    assert!(
        script.contains("--profile ship"),
        "stage-cli.sh should stage the ship profile, not a debug binary"
    );
}

#[test]
fn the_bundle_targets_every_tier_one_platform() {
    let config = config();
    let bundle = &config["bundle"];
    assert_eq!(bundle["active"], Value::Bool(true), "bundling is off");
    let targets = strings(&bundle["targets"]);
    for wanted in ["deb", "appimage", "msi", "app", "dmg"] {
        assert!(
            targets.contains(&wanted.to_owned()),
            "WP-19 asks for a {wanted} and the config does not list it"
        );
    }
    assert!(
        bundle["macOS"]["minimumSystemVersion"].is_string(),
        "the macOS bundle needs a minimum system version or it claims to run on 10.x"
    );
}

#[test]
fn the_identifier_is_one_macos_can_use() {
    // `cargo tauri build` warns rather than fails on this, and the warning
    // scrolls past in a four-minute build.
    let identifier = config()["identifier"]
        .as_str()
        .expect("an identifier")
        .to_owned();
    assert!(
        !identifier.ends_with(".app"),
        "identifier {identifier} ends in .app, which collides with the macOS bundle extension"
    );
    assert!(
        identifier.contains('.'),
        "identifier {identifier} is not reverse-domain"
    );
}

#[test]
fn the_deb_does_not_repeat_what_the_bundler_already_asked_for() {
    // Measured from a built package: listing these put each one in Depends
    // twice, because the bundler appends its own before ours.
    let config = config();
    let depends = strings(&config["bundle"]["linux"]["deb"]["depends"]);
    for automatic in ["libwebkit2gtk-4.1-0", "libgtk-3-0"] {
        assert!(
            !depends.contains(&automatic.to_owned()),
            "deb.depends lists {automatic}, which the bundler adds itself"
        );
    }
    assert!(
        depends.contains(&"libasound2".to_owned()),
        "the deb must depend on ALSA: the window enumerates devices on start-up"
    );
}

#[test]
fn the_window_is_locked_down() {
    let config = config();
    let csp = config["app"]["security"]["csp"].as_str().expect("a csp");
    assert!(
        csp.contains("default-src 'self'"),
        "the CSP does not default to self"
    );
    assert!(!csp.contains("unsafe-eval"), "the CSP allows eval");
    assert!(csp.contains("object-src 'none'"), "the CSP allows objects");
    // The asset protocol and the IPC origin have to be allowed by name, or the
    // release build cannot draw its own waveform.
    assert!(
        csp.contains("ipc:"),
        "the CSP forbids the IPC origin the shell talks over"
    );
}

#[test]
fn the_version_is_one_number_in_one_place() {
    // `CARGO_PKG_VERSION` is whatever the manifest resolved to, inherited from
    // the workspace or not, so this compares the built crate and not the text.
    let declared = config()["version"].as_str().expect("a version").to_owned();
    assert_eq!(
        declared,
        env!("CARGO_PKG_VERSION"),
        "tauri.conf.json names a different version from the crate it bundles"
    );

    let changelog =
        std::fs::read_to_string(src_tauri().join("../../CHANGELOG.md")).expect("the changelog");
    assert!(
        changelog.contains("## Unreleased") || changelog.contains(&declared),
        "the changelog names neither an unreleased section nor version {declared}"
    );
}

/// A guard on the guards: if `tauri.conf.json` moved or stopped being JSON,
/// every test above would fail for the same uninformative reason, so say it
/// once and clearly.
#[test]
fn the_configuration_is_where_these_tests_look() {
    assert!(
        Path::new(&src_tauri().join("tauri.conf.json")).is_file(),
        "tauri.conf.json is not beside the manifest any more"
    );
}
