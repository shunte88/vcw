/*
 *  drop.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The needle-drop trigger, against the levels a real side actually carries.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The numbers are the ones measured off the Boards of Canada rip, which is
// where the threshold came from - see the note at the top of `drop.ts`. A
// synthetic fixture would prove the comparison compiles and nothing else; the
// question these ask is whether 50 dB of margin and a 50 ms sustain really do
// separate a stylus landing from the clicks on the same lead-in.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "./api";
import { DROP_DB, SUSTAIN_MS, dropped, peakOf, rose, useNeedleDrop } from "./drop";
import type { Meter } from "./bindings/vcw";
import type { Store } from "./store";

vi.mock("./api", () => ({ transport: vi.fn(async () => undefined) }));

/** The electrical floor before the stylus lands, both sides of it. */
const FLOOR = [-55.2, -57.6, -62.3, -64.6] as const;

/** The landing transient, in the 5.3 ms window it occupied. */
const LANDING = [-4.5, -5.3, -6.0, -7.9] as const;

/** Where the groove noise settles once the stylus is tracking. */
const GROOVE = [-18, -24, -28] as const;

function snapshot(...peaks: number[]): Meter {
  return {
    channels: peaks.map((peakDb) => ({ peakDb }) as Meter["channels"][number]),
    frames: 0,
    clipped: false,
  };
}

/** Walks a run of levels through the detector at the meter's own 20 ms. */
function watch(peaks: readonly number[]): number {
  let since: number | null = null;
  for (const [tick, peak] of peaks.entries()) {
    const now = tick * 20;
    since = rose(peak, since, now);
    if (dropped(since, now)) {
      return now;
    }
  }
  return -1;
}

describe("the needle-drop trigger", () => {
  it("reads the loudest channel of a snapshot", () => {
    expect(peakOf(snapshot(-62.3, -64.6))).toBe(-62.3);
    expect(peakOf(snapshot())).toBe(Number.NEGATIVE_INFINITY);
  });

  it("leaves every measured floor well below the threshold", () => {
    for (const quiet of FLOOR) {
      expect(quiet).toBeLessThan(DROP_DB);
      expect(rose(quiet, null, 0)).toBeNull();
    }
  });

  it("fires on a landing followed by groove noise", () => {
    // One landing window, then the side playing. The trigger releases the
    // transport a sustain after the stylus touched down, rounded up to the
    // next meter tick - which is as fine as a 20 ms stream can resolve, and
    // the only reason this is a range rather than an equality.
    const side = [...FLOOR, LANDING[0], ...GROOVE, ...GROOVE, ...GROOVE];
    const late = watch(side) - FLOOR.length * 20;
    expect(late).toBeGreaterThanOrEqual(SUSTAIN_MS);
    expect(late).toBeLessThan(SUSTAIN_MS + 20);
  });

  it("ignores the clicks the same lead-in is full of", () => {
    // A click is about a millisecond: one meter tick shows it and the next is
    // back in the floor. Four of them, spread out, and nothing starts.
    const clicks = [-12, -20, -9, -17];
    const lead = FLOOR.flatMap((quiet) => [quiet, clicks.pop() ?? quiet, quiet, quiet]);
    expect(watch(lead)).toBe(-1);
  });

  it("needs the level to hold, not merely to reach", () => {
    // The same landing level, on its own. Loud enough, and still not a drop.
    expect(watch([...FLOOR, LANDING[3], ...FLOOR])).toBe(-1);
  });
});

// The second half of the trigger is the gate around those functions: it may
// only release a transport that is actually paused and waiting to be
// released. A detector that fired from `recording` would be harmless noise; a
// detector that fired from `armed` would start a capture nobody had cued.

function Harness({ store }: { store: Store }): React.JSX.Element {
  const drop = useNeedleDrop(store, async () => {});
  return (
    <button type="button" onClick={drop.toggle}>
      {drop.waiting ? "listening" : "idle"}
    </button>
  );
}

function store(phase: string, peak: number): Store {
  return {
    project: { path: "/library/a.vcw" },
    engine: { phase, meter: snapshot(peak) },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
  } as unknown as Store;
}

describe("the trigger's gate", () => {
  beforeEach(() => {
    vi.mocked(api.transport).mockClear();
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  /** Arms the trigger, then holds the level there for twice the sustain. */
  async function hold(phase: string, peak: number): Promise<void> {
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(<Harness store={store(phase, peak)} />);
    });
    await act(async () => {
      container.querySelector("button")?.click();
    });
    for (let tick = 0; tick < SUSTAIN_MS / 5; tick += 1) {
      await act(async () => {
        vi.advanceTimersByTime(20);
        root.render(<Harness store={store(phase, peak)} />);
      });
    }
    await act(async () => {
      root.unmount();
    });
    container.remove();
  }

  it("releases a paused transport once the level holds", async () => {
    await hold("paused", -20);
    expect(api.transport).toHaveBeenCalledWith("resume");
  });

  it("stays out of it unless the transport is paused", async () => {
    for (const phase of ["armed", "recording"]) {
      await hold(phase, -20);
    }
    expect(api.transport).not.toHaveBeenCalledWith("resume");
  });

  it("leaves a paused transport alone while the groove is quiet", async () => {
    await hold("paused", -60);
    expect(api.transport).not.toHaveBeenCalledWith("resume");
  });
});
