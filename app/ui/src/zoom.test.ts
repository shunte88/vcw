/*
 *  zoom.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  That a visible window cannot leave the capture, or close past a sample.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import { describe, expect, it } from "vitest";

import {
  CLOSEST,
  type Span,
  centre,
  clamp,
  fit,
  follow,
  pan,
  resolve,
  toRange,
  zoom,
} from "./zoom";

/** Side A of Spirit of Eden: 26:05.77 at 192 kHz. */
const SIDE = 300_628_000;

/** The window's width, with the nullable end already resolved. */
function wide(span: Span, frames = SIDE): number {
  const { start, end } = resolve(span, frames);
  return end - start;
}

describe("the visible window", () => {
  it("starts as the whole capture", () => {
    expect(fit()).toEqual({ startFrame: 0, endFrame: null });
    expect(wide(fit())).toBe(SIDE);
  });

  // The reason `endFrame` is nullable at all: a range that reaches the end has
  // to keep meaning "the end" while the capture is still growing.
  it("spells a window that reaches the end as null, wherever it starts", () => {
    expect(zoom(fit(), SIDE, 0.5, 1).endFrame).toBeNull();
    expect(pan(fit(), SIDE, 1_000_000).endFrame).toBeNull();
  });

  it("halves on the way in and doubles on the way out", () => {
    const half = zoom(fit(), SIDE, 0.5, 0.5);
    expect(wide(half)).toBe(SIDE / 2);
    expect(wide(zoom(half, SIDE, 0.5, 0.5))).toBe(SIDE / 4);
    expect(wide(zoom(half, SIDE, 2, 0.5))).toBe(SIDE);
  });

  // The whole point of an anchor: wheeling over a peak keeps that peak under
  // the pointer. A quarter of the way across a whole side is frame 75157000,
  // and it is still a quarter of the way across after the zoom.
  it("holds the frame under the pointer still", () => {
    const at = SIDE / 4;
    const after = zoom(fit(), SIDE, 0.5, 0.25);
    const { start, end } = resolve(after, SIDE);
    expect((at - start) / (end - start)).toBeCloseTo(0.25, 6);
  });

  it("holds an anchor at either edge", () => {
    const left = zoom(fit(), SIDE, 0.5, 0);
    expect(left.startFrame).toBe(0);
    const right = zoom(fit(), SIDE, 0.5, 1);
    expect(resolve(right, SIDE).end).toBe(SIDE);
  });

  it("cannot be zoomed out past the capture", () => {
    let span = fit();
    for (let press = 0; press < 40; press += 1) {
      span = zoom(span, SIDE, 2, 0.5);
    }
    expect(span).toEqual({ startFrame: 0, endFrame: null });
  });

  // A wheel does not know it has reached the end, so the limit is a clamp and
  // not a refusal - and a held key must come to rest rather than divide the
  // window forever.
  it("cannot be zoomed in past a frame to the column", () => {
    let span = fit();
    for (let press = 0; press < 60; press += 1) {
      span = zoom(span, SIDE, 0.5, 0.5);
    }
    expect(wide(span)).toBe(CLOSEST);
    expect(wide(zoom(span, SIDE, 0.5, 0.5))).toBe(CLOSEST);
  });

  it("cannot be panned off either end", () => {
    const window = zoom(fit(), SIDE, 0.25, 0.5);
    const left = pan(window, SIDE, -SIDE);
    expect(left.startFrame).toBe(0);
    expect(wide(left)).toBe(wide(window));
    const right = pan(window, SIDE, SIDE);
    expect(resolve(right, SIDE).end).toBe(SIDE);
    expect(wide(right)).toBe(wide(window));
  });

  it("keeps its width when panned", () => {
    const window = zoom(fit(), SIDE, 0.1, 0.5);
    for (const by of [-1e9, -5000, 0, 5000, 1e9]) {
      expect(wide(pan(window, SIDE, by))).toBe(wide(window));
    }
  });

  it("centres on a frame without changing width", () => {
    const window = zoom(fit(), SIDE, 0.1, 0.5);
    const at = SIDE / 3;
    const after = centre(window, SIDE, at);
    const { start, end } = resolve(after, SIDE);
    expect(end - start).toBe(wide(window));
    expect((start + end) / 2).toBeCloseTo(at, -1);
  });

  // A capture with nothing in it, which is what a project has before the first
  // record goes on. Every function has to answer, because the panel is drawn
  // before anything is recorded.
  it("answers for a capture with no frames", () => {
    for (const span of [
      clamp(fit(), 0),
      zoom(fit(), 0, 0.5, 0.5),
      pan(fit(), 0, 1000),
      centre(fit(), 0, 1000),
      toRange(0, 100, 0),
    ]) {
      expect(span).toEqual({ startFrame: 0, endFrame: null });
    }
  });

  // A capture shorter than the floor: 1000 frames is a hundredth of a second,
  // and the window is the whole of it rather than four times its length.
  it("shows a whole capture shorter than the floor", () => {
    expect(wide(zoom(fit(), 1000, 0.5, 0.5), 1000)).toBe(1000);
    // The floor is `min(CLOSEST, frames)`, so in a capture this short every
    // window is the whole of it. Nothing to zoom into, and nothing broken.
    expect(wide(clamp({ startFrame: 0, endFrame: 999 }, 1000), 1000)).toBe(
      1000,
    );
    expect(wide(pan({ startFrame: 0, endFrame: 999 }, 1000, 500), 1000)).toBe(
      1000,
    );
  });

  it("refuses a nonsense factor rather than producing a nonsense window", () => {
    const window = zoom(fit(), SIDE, 0.25, 0.5);
    for (const factor of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(wide(zoom(window, SIDE, factor, 0.5))).toBe(wide(window));
    }
  });
});

describe("clamping to a capture", () => {
  // The case that made `clamp` exist: the panel is zoomed into the end of a
  // long capture and the project is then re-read with a shorter one. Left
  // alone that is a window entirely past the end and an empty picture.
  it("pulls a window back inside a capture that got shorter", () => {
    const window = { startFrame: 200_000_000, endFrame: 210_000_000 };
    const after = clamp(window, 50_000_000);
    const { start, end } = resolve(after, 50_000_000);
    expect(end).toBe(50_000_000);
    expect(end - start).toBe(10_000_000);
    expect(start).toBeGreaterThanOrEqual(0);
  });

  it("leaves a window that is already inside alone", () => {
    const window = { startFrame: 1000, endFrame: 1_000_000 };
    expect(clamp(window, SIDE)).toEqual(window);
  });

  // Not "leaves alone": a window narrower than the floor is not a window a
  // person could have zoomed to, so clamping opens it to the floor.
  it("opens a window narrower than the floor", () => {
    expect(wide(clamp({ startFrame: 1000, endFrame: 5000 }, SIDE))).toBe(
      CLOSEST,
    );
  });
});

describe("zooming to a range", () => {
  it("shows the range with a margin at each end", () => {
    const after = toRange(10_000_000, 20_000_000, SIDE);
    const { start, end } = resolve(after, SIDE);
    expect(start).toBeLessThan(10_000_000);
    expect(end).toBeGreaterThan(20_000_000);
    // A tenth either side of a ten-million-frame range.
    expect(end - start).toBe(12_000_000);
  });

  it("takes the frames in either order", () => {
    expect(toRange(20_000_000, 10_000_000, SIDE)).toEqual(
      toRange(10_000_000, 20_000_000, SIDE),
    );
  });

  // A boundary is a single frame, and "zoom to it" has to mean something.
  it("gives a zero-width range a window it can be seen in", () => {
    expect(wide(toRange(5_000_000, 5_000_000, SIDE))).toBe(CLOSEST);
  });

  it("does not run off the end for a range at the very end", () => {
    const after = toRange(SIDE - 1000, SIDE, SIDE);
    expect(resolve(after, SIDE).end).toBe(SIDE);
    expect(after.endFrame).toBeNull();
  });
});

describe("following a playhead", () => {
  it("does nothing while the playhead is on screen", () => {
    const window = { startFrame: 1_000_000, endFrame: 2_000_000 };
    expect(follow(window, SIDE, 1_500_000)).toBe(window);
    expect(follow(window, SIDE, 1_000_000)).toBe(window);
    expect(follow(window, SIDE, 2_000_000)).toBe(window);
  });

  it("does nothing when the whole capture is on screen", () => {
    const window = fit();
    expect(follow(window, SIDE, 200_000_000)).toBe(window);
  });

  // Paged rather than centred: after the jump there are nine tenths of a
  // window in front of the playhead, so the picture sits still until it has
  // played through them.
  it("pages forward when the playhead leaves the right edge", () => {
    const window = { startFrame: 1_000_000, endFrame: 2_000_000 };
    const after = follow(window, SIDE, 2_000_001);
    const { start, end } = resolve(after, SIDE);
    expect(end - start).toBe(1_000_000);
    expect(2_000_001 - start).toBe(100_000);
  });

  it("pages back when the playhead is dragged behind the left edge", () => {
    const window = { startFrame: 5_000_000, endFrame: 6_000_000 };
    const after = follow(window, SIDE, 1_000_000);
    const { start } = resolve(after, SIDE);
    expect(1_000_000 - start).toBe(100_000);
  });

  it("stops at the end rather than paging past it", () => {
    const window = { startFrame: SIDE - 2_000_000, endFrame: SIDE - 1_000_000 };
    const after = follow(window, SIDE, SIDE);
    expect(resolve(after, SIDE).end).toBe(SIDE);
    expect(wide(after)).toBe(1_000_000);
  });
});
