/*
 *  metadata.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The sentences the metadata panel builds out of what accepting a release did.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import { describe, expect, it } from "vitest";

import { kb, sides } from "./panels/Metadata";
import { UNLOGGED, logged } from "./store";

describe("naming the sides a re-lay touched", () => {
  it("is one side when every position is on it", () => {
    expect(sides(["B1", "B2", "B5"])).toBe("side B");
  });

  it("is a list with an and for more than one", () => {
    expect(sides(["B1", "C1", "C2", "D3"])).toBe("sides B, C and D");
  });

  it("is in alphabetical order whatever order the moves came in", () => {
    expect(sides(["D1", "B1", "C1"])).toBe("sides B, C and D");
  });

  it("does not say anything about an empty list", () => {
    expect(sides([])).toBe("no sides");
  });
});

describe("a cover's size", () => {
  it("rounds to whole kilobytes", () => {
    expect(kb(146_432)).toBe("143 KB");
  });
});

describe("what the event log keeps", () => {
  // The flood: a 500-line log filled in about eight seconds of playback, so a
  // refusal was off the end of the buffer before the panel could be opened.
  it("drops the continuous streams", () => {
    for (const kind of ["meter-update", "playback-position"]) {
      expect(UNLOGGED.has(kind)).toBe(true);
    }
  });

  it("keeps the things that happen once", () => {
    for (const kind of [
      "command-refused",
      "capture-warning",
      "playback-finished",
      "export-failed",
    ]) {
      expect(logged({ kind } as never)).toBe(true);
    }
  });
});
