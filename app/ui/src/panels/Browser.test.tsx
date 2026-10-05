/*
 *  Browser.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Creating a project puts it in the list, which took a packaged build to find.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Driving the packaged AppImage found this: fill in the create helper, press
// Return, and the project is made, opened and named in the title bar - and
// absent from the list of projects until the application is restarted.
// `create()` called `store.reload()`, which re-reads the *open* project, and
// not `onLibraryChanged()`, which re-reads the directory. The two names are a
// paragraph apart in `Browser.tsx` and the distinction is documented on the
// prop, which is the kind of mistake no amount of reading catches.
//
// So this is a rendered test rather than a source-reading one: it renders the
// panel with a fake store, submits the form, and asks whether the library was
// asked for again. `wiring.test.ts` explains why the keyboard map is checked by
// reading text instead - the difference is that this behaviour is one component
// deep and needs no Tauri command stubbed but the two the panel calls.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";
import type { Project } from "../bindings/vcw";
import type { Store } from "../store";
import { Browser } from "./Browser";

vi.mock("../api", () => ({
  newProject: vi.fn(async () => ({ path: "/library/new.vcw" })),
  openProject: vi.fn(async () => undefined),
  artwork: vi.fn(async () => "data:image/jpeg;base64,/9j/"),
  // The splash asks who it is as soon as it draws, so the mock has to answer
  // even in the tests that are about the table.
  about: vi.fn(async () => ({ product: "VCW", version: "0.1.0", built: "2026-10-05" })),
}));

/** A library row, with only the fields the table draws filled in. */
function row(over: Partial<Project> = {}): Project {
  return {
    path: "/library/a.vcw",
    name: "a",
    album: "Tomorrow's Harvest",
    albumArtist: "Boards Of Canada",
    catalog: "WARPLP252",
    year: 2013,
    sides: 1,
    tracks: 17,
    captures: 1,
    seconds: 3692,
    fileBytes: 1_961_099_264,
    modified: 1_759_000_000,
    hasArtwork: false,
    preview: [],
    problem: null,
    ...over,
  };
}

/** Everything the panel touches, and nothing it does not. */
function fakeStore(): { store: Store; reloaded: () => number } {
  let reloads = 0;
  const store = {
    // The open project, which the row class list reads to mark it. Needed from
    // the moment this fixture renders a row at all: the first test here passed
    // an empty list, so nothing touched it.
    project: { path: null },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
    reload: () => {
      reloads += 1;
    },
    open: async () => undefined,
  } as unknown as Store;
  return { store, reloaded: () => reloads };
}

function find(container: HTMLElement, label: string): HTMLButtonElement {
  const button = Array.from(container.querySelectorAll("button")).find((each) =>
    (each.textContent ?? "").includes(label),
  );
  if (!button) {
    throw new Error(`no button says ${label}`);
  }
  return button;
}

describe("the project browser", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // The list-or-tiles choice outlives a render, which is the point of it and
    // the reason it has to be cleared between tests here.
    localStorage.clear();
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  });

  it("re-reads the library after creating a project", async () => {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const { store, reloaded } = fakeStore();
    const onLibraryChanged = vi.fn();

    await act(async () => {
      root.render(
        <Browser
          store={store}
          projects={[]}
          selected={null}
          onSelect={() => {}}
          onLibraryChanged={onLibraryChanged}
        />,
      );
    });

    await act(async () => {
      find(container, "New project").click();
    });

    const form = container.querySelector("form");
    expect(form, "the create helper should be showing").not.toBeNull();

    // Mono and the curve are the two answers nothing downstream can work out,
    // so the one check worth having here is that a tick reaches the command.
    // The text fields are optional and go as null; these go as booleans, and
    // an unticked box is "no" rather than "not asked".
    const ticks = container.querySelectorAll<HTMLInputElement>(
      "input[type=checkbox]",
    );
    expect(ticks).toHaveLength(2);
    await act(async () => {
      ticks[0]?.click();
    });

    await act(async () => {
      form?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    expect(api.newProject).toHaveBeenCalledTimes(1);
    expect(api.newProject).toHaveBeenCalledWith(
      expect.objectContaining({ isMono: true, riaaEq: false }),
    );
    expect(api.openProject).toHaveBeenCalledWith("/library/new.vcw");
    // The open project, and the directory it lives in. A created project
    // changes both, and only one of them was being asked again.
    expect(reloaded()).toBe(1);
    expect(onLibraryChanged).toHaveBeenCalledTimes(1);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  // The split the whole cover design rests on: the listing carries a flag and
  // the image is asked for separately. A row that says it has no cover must
  // never make the call, because a hundred-row library would otherwise read a
  // hundred blobs to draw a column of placeholders.
  it("asks for a cover only for the rows that have one", async () => {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const { store } = fakeStore();

    await act(async () => {
      root.render(
        <Browser
          store={store}
          projects={[
            row({ path: "/library/with.vcw", hasArtwork: true }),
            row({ path: "/library/without.vcw", hasArtwork: false }),
          ]}
          selected={null}
          onSelect={() => {}}
          onLibraryChanged={() => {}}
        />,
      );
    });

    expect(api.artwork).toHaveBeenCalledTimes(1);
    expect(api.artwork).toHaveBeenCalledWith("/library/with.vcw");

    // The one with a cover draws an image; the one without draws the sleeve
    // mark, and both cells exist so the rows are the same height either way.
    expect(container.querySelectorAll("img.cover")).toHaveLength(1);
    expect(container.querySelectorAll(".sleeve")).toHaveLength(1);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  // The tile view's whole premise: a project that has no cover still has a
  // picture of itself, so the grid is not a wall of identical placeholders.
  it("draws the waveform in tiles until a cover arrives, and remembers the view", async () => {
    const draw = async (container: HTMLElement) => {
      const root = createRoot(container);
      const { store } = fakeStore();
      await act(async () => {
        root.render(
          <Browser
            store={store}
            projects={[
              row({ path: "/library/raw.vcw", preview: [0, 0.25, 0.5] }),
              row({ path: "/library/known.vcw", hasArtwork: true }),
            ]}
            selected={null}
            onSelect={() => {}}
            onLibraryChanged={() => {}}
          />,
        );
      });
      return root;
    };

    const first = document.createElement("div");
    document.body.append(first);
    const root = await draw(first);

    // Starts as the list it has always been, and the table is still the table.
    expect(first.querySelectorAll("table.rows")).toHaveLength(1);
    expect(first.querySelectorAll(".tiles")).toHaveLength(0);

    const toggle = first.querySelector<HTMLButtonElement>(
      '.view-toggle button[title="Tiles"]',
    );
    await act(async () => {
      toggle?.click();
    });

    expect(first.querySelectorAll("table.rows")).toHaveLength(0);
    // Two projects and the cell that creates a third.
    expect(first.querySelectorAll(".tiles .tile")).toHaveLength(3);
    expect(first.querySelectorAll(".tile.new")).toHaveLength(1);
    // The one with a cover shows it; the one without shows its own waveform,
    // and neither shows the sleeve mark the table falls back to.
    expect(first.querySelectorAll(".tile img.cover")).toHaveLength(1);
    expect(first.querySelectorAll(".tile svg.preview")).toHaveLength(1);
    expect(first.querySelectorAll(".tile .sleeve")).toHaveLength(0);

    // The shape itself, because an area mirrored about a centre line is the
    // one piece of arithmetic here and an off-by-one in the return leg would
    // still draw something plausible. Out along the top, back along the
    // bottom, closed. The peaks go in at half scale and come out full height,
    // so this one string covers the normalisation as well as the mirror.
    expect(
      first.querySelector(".tile svg.preview path")?.getAttribute("d"),
    ).toBe("M0,1L0,1L1,0.5L2,0L2,2L1,1.5L0,1Z");

    await act(async () => {
      root.unmount();
    });
    first.remove();

    // And the choice survives the unmount, which is every tab switch.
    const second = document.createElement("div");
    document.body.append(second);
    const again = await draw(second);
    expect(second.querySelectorAll(".tiles .tile")).toHaveLength(3);
    await act(async () => {
      again.unmount();
    });
    second.remove();
  });

  // The panel unmounts on every tab switch, so a cache that died with it would
  // refetch the library on every Ctrl+1.
  it("does not fetch a cover it has already read", async () => {
    const project = row({ path: "/library/cached.vcw", hasArtwork: true });
    for (const pass of [1, 2]) {
      const container = document.createElement("div");
      document.body.append(container);
      const root = createRoot(container);
      const { store } = fakeStore();
      await act(async () => {
        root.render(
          <Browser
            store={store}
            projects={[project]}
            selected={null}
            onSelect={() => {}}
            onLibraryChanged={() => {}}
          />,
        );
      });
      expect(api.artwork, `pass ${pass}`).toHaveBeenCalledTimes(1);
      await act(async () => {
        root.unmount();
      });
      container.remove();
    }
  });

  // Keyed by `modified` as well as by path, so assigning a release - which
  // rewrites the file and moves its timestamp - shows the new cover without
  // anything having to remember to clear a cache.
  it("fetches again when the file has changed underneath the row", async () => {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    const { store } = fakeStore();
    const draw = async (modified: number) => {
      await act(async () => {
        root.render(
          <Browser
            store={store}
            projects={[row({ path: "/library/moved.vcw", hasArtwork: true, modified })]}
            selected={null}
            onSelect={() => {}}
            onLibraryChanged={() => {}}
          />,
        );
      });
    };

    await draw(1_759_000_000);
    expect(api.artwork).toHaveBeenCalledTimes(1);
    await draw(1_759_000_001);
    expect(api.artwork).toHaveBeenCalledTimes(2);

    await act(async () => {
      root.unmount();
    });
    container.remove();
  });
});
