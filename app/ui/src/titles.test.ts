/*
 *  titles.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What the window is called, what to do next, and what an empty list means.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Three strings a person reads constantly and nobody tests, which is how all
// three came to be wrong at once. Driving the window found them: the title bar
// said " - Tomorrow's Harvest (Vinyl)" on every imported project, the status
// line said "Ready" before there was anything to be ready for, and Metadata
// told a person to fill in a field that the panel had already filled in.
//
// They are pure functions of state, so they are tested here rather than by
// rendering: the bug in each case was the sentence, not the wiring.

import { describe, expect, it } from "vitest";

import { next, titled } from "./App";
import { nothing } from "./panels/Metadata";
import type { ProjectState } from "./store";

/** A project state with only what these functions read. */
function project(over: Partial<ProjectState> = {}): ProjectState {
  return {
    path: "/library/a.vcw",
    captures: [],
    sides: [],
    tracks: [],
    boundaries: [],
    release: null,
    ...over,
  };
}

/** A release with only the two fields the title reads. */
function release(albumArtist: string, album: string) {
  return { albumArtist, album } as unknown as ProjectState["release"];
}

/** One capture row, of which these functions read only the count. */
const ONE_CAPTURE = [{}] as unknown as ProjectState["captures"];

/** One track row, likewise. */
const ONE_TRACK = [{}] as unknown as ProjectState["tracks"];

describe("the window title", () => {
  it("joins the artist and the album", () => {
    const state = project({ release: release("Talk Talk", "Spirit of Eden") });
    expect(titled(state)).toBe("Talk Talk - Spirit of Eden");
  });

  // The one that was wrong. An Audacity project carries an album tag and no
  // artist tag, so every imported rip titled the window with a leading dash.
  it("does not lead with a dash when there is no artist", () => {
    const state = project({ release: release("", "Tomorrow's Harvest") });
    expect(titled(state)).toBe("Tomorrow's Harvest");
  });

  it("does not trail with a dash when there is no album", () => {
    const state = project({ release: release("Kraftwerk", "") });
    expect(titled(state)).toBe("Kraftwerk");
  });

  it("falls back to the path when the release says nothing", () => {
    const state = project({ release: release("", "") });
    expect(titled(state)).toBe("/library/a.vcw");
  });

  it("names the path when there is no release at all", () => {
    expect(titled(project())).toBe("/library/a.vcw");
  });

  it("says so when nothing is open", () => {
    expect(titled(project({ path: null }))).toBe("no project open");
  });
});

describe("the next step", () => {
  it("sends a person to the library when nothing is open", () => {
    expect(next(project({ path: null }), "idle", "/data2/rips")).toContain("Ctrl+1");
  });

  it("does not send a person to an empty library", () => {
    // A fresh install has no library, so Ctrl+1 opens a panel with nothing in
    // it. The line has to name the thing that is actually missing.
    const said = next(project({ path: null }), "idle", null);
    expect(said).not.toContain("Ctrl+1");
    expect(said).toMatch(/no library/i);
  });

  it("names the record key once a device is armed", () => {
    expect(next(project(), "armed")).toContain("Press r");
  });

  it("sends a person to capture when the project is empty", () => {
    expect(next(project(), "idle")).toContain("Ctrl+2");
  });

  it("sends a person to detection once there is audio", () => {
    const state = project({ captures: ONE_CAPTURE });
    expect(next(state, "idle")).toContain("Ctrl+3");
  });

  // Every branch ends with the keyboard map, which is the offer the line used
  // to carry on its own.
  it("always offers the keyboard map", () => {
    const states = [
      next(project({ path: null }), "idle"),
      next(project(), "armed"),
      next(project(), "idle"),
      next(project({ captures: ONE_CAPTURE }), "idle"),
      next(project({ captures: ONE_CAPTURE, tracks: ONE_TRACK }), "idle"),
    ];
    for (const line of states) {
      expect(line).toContain("keyboard map");
    }
  });
});

describe("the metadata empty line", () => {
  const blank = { artist: "", album: "", catalog: "", barcode: "" };
  const seeded = { ...blank, album: "Tomorrow's Harvest" };

  it("asks for a field when every field is blank", () => {
    expect(nothing(blank, false, false)).toContain("Fill in at least one");
  });

  // The one that was wrong: the panel seeds itself from the release, so the
  // usual state is a filled form that has not been looked up, and it was being
  // told to fill in a field.
  it("offers the lookup when a field is filled", () => {
    expect(nothing(seeded, false, false)).toBe("Press l to look this up.");
  });

  it("says it is working while it works", () => {
    expect(nothing(seeded, true, false)).toBe("Looking.");
  });

  it("distinguishes no match from no lookup", () => {
    expect(nothing(seeded, false, true)).toContain("Nothing matched");
  });

  it("treats whitespace as blank", () => {
    expect(nothing({ ...blank, artist: "   " }, false, false)).toContain(
      "Fill in at least one",
    );
  });
});
