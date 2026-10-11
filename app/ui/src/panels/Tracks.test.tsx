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

import type { Boundary, Side } from "../bindings/vcw";
import type { Store } from "../store";
import { Tracks } from "./Tracks";
import type { Region } from "./Waveform";

vi.mock("../api", () => ({
  addTrack: vi.fn(async () => 7),
  deleteMarker: vi.fn(async () => undefined),
  detectTracks: vi.fn(async () => undefined),
}));

function boundary(id: number): Boundary {
  return {
    id,
    sideId: 1,
    side: "A",
    atFrame: id * 48_000,
    seconds: id,
    edge: "start",
    confidence: 0.8,
    provenance: "silence",
    sources: ["silence"],
    agreement: 1,
    locked: false,
    promoted: true,
    evidence: [],
  } as unknown as Boundary;
}

function store(
  discs: number | null,
  boundaries: readonly Boundary[] = [],
): Store {
  return {
    engine: { detection: null, playhead: 0 },
    project: {
      path: "/data2/rips/side-b.vcw",
      captures: [{ id: 1 }],
      tracks: [],
      boundaries,
      sides: [],
      release: discs === null ? null : { discs },
    },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
    reload: () => {},
  } as unknown as Store;
}

async function render(
  discs: number | null = 1,
  {
    side = null,
    selection = null,
    onSelect = () => {},
    boundaries = [],
  }: {
    side?: Side | null;
    selection?: Region | null;
    onSelect?: (selection: Region | null) => void;
    boundaries?: readonly Boundary[];
  } = {},
): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <Tracks
        store={store(discs, boundaries)}
        chosen={{ track: null, boundary: null }}
        onChoose={() => {}}
        side={side}
        selection={selection}
        onSelect={onSelect}
      />,
    );
  });
  return container;
}

/** The panel button whose label starts with `word`. */
function button(container: HTMLElement, word: RegExp): HTMLButtonElement {
  const found = [...container.querySelectorAll("button")].find((at) =>
    word.test((at.textContent ?? "").trim()),
  );
  expect(found, `the panel has a ${word.source} button`).toBeTruthy();
  return found as HTMLButtonElement;
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

describe("correcting what detection got wrong", () => {
  // Both of these came off a first user's forum report, and both were verbs
  // the contract already had and the window did not offer. "I can drag across
  // a track to select it ... but how do I then say that this highlighted area
  // should be A1 and all of the later ones should be renamed", and "I can't
  // see how to remove the marker later if I am not 100% happy with where I
  // put it".

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.mocked(api.addTrack).mockClear();
    vi.mocked(api.deleteMarker).mockClear();
  });

  const side = { id: 4, letter: "A" } as unknown as Side;

  it("makes a track of the stretch chosen on the waveform", async () => {
    let cleared: Region | null | undefined;
    const container = await render(1, {
      side,
      selection: { from: 12.5, to: 230.25 },
      onSelect: (next) => {
        cleared = next;
      },
    });
    await act(async () => {
      button(container, /^add track/i).click();
    });
    expect(vi.mocked(api.addTrack)).toHaveBeenCalledWith({
      sideId: 4,
      start: 12.5,
      end: 230.25,
    });
    expect(cleared, "the band goes away once it is a track").toBeNull();
  });

  it("sends the span the right way round, whichever way it was dragged", async () => {
    const container = await render(1, {
      side,
      selection: { from: 230.25, to: 12.5 },
    });
    await act(async () => {
      button(container, /^add track/i).click();
    });
    expect(vi.mocked(api.addTrack)).toHaveBeenCalledWith({
      sideId: 4,
      start: 12.5,
      end: 230.25,
    });
  });

  it("says what to do rather than going dead with nothing selected", async () => {
    const container = await render(1, { side });
    const add = button(container, /^add track/i);
    expect(add.disabled).toBe(true);
    expect(add.title).toMatch(/drag across the waveform/i);
  });

  it("offers a boundary a way out that is not a keystroke", async () => {
    const container = await render(1, { boundaries: [boundary(3)] });
    const rows = container.querySelectorAll("tbody tr");
    expect(rows).toHaveLength(1);
    const remove = [...rows[0]!.querySelectorAll("button")].find((at) =>
      /^delete$/i.test((at.textContent ?? "").trim()),
    );
    expect(remove, "the boundary row offers Delete").toBeTruthy();
    await act(async () => {
      remove?.click();
    });
    expect(vi.mocked(api.deleteMarker)).toHaveBeenCalledWith({
      boundaryId: 3,
      force: true,
    });
  });
});
