/*
 *  main.tsx
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  Mounts the React tree. Nothing else belongs here.
 *
 *  MIT License - see the header in any Rust source file for the full text.
 */

import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "./App";
import "./app.css";
import { applyScale, scaleOf } from "./scale";

// Before the first paint, so a window at 175% does not start at 13px and
// jump. It is one inline style on <html> and the stylesheet does the rest.
applyScale(scaleOf());

const root = document.getElementById("root");
if (!root) {
  throw new Error("index.html has no #root to mount on");
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
