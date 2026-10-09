/*
 *  Settings.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The Metadata group: switches, the token it cannot hold, and a page by name.
 *  The Export group: two numbering controls over one stored word.
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
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { Credential, Settings as Values } from "../bindings/vcw";
import type { Store } from "../store";
import { Settings } from "./Settings";

/** Which pages the shell was asked to open, in order. */
let opened: string[] = [];

/** Every panel this file has mounted, so it can be taken down again.
 *
 * Not tidiness: the panel now carries a 600 ms auto-save timer, and a panel
 * left mounted fires it into whatever test happens to be running by then. The
 * first version of the two tests below failed for exactly that reason - a
 * select changed three tests earlier arrived as a write nobody had asked for.
 */
let mounted: { root: Root; container: HTMLElement }[] = [];

function mount(): { root: Root; container: HTMLElement } {
  const container = document.createElement("div");
  document.body.append(container);
  const entry = { root: createRoot(container), container };
  mounted.push(entry);
  return entry;
}

afterEach(() => {
  for (const { root, container } of mounted) {
    act(() => root.unmount());
    container.remove();
  }
  mounted = [];
});

vi.mock("../api", () => ({
  saveSettings: vi.fn(async () => undefined),
  languages: vi.fn(async () => ["en-US", "pt-BR"]),
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
    export: {
      format: "flac",
      quality: "transparent",
      compression: "5",
      output: null,
      template: "{tracknum} - {title}",
      artwork: "both",
      narrowing: "24",
      dither: "tpdf",
      headroom: "0",
      numbering: "alpha",
    },
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

/** Renders the panel with one group showing. */
async function shown(
  credentials: readonly Credential[],
  section = "Metadata",
  settings: Values = values(),
): Promise<HTMLElement> {
  localStorage.setItem("vcw.settings.section", section);
  const store = { run: async (what: () => Promise<unknown>) => what() };
  const { root, container } = mount();
  await act(async () => {
    root.render(
      <Settings
        store={store as unknown as Store}
        settings={settings}
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

describe("the export numbering controls", () => {
  /** The two selects, in the order they appear. */
  async function selects(
    numbering: string,
  ): Promise<[HTMLSelectElement, HTMLSelectElement]> {
    const settings = values();
    (settings.export as { numbering: string }).numbering = numbering;
    const container = await shown([token(false)], "Export", settings);
    const labels = [...container.querySelectorAll("label")].filter((label) =>
      /^(Track numbers|Counted)/.test(label.textContent ?? ""),
    );
    expect(labels).toHaveLength(2);
    return [
      labels[0]!.querySelector("select")!,
      labels[1]!.querySelector("select")!,
    ];
  }

  it("shows alpha as a per-side count that cannot be changed", async () => {
    // Not a cosmetic detail: `A6` is not a numbering anybody prints, so the
    // second control has nothing to offer while the first says Label - and
    // "Per side" is the true reading of the label's own numbering, not a
    // placeholder.
    const [scheme, counted] = await selects("alpha");
    expect(scheme.value).toBe("alpha");
    expect(counted.value).toBe("numeric");
    expect(counted.disabled).toBe(true);
  });

  it("writes one word from two controls, and Label disables the second", async () => {
    const [scheme, counted] = await selects("sequence");
    expect(scheme.value).toBe("numeric");
    expect(counted.value).toBe("sequence");
    expect(counted.disabled).toBe(false);

    // Choosing Label writes one word for both controls, and the second one
    // goes dead rather than lingering on an answer nothing reads.
    await act(async () => {
      scheme.value = "alpha";
      scheme.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(scheme.value).toBe("alpha");
    expect(counted.disabled).toBe(true);
  });
});

describe("a settings panel with no Save button", () => {
  // The two halves of auto-save, and the second is the one that bites. A draft
  // that saves on every change is easy; a draft that stops is not, because the
  // write refetches the settings and hands the panel back a brand new object
  // holding exactly what it just wrote. Compared by reference that is a change,
  // and the panel writes for as long as it is open.

  it("writes a change once, after the typing stops", async () => {
    const save = vi.mocked(api.saveSettings);
    save.mockClear();
    // Re-rendered under the change, because that is what the window does: it
    // redraws at meter rate, and the first version of the debounce watched the
    // whole store object - which `useStore` rebuilds every render - so its own
    // cleanup cancelled the timer sixty times a second and nothing was ever
    // written. The panel said "Saving..." the whole time, which is why only the
    // window caught it. A fresh store object each pass reproduces that here.
    localStorage.setItem("vcw.settings.section", "Metadata");
    const { root, container } = mount();
    // The same settings object every pass. A fresh one would be a real change
    // and would reset the draft, which is the other effect's job and not what
    // this test is about.
    const stored = values();
    // A fresh store object every pass holding the same `run`, which is what
    // `useStore` hands the window: the wrapper is an object literal rebuilt on
    // every render, and `run` inside it is a `useCallback` that holds still.
    const run = async (what: () => Promise<unknown>) => what();
    const redraw = async () =>
      await act(async () => {
        root.render(
          <Settings
            store={{ run } as unknown as Store}
            settings={stored}
            credentials={[token(true)]}
            devices={[]}
            onSaved={() => {}}
          />,
        );
      });
    await redraw();
    const toggle = container.querySelector<HTMLInputElement>(
      ".setting-switch input",
    )!;
    expect(toggle.checked).toBe(true);

    await act(async () => {
      toggle.click();
    });
    // Not yet: the debounce is what keeps a text field from writing once a
    // keystroke, and it has to be visible from here or it is not really there.
    expect(save).not.toHaveBeenCalled();
    expect(container.querySelector(".panel-head")?.textContent).toContain(
      "Saving",
    );

    // Kept redrawing right through the debounce, which is the part that
    // matters: a quiet window would let even a timer that restarts on every
    // render eventually fire, and the window never goes quiet.
    const until = Date.now() + 1200;
    while (Date.now() < until) {
      await redraw();
      await new Promise((done) => setTimeout(done, 50));
    }
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0]![0]!.metadata.online).toBe(false);
  });

  it("does not write back the settings it was just given", async () => {
    const save = vi.mocked(api.saveSettings);
    save.mockClear();
    const store = { run: async (what: () => Promise<unknown>) => what() };
    const { root, container } = mount();
    const panel = (settings: Values) => (
      <Settings
        store={store as unknown as Store}
        settings={settings}
        credentials={[token(true)]}
        devices={[]}
        onSaved={() => {}}
      />
    );
    await act(async () => {
      root.render(panel(values()));
    });
    // The same values in a different object, which is what a refetch returns
    // and what an identity test would read as a change.
    await act(async () => {
      root.render(panel(values()));
    });
    expect(container.querySelector(".panel-head")?.textContent).not.toContain(
      "Saving",
    );
    await new Promise((done) => setTimeout(done, 900));
    expect(save).not.toHaveBeenCalled();
  });
});

describe("the language menu", () => {
  // The menu is built from what the shell found on disk, so a translation
  // appears by being submitted rather than by being added to a list here.
  // What this guards is the wiring: a select whose options come from
  // somewhere else, and a value that goes into `Settings` and not into local
  // storage - the CLI reads that file and has no local storage to read.

  it("offers what the shell found, named in its own language", async () => {
    const panel = await shown([token(true)], "Appearance");
    const select = panel.querySelector<HTMLSelectElement>("select");
    const options = [...(select?.options ?? [])];
    expect(options.map((option) => option.value)).toEqual(["en-US", "pt-BR"]);
    // Endonyms from `Intl.DisplayNames`, not a table of our own.
    expect(options[1]?.textContent?.toLowerCase()).toContain("portugu");
    // A settings file with no language in it is US English.
    expect(select?.value).toBe("en-US");
  });

  it("writes the choice into the settings file", async () => {
    const save = vi.mocked(api.saveSettings);
    save.mockClear();
    const panel = await shown([token(true)], "Appearance");
    const select = panel.querySelector<HTMLSelectElement>("select");
    if (select === null) throw new Error("no language select");
    await act(async () => {
      select.value = "pt-BR";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => {
      await new Promise((done) => setTimeout(done, 900));
    });
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0]?.[0].language).toBe("pt-BR");
  });
});
