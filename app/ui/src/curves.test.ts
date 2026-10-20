/*
 *  curves.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The tracking help reads as sentences, and says a different one per chain.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The text this builds is a tooltip, which is the one piece of prose in the
// window nothing renders in a test and nobody reads until they hover it. It
// shipped joining a label to a sentence on a bare space, so it said "Columbia
// LP (June 1948 to 1956) Cut by Columbia" to anyone who did.

import { describe, expect, it } from "vitest";

import type { Curve } from "./bindings/vcw";
import { RIAA, chainOf, help } from "./curves";

/** The key, not a translation: enough to see which clause was chosen. */
const text = (key: string) => key;

const columbia: Curve = {
  id: "columbia-lp",
  name: "Columbia LP",
  era: "June 1948 to 1956",
  labels: "Columbia",
} as Curve;

const riaa: Curve = {
  id: RIAA,
  name: "RIAA",
  era: "August 1952 onward",
  labels: null,
} as Curve;

describe("the tracking help", () => {
  it("ends the curve's name before the next sentence starts", () => {
    expect(help(columbia, "flat", text)).toMatch(
      /^Columbia LP \(June 1948 to 1956\)\. library\.new\.eq\.labels /,
    );
  });

  it("ends it the same way for a curve with no labels", () => {
    expect(help(riaa, "flat", text)).toBe(
      "RIAA (August 1952 onward). library.new.eq.chain.flat",
    );
  });

  it("names what the chain already did, one clause per chain", () => {
    expect(help(columbia, "riaa", text)).toContain("chain.undo");
    expect(help(riaa, "riaa", text)).toContain("chain.match");
    expect(help(columbia, "flat", text)).toContain("chain.flat");
    expect(help(columbia, "unstated", text)).toContain("chain.unstated");
  });

  it("reads anything that is not flat or RIAA as unstated", () => {
    expect(chainOf("riaa")).toBe("riaa");
    expect(chainOf("flat")).toBe("flat");
    expect(chainOf("unknown")).toBe("unstated");
    expect(chainOf(null)).toBe("unstated");
    expect(chainOf(undefined)).toBe("unstated");
  });
});
