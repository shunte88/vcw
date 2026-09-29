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
import type { Store } from "../store";
import { Browser } from "./Browser";

vi.mock("../api", () => ({
  newProject: vi.fn(async () => ({ path: "/library/new.vcw" })),
  openProject: vi.fn(async () => undefined),
}));

/** Everything the panel touches, and nothing it does not. */
function fakeStore(): { store: Store; reloaded: () => number } {
  let reloads = 0;
  const store = {
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

    await act(async () => {
      form?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });

    expect(api.newProject).toHaveBeenCalledTimes(1);
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
});
