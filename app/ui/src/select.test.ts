/*
 *  select.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What ArrowUp and ArrowDown do at the ends of a list, and on an empty one.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import { describe, expect, it } from "vitest";

import { step } from "./select";

describe("moving a selection", () => {
  const rows = ["a", "b", "c"];

  it("moves one row at a time in both directions", () => {
    expect(step(rows, "a", 1)).toBe("b");
    expect(step(rows, "b", 1)).toBe("c");
    expect(step(rows, "c", -1)).toBe("b");
    expect(step(rows, "b", -1)).toBe("a");
  });

  // The case the whole function exists for: a window that has just opened has
  // nothing selected, and until WP-16a there was no way out of that state
  // without a mouse.
  it("selects an end when nothing is selected yet", () => {
    expect(step(rows, null, 1)).toBe("a");
    expect(step(rows, null, -1)).toBe("c");
  });

  it("clamps rather than wrapping", () => {
    expect(step(rows, "c", 1)).toBe("c");
    expect(step(rows, "a", -1)).toBe("a");
  });

  // Holding the key is the normal way to cross a long list, and a wrap would
  // make the bottom row impossible to rest on.
  it("comes to rest at the bottom when the key is held", () => {
    let at: string | null = null;
    for (let press = 0; press < 10; press += 1) {
      at = step(rows, at, 1);
    }
    expect(at).toBe("c");
  });

  it("has nothing to select in an empty list", () => {
    expect(step([], null, 1)).toBeNull();
    expect(step([], "a", -1)).toBeNull();
  });

  // A track that was deleted, or a boundary that was renumbered, leaves the
  // panel holding an id that is no longer on show. Treated as a fresh start
  // rather than as an error, so the next press still lands somewhere.
  it("starts again when the selection is no longer in the list", () => {
    expect(step(rows, "gone", 1)).toBe("a");
    expect(step(rows, "gone", -1)).toBe("c");
  });

  // Ids, not just strings: three of the four lists select by a numeric id.
  it("works on the numeric ids the track lists use", () => {
    expect(step([7, 9, 11], 9, 1)).toBe(11);
    expect(step([7, 9, 11], 7, -1)).toBe(7);
  });
});
