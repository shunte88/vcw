/*
 *  mount.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Renders a component into the document for one test, and takes it down again.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// A root nobody unmounts outlives the test that made it: its effects, its
// timers and its state queue are all still live when Vitest tears jsdom down
// at the end of the file, and whatever was queued then runs against a window
// that no longer exists. Every test in the file passes and the file fails,
// with `window is not defined` and a stack in React. That is what cost the
// `v0.3.5-alpha` tag, and eight files were still mounting roots they never
// took down.
//
// The cleanup is registered here rather than written into each file, because
// one a caller has to remember is one some caller will not - which is the
// state this replaced. `onTestFinished` rather than a module-level
// `afterEach`: the hook belongs to the test that is running when `mount` is
// called, so it does not depend on this module being evaluated once per file.

import { act } from "react";
import type { ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { onTestFinished } from "vitest";

/** Renders `node` into a fresh container and returns it. */
export async function mount(node: ReactNode): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  onTestFinished(async () => {
    await act(async () => {
      root.unmount();
    });
    container.remove();
  });
  await act(async () => {
    root.render(node);
  });
  return container;
}
