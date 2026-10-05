/*
 *  effort.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The export effort field follows the format, and never mislabels FLAC.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import { describe, expect, it } from "vitest";

import { effortOf } from "./effort";

describe("the export effort field", () => {
  // The defect this was written for: FLAC was offered "Lossy quality", which
  // says the one thing about FLAC that is not true.
  it("never calls FLAC lossy, and defaults it to 5", () => {
    const flac = effortOf("flac");
    expect(flac?.caption).toBe("Compression");
    expect(flac?.field).toBe("compression");
    expect(flac?.options.map(([value]) => value)).toEqual([
      "0",
      "1",
      "2",
      "3",
      "4",
      "5",
      "6",
      "7",
      "8",
    ]);
    expect(flac?.options.find(([value]) => value === "5")?.[1]).toContain(
      "default",
    );
  });

  it("gives the lossy containers a quality and WAV nothing at all", () => {
    for (const format of ["mp3", "ogg", "oga", "vorbis"]) {
      expect(effortOf(format)?.field, format).toBe("quality");
      expect(effortOf(format)?.caption, format).toBe("Lossy quality");
    }
    expect(effortOf("wav")).toBeNull();
  });

  it("reads the spellings the backend accepts", () => {
    expect(effortOf("FLAC")).toEqual(effortOf("flac"));
    expect(effortOf(".flac")).toEqual(effortOf("flac"));
    expect(effortOf(" ogg ")).toEqual(effortOf("ogg"));
    expect(effortOf("aiff")).toBeNull();
  });
});
