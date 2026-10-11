/*
 *  i18n.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Substitution, fallback and plurals. Whether a key exists is checked in
 *  `crates/i18n/tests/callers.rs`, which can see both languages' call sites.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import { describe, expect, it } from "vitest";

import { count, substitute, t } from "./i18n";

describe("substitute", () => {
  it("puts an argument where its name is", () => {
    expect(substitute("Wrote {files}, {size}.", { files: "3 files", size: "1 KiB" })).toBe(
      "Wrote 3 files, 1 KiB.",
    );
  });

  it("formats a number without the caller doing it", () => {
    expect(substitute("{n} file", { n: 3 })).toBe("3 file");
  });

  it("leaves a name it was given no argument for", () => {
    // Legible half-application: this says both what worked and what did not,
    // where blanking the name would only say that something is missing.
    expect(substitute("{count} of {total}", { count: 3 })).toBe("3 of {total}");
  });

  it("leaves a string alone when there are no arguments", () => {
    expect(substitute("Export")).toBe("Export");
  });
});

describe("t", () => {
  it("reads the compiled-in English with no backend", () => {
    expect(t("export.title")).toBe("Export");
  });

  it("renders a missing key as itself", () => {
    // Deliberate, and the same rule `vcw_i18n::t` applies. A panel showing
    // `export.nothing.here` is a bug report that writes itself.
    expect(t("export.nothing.here")).toBe("export.nothing.here"); // i18n-exempt
  });

  it("substitutes into a catalog string", () => {
    expect(t("export.run", { chord: "Ctrl+E" })).toBe("Export (Ctrl+E)");
  });
});

describe("count", () => {
  it("takes the singular at one and the plural otherwise", () => {
    expect(count(1, "export.wrote.files")).toBe("1 file");
    expect(count(2, "export.wrote.files")).toBe("2 files");
    expect(count(0, "export.wrote.files")).toBe("0 files");
  });
});
