/*
 *  Metadata.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The two release facts no provider carries, stated more than once.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Mono and the cut characteristic are asked at project setup, and until this
// panel grew them that was the only place they could be answered: a person who
// ticked the wrong box, or who read the deadwax after the fact, had to reach
// for the CLI. Three things are asserted here, and each of them was broken at
// some point while it was being built.
//
// The first is that the switch and the picker write at all. The second is the
// three-state rule: a request that states one fact sends `null` for the other,
// because `false` would quietly turn a mono release stereo on the way past.
// The third is that "not stated" is an offered answer rather than a
// placeholder - a release that names nothing is played back as RIAA, which is
// not the same as an operator saying it was cut to RIAA, and clearing the
// statement is how somebody takes back a guess.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api";

import type { Curve, Release } from "../bindings/vcw";
import type { Store } from "../store";
import { Metadata } from "./Metadata";

vi.mock("../api", () => ({
  curves: vi.fn(async () => [
    { id: "riaa", name: "RIAA", era: "1954 on", labels: "" },
    { id: "columbia_lp", name: "Columbia LP", era: "1948-1954", labels: "" },
  ] as Curve[]),
  editRelease: vi.fn(async () => undefined),
  searchMetadata: vi.fn(async () => []),
  selectRelease: vi.fn(async () => undefined),
}));

function store(release: Release | null): Store {
  return {
    engine: { negotiated: null, verified: false, divergences: [] },
    project: { path: "/data2/rips/side-a.vcw", release },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
    reload: () => {},
    open: async () => undefined,
  } as unknown as Store;
}

function release(extra: Partial<Release> = {}): Release {
  return {
    album: "Kind of Blue",
    albumArtist: "Miles Davis",
    year: 1959,
    genres: [],
    label: "Columbia",
    catalog: "CL 1355",
    country: "US",
    barcode: null,
    composer: "",
    comments: "",
    discs: 1,
    numbering: "alpha",
    musicbrainzId: null,
    discogsId: null,
    confirmed: false,
    isMono: false,
    playbackEq: null,
    ...extra,
  } as unknown as Release;
}

async function render(held: Release | null): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<Metadata store={store(held)} />);
  });
  return container;
}

/** The panel's curve picker, which is the only select outside the query form. */
function curve(container: HTMLElement): HTMLSelectElement {
  const select = container.querySelector<HTMLSelectElement>(".pressing select");
  expect(select, "the panel offers a characteristic").toBeTruthy();
  return select as HTMLSelectElement;
}

describe("the two facts a provider cannot supply", () => {
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.mocked(api.editRelease).mockClear();
  });

  it("states mono without touching the characteristic", async () => {
    const container = await render(release({ playbackEq: "columbia_lp" }));
    const box = container.querySelector<HTMLInputElement>(
      ".pressing input[type=checkbox]",
    );
    expect(box, "the panel offers a channel switch").toBeTruthy();
    await act(async () => {
      box?.click();
    });
    expect(vi.mocked(api.editRelease)).toHaveBeenCalledWith({
      isMono: true,
      playbackEq: null,
    });
  });

  it("states the characteristic without touching mono", async () => {
    const container = await render(release({ isMono: true }));
    const select = curve(container);
    await act(async () => {
      select.value = "columbia_lp";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(vi.mocked(api.editRelease)).toHaveBeenCalledWith({
      isMono: null,
      playbackEq: "columbia_lp",
    });
  });

  it("offers taking the statement back, which is not the same as saying RIAA", async () => {
    const container = await render(release({ playbackEq: "riaa" }));
    const select = curve(container);
    const unstated = select.querySelector<HTMLOptionElement>('option[value=""]');
    expect(unstated, "nothing-stated is an answer the picker offers").toBeTruthy();
    await act(async () => {
      select.value = "";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(vi.mocked(api.editRelease)).toHaveBeenCalledWith({
      isMono: null,
      playbackEq: "",
    });
  });

  it("answers for a project nobody has identified yet", async () => {
    // No release row at all, which is every project until a lookup or a seed
    // writes one. The write creates it; the panel must not go dead first.
    const container = await render(null);
    expect(curve(container).value).toBe("");
    const box = container.querySelector<HTMLInputElement>(
      ".pressing input[type=checkbox]",
    );
    await act(async () => {
      box?.click();
    });
    expect(vi.mocked(api.editRelease)).toHaveBeenCalledWith({
      isMono: true,
      playbackEq: null,
    });
  });
});
