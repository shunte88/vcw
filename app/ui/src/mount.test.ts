/*
 *  mount.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The cleanup nobody sees run, asserted from the test after it.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// A leaked root is invisible from inside the test that leaked it: everything
// passes, and the file fails later for a reason that names React rather than
// the test. So the claim is made from the next test along, which is the first
// moment the teardown can be observed at all.

import { createElement, useEffect } from "react";
import { describe, expect, it } from "vitest";

import { mount } from "./mount";

/** Records its own unmount, which is the half a removed container hides. */
let torn = false;
function Probe(): null {
  useEffect(() => () => {
    torn = true;
  }, []);
  return null;
}

describe("a mounted component", () => {
  it("is in the document while the test is running", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const container = await mount(createElement(Probe));
    expect(document.body.contains(container)).toBe(true);
    expect(torn).toBe(false);
  });

  it("is unmounted and out of it by the next one", () => {
    expect(torn, "the root was never unmounted").toBe(true);
    expect(document.body.children.length, "a container was left behind").toBe(0);
  });
});
