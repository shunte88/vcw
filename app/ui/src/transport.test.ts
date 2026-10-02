/*
 *  transport.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What Play plays, which is the selection when there is one.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// One sentence of behaviour, and it is the one the waveform's drag gesture
// exists for: a band drawn on the picture is a stretch to listen to, so the
// transport has to mean that stretch and not the whole side. Tested here
// rather than by pressing the key, for the reason `wiring.test.ts` gives at
// length: the key reaching a handler is that file's job, and what the handler
// decides is this one's.

import { describe, expect, it } from "vitest";

import { scopeOf } from "./panels/Transport";

describe("what Play plays", () => {
  it("is the whole capture when nothing is selected", () => {
    expect(scopeOf(null)).toEqual({ scope: "whole" });
  });

  it("is the selection when there is one", () => {
    // The two numbers go out as they came in: the clamping happened where the
    // drag did, against the capture's own length, and rounding them again
    // here would move an edge a person placed by hand.
    expect(scopeOf({ from: 125.5, to: 310.25 })).toEqual({
      scope: "region",
      start: 125.5,
      end: 310.25,
    });
  });
});
