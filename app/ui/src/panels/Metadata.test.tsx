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

import type { Candidate, Curve, Identified, Release } from "../bindings/vcw";
import type { Store } from "../store";
import { Metadata } from "./Metadata";

vi.mock("../api", () => ({
  curves: vi.fn(async () => [
    { id: "riaa", name: "RIAA", era: "1954 on", labels: "" },
    { id: "columbia_lp", name: "Columbia LP", era: "1948-1954", labels: "" },
  ] as Curve[]),
  editRelease: vi.fn(async () => undefined),
  identify: vi.fn(async () => ({
    resolved: false,
    candidates: [],
    because: "nothing",
    lookups: 3,
    refusals: [],
  })),
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
    item: "",
    condition: "",
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

/**
 * Types into a controlled box the way a keystroke does.
 *
 * React holds the last value it rendered and a direct assignment updates that
 * record on the way past, so the event arrives and React decides nothing
 * changed. Going through the prototype's setter leaves the record alone.
 */
function type(node: HTMLInputElement, word: string): void {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
    node,
    word,
  );
  node.dispatchEvent(new Event("input", { bubbles: true }));
}

/** The item and condition boxes, in the order the fieldset asks for them. */
function copy(container: HTMLElement): HTMLInputElement[] {
  const boxes = Array.from(
    container.querySelectorAll<HTMLInputElement>(
      ".pressing input:not([type=checkbox])",
    ),
  );
  expect(boxes.length, "the panel asks for the item and the condition").toBe(2);
  return boxes;
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
      item: null,
      condition: null,
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
      item: null,
      condition: null,
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
      item: null,
      condition: null,
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
      item: null,
      condition: null,
    });
  });

  // WP-31. These two are free text, so unlike the switch and the picker they
  // cannot be written on every keystroke: a reload landing mid-word over
  // `vcw serve` would eat the rest of it. They are held and committed when the
  // field is left, and a field that was only looked at commits nothing - a
  // blur that always wrote would make tabbing through the panel a save storm.
  it("holds the copy's facts until the field is left", async () => {
    const container = await render(release());
    const [item] = copy(container);
    await act(async () => {
      type(item as HTMLInputElement, "SH-LP-0412");
    });
    expect(
      vi.mocked(api.editRelease),
      "typing is not stating",
    ).not.toHaveBeenCalled();
    await act(async () => {
      item?.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    expect(vi.mocked(api.editRelease)).toHaveBeenCalledWith({
      isMono: null,
      playbackEq: null,
      item: "SH-LP-0412",
      condition: null,
    });
  });

  it("states nothing for a field that was only looked at", async () => {
    const container = await render(release({ condition: "VG+" }));
    const [, condition] = copy(container);
    expect(condition?.value, "the stored answer is in the box").toBe("VG+");
    await act(async () => {
      condition?.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    expect(vi.mocked(api.editRelease)).not.toHaveBeenCalled();
  });
});

/** The panel's Identify button, which is the second in the head. */
function identifyButton(container: HTMLElement): HTMLButtonElement {
  const buttons = [
    ...container.querySelectorAll<HTMLButtonElement>(".panel-head button"),
  ];
  const found = buttons.find((one) => one.textContent?.startsWith("Identify"));
  expect(found, "the panel offers an automatic lookup").toBeTruthy();
  return found as HTMLButtonElement;
}

function pressing(id: string, country: string): Candidate {
  return {
    provider: "discogs",
    id,
    album: "Vienna",
    artist: "Ultravox",
    year: 1980,
    label: "Chrysalis",
    catalog: "CHR 1296",
    country,
    format: "Vinyl, LP, Album",
    tracks: 9,
  };
}

// The walk is the one lookup a person does not type a query for, so the panel
// has to account for it: what it concluded, and what it cost. A shortlist with
// no sentence over it is indistinguishable from a search that went badly.
describe("the automatic lookup", () => {
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.mocked(api.identify).mockClear();
    // jsdom has no layout, so it has no `scrollIntoView`. The panel scrolls
    // the selected row into view and these are the first tests to render one.
    HTMLElement.prototype.scrollIntoView = () => {};
  });

  it("fills the same table a search does, and says why it is asking", async () => {
    const answer: Identified = {
      resolved: false,
      candidates: [pressing("1865584", "Spain"), pressing("1107608", "Israel")],
      because: "tooClose",
      lookups: 1,
      refusals: [],
    };
    vi.mocked(api.identify).mockResolvedValueOnce(answer);
    const container = await render(release());
    await act(async () => {
      identifyButton(container).click();
    });
    expect(vi.mocked(api.identify)).toHaveBeenCalledWith({
      provider: null,
      listen: false,
    });
    const rows = container.querySelectorAll("table.rows tbody tr");
    expect(rows.length, "both pressings are offered").toBe(2);
    const verdict = container.querySelector(".verdict")?.textContent ?? "";
    expect(verdict).toContain("too close to separate");
    // The headline, because every lookup is a request to somebody else.
    expect(verdict).toContain("1 lookup.");
  });

  // A walk that found nothing is not a walk that failed, and a token that
  // expired is not a record no database has. Both used to read the same.
  it("separates nothing-found from a lookup that could not be made", async () => {
    vi.mocked(api.identify).mockResolvedValueOnce({
      resolved: false,
      candidates: [],
      because: "nothing",
      lookups: 2,
      refusals: ["discogs-catalog: no token"],
    });
    const container = await render(release());
    await act(async () => {
      identifyButton(container).click();
    });
    const verdict = container.querySelector(".verdict")?.textContent ?? "";
    expect(verdict).toContain("Nothing the project's own facts support");
    expect(verdict).toContain("discogs-catalog: no token");
  });

  // Nothing found and nothing asked read the same before this, and only one of
  // them is a statement about the record.
  it("does not call a walk it never made an answer", async () => {
    vi.mocked(api.identify).mockResolvedValueOnce({
      resolved: false,
      candidates: [],
      because: "nothing",
      lookups: 0,
      refusals: [],
    });
    const container = await render(release());
    await act(async () => {
      identifyButton(container).click();
    });
    const verdict = container.querySelector(".verdict")?.textContent ?? "";
    expect(verdict).toContain("No lookup could be made");
  });

  it("offers a certain answer rather than writing it", async () => {
    vi.mocked(api.identify).mockResolvedValueOnce({
      resolved: true,
      candidates: [pressing("1865584", "Spain")],
      because: null,
      lookups: 1,
      refusals: [],
    });
    const container = await render(release());
    await act(async () => {
      identifyButton(container).click();
    });
    expect(vi.mocked(api.selectRelease), "nothing is written by a walk")
      .not.toHaveBeenCalled();
    const verdict = container.querySelector(".verdict")?.textContent ?? "";
    expect(verdict).toContain("Accept it to write it in");
  });
});
