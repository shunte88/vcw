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
import { Settings, marked } from "./Settings";

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
  // Three rules, and the second is the one a piCorePlayer user found. A write
  // refetches, and the refetch hands the panel a brand new object holding
  // exactly what was just written - so a panel that adopts whatever arrives
  // replaces what is being typed with the last thing saved. Through the Tauri
  // shell that trip is a millisecond and nothing shows; over `vcw serve` to a
  // Pi in another room it eats words.
  //
  // So: one click writes at once, a typed field waits for the blur, and an
  // echo never overwrites an edit in progress.

  /**
   * Type into a controlled input the way a person does.
   *
   * Not `node.value = word`: React keeps its own record of what it last
   * rendered into the box, and a direct assignment updates that record on
   * the way past - so the change event arrives and React decides nothing
   * changed. Going through the prototype's setter leaves the record alone,
   * which is what a keystroke does.
   */
  function type(node: HTMLInputElement, word: string): void {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(node, word);
    node.dispatchEvent(new Event("input", { bubbles: true }));
  }

  /** A panel that can be redrawn with different settings. */
  function panel(stored: Values) {
    localStorage.setItem("vcw.settings.section", "Metadata");
    const { root, container } = mount();
    // A fresh store object every pass holding the same `run`, which is what
    // `useStore` hands the window: the wrapper is an object literal rebuilt on
    // every render, and `run` inside it is a `useCallback` that holds still.
    const run = async (what: () => Promise<unknown>) => what();
    let showing = stored;
    const redraw = async (settings: Values = showing) => {
      showing = settings;
      await act(async () => {
        root.render(
          <Settings
            store={{ run } as unknown as Store}
            settings={settings}
            credentials={[token(true)]}
            devices={[]}
            onSaved={() => {}}
          />,
        );
      });
    };
    return { root, container, redraw };
  }

  it("writes a switch as soon as it is clicked", async () => {
    const save = vi.mocked(api.saveSettings);
    save.mockClear();
    const { container, redraw } = panel(values());
    await redraw();
    const toggle = container.querySelector<HTMLInputElement>(
      ".setting-switch input",
    )!;
    expect(toggle.checked).toBe(true);

    await act(async () => {
      toggle.click();
    });
    // One discrete choice making one save is not chatter, and waiting on it
    // would mean a switch a person flicked and walked away from was never
    // written.
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0]![0]!.metadata.online).toBe(false);

    // Kept redrawing, which is the part that bites: the window redraws at
    // meter rate, and an effect that watched the whole store would write on
    // every pass.
    for (let pass = 0; pass < 10; pass += 1) {
      await redraw();
    }
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("holds a typed field until the person leaves it", async () => {
    const save = vi.mocked(api.saveSettings);
    save.mockClear();
    const { container, redraw } = panel(values());
    await redraw();
    const contact = [...container.querySelectorAll("label")]
      .find((label) => label.textContent?.startsWith("Contact"))!
      .querySelector("input")!;

    // One keystroke at a time, redrawing between them the way the window
    // does. Nothing goes out: this is the whole of the complaint.
    for (const word of ["a", "ab", "abc"]) {
      await act(async () => {
        type(contact, word);
      });
      await redraw();
    }
    expect(save).not.toHaveBeenCalled();
    expect(container.querySelector(".panel-head")?.textContent).toContain(
      "Unsaved",
    );

    await act(async () => {
      contact.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0]![0]!.metadata.contact).toBe("abc");
  });

  it("does not replace what is being typed with the echo of a save", async () => {
    const save = vi.mocked(api.saveSettings);
    save.mockClear();
    const { container, redraw } = panel(values());
    await redraw();
    const contact = [...container.querySelectorAll("label")]
      .find((label) => label.textContent?.startsWith("Contact"))!
      .querySelector("input")!;
    await act(async () => {
      type(contact, "half a wo");
    });

    // What a refetch after somebody else's write looks like: a new object,
    // the same words, arriving mid-sentence.
    await redraw(values());
    expect(contact.value).toBe("half a wo");
  });

  it("does not write back the settings it was just given", async () => {
    const save = vi.mocked(api.saveSettings);
    save.mockClear();
    const { container, redraw } = panel(values());
    await redraw();
    // The same values in a different object, which is what a refetch returns
    // and what an identity test would read as a change.
    await redraw(values());
    expect(container.querySelector(".panel-head")?.textContent).not.toContain(
      "Unsaved",
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

// A fresh install opens on an empty browser, because no library is set, and
// nothing on that panel says so - the first outside user to get the window
// open asked for a wizard on exactly this. The wizard is one redirect, so the
// thing worth testing is that it fires once and only on a first run.
describe("where a first run lands", () => {
  /** Renders with the library set or not, and nothing remembered. */
  async function fresh(library: string | null, remembered?: string): Promise<HTMLElement> {
    localStorage.removeItem("vcw.settings.section");
    if (remembered !== undefined) {
      localStorage.setItem("vcw.settings.section", remembered);
    }
    const held = values();
    (held.recording as { library: string | null }).library = library;
    const store = { run: async (what: () => Promise<unknown>) => what() };
    const { root, container } = mount();
    await act(async () => {
      root.render(
        <Settings
          store={store as unknown as Store}
          settings={held}
          credentials={[]}
          devices={[]}
          onSaved={() => {}}
        />,
      );
    });
    return container;
  }

  /** The section tab the panel is showing. */
  function current(container: HTMLElement): string {
    return container.querySelector('[aria-current="true"]')?.textContent ?? "";
  }

  it("opens on Library when there is none, with the field focused", async () => {
    const container = await fresh(null);
    expect(current(container)).toBe("Library");
    const input = container.querySelector<HTMLInputElement>(".prefs-pane input");
    expect(input?.placeholder).toContain("no library");
    expect(document.activeElement).toBe(input);
  });

  it("opens on Appearance once a library is set", async () => {
    expect(current(await fresh("/data2/rips"))).toBe("Appearance");
  });

  it("leaves a remembered section alone even with no library", async () => {
    // Clearing the library is a thing a person can do on purpose. Having done
    // it, they do not want every visit to Settings to start over here.
    expect(current(await fresh(null, "Export"))).toBe("Export");
  });
});

describe("a marked-up hint", () => {
  // The one thing in this panel that is parsing rather than wiring. A
  // paragraph is a single catalog entry so a translator can reorder it, and
  // the file names and option names inside it are marked rather than cut out
  // into keys of their own - so the marks have to survive to the screen.

  it("renders a literal as code and an emphasis as emphasis", () => {
    expect(marked("copy `en-US.toml` and choose *Refuse*")).toEqual([
      ["text", "copy "],
      ["code", "en-US.toml"],
      ["text", " and choose "],
      ["em", "Refuse"],
    ]);
  });

  it("leaves an unmarked paragraph as one run", () => {
    expect(marked("Nothing to mark here.")).toEqual([
      ["text", "Nothing to mark here."],
    ]);
  });

  it("puts the marks in the panel, not in the words", async () => {
    const pane = (await shown([token(true)], "Appearance")).querySelector(
      ".prefs-pane",
    );
    const hint = [...(pane?.querySelectorAll("p.hint") ?? [])].find((line) =>
      line.textContent?.includes("ships in US English"),
    );
    expect(hint?.textContent).toContain("copy i18n/en-US.toml out of");
    expect(
      [...(hint?.querySelectorAll("code") ?? [])].map((at) => at.textContent),
    ).toEqual(["i18n/en-US.toml", "text", "i18n/<language tag>.toml"]);
  });
});
