/*
 *  About.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Which build this is, what it is licensed under, and what it links (WP-28).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Every fact here arrives from `api.about()`. Nothing is typed into this file:
// not the version, not the license, and above all not the notices.
//
// The notices are a license obligation rather than a courtesy. A build with the
// `mp3` feature compiles libmp3lame into the binary under LGPL-3.0, inside a
// product whose own code is MIT, and LGPL-3.0 section 4 wants the person holding
// that binary told where the source is and that they may relink it. A build
// without the feature has no such component - so a sentence about LGPL written
// here would be false in one of the four combinations the gate builds, and
// nothing in a webview could ever know which.
//
// The credits are the one part of the dialog that is prose, because there is no
// fact to derive them from.
//
// The body scrolls rather than the whole box, so that Close stays on screen at
// any window height: with the logo at the top, this content is taller than the
// 700px minimum the shell's own window allows.

import { useEffect, useState } from "react";

import * as api from "../api";
import type { About as Build } from "../bindings/vcw";

/** The people, and the model, the work is by. */
const CREDITS: [string, string][] = [
  ["Stue Hunter", "design, architecture and implementation"],
  [
    "Claude Opus 5 (Anthropic)",
    "pair programming throughout, by the author's invitation",
  ],
];

/** The about overlay. */
export function About({
  onClose,
}: {
  onClose: () => void;
}): React.JSX.Element {
  const [build, setBuild] = useState<Build | null>(null);
  // A browser that will not start is the one thing in this dialog that can
  // fail, and a button that silently does nothing is worse than a sentence
  // saying why it did nothing.
  const [refused, setRefused] = useState<string | null>(null);

  /** Asks the shell for a page, and says so if it will not open one. */
  function visit(page: "coffee" | "shirts"): void {
    setRefused(null);
    void api.support(page).catch((error: unknown) => {
      setRefused(api.asFailure(error).message);
    });
  }

  useEffect(() => {
    let live = true;
    void api.about().then((answer) => {
      if (live) {
        setBuild(answer);
      }
    });
    return () => {
      live = false;
    };
  }, []);

  return (
    <div className="overlay" role="dialog" aria-label="About VCW">
      <div className="overlay-box about">
        <header className="panel-head">
          <h2>About VCW</h2>
          <button type="button" onClick={onClose}>
            Close (Esc)
          </button>
        </header>

        <div className="about-scroll">
          {build === null ? (
            <p className="dim">Reading the build...</p>
          ) : (
            <>
              {/* The name reads before the art rather than under it, and the
                  art is the master logo scaled down rather than a second
                  drawing of it - see `assets/vcw_logo_main.webp`. */}
              <p className="about-title">{build.product}</p>

              {/* The art beside the build facts rather than above them: the
                  logo is as tall as five table rows, and stacked they spent a
                  third of the panel on a column of air to the logo's left and
                  right. */}
              <div className="about-masthead">
                <img
                  className="about-logo"
                  src="/vcw-logo.webp"
                  alt=""
                  width={280}
                />
                <section className="about-group">
                  <h3>This build</h3>
                  <table className="rows">
                    <tbody>
                      <tr>
                        <td className="dim">Version</td>
                        <td>
                          {build.version} ({build.profile})
                        </td>
                      </tr>
                      <tr>
                        <td className="dim">Built</td>
                        <td>{build.built}</td>
                      </tr>
                      <tr>
                        <td className="dim">Platform</td>
                        <td>
                          {build.os} {build.arch}
                        </td>
                      </tr>
                      <tr>
                        <td className="dim">SQLite</td>
                        <td>{build.sqlite} (bundled)</td>
                      </tr>
                      <tr>
                        <td className="dim">Project format</td>
                        <td>
                          schema v{build.schemaVersion}, audio v
                          {build.formatVersion}
                        </td>
                      </tr>
                      <tr>
                        <td className="dim">Source</td>
                        <td>{build.repository}</td>
                      </tr>
                    </tbody>
                  </table>
                </section>
              </div>

              <section className="about-group">
                <h3>License</h3>
                <p>
                  VCW&apos;s own code is {build.license}. Copyright (c) 2026{" "}
                  {build.authors.join(", ")}.
                </p>
              </section>

              <section className="about-group">
                <h3>Third-party components</h3>
                <p className="dim">
                  What this build links, derived from the features it was
                  compiled with. THIRD-PARTY-NOTICES.md and Cargo.lock in the
                  repository are the full inventory.
                </p>
                <table className="rows notices">
                  <tbody>
                    {build.notices.map((notice) => (
                      <tr key={notice.component}>
                        <td>{notice.component}</td>
                        <td>{notice.license}</td>
                        <td className="dim">{notice.provides}</td>
                        <td>{notice.source}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {build.notices.some((notice) => notice.copyleft) && (
                  <p>
                    The components marked above as{" "}
                    {build.notices
                      .filter((notice) => notice.copyleft)
                      .map((notice) => notice.license)
                      .join(", ")}{" "}
                    are weak copyleft. You may modify them and relink them into
                    VCW: their complete source is at the addresses listed, the
                    exact versions this build used are recorded in Cargo.lock at
                    its release tag, and the complete source of VCW itself is
                    available under its own license at the repository above.
                  </p>
                )}
              </section>

              <section className="about-group">
                <h3>Credits</h3>
                <table className="rows">
                  <tbody>
                    {CREDITS.map(([who, what]) => (
                      <tr key={who}>
                        <td>{who}</td>
                        <td className="dim">{what}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>

              {/* Buttons and not anchors: there is no address this webview
                  should navigate to, and the two it can open are a table in
                  the shell rather than anything this side could name. */}
              <section className="about-group about-support">
                <button
                  type="button"
                  className="bmc"
                  onClick={() => {
                    visit("coffee");
                  }}
                >
                  <img src="/bmac-icon.png" alt="" width={56} height={56} />
                  <span>Buy me a coffee</span>
                </button>

                <h3>Like The App - Git The Shirt</h3>
                <p>
                  Team Badger shirts and other goodies are available at{" "}
                  <button
                    type="button"
                    className="as-link"
                    onClick={() => {
                      visit("shirts");
                    }}
                  >
                    shunte88
                  </button>
                </p>
                {refused !== null && <p className="dim">{refused}</p>}
              </section>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
