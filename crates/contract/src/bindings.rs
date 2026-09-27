/*
 *  bindings.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Renders this crate's types as the TypeScript declaration file the frontend compiles against.
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

//! Renders this crate's types as the TypeScript declaration file the frontend
//! compiles against.
//!
//! D9: the contract is generated and a drift check fails the build when the
//! committed copy and the Rust disagree. Hand-written TypeScript interfaces are
//! how §2 erodes - not in one commit, but in the third one, where a field is
//! added on the Rust side and the interface that was right yesterday is quietly
//! describing something that no longer arrives.
//!
//! # Why one file, assembled here, rather than `ts-rs`'s own export
//!
//! `ts-rs` can write a file per type with `import` statements between them, and
//! that is the wrong artifact for two reasons. A contract reviewed in a diff
//! wants to be one diff; and a directory of generated files has to be compared
//! entry by entry, so a *deleted* type is the case a drift check gets wrong.
//! One file is one string comparison, and a removed type is a removed block.
//!
//! The declarations themselves still come from `ts-rs`, which is the part that
//! would drift: it is what knows that `Option<T>` is `T | null`, that a
//! `#[serde(rename_all)]` moves the field names and that `Vec<f32>` is
//! `Array<number>`.
//!
//! # `u64` is `number`, on purpose
//!
//! `ts-rs` maps 64-bit integers to `bigint` by default, which is honest about
//! the type and wrong about the value: `serde_json` writes a `u64` as a JSON
//! number and `JSON.parse` gives back a double, so nothing that crosses this
//! boundary is ever a `bigint` at run time. A frontend typed that way cannot do
//! arithmetic on a frame count without a cast that is a lie in the other
//! direction. The ceiling this accepts is 2^53 frames, which at 192 kHz is
//! rather more than a billion years of recording.

use std::fmt::Write as _;

use ts_rs::{Config, TS};

/// Where the generated file is committed, relative to the repository root.
pub const PATH: &str = "app/ui/src/bindings/vcw.d.ts";

/// The header written above the declarations.
///
/// No timestamp and no version: either one would make every regeneration a
/// diff, and a drift check whose output changes on its own cannot be trusted
/// to mean anything when it does change.
const HEADER: &str = "\
/*
 *  vcw.d.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  GENERATED FILE - do not edit.
 *
 *  Produced by `vcw_contract::bindings::typescript` from the Rust types in
 *  `crates/contract/src/`, which are the authority for anything this file does
 *  not say. Regenerate with:
 *
 *      VCW_BLESS=1 cargo test -p vcw-contract --test bindings
 *
 *  The types are §35's three groups: `Wire` is every event the core publishes,
 *  the request payloads are what a UI may ask for, and the rest are view
 *  models - what a UI is given to draw, with the units already resolved.
 *
 * MIT License
 *
 * Copyright (c) 2026 Stue Hunter
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the \"Software\"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */
";

/// The configuration every declaration is rendered under.
///
/// One place, because a config that differed between two calls would produce a
/// file that no single regeneration reproduces.
fn config() -> Config {
    Config::new().with_large_int("number")
}

/// Emits one section of the file.
fn section(out: &mut String, title: &str, why: &str, decls: &[String]) {
    let rule = "-".repeat(70);
    let _ = write!(out, "\n// {rule}\n// {title}\n//\n");
    for line in why.lines() {
        let _ = writeln!(out, "// {line}");
    }
    let _ = writeln!(out, "// {rule}");
    for decl in decls {
        let _ = write!(out, "\nexport {decl}\n");
    }
}

/// Renders a type's declaration.
///
/// Trailing whitespace is stripped, because ts-rs leaves a space after every
/// field and a committed file should not carry any: the first editor or
/// formatter to touch the file would remove it, and the drift check would then
/// fail on whitespace nobody typed.
fn decl<T: TS + 'static + ?Sized>() -> String {
    T::decl(&config())
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The whole declaration file, as a string.
///
/// Deterministic: same input, same bytes, every time. The order is the order
/// written here rather than anything derived, because a stable diff is the
/// point and a sorted list still reorders when a name changes.
#[must_use]
pub fn typescript() -> String {
    let mut out = String::from(HEADER);

    section(
        &mut out,
        "Events (§35)",
        "Everything the core publishes, as one discriminated union tagged on\n\
         `kind`. The tag values are the names `vcw_core::Event::name` declares,\n\
         which is what makes a `switch` over this union exhaustive and stable.",
        &[
            decl::<crate::event::Wire>(),
            decl::<crate::event::PhaseName>(),
            decl::<crate::event::EdgeName>(),
            decl::<crate::event::ProvenanceName>(),
            decl::<crate::event::CaptureStateName>(),
        ],
    );

    section(
        &mut out,
        "Commands (§35)",
        "Request payloads. `Request` is the whole set as one union; the Tauri\n\
         shell exposes one command per verb and takes the payload types\n\
         directly, which is the idiom and gives a better error.",
        &[
            decl::<crate::command::Request>(),
            decl::<crate::command::Failure>(),
            decl::<crate::command::Arm>(),
            decl::<crate::command::Transport>(),
            decl::<crate::command::Audition>(),
            decl::<crate::command::Playback>(),
            decl::<crate::command::Seek>(),
            decl::<crate::command::Marker>(),
            decl::<crate::command::Placement>(),
            decl::<crate::command::Removal>(),
            decl::<crate::command::Lock>(),
            decl::<crate::command::TrackEdit>(),
            decl::<crate::command::Split>(),
            decl::<crate::command::Merge>(),
            decl::<crate::command::Detect>(),
            decl::<crate::command::Region>(),
            decl::<crate::command::Zoom>(),
            decl::<crate::command::Search>(),
            decl::<crate::command::Selection>(),
            decl::<crate::command::Export>(),
            decl::<crate::command::NewProject>(),
        ],
    );

    section(
        &mut out,
        "View models (§35)",
        "What a UI is given to draw. Units are resolved on the Rust side: dBFS\n\
         rather than amplitudes, seconds beside frames, a side letter rather\n\
         than a side index. §2 is why - each of those is a calculation, and a\n\
         calculation in the view layer is application logic in the wrong place.",
        &[
            decl::<crate::view::Meter>(),
            decl::<crate::view::Levels>(),
            decl::<crate::view::Diagnostics>(),
            decl::<crate::view::Device>(),
            decl::<crate::view::Capture>(),
            decl::<crate::view::Side>(),
            decl::<crate::view::Track>(),
            decl::<crate::view::Boundary>(),
            decl::<crate::view::Measurement>(),
            decl::<crate::view::Project>(),
            decl::<crate::view::Release>(),
            decl::<crate::view::Candidate>(),
            decl::<crate::view::Accepted>(),
            decl::<crate::view::Waveform>(),
            decl::<crate::view::ExportPlan>(),
            decl::<crate::view::ExportFile>(),
        ],
    );

    section(
        &mut out,
        "Settings (\u{a7}39)",
        "Five groups, named as \u{a7}39 names them. Every default is either `null`\n\
         - let the engine negotiate and report what it got - or a constant read\n\
         out of the crate that owns the behaviour, which is why none of them\n\
         appears in this declaration. `Credential` is the one type here that\n\
         carries nothing: whether a token is configured and how long it is,\n\
         never the value.",
        &[
            decl::<crate::settings::Settings>(),
            decl::<crate::settings::Audio>(),
            decl::<crate::settings::Recording>(),
            decl::<crate::settings::Detection>(),
            decl::<crate::settings::Metadata>(),
            decl::<crate::settings::Export>(),
            decl::<crate::settings::Credential>(),
        ],
    );

    out
}
