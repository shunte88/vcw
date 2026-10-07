/*
 *  About.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The dialog shows the notices it is given, and the relink offer only when one
 *  of them is copyleft.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The Rust side owns the hard half of WP-28: `vcw_export::notices` derives the
// list from the cargo features and the gate's `features` leg asserts it in all
// four combinations. What a webview test can add is the other half - that the
// panel renders whatever it was handed and invents nothing.
//
// The support buttons are the other case worth one: they are the only controls
// in this product that leave it, the addresses they open are a table in the
// shell, and a click that refuses has to say so rather than look like a dead
// button.
//
// The relink offer is the case worth a test. LGPL-3.0 section 4 requires the
// offer be made when the component is linked, and nothing requires it otherwise:
// a build with no copyleft component that printed it anyway would be claiming an
// obligation it does not have, and a build with one that omitted it would be the
// failure the whole work package exists to prevent. Both directions, below.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";

import type { About as Build, Notice } from "../bindings/vcw";
import { About } from "./About";

const flacenc: Notice = {
  component: "flacenc",
  license: "Apache-2.0",
  provides: "FLAC export",
  source: "https://crates.io/crates/flacenc",
  copyleft: false,
};

const lame: Notice = {
  component: "libmp3lame, via mp3lame-encoder and mp3lame-sys",
  license: "LGPL-3.0",
  provides: "MP3 export",
  source: "https://crates.io/crates/mp3lame-sys",
  copyleft: true,
};

function build(notices: Notice[]): Build {
  return {
    product: "VCW - The Vinyl Capture Workstation",
    description: "the de facto tool for vinyl capture across platforms",
    version: "0.1.0",
    profile: "release",
    built: "2026-10-05",
    repository: "https://github.com/shunte88/vcw",
    authors: ["Stue Hunter"],
    license: "MIT",
    os: "linux",
    arch: "x86_64",
    sqlite: "3.50.4",
    schemaVersion: 3,
    formatVersion: 1,
    notices,
  };
}

// Read when `about()` is called rather than when the mock is built, so one
// factory serves every test.
vi.mock("../api", () => ({
  about: async () => build(current),
  support: async (page: string) => {
    opened.push(page);
    if (refusal !== null) {
      throw { code: "invalid-argument", message: refusal };
    }
  },
  asFailure: (error: unknown) => error,
}));

/** Which notices the backend reports. Set by [`shown`] before each render. */
let current: Notice[] = [];

/** Which pages the shell was asked to open, in order. */
let opened: string[] = [];

/** What the shell refuses the next `support` with, or null to accept it. */
let refusal: string | null = null;

/** Renders the dialog with the backend reporting these notices. */
async function shown(notices: Notice[]): Promise<HTMLElement> {
  current = notices;
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<About onClose={() => {}} />);
  });
  return container;
}

describe("the about dialog", () => {
  it("shows the build identity a bug report asks for", async () => {
    const container = await shown([flacenc]);
    const text = container.textContent ?? "";
    expect(text).toContain("0.1.0");
    expect(text).toContain("release");
    expect(text).toContain("linux");
    expect(text).toContain("x86_64");
    expect(text).toContain("3.50.4");
    // The schema version, because a project that will not open elsewhere is a
    // question about this number.
    expect(text).toContain("schema v3");
  });

  it("names every notice it was given and nothing else", async () => {
    const container = await shown([flacenc, lame]);
    const text = container.textContent ?? "";
    expect(text).toContain("flacenc");
    expect(text).toContain("Apache-2.0");
    expect(text).toContain("libmp3lame, via mp3lame-encoder and mp3lame-sys");
    expect(text).not.toContain("vorbis");
  });

  it("says what the project is, in the sentence the project uses", async () => {
    const container = await shown([flacenc]);
    expect(container.textContent ?? "").toContain(
      "the de facto tool for vinyl capture across platforms",
    );
  });

  it("makes the relink offer when a component is copyleft", async () => {
    const container = await shown([flacenc, lame]);
    const text = container.textContent ?? "";
    expect(text).toContain("LGPL-3.0");
    expect(text).toMatch(/relink/i);
  });

  it("makes no relink offer when nothing linked is copyleft", async () => {
    const container = await shown([flacenc]);
    const text = container.textContent ?? "";
    expect(text).not.toMatch(/relink/i);
    expect(text).not.toContain("LGPL");
  });

  it("states VCW's own license from the build rather than from this file", async () => {
    const container = await shown([flacenc]);
    expect(container.textContent ?? "").toContain("MIT");
  });

  it("asks the shell for a page by name, naming no address of its own", async () => {
    opened = [];
    refusal = null;
    const container = await shown([flacenc]);
    const coffee = container.querySelector<HTMLButtonElement>("button.bmc");
    const shirts = container.querySelector<HTMLButtonElement>("button.as-link");

    // Buttons, not anchors: an `href` here would be an address the webview
    // chose, and the whole point is that it cannot choose one.
    expect(coffee).not.toBeNull();
    expect(shirts).not.toBeNull();
    expect(container.querySelector("a[href]")).toBeNull();

    await act(async () => {
      coffee?.click();
    });
    await act(async () => {
      shirts?.click();
    });
    expect(opened).toEqual(["coffee", "shirts"]);
  });

  it("opens the repository from the logo and each component from its globe", async () => {
    opened = [];
    refusal = null;
    const container = await shown([flacenc, lame]);

    await act(async () => {
      container.querySelector<HTMLButtonElement>("button.about-logo-link")?.click();
    });
    for (const globe of container.querySelectorAll<HTMLButtonElement>(
      "button.globe",
    )) {
      await act(async () => {
        globe.click();
      });
    }

    // The names, not the addresses: the shell holds the table, and a webview
    // that knew the URL to pass would be the thing WP-28 is built to avoid.
    expect(opened).toEqual(["repository", flacenc.component, lame.component]);
  });

  it("offers the shirts as well as the coffee", async () => {
    const text = (await shown([flacenc])).textContent ?? "";
    expect(text).toContain("Buy me a coffee");
    expect(text).toMatch(/Git The Shirt/i);
    expect(text).toContain("Team Badger shirts");
  });

  it("says why when no browser would start, rather than looking dead", async () => {
    opened = [];
    refusal = "no browser could be started for https://www.buymeacoffee.com/shunte88";
    const container = await shown([flacenc]);

    await act(async () => {
      container.querySelector<HTMLButtonElement>("button.bmc")?.click();
    });
    expect(container.textContent ?? "").toContain("no browser could be started");
  });
});
