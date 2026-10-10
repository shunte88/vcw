/*
 *  host.test.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  What an HTTP answer means, which is not always what a `??` would make of it.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// Three answers a command can give over §52's transport, and the difference
// between two of them is the whole file. `null` is a real answer from
// `open_path`, `library_root` and `release`; nothing at all is what a
// `Result<(), _>` sends. Collapsing the first into the second put `no project
// is open` in the status bar of a window nobody had touched - see the comment
// in `host.ts` - and it only happened under `serve`, because the desktop
// shell's `invoke` hands the `null` straight back.

import { afterEach, describe, expect, it, vi } from "vitest";

import { invoke } from "./host";

function answers(body: string, status = 200): void {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () =>
      Promise.resolve({
        ok: status < 400,
        status,
        text: async () => Promise.resolve(body),
      }),
    ),
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("an answer over HTTP", () => {
  it("keeps a null that the command meant", async () => {
    answers("null");
    await expect(invoke("open_path")).resolves.toBeNull();
  });

  it("reads an empty body as nothing at all", async () => {
    answers("");
    await expect(invoke("poll")).resolves.toBeUndefined();
  });

  it("hands back a value", async () => {
    answers('{"version":"0.2.2"}');
    await expect(invoke("about")).resolves.toEqual({ version: "0.2.2" });
  });

  it("rejects with the Failure the host sent", async () => {
    answers('{"code":"no-project","message":"no project is open","field":null}', 422);
    await expect(invoke("tracks")).rejects.toMatchObject({
      code: "no-project",
    });
  });

  it("rejects with the status when the body is not a Failure", async () => {
    answers("<html>502 Bad Gateway</html>", 502);
    await expect(invoke("tracks")).rejects.toMatchObject({
      code: "transport",
      message: "the VCW host answered 502",
    });
  });
});
