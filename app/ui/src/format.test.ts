/*
 *  format.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The clock has one format, and these are the cases that prove it.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import { describe, expect, it } from "vitest";

import { clock } from "./format";

describe("clock", () => {
  // The point of a single format is that the field positions never move, so
  // the test is the width as much as the value.
  it("is hh:mm:ss.fff at every magnitude", () => {
    expect(clock(0)).toBe("00:00:00.000");
    expect(clock(4.3)).toBe("00:00:04.300");
    expect(clock(59.999)).toBe("00:00:59.999");
    expect(clock(1567.5)).toBe("00:26:07.500");
    expect(clock(3600)).toBe("01:00:00.000");
    expect(clock(45296.789)).toBe("12:34:56.789");
  });

  it("never renders a sixtieth second or minute", () => {
    // 59.9999 rounds up in the millisecond field; split the fields first and
    // it reads 00:00:60.000, which is not a time.
    expect(clock(59.9999)).toBe("00:01:00.000");
    expect(clock(3599.9999)).toBe("01:00:00.000");
  });

  it("keeps the shape for a negative offset and for no answer at all", () => {
    expect(clock(-1.25)).toBe("-00:00:01.250");
    expect(clock(Number.NaN)).toBe("--:--:--.---");
    expect(clock(Number.POSITIVE_INFINITY)).toBe("--:--:--.---");
  });
});
