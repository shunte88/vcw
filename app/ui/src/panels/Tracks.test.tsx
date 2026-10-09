/*
 *  Tracks.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Detection analyzes the face the operator names, not the one it assumed.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Nothing in the capture path writes a side row, so until a person says which
// face a capture holds the detector has to assume one, and it assumes A. The
// panel sent `side: null` on both its Detect seams, which meant a side B only
// rip had its tracks promoted on to side A - and then identification matched
// them against the release's side A entries and wrote side A's titles over
// side B's audio. Every step succeeded and the result was wrong.
//
// So the panel states it. Three things are asserted: the picker exists with A
// first, both Detect seams carry what it holds, and a double album offers the
// faces it actually has rather than a fixed four.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";

import type { Store } from "../store";
import { Tracks } from "./Tracks";

vi.mock("../api", () => ({
  detectTracks: vi.fn(async () => undefined),
}));

function store(discs: number | null): Store {
  return {
    engine: { detection: null, playhead: 0 },
    project: {
      path: "/data2/rips/side-b.vcw",
      captures: [{ id: 1 }],
      tracks: [],
      boundaries: [],
      sides: [],
      release: discs === null ? null : { discs },
    },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
    reload: () => {},
  } as unknown as Store;
}

async function render(discs: number | null = 1): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <Tracks store={store(discs)} chosen={{ track: null, boundary: null }} onChoose={() => {}} />,
    );
  });
  return container;
}

/** The panel's side picker: the only select in its head. */
function picker(container: HTMLElement): HTMLSelectElement {
  const select = container.querySelector<HTMLSelectElement>(".panel-head select");
  expect(select, "the panel offers a side").toBeTruthy();
  return select as HTMLSelectElement;
}

describe("the track editor", () => {
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.mocked(api.detectTracks).mockClear();
  });

  it("opens on side A, because one face is the common record", async () => {
    expect(picker(await render()).value).toBe("A");
  });

  it("detects the face the operator named", async () => {
    const container = await render();
    const select = picker(container);
    await act(async () => {
      select.value = "B";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    const button = [...container.querySelectorAll("button")].find((b) =>
      /^detect\b/i.test((b.textContent ?? "").trim()),
    );
    expect(button, "the panel has a detect button").toBeTruthy();
    await act(async () => {
      button?.click();
    });
    expect(vi.mocked(api.detectTracks)).toHaveBeenCalledWith({ side: "B", promote: true });
  });

  it("offers two faces a disc, and no more than the release has", async () => {
    const single = [...picker(await render(1)).options].map((o) => o.value);
    expect(single).toEqual(["A", "B"]);
    const double = [...picker(await render(2)).options].map((o) => o.value);
    expect(double).toEqual(["A", "B", "C", "D"]);
  });
});
