/*
 *  vite-env.d.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The bundler's ambient types, which are the only non-browser ones we take.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

// One reference, and the reason it is a file of its own rather than a `types`
// entry in `tsconfig.json`: naming `types` there would replace the implicit
// "every `@types` package" with a list, and the next dependency that ships
// ambient types would go missing for no stated reason.
//
// What this buys is `import.meta.glob`, used by `wiring.test.ts` to read the
// components' own source. Nothing that ships uses it. The frontend still has no
// Node types, which is deliberate: `lib` is `ES2022` and `DOM`, because
// everything here runs in a webview.

/// <reference types="vite/client" />
