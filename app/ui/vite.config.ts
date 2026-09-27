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
