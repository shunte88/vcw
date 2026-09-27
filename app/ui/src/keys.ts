/*
 *  keys.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Turns a KeyboardEvent into one of §43's chords, and dispatches it.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// One listener on the window, one spelling of a chord, one place that decides
// whether a keystroke was meant for the application or for the field the person
// is typing in. All three matter: a per-component listener is how two panels end
// up both handling `Space`, and two spellings of a chord is how a binding shown
// in the help overlay turns out not to fire.

import { useEffect, useRef } from "react";

import { type Action, type Binding, type Scope, entries, inScope, spellings } from "./keymap";

/**
 * The chord an event names, in the spelling `keymap.ts` uses.
 *
 * Modifiers in a fixed order - Ctrl, Alt, Shift - because `"Ctrl+Shift+x"` and
 * `"Shift+Ctrl+x"` being two strings for one chord is a bug that only shows up
 * on the one binding nobody tested.
 *
 * Shift is *not* included for a single-character key, and that is the rule
 * worth stating. `?` is Shift and `/` on this keyboard and something else
 * entirely on a German one, but `event.key` is `"?"` either way - the character
 * already carries the shift. Adding the modifier as well would mean binding
 * `Shift+?`, which is a chord no keyboard can produce. For a named key like
 * `ArrowRight` there is no character to carry it, so Shift is named.
 *
 * `Meta` is deliberately not read. Command on macOS is the platform's own
 * modifier and §43 asks for `Ctrl+S`; Tauri reports Command as `metaKey`, so a
 * macOS binding is a separate decision rather than a silent alias.
 */
export function chord(event: KeyboardEvent): string {
  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("Ctrl");
  }
  if (event.altKey) {
    parts.push("Alt");
  }

  const key = event.key === " " ? "space" : event.key;
  if (event.shiftKey && key.length > 1) {
    parts.push("Shift");
  }
  parts.push(key.length === 1 ? key.toLowerCase() : key);
  return parts.join("+");
}

/**
 * Whether a keystroke belongs to the field it landed in rather than to us.
 *
 * A person typing a track title must be able to type `r` and `s` and press
 * space, so an unmodified chord inside a text field is theirs. A modified one
 * is ours: `Ctrl+S` while typing a title should still commit, because that is
 * what `Ctrl+S` means everywhere and losing it inside a form is how a person
 * loses work.
 *
 * `Escape` is ours in every case, because it is how a person gets *out* of the
 * field.
 */
export function typing(event: KeyboardEvent): boolean {
  if (event.ctrlKey || event.altKey || event.key === "Escape") {
    return false;
  }
  const target = event.target;
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  if (target.isContentEditable) {
    return true;
  }
  const tag = target.tagName;
  if (tag === "TEXTAREA" || tag === "SELECT") {
    return true;
  }
  if (tag !== "INPUT") {
    return false;
  }
  // A checkbox, a radio or a button-like input has no text being typed into it,
  // so space and the letters are still ours. This is the difference between a
  // settings panel where `R` records and one where it does nothing because the
  // focus happens to be on a tickbox.
  const type = (target as HTMLInputElement).type;
  return type !== "checkbox" && type !== "radio" && type !== "button";
}

/** What a panel gives `useKeys`: an action to run, or nothing. */
export type Handlers = Partial<Record<Action, () => void>>;

/**
 * Binds the keyboard map for a scope.
 *
 * One `keydown` listener on the window, added once. The handlers are held in a
 * ref so that a panel re-rendering does not tear the listener down and put it
 * back - a gap between the two is a keystroke that does nothing, which is the
 * kind of intermittent fault nobody can reproduce.
 *
 * An action with no handler is not an error: `scope` decides which bindings are
 * live, and a global binding whose panel is not mounted simply has nobody to
 * run it. The help overlay shows the whole map regardless, because a person
 * wants to know what the application can do and not what is currently wired.
 */
export function useKeys(scope: Scope, handlers: Handlers): void {
  const held = useRef(handlers);
  held.current = handlers;

  const live = useRef(scope);
  live.current = scope;

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (typing(event)) {
        return;
      }
      const pressed = chord(event);
      for (const [action, binding] of inScope(live.current)) {
        if (!spellings(binding).includes(pressed)) {
          continue;
        }
        const run = held.current[action];
        if (run) {
          // Only once a handler is found, so a chord this scope does not
          // handle still reaches the browser: `Ctrl+R` reloading the window in
          // development is worth keeping, and swallowing every key that
          // happens to be in the map would take it away.
          event.preventDefault();
          run();
        }
        return;
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}

/** The whole map, grouped by scope, for the help overlay. */
export function grouped(): [Scope, [Action, Binding][]][] {
  const scopes: Scope[] = [
    "global",
    "browser",
    "capture",
    "tracks",
    "metadata",
    "export",
    "settings",
  ];
  return scopes
    .map((scope): [Scope, [Action, Binding][]] => [
      scope,
      entries().filter(([, binding]) => binding.scope === scope),
    ])
    .filter(([, bindings]) => bindings.length > 0);
}
