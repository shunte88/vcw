/*
 *  Waveform.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A drag that lands on a boundary moves it, and a double-click vetoes a defect.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The first outside report could place a marker and then not adjust it: the
// only gesture over the picture was a selection, so taking hold of a line and
// pulling it was something the window did not do. This is that gesture, and
// the three things it has to get right - which line it took hold of, where it
// put it, and that it did not draw a band on the way past.
//
// jsdom lays nothing out, so the two measurements the component takes from the
// DOM are stubbed: the box's size, which decides whether it asks for peaks at
// all, and the canvas's own rectangle, which is how a client x becomes a
// second. Everything else is the real component.

import { act } from "react";
import { type Root, createRoot } from "react-dom/client";
import {
  type Mock,
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

import * as api from "../api";
import type { Boundary, Capture, Defect, Track } from "../bindings/vcw";
import { Waveform, type Region } from "./Waveform";

/** Ten seconds at 48 kHz, drawn 800 CSS pixels wide. */
const WIDE = 800;
const HIGH = 400;
const RATE = 48_000;

vi.mock("../api", () => ({
  waveform: vi.fn(async () => ({
    captureId: 1,
    channel: 0,
    startFrame: 0,
    endFrame: 10 * RATE,
    startSeconds: 0,
    endSeconds: 10,
    columns: [],
  })),
  asFailure: (error: unknown) => ({
    code: "unknown",
    message: String(error),
    field: null,
  }),
}));

function boundary(id: number, seconds: number): Boundary {
  return {
    id,
    sideId: 1,
    side: "A",
    atFrame: seconds * RATE,
    seconds,
    edge: "start",
    confidence: 0.8,
    provenance: "silence",
    sources: ["silence"],
    agreement: 1,
    locked: true,
    promoted: true,
    evidence: [],
  } as unknown as Boundary;
}

function track(id: number, start: number, end: number): Track {
  return {
    id,
    sideId: 1,
    side: "A",
    number: id,
    position: `A${id}`,
    start,
    end,
    seconds: end - start,
    startFrame: start * RATE,
    endFrame: end * RATE,
    startBoundary: id * 10,
    endBoundary: id * 10 + 1,
    title: "",
    artist: null,
    composer: null,
    comments: null,
    musicbrainzId: null,
    confirmed: false,
  } as unknown as Track;
}

/** A defect over a stretch of seconds, repaired unless it has been vetoed. */
function defect(id: number, from: number, to: number, audible: boolean): Defect {
  return {
    id,
    captureId: 1,
    startFrame: from * RATE,
    endFrame: to * RATE,
    startSeconds: from,
    endSeconds: to,
    frames: (to - from) * RATE,
    channels: 3,
    kind: "click",
    confidence: 1,
    provenance: "declick",
    repair: audible ? "interpolate" : "none",
    audible,
    locked: false,
    evidence: [],
  } as unknown as Defect;
}

const capture = {
  id: 1,
  rate: RATE,
  frames: 10 * RATE,
  seconds: 10,
} as unknown as Capture;

/** Where on the picture a second falls, in client pixels. */
function at(seconds: number): number {
  return (seconds / 10) * WIDE;
}

function press(node: Element, type: string, clientX: number) {
  node.dispatchEvent(
    new MouseEvent(type, { bubbles: true, clientX, clientY: HIGH / 2 }),
  );
}

/**
 * The middle of the label lane, in client pixels down the box.
 *
 * The panel puts the lane between the picture and the bottom ruler, one
 * twenty-pixel row per side, so with one side on screen it is the twenty
 * pixels above the ruler. Written out rather than imported because it is the
 * geometry under test: a lane that moved would have to break this.
 */
const LANE_Y = HIGH - 18 - 10;

function press2(node: Element, clientX: number, clientY: number) {
  node.dispatchEvent(
    new MouseEvent("dblclick", { bubbles: true, clientX, clientY }),
  );
}

describe("taking hold of a boundary", () => {
  let moved: Mock<(boundaryId: number, seconds: number) => void>;
  let selected: Mock<(selection: Region | null) => void>;
  let cued: Mock<(seconds: number) => void>;
  let vetoed: Mock<(defect: Defect) => void>;
  let renamed: Mock<(trackId: number, title: string) => void>;
  let canvas: HTMLCanvasElement;
  let root: Root;
  let host: HTMLDivElement;

  beforeEach(async () => {
    vi.clearAllMocks();
    (
      globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    // jsdom has neither a resize observer nor a layout, so the component's
    // one measurement is answered rather than observed.
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    for (const name of ["clientWidth", "clientHeight"] as const) {
      Object.defineProperty(HTMLDivElement.prototype, name, {
        configurable: true,
        get: () => (name === "clientWidth" ? WIDE : HIGH),
      });
    }
    HTMLCanvasElement.prototype.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: WIDE, height: HIGH }) as DOMRect;
    HTMLCanvasElement.prototype.setPointerCapture = () => {};

    moved = vi.fn();
    selected = vi.fn();
    cued = vi.fn();
    vetoed = vi.fn();
    renamed = vi.fn();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => {
      root.render(
        <Waveform
          capture={capture}
          view={{ captureId: 1, channel: 0, startFrame: 0, endFrame: null }}
          onView={() => {}}
          boundaries={[boundary(10, 2), boundary(11, 6)]}
          defects={[defect(1, 8.5, 8.502, true), defect(2, 4, 4.002, false)]}
          onVeto={vetoed}
          tracks={[track(1, 2, 6)]}
          onRename={renamed}
          playhead={0}
          playing={false}
          onSeek={() => {}}
          onCue={cued}
          onMoveBoundary={moved}
          selection={null}
          onSelect={selected}
          generation={0}
        />,
      );
    });
    canvas = host.querySelector("canvas")!;
    expect(api.waveform).toHaveBeenCalled();
  });

  // React 19 runs its render work on a macrotask, so an update still queued
  // when the file ends runs after Vitest has torn the jsdom environment down
  // and throws `window is not defined` - which fails the whole file even
  // though every test in it passed. Unmounting flushes that work while there
  // is still a document to flush it into.
  afterEach(async () => {
    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("moves the line the press landed on, and draws no band", async () => {
    await act(async () => {
      press(canvas, "pointerdown", at(2));
      press(canvas, "pointermove", at(3));
      press(canvas, "pointerup", at(3));
    });
    expect(moved).toHaveBeenCalledWith(10, 3);
    expect(selected).not.toHaveBeenCalled();
  });

  it("leaves the other boundaries where they are", async () => {
    await act(async () => {
      press(canvas, "pointerdown", at(6));
      press(canvas, "pointermove", at(5.5));
      press(canvas, "pointerup", at(5.5));
    });
    expect(moved).toHaveBeenCalledWith(11, 5.5);
  });

  it("selects across a stretch with no boundary in it", async () => {
    await act(async () => {
      press(canvas, "pointerdown", at(8));
      press(canvas, "pointermove", at(9));
      press(canvas, "pointerup", at(9));
    });
    expect(moved).not.toHaveBeenCalled();
    expect(selected).toHaveBeenCalledWith({ from: 8, to: 9 });
  });

  it("does not move a boundary the press never carried anywhere", async () => {
    await act(async () => {
      press(canvas, "pointerdown", at(2));
      press(canvas, "pointerup", at(2));
      press(canvas, "click", at(2));
    });
    expect(moved).not.toHaveBeenCalled();
    expect(cued).toHaveBeenCalledWith(2);
  });

  // Wrapped like every other test here: a bare dispatch sets state that React
  // has not been told to flush, and the work outlives the test.
  it("says which track the line under the pointer belongs to", async () => {
    await act(async () => {
      press(canvas, "pointermove", at(2));
    });
    expect(canvas.title).toBe("A1 start, at 00:00:02.000 - drag to move it.");
    await act(async () => {
      press(canvas, "pointermove", at(6));
    });
    expect(canvas.title).toBe("A1 end, at 00:00:06.000 - drag to move it.");
    await act(async () => {
      press(canvas, "pointermove", at(9));
    });
    expect(canvas.title).toBe("");
  });

  // WP-30 §6: the chain is only non-destructive if the person can see what it
  // will do and refuse one of them. Both halves are here, because a veto that
  // cannot be read off the picture is a veto nobody will find: the tooltip
  // names the gesture, and the gesture reaches the project.
  it("says what a defect is and takes the veto back off it", async () => {
    await act(async () => {
      press(canvas, "pointermove", at(8.5));
    });
    expect(canvas.title).toBe(
      "Damage at 00:00:08.500, 2 ms - repaired on the way out. Double-click to leave it as captured.",
    );
    await act(async () => {
      press(canvas, "pointermove", at(4));
    });
    expect(canvas.title).toBe(
      "Damage at 00:00:04.000, 2 ms - left as captured. Double-click to repair it.",
    );

    await act(async () => {
      press(canvas, "dblclick", at(8.5));
    });
    expect(vetoed).toHaveBeenCalledWith(
      expect.objectContaining({ id: 1, audible: true }),
    );
  });

  // The hit test is by the gap to the span, not by the distance to its
  // midpoint, and nothing else in this file would notice the difference: at
  // this zoom every defect is a thousandth of a pixel wide. A double-click on
  // bare audio must not veto the nearest click half a second away.
  it("vetoes nothing when the pointer is not on a defect", async () => {
    await act(async () => {
      press(canvas, "dblclick", at(7));
    });
    expect(vetoed).not.toHaveBeenCalled();
  });

  // One gesture, two meanings, told apart by which strip of the canvas was
  // hit. The lane is the only part of the picture that is not audio, so a
  // double-click in it renames and a double-click a few pixels higher vetoes
  // - and the second of those is the one that was already here, which is why
  // the field must not open over the audio.
  it("opens a field over the label the gesture landed on", async () => {
    await act(async () => {
      press2(canvas, at(4), LANE_Y);
    });
    const field = host.querySelector<HTMLInputElement>(".waveform-rename");
    expect(field, "the lane offers a field").toBeTruthy();
    expect(vetoed, "a rename is not a veto").not.toHaveBeenCalled();
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(field, "Kinloch Rannoch");
      field?.dispatchEvent(new KeyboardEvent("keydown", {
        bubbles: true,
        key: "Enter",
      }));
    });
    expect(renamed).toHaveBeenCalledWith(1, "Kinloch Rannoch");
    expect(
      host.querySelector(".waveform-rename"),
      "the field closes behind the write",
    ).toBeNull();
  });

  it("opens no field over the audio", async () => {
    await act(async () => {
      press2(canvas, at(4), HIGH / 2);
    });
    expect(host.querySelector(".waveform-rename")).toBeNull();
  });

  it("takes the name back on Escape", async () => {
    await act(async () => {
      press2(canvas, at(4), LANE_Y);
    });
    const field = host.querySelector<HTMLInputElement>(".waveform-rename");
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(field, "Wrong");
      field?.dispatchEvent(new KeyboardEvent("keydown", {
        bubbles: true,
        key: "Escape",
      }));
    });
    expect(renamed, "Escape is not a slow Enter").not.toHaveBeenCalled();
    expect(host.querySelector(".waveform-rename")).toBeNull();
  });
});
