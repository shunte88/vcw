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
  center,
  clamp,
  fit,
  follow,
  nearestFrame,
  pan,
  region,
  resolve,
  toBand,
  toRange,
  toSelection,
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

  it("centers on a frame without changing width", () => {
    const window = zoom(fit(), SIDE, 0.1, 0.5);
    const at = SIDE / 3;
    const after = center(window, SIDE, at);
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
      center(fit(), 0, 1000),
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

  // Paged rather than centered: after the jump there are nine tenths of a
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

// A plain drag across the picture draws a band, and the band is what the zoom
// key then zooms to. The gesture is tested here rather than in the panel
// because what can go wrong with it is arithmetic: a backwards drag, a drag
// that leaves the picture under pointer capture, and a drag so short it asks
// for a window narrower than the panel can draw.
describe("zooming to a selection", () => {
  it("becomes exactly the frames it covered, with no margin", () => {
    const after = toSelection(40_000_000, 60_000_000, SIDE);
    expect(resolve(after, SIDE)).toEqual({ start: 40_000_000, end: 60_000_000 });
  });

  it("does not pad the way zooming to a track does", () => {
    // The one difference between the two, stated as a test so that a later
    // tidy-up cannot quietly make them the same function.
    expect(wide(toSelection(40_000_000, 60_000_000, SIDE))).toBe(20_000_000);
    expect(wide(toRange(40_000_000, 60_000_000, SIDE))).toBeGreaterThan(20_000_000);
  });

  it("reads a backwards drag the same as a forwards one", () => {
    expect(toSelection(60_000_000, 40_000_000, SIDE)).toEqual(
      toSelection(40_000_000, 60_000_000, SIDE),
    );
  });

  it("widens a drag too short to draw, about its own middle", () => {
    const after = toSelection(100_000, 100_010, SIDE);
    expect(wide(after)).toBe(CLOSEST);
    const { start, end } = resolve(after, SIDE);
    expect((start + end) / 2).toBeCloseTo(100_005, -1);
  });

  it("keeps a drag against the end inside the capture", () => {
    const after = toSelection(SIDE - 1000, SIDE + 5_000_000, SIDE);
    const { start, end } = resolve(after, SIDE);
    expect(end).toBe(SIDE);
    expect(start).toBeGreaterThanOrEqual(0);
    expect(end - start).toBe(CLOSEST);
  });

  it("gives back the whole capture when there is none", () => {
    expect(toSelection(10, 20, 0)).toEqual(fit());
    expect(toSelection(Number.NaN, 20, SIDE)).toEqual(fit());
  });
});

// What a drag leaves behind, which is a region in seconds because the next
// thing to happen to it is `api.play`. Clamping is the whole job: pointer
// capture reports past both ends of the capture, and a region that asks to
// play from -3 seconds is a refusal from the shell rather than a selection.
describe("the region a drag selects", () => {
  const SECONDS = 1800;

  it("is the two seconds it was drawn between", () => {
    expect(region(300.5, 420.25, SECONDS)).toEqual({ from: 300.5, to: 420.25 });
  });

  it("reads a backwards drag the same as a forwards one", () => {
    expect(region(420.25, 300.5, SECONDS)).toEqual(region(300.5, 420.25, SECONDS));
  });

  it("stops at both ends of the capture", () => {
    expect(region(-30, 90, SECONDS)).toEqual({ from: 0, to: 90 });
    expect(region(1700, 9000, SECONDS)).toEqual({ from: 1700, to: SECONDS });
  });

  it("is nothing when a drag selected nothing", () => {
    // A press and release in one place, which is a click: the panel seeks and
    // clears the selection, and must not be handed a region to play.
    expect(region(90, 90, SECONDS)).toBeNull();
    expect(region(-30, -20, SECONDS)).toBeNull();
    expect(region(10, 20, 0)).toBeNull();
    expect(region(Number.NaN, 20, SECONDS)).toBeNull();
  });
});

describe("the band a drag paints", () => {
  it("is the fraction of the window the drag covers", () => {
    expect(toBand(250, 500, 0, 1000)).toEqual({ left: 25, width: 25 });
  });

  it("stops at the edges when a drag runs off the picture", () => {
    // Pointer capture keeps the reports coming after the pointer has left the
    // canvas, which is the ordinary way to select up to the edge.
    expect(toBand(-4000, 500, 0, 1000)).toEqual({ left: 0, width: 50 });
    expect(toBand(500, 9000, 0, 1000)).toEqual({ left: 50, width: 50 });
  });

  it("is nothing when the selection is off the window entirely", () => {
    expect(toBand(2000, 3000, 0, 1000)).toBeNull();
    expect(toBand(-3000, -2000, 0, 1000)).toBeNull();
  });

  it("is nothing when the drag has not moved, and nothing to divide by", () => {
    expect(toBand(500, 500, 0, 1000)).toBeNull();
    expect(toBand(250, 500, 1000, 1000)).toBeNull();
  });
});

describe("snapping a click to a boundary", () => {
  // A thousand pixels across a thousand frames, so a frame is a pixel and the
  // arithmetic stays out of the way of what is being asserted.
  const PICTURE = { width: 1000, start: 0, end: 1000, within: 6 };
  const snap = (frames: readonly number[], x: number) =>
    nearestFrame(
      frames,
      x,
      PICTURE.width,
      PICTURE.start,
      PICTURE.end,
      PICTURE.within,
    );

  it("takes the boundary under the pointer", () => {
    expect(snap([200, 500, 800], 500)).toBe(500);
  });

  it("reaches a boundary a few pixels away, which is the whole point", () => {
    // Nobody hits a two-pixel line exactly, and `split` needs the frame and
    // not the neighborhood.
    expect(snap([200, 500, 800], 504)).toBe(500);
    expect(snap([200, 500, 800], 496)).toBe(500);
  });

  it("lets go of a click that was not aimed at one", () => {
    // Null rather than the nearest, because the caller falls back to the
    // pixel: a click in the middle of a track means that spot, not the
    // boundary four hundred frames away.
    expect(snap([200, 500, 800], 400)).toBeNull();
  });

  it("takes the nearer of two, not the first in reach", () => {
    expect(snap([498, 503], 502)).toBe(503);
    expect(snap([498, 503], 499)).toBe(498);
  });

  it("keeps the earlier of two equally close, so the answer is not arbitrary", () => {
    // Dead between them. Either is defensible and neither is obviously right,
    // so the rule is written down here and the first one wins - without this
    // the comparison could be loosened to `<=` and the answer would quietly
    // change to the later frame with nothing to notice.
    expect(snap([498, 502], 500)).toBe(498);
  });

  it("holds at the edge of the tolerance rather than just inside it", () => {
    expect(snap([500], 506)).toBe(500);
    expect(snap([500], 507)).toBeNull();
  });

  it("has nothing to say about an empty side or a picture with no width", () => {
    expect(snap([], 500)).toBeNull();
    expect(nearestFrame([500], 500, 0, 0, 1000, 6)).toBeNull();
    expect(nearestFrame([500], 500, 1000, 1000, 1000, 6)).toBeNull();
  });

  it("works zoomed in, where a pixel is a fraction of a frame", () => {
    // 100 frames across 1000 pixels: ten pixels to the frame, so the six-pixel
    // tolerance is less than one frame and only a near-exact click lands.
    expect(nearestFrame([50], 500, 1000, 0, 100, 6)).toBe(50);
    expect(nearestFrame([50], 505, 1000, 0, 100, 6)).toBe(50);
    expect(nearestFrame([50], 520, 1000, 0, 100, 6)).toBeNull();
  });
});
