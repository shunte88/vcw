import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The dev server's port is in `app/src-tauri/tauri.conf.json` as `devUrl`, and
// `strictPort` is why: a port that quietly moved to 5174 would leave the window
// pointed at nothing, which is the same blank-window failure the
// `custom-protocol` feature guards against from the other side.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  // `src/*.test.ts` runs under jsdom, because the two things worth testing on
  // this side of the boundary both touch the DOM: which chord a `KeyboardEvent`
  // names, and whether a keystroke belonged to the field it landed in. Neither
  // can be checked without `HTMLInputElement` existing.
  //
  // There is not much else to test here, and that is the point - §2 leaves the
  // frontend nothing to compute, so what is left is the keyboard map, which is
  // WP-16's exit criterion.
  test: {
    environment: "jsdom",
    // `.tsx` as well since WP-19: the project browser earned a rendered test
    // when a project created in the window turned out to be missing from the
    // list until a restart, and that is not a question source text can answer.
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // Tauri ships its own WebView, so there is no old browser to support and
    // no reason to down-level. Safari 16 is the floor WebKitGTK and WKWebView
    // agree on.
    target: "safari16",
    sourcemap: true,
  },
});
