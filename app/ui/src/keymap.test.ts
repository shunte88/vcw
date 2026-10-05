/*
 *  keymap.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The half of WP-16's exit criterion a type cannot check.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// `pnpm check` already proves every §44 workflow has a binding, because
// `COVERAGE` is declared as an exhaustive `Record<Workflow, ...>`. What a type
// cannot say is that the bindings do not collide, that the chords are spellable
// by a real keyboard, and that `COVERAGE` names the bindings it claims to.
//
// That last one is the interesting test. A `Record<Workflow, Action[]>` is
// satisfied by any action at all, so `"delete-marker": ["play"]` typechecks
// perfectly and is a lie. Checking that the named binding actually declares the
// workflow is what closes it.

import { describe, expect, it } from "vitest";

import {
  BINDINGS,
  COVERAGE,
  type Action,
  entries,
  inScope,
  spellings,
} from "./keymap";
import { chord, typing } from "./keys";

/** A `KeyboardEvent` as the browser would report it, without a DOM. */
function press(
  key: string,
  modifiers: { ctrl?: boolean; alt?: boolean; shift?: boolean } = {},
): KeyboardEvent {
  return {
    key,
    ctrlKey: modifiers.ctrl ?? false,
    altKey: modifiers.alt ?? false,
    shiftKey: modifiers.shift ?? false,
    target: null,
  } as unknown as KeyboardEvent;
}

/**
 * A spelling split back into its modifiers and its key.
 *
 * `split("+")` alone is not enough, and the zoom keys are why. `chord()` joins
 * with `+`, so the chord for the plus key is the one-character string `"+"` and
 * splitting that gives two empty strings - which the assertions below would
 * read as a modifier named `""`. A trailing empty segment means the key *is*
 * the separator, which is exactly what a browser reports for Shift and the
 * equals key.
 */
function spelled(spelling: string): { modifiers: string[]; key: string } {
  const split = spelling.split("+");
  if (split[split.length - 1] === "") {
    return { modifiers: split.slice(0, -2), key: "+" };
  }
  return {
    modifiers: split.slice(0, -1),
    key: split[split.length - 1] ?? "",
  };
}

describe("the keyboard map", () => {
  it("claims coverage only of bindings that declare the workflow", () => {
    for (const [workflow, actions] of Object.entries(COVERAGE)) {
      expect(actions.length, `${workflow} lists no binding`).toBeGreaterThan(0);
      for (const action of actions) {
        expect(
          BINDINGS[action].workflow,
          `COVERAGE says ${action} serves ${workflow}, but it declares ${BINDINGS[action].workflow}`,
        ).toBe(workflow);
      }
    }
  });

  it("covers every binding, so none is unreachable", () => {
    const claimed = new Set(Object.values(COVERAGE).flat());
    const orphans = (Object.keys(BINDINGS) as Action[]).filter(
      (action) => !claimed.has(action),
    );
    expect(
      orphans,
      "a binding in no workflow is a key that does something nobody asked for",
    ).toEqual([]);
  });

  it("binds no chord twice in one scope", () => {
    // Per scope rather than globally, because `Enter` meaning "open the
    // project" in the browser and "edit the track" in the editor is correct -
    // that is what a scope is for. Two bindings live at once is the fault.
    const scopes = new Set(Object.values(BINDINGS).map((b) => b.scope));
    for (const scope of scopes) {
      const seen = new Map<string, Action>();
      for (const [action, binding] of inScope(scope)) {
        for (const spelling of spellings(binding)) {
          const clash = seen.get(spelling);
          expect(
            clash,
            `${spelling} is bound to both ${clash} and ${action} in ${scope}`,
          ).toBeUndefined();
          seen.set(spelling, action);
        }
      }
    }
  });

  it("spells every chord the way an event does", () => {
    // The map is written by hand and matched against `chord(event)` output. A
    // binding spelled `"Ctrl+S"` with a capital S would appear in the help
    // overlay and never fire, which is worse than not having it.
    for (const [action, binding] of entries()) {
      for (const spelling of spellings(binding)) {
        const { modifiers, key } = spelled(spelling);
        expect(
          modifiers.every((m: string) => ["Ctrl", "Alt", "Shift"].includes(m)),
          `${action}: ${spelling} has a modifier that is not Ctrl, Alt or Shift`,
        ).toBe(true);
        expect(
          modifiers,
          `${action}: ${spelling} has its modifiers out of order`,
        ).toEqual(["Ctrl", "Alt", "Shift"].filter((m) => modifiers.includes(m)));
        if (key.length === 1) {
          expect(
            key,
            `${action}: a single-character key must be lower case`,
          ).toBe(key.toLowerCase());
          expect(
            modifiers.includes("Shift"),
            `${action}: a character key already carries its shift`,
          ).toBe(false);
        }
      }
    }
  });

  // The plus key, which is the one chord the `+`-joined spelling cannot say by
  // splitting. Both ends: that `chord()` really does report `"+"` for Shift and
  // the equals key, so the binding is reachable, and that the reader above
  // takes it apart the way it was put together.
  it("can spell the key that is also the separator", () => {
    expect(chord(press("+", { shift: true }))).toBe("+");
    expect(spelled("+")).toEqual({ modifiers: [], key: "+" });
    expect(spelled("Ctrl++")).toEqual({ modifiers: ["Ctrl"], key: "+" });
    expect(spelled("Ctrl+ArrowLeft")).toEqual({
      modifiers: ["Ctrl"],
      key: "ArrowLeft",
    });
  });

  it("keeps every chord §43 suggests", () => {
    // §43's eight defaults, exactly as the requirement writes them. Rebinding
    // one is a decision worth arguing about rather than a tidy-up, so this
    // fails until somebody changes the list here too.
    const required: Record<string, string> = {
      space: "Play/Pause",
      r: "Record",
      s: "Stop",
      m: "Add marker",
      Delete: "Delete selected marker",
      ArrowLeft: "Seek",
      ArrowRight: "Seek",
      "Ctrl+s": "Save/checkpoint",
      "Ctrl+e": "Export",
    };
    const bound = new Set(
      entries().flatMap(([, binding]) => spellings(binding)),
    );
    for (const [spelling, what] of Object.entries(required)) {
      expect(bound.has(spelling), `§43 asks for ${spelling} (${what})`).toBe(
        true,
      );
    }
  });
});

describe("chord", () => {
  it("names a plain key", () => {
    expect(chord(press("r"))).toBe("r");
    expect(chord(press("R", { shift: true }))).toBe("r");
  });

  it("calls the space bar space", () => {
    expect(chord(press(" "))).toBe("space");
  });

  it("orders modifiers", () => {
    expect(chord(press("s", { ctrl: true }))).toBe("Ctrl+s");
    expect(chord(press("ArrowRight", { shift: true }))).toBe(
      "Shift+ArrowRight",
    );
    expect(chord(press("ArrowLeft", { ctrl: true, alt: true, shift: true }))).toBe(
      "Ctrl+Alt+Shift+ArrowLeft",
    );
  });

  it("does not add Shift to a character that already carries it", () => {
    // `?` is Shift and `/` here and something else on another layout, and
    // `event.key` is `"?"` either way. `Shift+?` is a chord no keyboard makes.
    expect(chord(press("?", { shift: true }))).toBe("?");
  });
});

describe("typing", () => {
  function into(tag: string, type?: string): KeyboardEvent {
    const element = document.createElement(tag);
    if (type !== undefined && element instanceof HTMLInputElement) {
      element.type = type;
    }
    return { key: "r", ctrlKey: false, altKey: false, shiftKey: false, target: element } as unknown as KeyboardEvent;
  }

  it("leaves a text field alone", () => {
    expect(typing(into("input", "text"))).toBe(true);
    expect(typing(into("textarea"))).toBe(true);
  });

  it("takes the key back for a tickbox, which has no text", () => {
    expect(typing(into("input", "checkbox"))).toBe(false);
    expect(typing(into("input", "radio"))).toBe(false);
  });

  it("keeps a modified chord even inside a field", () => {
    const element = document.createElement("input");
    const event = {
      key: "s",
      ctrlKey: true,
      altKey: false,
      shiftKey: false,
      target: element,
    } as unknown as KeyboardEvent;
    expect(
      typing(event),
      "Ctrl+S has to commit while a title is being edited",
    ).toBe(false);
  });

  it("always takes Escape, which is how a person leaves the field", () => {
    const element = document.createElement("input");
    const event = {
      key: "Escape",
      ctrlKey: false,
      altKey: false,
      shiftKey: false,
      target: element,
    } as unknown as KeyboardEvent;
    expect(typing(event)).toBe(false);
  });
});
