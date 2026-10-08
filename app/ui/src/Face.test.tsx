/*
 *  Face.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A button keeps its name in icon mode, and every glyph named has a rule.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Two things, and the second is the one that will actually fire.
//
// The word has to survive the switch. An icon row where the buttons have no
// accessible name is a row that can only be used by someone who already knows
// what the pictures mean, which is exactly the person who did not need the
// labels in the first place.
//
// And every glyph a component asks for has to be a glyph the stylesheet can
// draw. `Face` takes a class name as a string, so a seventh panel tab added
// without its rule compiles, renders, and shows an empty 16 px gap - there is
// nothing between the tab and the stylesheet that would complain. This reads
// both files and insists they agree.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";

import { Face } from "./Face";
import { applyButtonStyle } from "./scale";

const css = Object.values(
  import.meta.glob("./*.css", { eager: true, query: "?raw", import: "default" }),
).join("\n");

/** Every `.icon.<name>` the stylesheet draws. */
const drawn = new Set(
  [...css.matchAll(/\.icon\.([\w-]+)\s*\{/g)].map((found) => found[1]!),
);

/** Every glyph a component asks `Face` for, and every one written by hand.
 *
 * Two passes because the call sites take three shapes: the attribute
 * (`icon="record"`), the table entry (`icon: "library"`), and the toggle
 * (`icon={recording ? "pause" : "play"}`), which names two glyphs in one
 * expression. So the scan takes everything quoted up to the end of the value
 * rather than the first string it meets.
 */
const asked = new Set(
  Object.values(
    import.meta.glob("./**/*.tsx", {
      eager: true,
      query: "?raw",
      import: "default",
    }),
  ).flatMap((source) =>
    [
      ...(source as string).matchAll(/\bicon[=:]\s*(\{[^}]*\}|"[\w-]+")/g),
      ...(source as string).matchAll(/className="icon (?<one>[\w-]+)"/g),
    ].flatMap((found) =>
      [
        ...(found[1] ?? `"${found.groups?.one ?? ""}"`).matchAll(/"([\w-]+)"/g),
      ].map((word) => word[1]!),
    ),
  ),
);

function render(node: React.ReactNode): HTMLElement {
  const container = document.createElement("div");
  document.body.append(container);
  act(() => {
    createRoot(container).render(node);
  });
  return container;
}

describe("a button's face", () => {
  afterEach(() => {
    applyButtonStyle("text");
  });

  it("is the word, and only the word, out of the box", () => {
    const container = render(<Face icon="record">Record</Face>);
    expect(container.textContent).toBe("Record");
    expect(container.querySelector(".icon")).toBeNull();
  });

  it("keeps the word in the markup when it shows the glyph", () => {
    applyButtonStyle("icons");
    const container = render(<Face icon="record">Record</Face>);
    const glyph = container.querySelector(".icon");
    expect(glyph?.className).toBe("icon record");
    expect(glyph?.getAttribute("aria-hidden")).toBe("true");
    // Off the page, not out of it: `.btn-label` is clipped by the stylesheet.
    expect(container.querySelector(".btn-label")?.textContent).toBe("Record");
  });

  it("asks for no glyph the stylesheet cannot draw", () => {
    expect(asked.size).toBeGreaterThan(10);
    expect([...asked].filter((name) => !drawn.has(name))).toEqual([]);
  });
});
