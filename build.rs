/*
 *  build.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Worker supervision and the lifecycle of a capture session (§36).
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

#[allow(dead_code)]
#[allow(unused_imports)]

use chrono::Utc;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir      = env::var("OUT_DIR").unwrap();
    let dest_path    = Path::new(&out_dir).join("build_info.rs");

    let version    = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let pkg_name   = env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "vripr".to_string());

    let now              = Utc::now();
    let build_date       = now.format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let build_date_short = now.format("%Y-%m-%d").to_string();

    fs::write(
        &dest_path,
        format!(
            "pub const APP_NAME: &str = \"{pkg_name}\";\n\
             pub const VERSION: &str = \"{version}\";\n\
             pub const BUILD_DATE: &str = \"{build_date}\";\n",
        ),
    )
    .unwrap();

    // Generate a version badge SVG in the project root
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let svg_path = Path::new(&manifest_dir).join("version.svg");

    let value      = format!("VRipr v{} | Built: {}", version, build_date_short);
    let char_width = 6.4_f64;
    let padding    = 12.0_f64;
    let value_width = (value.len() as f64 * char_width + padding * 2.0).round();

    let svg = format!(
        r##"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<svg
   width="{value_w}" height="20" role="img" aria-label="{value}"
   version="1.1" id="svg01"
   xmlns="http://www.w3.org/2000/svg">
  <title id="title1">{value}</title>
  <defs id="defs1">
    <linearGradient id="grad01" x2="0" y2="78.993668"
       gradientTransform="matrix(3.9496835,0,0,0.25318484,2.2538372,-53.528634)"
       x1="0" y1="0" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#bbbbbb" stop-opacity=".1" id="stop1"
         style="stop-color:#000000;stop-opacity:0.1;" />
      <stop offset="1" stop-opacity=".1" id="stop2" />
    </linearGradient>
    <clipPath id="clipper">
      <rect width="{value_w}" height="20" rx="3" fill="#ffffff" id="rect2" x="0" y="0" />
    </clipPath>
  </defs>
  <g clip-path="url(#clipper)" id="g5">
    <rect width="{value_w}" height="20" fill="#555555" id="rect3" x="0" y="0" />
    <rect x="{value_w}" width="{value_w}" height="20" fill="#44cc11" id="rect4" y="0" />
    <rect width="{value_w}" height="20" fill="url(#grad01)" id="rect5"
       style="fill:url(#grad01);stroke:#000000;stroke-opacity:1"
       x="2.2538373" y="-53.528633" />
    <text xml:space="preserve"
       style="font-style:normal;font-size:10px;font-family:'DejaVu Serif';text-align:center;text-anchor:middle;fill:#FFD700;stroke:#BE8400;stroke-width:0.5;stroke-linecap:round;stroke-linejoin:round;stroke-opacity:1"
       x="{value_cx}" y="13" id="text7">&gt;&gt; {value} &lt;&lt;</text>
  </g>
</svg>"##,
        value_w  = value_width,
        value_cx = (value_width * 0.5),
        value    = value,
    );

    // Only rewrite when the badge actually changes, so an unchanged rebuild
    // doesn't touch the file's mtime or show up as a dirty working tree.
    let unchanged = fs::read_to_string(&svg_path).map(|current| current == svg).unwrap_or(false);
    if !unchanged {
        // Best-effort: the manifest directory is read-only for `cargo install`
        // and vendored builds, where a stale badge is preferable to a failed build.
        if let Err(error) = fs::write(&svg_path, svg) {
            println!("cargo:warning=could not regenerate version.svg: {error}");
        }
    }

    // `src` is listed so ordinary source edits regenerate the badge. Cargo only
    // re-runs this script for the paths named here, so without it the badge's
    // build date would only move when build.rs or Cargo.toml changed.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
