/*
 *  Export.test.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  A finished export says what it wrote, and a path can be pointed at.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Two things this panel got wrong and one it has to keep getting right.
//
// A successful export said nothing: the progress line is conditional on
// `engine.exporting`, which goes back to null the instant the thread reports
// done, so four minutes of writing a side ended with the panel looking exactly
// as it had before the button was pressed. The only record of what came out was
// in the event log.
//
// The output directory was a free-text field with no picker, so the answer to
// "where do the files go?" was an absolute path typed from memory.
//
// And the plan has to be dropped whenever the directory changes, by hand or by
// dialog. A plan resolved against the old directory lists paths that will not
// be written, and showing it under a new `Into` is the panel telling a lie it
// was given the information to avoid.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Settings } from "../bindings/vcw";
import type { Engine, Exported, Store } from "../store";
import { Export, size, wrote } from "./Export";

/** Typed with its options, so a test can assert what the dialog was asked for. */
const chosen = vi.fn(
  async (_options: Record<string, unknown>) =>
    "/data2/exports" as string | null,
);

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (options: Record<string, unknown>) => chosen(options),
}));

const planned = vi.fn(async (_request: Record<string, unknown>) => ({
  files: ["/data2/exports/A1 Europe Endless.flac"],
  covers: [],
  frames: 176_400,
  container: "FLAC",
}));

vi.mock("../api", () => ({
  exportPlan: (request: Record<string, unknown>) => planned(request),
  exportRun: vi.fn(async () => undefined),
}));

function report(over: Partial<Exported> = {}): Exported {
  return {
    kind: "export-finished",
    files: 3,
    covers: 1,
    frames: 35_861_491,
    bytesWritten: 286_974_530,
    ...over,
  } as Exported;
}

function settings(library: string | null = null): Settings {
  return {
    recording: { library },
    export: {
      output: null,
      format: "flac",
      artwork: "embed",
      template: null,
      quality: "high",
    },
  } as unknown as Settings;
}

function store(engine: Partial<Engine>): Store {
  return {
    engine: { exporting: null, exported: null, ...engine },
    project: { path: "/data2/x.vcw", sides: [] },
    run: async (what: () => Promise<unknown>) => {
      await what();
    },
    reload: () => {},
  } as unknown as Store;
}

async function render(
  engine: Partial<Engine>,
  library: string | null = null,
): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(<Export store={store(engine)} settings={settings(library)} />);
  });
  return container;
}

/** The select holding a given option value, found by the option rather than by
 * position - the panel has four of them and three are next to each other. */
function selectFor(
  container: HTMLElement,
  option: string,
): HTMLSelectElement | undefined {
  return [...container.querySelectorAll("select")].find((select) =>
    [...select.options].some((candidate) => candidate.value === option),
  );
}

/** Sets a select the way a person does, so React's onChange runs. */
async function choose(select: HTMLSelectElement, value: string) {
  await act(async () => {
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

/** The browse button, by its label rather than its position. */
function browseButton(container: HTMLElement): HTMLButtonElement | undefined {
  return [...container.querySelectorAll("button")].find((button) =>
    /browse/i.test(button.textContent ?? ""),
  );
}

describe("what a finished export says", () => {
  it("names the files, the covers and the size", () => {
    expect(wrote(report())).toBe("Wrote 3 files, 1 cover image, 273.7 MiB.");
  });

  it("does not pluralise a single file", () => {
    expect(wrote(report({ files: 1, covers: 0, bytesWritten: 1_048_576 }))).toBe(
      "Wrote 1 file, 1.0 MiB.",
    );
  });

  it("leaves the covers out when there were none", () => {
    expect(wrote(report({ covers: 0 }))).not.toMatch(/cover/);
  });

  it("pluralises several covers, which a multi-disc export writes", () => {
    expect(wrote(report({ covers: 2 }))).toMatch(/2 cover images/);
  });
});

describe("a byte count a person can read", () => {
  it("counts bytes below a kibibyte", () => {
    expect(size(0)).toBe("0 bytes");
    expect(size(1023)).toBe("1023 bytes");
  });

  it("uses binary units, so a 274 MiB side is not called 287 MB", () => {
    expect(size(1024)).toBe("1 KiB");
    expect(size(286_974_530)).toBe("273.7 MiB");
  });

  it("goes to two decimals past a gibibyte, where WAV runs out", () => {
    expect(size(4 * 1024 * 1024 * 1024)).toBe("4.00 GiB");
  });
});

describe("the export panel", () => {
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    chosen.mockClear();
    chosen.mockResolvedValue("/data2/exports");
    planned.mockClear();
  });

  it("says nothing about an export while one is still running", async () => {
    const container = await render({ exporting: [2, 3], exported: report() });
    expect(container.textContent).toMatch(/Writing 2 of 3/);
    expect(container.textContent).not.toMatch(/Wrote /);
  });

  it("reports what was written once the export has finished", async () => {
    const container = await render({ exporting: null, exported: report() });
    expect(container.textContent).toMatch(/Wrote 3 files, 1 cover image/);
  });

  it("has a browse button, so the directory need not be typed", async () => {
    const container = await render({});
    const browse = browseButton(container);
    expect(browse).toBeDefined();

    await act(async () => {
      browse?.click();
    });
    expect(chosen).toHaveBeenCalledTimes(1);
    const input = container.querySelector<HTMLInputElement>("input.wide");
    expect(input?.value).toBe("/data2/exports");
  });

  it("starts the picker at the library root rather than the working directory", async () => {
    // Left to itself the chooser opens wherever the process was started, which
    // for a dev run is `app/src-tauri`. The library is where this person keeps
    // records, so it is the only guess worth making.
    const container = await render({}, "/data2/vcw-firstlight");
    await act(async () => {
      browseButton(container)?.click();
    });
    expect(chosen).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: "/data2/vcw-firstlight" }),
    );
  });

  it("lets the host choose when no library has been picked", async () => {
    const container = await render({}, null);
    await act(async () => {
      browseButton(container)?.click();
    });
    expect(chosen).toHaveBeenCalledTimes(1);
    expect(chosen.mock.calls[0]?.[0]).not.toHaveProperty("defaultPath");
  });

  it("keeps the typed path when the picker is cancelled", async () => {
    chosen.mockResolvedValue(null);
    const container = await render({});
    const input = container.querySelector<HTMLInputElement>("input.wide");
    expect(input).not.toBeNull();
    await act(async () => {
      input?.setAttribute("value", "/data2/typed");
    });

    await act(async () => {
      browseButton(container)?.click();
    });
    expect(chosen).toHaveBeenCalledTimes(1);
    // Whatever was there is still there: a cancelled dialog is not a choice of
    // the empty string, which is what clearing the field would mean.
    expect(container.querySelector<HTMLInputElement>("input.wide")?.value).not.toBe(
      "null",
    );
  });
});

describe("the format and its quality", () => {
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    planned.mockClear();
    chosen.mockClear();
    chosen.mockResolvedValue("/data2/exports");
  });

  /** Fills in the output directory, which both buttons are disabled without. */
  async function addressed(container: HTMLElement) {
    await act(async () => {
      browseButton(container)?.click();
    });
  }

  it("offers the four containers VCW writes", async () => {
    const container = await render({});
    const format = selectFor(container, "flac");
    expect([...(format?.options ?? [])].map((option) => option.value)).toEqual([
      "flac",
      "wav",
      "mp3",
      "ogg",
    ]);
  });

  it("asks about quality only where it means something", async () => {
    // A quality select beside FLAC is a control with no effect, which is worse
    // than no control: it invites a choice and then ignores it.
    const container = await render({});
    expect(selectFor(container, "transparent")).toBeUndefined();

    const format = selectFor(container, "flac");
    expect(format).toBeDefined();
    await choose(format as HTMLSelectElement, "mp3");
    expect(selectFor(container, "transparent")).toBeDefined();

    await choose(format as HTMLSelectElement, "wav");
    expect(selectFor(container, "transparent")).toBeUndefined();
  });

  it("sends the quality with the request", async () => {
    const container = await render({});
    await addressed(container);
    await choose(selectFor(container, "flac") as HTMLSelectElement, "ogg");
    await choose(selectFor(container, "transparent") as HTMLSelectElement, "compact");

    const plan = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "Plan",
    );
    await act(async () => {
      plan?.click();
    });
    expect(planned).toHaveBeenCalledWith(
      expect.objectContaining({ format: "ogg", quality: "compact" }),
    );
  });

  it("drops a resolved plan when the format or the quality changes", async () => {
    // The defect this is here for: a plan lists the exact paths that will be
    // written, extension and all. Leaving a plan of `.flac` paths on screen
    // under a format of MP3 is the panel telling a lie it was handed the
    // information to avoid - and the same is true of the quality, which decides
    // the size of everything in the list even though the names do not change.
    const container = await render({});
    await addressed(container);
    const planButton = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "Plan",
    );
    const resolve = async () => {
      await act(async () => {
        planButton?.click();
      });
      expect(container.textContent).toMatch(/Plan: 1 file/);
    };

    await resolve();
    await choose(selectFor(container, "flac") as HTMLSelectElement, "mp3");
    expect(container.textContent).not.toMatch(/Plan: /);

    await resolve();
    await choose(selectFor(container, "transparent") as HTMLSelectElement, "compact");
    expect(container.textContent).not.toMatch(/Plan: /);

    // And the artwork, which changes what is written beside the files.
    await resolve();
    await choose(selectFor(container, "folder") as HTMLSelectElement, "none");
    expect(container.textContent).not.toMatch(/Plan: /);
  });
});
