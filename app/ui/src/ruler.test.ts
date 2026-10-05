/*
 *  ruler.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  That the ruler's marks land on round times, stay apart, and say the truth.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import { describe, expect, it } from "vitest";

import { mark, ticks } from "./ruler";

/** The labeled marks only, which is what a person actually reads. */
function labeled(
  startSeconds: number,
  endSeconds: number,
  pixels: number,
): string[] {
  return ticks(startSeconds, endSeconds, pixels)
    .filter((tick) => tick.major)
    .map((tick) => tick.label);
}

/** The gap between consecutive labeled marks, in pixels. */
function gaps(
  startSeconds: number,
  endSeconds: number,
  pixels: number,
): number[] {
  const perPixel = (endSeconds - startSeconds) / pixels;
  const majors = ticks(startSeconds, endSeconds, pixels).filter(
    (tick) => tick.major,
  );
  return majors
    .slice(1)
    .map((tick, index) => (tick.seconds - (majors[index]?.seconds ?? 0)) / perPixel);
}

describe("a ruler's marks", () => {
  // The real case: side A of Spirit of Eden, 26:05.77, across the width the
  // window actually has.
  it("labels a whole side at an interval a person thinks in", () => {
    expect(labeled(0, 1565.77, 1580)).toEqual([
      "0:00",
      "2:00",
      "4:00",
      "6:00",
      "8:00",
      "10:00",
      "12:00",
      "14:00",
      "16:00",
      "18:00",
      "20:00",
      "22:00",
      "24:00",
      "26:00",
    ]);
  });

  // The point of the fixed step list. Ten marks across any range would put
  // this one on 2.6-second centers, and reading a time off that is division.
  it("never invents an interval, whatever the range", () => {
    const allowed = new Set([
      0.05, 0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800,
      3600,
    ]);
    for (const span of [0.4, 2.6, 7.3, 26, 61, 197, 620, 1565.77, 2700]) {
      const majors = ticks(0, span, 1400).filter((tick) => tick.major);
      if (majors.length < 2) {
        continue;
      }
      const step = (majors[1]?.seconds ?? 0) - (majors[0]?.seconds ?? 0);
      // Rounded to the microsecond: the step comes out of a division.
      expect(allowed.has(Math.round(step * 1e6) / 1e6)).toBe(true);
    }
  });

  it("keeps labels far enough apart to read at every zoom", () => {
    for (const span of [0.4, 2.6, 7.3, 26, 61, 197, 620, 1565.77, 2700]) {
      for (const width of [400, 900, 1400, 3400]) {
        for (const gap of gaps(0, span, width)) {
          expect(gap).toBeGreaterThanOrEqual(78);
        }
      }
    }
  });

  // Panning must not slide the numbers about, so the marks are on absolute
  // time and the first one is the first round time inside the window.
  it("puts marks on round times, not on the start of the window", () => {
    expect(labeled(483.2, 581.4, 1400)).toEqual([
      "8:10",
      "8:20",
      "8:30",
      "8:40",
      "8:50",
      "9:00",
      "9:10",
      "9:20",
      "9:30",
      "9:40",
    ]);
  });

  it("divides each label's gap into five", () => {
    const marks = ticks(0, 60, 1400);
    expect(marks.filter((tick) => !tick.major).length).toBeGreaterThan(0);
    expect(marks[0]?.major).toBe(true);
    expect(marks[1]?.major).toBe(false);
    expect(marks[4]?.major).toBe(false);
    expect(marks[5]?.major).toBe(true);
  });

  // The reason `ticks` has no "only if there is room" test for the minor
  // marks: it could never fire. A label is given at least 78 pixels, so a
  // fifth of that gap is at least 15, at every zoom and every width. The first
  // draft had the guard, this is the assertion that replaced it, and it is the
  // invariant rather than the branch.
  it("never smears the minor marks, at any zoom or width", () => {
    for (const span of [0.4, 2.6, 7.3, 26, 61, 197, 620, 1565.77, 2700]) {
      for (const width of [400, 900, 1400, 3400]) {
        const perPixel = span / width;
        const marks = ticks(0, span, width);
        for (let n = 1; n < marks.length; n += 1) {
          const gap =
            ((marks[n]?.seconds ?? 0) - (marks[n - 1]?.seconds ?? 0)) /
            perPixel;
          expect(gap).toBeGreaterThanOrEqual(15);
        }
      }
    }
  });

  it("has nothing to draw on a range with no width", () => {
    expect(ticks(0, 0, 1400)).toEqual([]);
    expect(ticks(10, 5, 1400)).toEqual([]);
    expect(ticks(0, Number.NaN, 1400)).toEqual([]);
  });

  // What a panel one pixel wide asks for during a layout pass, and what the
  // component asks for before the ResizeObserver has said anything.
  it("has nothing to draw on a strip too narrow to label", () => {
    expect(ticks(0, 60, 1)).toEqual([]);
    expect(ticks(0, 60, 0)).toEqual([]);
  });
});

describe("a ruler's labels", () => {
  it("prints minutes and seconds, with no centiseconds it does not need", () => {
    expect(mark(0, 1)).toBe("0:00");
    expect(mark(5, 5)).toBe("0:05");
    expect(mark(65, 5)).toBe("1:05");
    expect(mark(1560, 60)).toBe("26:00");
  });

  it("goes to hours past the hour, and not before", () => {
    expect(mark(3599, 1)).toBe("59:59");
    expect(mark(3600, 60)).toBe("1:00:00");
    expect(mark(3725, 5)).toBe("1:02:05");
  });

  // Zoomed in far enough and five marks all saying `0:05` is not a ruler.
  it("adds decimals only when the step is under a second", () => {
    expect(mark(5.5, 0.5)).toBe("0:05.5");
    expect(mark(5.25, 0.05)).toBe("0:05.25");
    expect(mark(65.5, 0.5)).toBe("1:05.5");
  });

  it("does not print a sixtieth second", () => {
    expect(mark(59.999, 1)).toBe("1:00");
  });
});
