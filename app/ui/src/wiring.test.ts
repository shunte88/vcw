/*
 *  wiring.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-16's exit criterion, asserted rather than claimed.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// "Every §44 workflow completable by keyboard alone" is a claim about three
// things lining up, and `keymap.test.ts` only checks the first two:
//
//  1. every workflow has a binding      - `pnpm check`, via the exhaustive record
//  2. the bindings are spellable and do not collide - `keymap.test.ts`
//  3. every binding has a handler behind it         - here
//
// The third is the one that actually fails in practice. A binding whose action
// nobody handles is a key that is documented in the help overlay, spelled
// correctly, claimed by a workflow, and does nothing at all when pressed. That
// was the state of four workflows before WP-16, and it is invisible to every
// check except pressing the key.
//
// This is done by reading the sources, which wants saying out loud: rendering
// the whole application and dispatching 31 synthetic keystrokes would be a
// better test and a much worse one to own, because it would need a Tauri
// `invoke` stubbed for every command and would fail for reasons that have
// nothing to do with the keyboard. What is asserted here is narrow and true:
// for each action there is a `useKeys` handler object somewhere with that
// action as a key. Whether the handler does the right thing is what the panel's
// own tests and the shell's tests are for.

import { describe, expect, it } from "vitest";

import { BINDINGS, COVERAGE, type Action } from "./keymap";

/**
 * Every `.tsx` under `src`, which is where a `useKeys` call can live.
 *
 * Vite's glob rather than `node:fs`, and that is not a style choice: the
 * frontend's `tsconfig.json` declares `lib: [ES2022, DOM]` and no Node types,
 * on purpose, because nothing that ships runs in Node. Reading the tree with
 * `readdirSync` meant adding `@types/node` to the one project that should not
 * have it, and `pnpm check` said so. `import.meta.glob` is resolved at
 * transform time by the bundler that is already there.
 */
const SOURCES = import.meta.glob("./**/*.tsx", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** The source text of every component. */
function sources(): string[] {
  return Object.values(SOURCES);
}

/**
 * The handler objects, as text: each `useKeys(...)` call's arguments.
 *
 * Text rather than a parse, because the thing being checked is a property name
 * in an object literal and a regular expression finds those exactly as well as
 * a syntax tree would, without a parser dependency for one test.
 *
 * The extent is found by counting brackets rather than by matching a closing
 * `});`, which is the mistake this function was written with the first time: a
 * `useKeys("metadata", { lookup: search, accept });` written on one line has no
 * such line to match, so two panels' worth of handlers were invisible and the
 * assertions below passed for the wrong reason. Counting is the only way that
 * does not depend on how the call happens to be formatted.
 */
function handlers(): string {
  const blocks: string[] = [];
  for (const text of sources()) {
    let at = text.indexOf("useKeys(");
    while (at !== -1) {
      let depth = 0;
      let end = at + "useKeys".length;
      for (; end < text.length; end += 1) {
        const character = text[end];
        if (character === "(" || character === "{") {
          depth += 1;
        } else if (character === ")" || character === "}") {
          depth -= 1;
          if (depth === 0) {
            break;
          }
        }
      }
      blocks.push(text.slice(at, end + 1));
      at = text.indexOf("useKeys(", end);
    }
  }
  return blocks.join("\n");
}

/**
 * Whether an action is a key of some handler object.
 *
 * Both spellings count: `record: () => ...` and the shorthand `arm,` that a
 * handler named after its action gets written as. The leading class keeps
 * `api.play(` from counting as a handler for `play`.
 */
function handled(text: string, action: Action): boolean {
  return new RegExp(`(^|[\\s{,])${action}\\s*[:,}]`, "m").test(text);
}

describe("the keyboard map's wiring", () => {
  it("finds the handler blocks to read", () => {
    // If this fails the regex above has stopped matching and every assertion
    // below would pass by finding an empty string.
    const text = handlers();
    expect(text).toContain("useKeys(");
    expect(text.length).toBeGreaterThan(500);
  });

  it("has a handler for every binding", () => {
    const text = handlers();
    const orphans = (Object.keys(BINDINGS) as Action[]).filter(
      (action) => !handled(text, action),
    );
    expect(
      orphans,
      "these actions are bound to a chord, shown in the help overlay, and " +
        "handled by nothing. A key that does nothing is worse than no key",
    ).toEqual([]);
  });

  it("has a handler for every §44 workflow", () => {
    // The same assertion from the requirement's end rather than the map's, so
    // that a workflow whose only binding lost its handler is named as a
    // *workflow* - which is what the exit criterion is written in terms of.
    const text = handlers();
    const unreachable = Object.entries(COVERAGE)
      .filter(([, actions]) => !actions.some((action) => handled(text, action)))
      .map(([workflow]) => workflow);
    expect(
      unreachable,
      "§44 lists these workflows and no key completes one of them",
    ).toEqual([]);
  });
});
