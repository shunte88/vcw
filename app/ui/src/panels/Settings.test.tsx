/*
 *  Settings.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The Metadata group: switches, the token it cannot hold, and a page by name.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Three things worth a test in this group, all of them wiring rather than
// behavior.
//
// The switches are switches. `Switch` is the product's two-state control from
// here on, and the thing a checkbox-to-switch conversion gets wrong is leaving
// one behind - which looks fine until the two sit in the same column.
//
// The Discogs token is reported where it is asked for. The panel has no field
// for it and never will (§39), so the only thing it can offer is "did yours
// arrive", and that answer comes from the credential survey keyed by variable
// name. A rename on either side makes it silently read "not set", which is
// exactly the complaint that put this line here.
//
// And the token page is asked for *by name*. This webview is handed no
// addresses; the shell holds the table. A literal URL here would compile.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Credential, Settings as Values } from "../bindings/vcw";
import type { Store } from "../store";
import { Settings } from "./Settings";

/** Which pages the shell was asked to open, in order. */
let opened: string[] = [];

vi.mock("../api", () => ({
  saveSettings: vi.fn(async () => undefined),
  support: async (page: string) => {
    opened.push(page);
  },
}));

function values(): Values {
  return {
    audio: {},
    recording: {},
    detection: {},
    metadata: {
      online: true,
      musicbrainz: true,
      discogs: false,
      contact: null,
      genreMap: null,
    },
    export: { format: "flac" },
  } as unknown as Values;
}

function token(present: boolean): Credential {
  return {
    name: "discogs",
    present,
    characters: present ? 40 : 0,
    variable: "VCW_DISCOGS_TOKEN",
  };
}

/** Renders the panel with the Metadata group showing. */
async function shown(credentials: readonly Credential[]): Promise<HTMLElement> {
  localStorage.setItem("vcw.settings.section", "Metadata");
  const store = { run: async (what: () => Promise<unknown>) => what() };
  const container = document.createElement("div");
  document.body.append(container);
  await act(async () => {
    createRoot(container).render(
      <Settings
        store={store as unknown as Store}
        settings={values()}
        credentials={credentials}
        devices={[]}
        onSaved={() => {}}
      />,
    );
  });
  return container;
}

describe("the metadata settings", () => {
  beforeEach(() => {
    opened = [];
  });

  it("asks its three questions with switches and no tickbox", async () => {
    const container = await shown([token(true)]);
    const pane = container.querySelector(".prefs-pane");
    expect(pane?.querySelectorAll("label.switch")).toHaveLength(3);
    expect(pane?.querySelectorAll("label.tick")).toHaveLength(0);
    const text = pane?.textContent ?? "";
    expect(text).toContain("Allow network lookups");
    expect(text).toContain("MusicBrainz");
    expect(text).toContain("Discogs");
  });

  it("says whether the Discogs token arrived, and how long it is", async () => {
    const text = (await shown([token(true)])).textContent ?? "";
    expect(text).toContain("VCW_DISCOGS_TOKEN");
    expect(text).toContain("set, 40 characters");
  });

  it("says it did not arrive rather than nothing, when it did not", async () => {
    const text = (await shown([token(false)])).textContent ?? "";
    expect(text).toContain("not set in this process");
    expect(text).not.toContain("40 characters");
  });

  it("asks the shell for the token page by name, naming no address", async () => {
    const container = await shown([token(false)]);
    const link = container.querySelector<HTMLButtonElement>("button.globe-link");
    expect(link).not.toBeNull();
    expect(container.querySelector(".prefs-pane a[href]")).toBeNull();

    await act(async () => {
      link?.click();
    });
    expect(opened).toEqual(["discogs-token"]);
  });
});
