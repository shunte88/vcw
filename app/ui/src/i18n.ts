/*
 *  i18n.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  One string, in whatever language the settings ask for - the window's half.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The Rust twin of this file is `crates/i18n`, and the two agree on three
// things: a key is a dotted lower-case path, a missing key renders as itself,
// and a placeholder is `{name}`.
//
// Rendering a missing key as the key is deliberate and is not a silent
// failure. A panel showing `export.format` where a label should be is a bug
// report that writes itself; an empty string is a layout that merely looks a
// bit odd, and nobody files that.
//
// The English is compiled in. `bindings/strings.ts` is generated from
// `i18n/en-US.toml` by `cargo test -p vcw-i18n --test frontend_catalog`, so
// the first paint is English before the shell has answered anything and a
// rendered test needs no backend to say "Export". A translation can only ever
// arrive at run time - it is a file a person drops beside their settings long
// after this bundle was built - so `load` fetches it and overlays it per key.

import { useSyncExternalStore } from "react";

import * as api from "./api";
import { SOURCE } from "./bindings/strings";

/** What a placeholder may be given. Numbers are formatted by the caller. */
export type Args = Readonly<Record<string, string | number>>;

/** A key, and whatever its placeholders need, to a sentence. */
export type Translate = (key: string, args?: Args) => string;

/**
 * Substitutes `{name}` for each argument, and leaves the rest alone.
 *
 * Leaving an unknown name alone rather than blanking it keeps a half-applied
 * string legible: `{count} of {total}` with only `count` given reads as
 * "3 of {total}", which says both what worked and what did not. A catalog
 * entry cannot contain a brace that is not a placeholder - the generator
 * refuses one, in `crates/i18n/tests/frontend_catalog.rs` - so there is
 * nothing here to escape.
 */
export function substitute(text: string, args?: Args): string {
  if (!args) {
    return text;
  }
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in args ? String(args[name]) : whole,
  );
}

/**
 * Marks a string as a catalog key.
 *
 * An identity function, and it earns its keep in one place: a key held as
 * *data* rather than written at the call site - `keymap.ts` gives every
 * binding the key for its help label, and `Help.tsx` gives every scope the key
 * for its heading. Those strings are never inside a `t(...)`, so the coverage
 * test in `crates/i18n/tests/callers.rs` would call them orphans and a
 * translator would be asked to translate nothing. Wrapping them is how the key
 * stays visible to a reader that only knows how to see a call.
 */
export function key(name: string): string {
  return name;
}

/** The English, with any active translation laid over it key by key. */
let texts: Readonly<Record<string, string>> = SOURCE;

/**
 * The current translate function.
 *
 * A value rather than a closure over `texts`, because `useSyncExternalStore`
 * compares snapshots by identity: a new function here is what tells React the
 * language changed, and a stable one is what stops it re-rendering forever.
 */
let translate: Translate = make(texts);

const listeners = new Set<() => void>();

function make(from: Readonly<Record<string, string>>): Translate {
  return (key, args) => substitute(from[key] ?? key, args);
}

/**
 * Asks the shell for the active language and overlays it on the English.
 *
 * Call at startup and again after the language setting changes. A key the
 * translation does not carry keeps its English rather than going blank, which
 * is the same rule `vcw_i18n::strings` applies on the other side - applied
 * twice, because a catalog served by an older shell than this bundle could be
 * missing a key this bundle knows about.
 *
 * A failure here is not fatal and is not thrown: the window stays in the
 * language it was built in, which is a working window. It is logged, because
 * a language that was chosen and did not arrive is worth a line in the
 * console.
 */
export async function load(): Promise<void> {
  try {
    const served = await api.strings();
    texts = { ...SOURCE, ...served };
  } catch (error) {
    console.error("the language could not be read; staying in English", error);
    texts = SOURCE;
  }
  translate = make(texts);
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Translates outside a component.
 *
 * Reads the catalog live, so a module that holds onto this function still
 * follows a language change. Inside a component use {@link useText}, which
 * also re-renders.
 */
export function t(key: string, args?: Args): string {
  return translate(key, args);
}

/**
 * A string that reads differently for one than for several.
 *
 * `count(1, "export.wrote.files")` reads `export.wrote.files.one`, and any
 * other number reads `.other`. `n` is passed on as an argument, so the text
 * is `"{n} file"` and `"{n} files"` rather than a bare noun this would have
 * to glue a number onto - gluing is where word order goes wrong.
 *
 * ponytail: two forms, which is English and most of western Europe and is
 * wrong for Polish, Russian and Arabic, which have three to six. The upgrade
 * is `Intl.PluralRules` and a key per category it names; it is not here
 * because it would be machinery with no translation to exercise it, and the
 * key shape above is the one `Intl.PluralRules` uses, so the change is
 * additive when a translator asks for it.
 */
export function count(n: number, key: string, args?: Args): string {
  return t(`${key}.${n === 1 ? "one" : "other"}`, { n, ...args });
}

function listen(onChange: () => void): () => void {
  listeners.add(onChange);
  return () => {
    listeners.delete(onChange);
  };
}

function snapshot(): Translate {
  return translate;
}

/**
 * The translate function, and a re-render when the language changes.
 *
 * ```tsx
 * const text = useText();
 * return <button>{text("export.run")}</button>;
 * ```
 */
export function useText(): Translate {
  return useSyncExternalStore(listen, snapshot, snapshot);
}

/**
 * Puts the catalog back to the compiled-in English.
 *
 * For tests. A rendered test that calls {@link load} against a stubbed `api`
 * would otherwise leave its catalog behind for whatever runs next in the same
 * worker.
 */
export function reset(): void {
  texts = SOURCE;
  translate = make(texts);
  for (const listener of listeners) {
    listener();
  }
}
