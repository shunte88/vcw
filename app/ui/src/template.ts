/*
 *  template.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The naming template's tokens, for the settings field that types one (§33).
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// The same list as `vcw_export::naming::TOKENS` and the same reading of a
// template as `naming::validate`, which is a duplication and is deliberate: the
// point of this copy is feedback while a person is still typing, and a round
// trip to the shell for every keystroke would be a command that exists only to
// answer a question whose whole answer is sixteen words long.
//
// It cannot drift. `crates/export/tests/tokens.rs` reads this file and fails if
// the two lists stop agreeing, so the duplication is checked by the gate rather
// than by remembering.
//
// What is NOT duplicated is the suggestion machinery - the alias table and the
// edit distance behind `naming::suggest`. An unknown token here is named and
// the supported ones are listed beside the field, which is the same help in the
// place where it can be acted on; the export's own refusal still says "did you
// mean", because by then there is no field to look at.

/** Every token a naming template may use, in `naming::TOKENS` order. */
export const TOKENS: readonly string[] = [
  "title",
  "artist",
  "album",
  "album_artist",
  "genre",
  "year",
  "tracknum",
  "composer",
  "country",
  "country_iso",
  "catalog",
  "label",
  "discogs_id",
  "side",
  "position",
  "disc",
];

/**
 * The tokens in `template` that do not exist, in the order they were written.
 *
 * An unclosed brace is not one: a template is typed a character at a time, and
 * `{tit` on the way to `{title}` is not a mistake worth reporting. Duplicates
 * are reported once, because a template that says `{titel}` twice has one typo
 * in it.
 */
export function unknownTokens(template: string): string[] {
  const found: string[] = [];
  let rest = template;
  for (;;) {
    const open = rest.indexOf("{");
    if (open === -1) {
      break;
    }
    rest = rest.slice(open + 1);
    const close = rest.indexOf("}");
    if (close === -1) {
      break;
    }
    const token = rest.slice(0, close);
    rest = rest.slice(close + 1);
    if (token !== "" && !TOKENS.includes(token) && !found.includes(token)) {
      found.push(token);
    }
  }
  return found;
}
