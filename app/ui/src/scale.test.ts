/*
 *  scale.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  That a screen property survives a round trip and refuses a stranger.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// These values come back off a disk that a person can edit, that an older
// version of the window wrote, and that a browser will happily hand back
// empty. A reader that trusted what it found there would put an unknown word
// on the meter strip, where it matches no rule and the dial loses its face
// with no way to say why.

import { afterEach, describe as group, expect, it } from "vitest";

import { applyBacklight, backlightOf, onBacklight } from "./scale";

afterEach(() => localStorage.clear());

group("the bulb behind the dials", () => {
  it("is off until somebody turns it on", () => {
    expect(backlightOf()).toBe("off");
  });

  it("comes back as it was set, and tells the window it changed", () => {
    let told = 0;
    const stop = onBacklight(() => {
      told += 1;
    });
    applyBacklight("macintosh");
    expect(backlightOf()).toBe("macintosh");
    // The event, not `storage`: that one does not fire in the tab that wrote
    // it, so the strip would keep the old bulb until something else redrew.
    expect(told).toBe(1);
    stop();
    applyBacklight("classic");
    expect(told).toBe(1);
  });

  it("is off when what was stored is not a bulb of ours", () => {
    localStorage.setItem("vcw.backlight", "lava");
    expect(backlightOf()).toBe("off");
  });
});
