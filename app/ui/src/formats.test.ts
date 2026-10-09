/*
 *  formats.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The two format pickers offer the same formats, and each one has an effort.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// There are two format `<select>`s in this application - the one in Export
// that chooses what this export writes, and the one in Settings that chooses
// what the next one defaults to - and they are two hand-written lists of
// `<option>` elements with nothing between them. A container added to one and
// not the other is a build where the default can be set to something the
// export panel cannot show, and neither typecheck nor any rendering test would
// say so: both lists are valid TSX either way.
//
// So this reads the source of both and insists they agree, the same trick
// `Face.test.tsx` plays on the stylesheet. It cannot know about a container
// added to the Rust enum and to neither list - `Container::ALL` is not visible
// from here - but that one is caught at the other end, by
// `this_build_writes_the_containers_it_is_supposed_to`.

import { describe, expect, it } from "vitest";

import { effortOf } from "./effort";

const sources = import.meta.glob("./panels/*.tsx", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

/** The `value="..."` of every option in the first `<select>` of a panel's format field. */
function options(panel: string): string[] {
  const source = sources[`./panels/${panel}.tsx`];
  if (source === undefined) {
    throw new Error(`no source for ${panel}`);
  }
  // Anchored on the FLAC option, which is first in both and is the only
  // format that is never going away, rather than on the `<select>` tag -
  // there are other selects in both panels.
  const block = source.slice(source.indexOf('<option value="flac"'));
  return [...block.slice(0, block.indexOf("</select>")).matchAll(/value="([\w-]+)"/g)].map(
    (found) => found[1]!,
  );
}

describe("the format pickers", () => {
  it("offer the same formats in the same order", () => {
    expect(options("Export")).toEqual(options("Settings"));
  });

  it("offer more than one, so an empty match is not a pass", () => {
    expect(options("Export").length).toBeGreaterThan(3);
  });

  it("know whether each format has an effort to set", () => {
    // `effortOf` returns null for a format it does not recognise, which is
    // also what it returns for a format with nothing to set. The two are
    // indistinguishable at the call site, so the list is written out: a new
    // lossy container that nobody taught `effortOf` about would otherwise ship
    // with its quality control missing and no test to say so.
    const efforts: Record<string, string | null> = {
      flac: "compression",
      wav: null,
      aiff: null,
      mp3: "quality",
      ogg: "quality",
    };
    for (const format of options("Export")) {
      expect(Object.keys(efforts)).toContain(format);
      expect(effortOf(format)?.field ?? null).toBe(efforts[format]);
    }
  });
});
