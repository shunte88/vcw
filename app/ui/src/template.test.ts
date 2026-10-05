/*
 *  template.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  That the template field names a typo and not a half-typed token.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import { describe, expect, it } from "vitest";

import { TOKENS, unknownTokens } from "./template";

describe("the naming template's tokens", () => {
  it("passes the default template and every token in the list", () => {
    expect(
      unknownTokens("{album_artist}/{album}/{tracknum} - {title}"),
    ).toEqual([]);
    const all = TOKENS.map((token) => `{${token}}`).join("");
    expect(unknownTokens(all)).toEqual([]);
  });

  it("names an unknown token once, however often it is written", () => {
    expect(unknownTokens("{titel} - {titel} [{year}]")).toEqual(["titel"]);
  });

  it("says nothing about a token still being typed", () => {
    // The field validates on every keystroke, so `{tit` is a state every
    // template passes through on its way to being right.
    expect(unknownTokens("{album}/{tit")).toEqual([]);
    expect(unknownTokens("{}")).toEqual([]);
  });
});
