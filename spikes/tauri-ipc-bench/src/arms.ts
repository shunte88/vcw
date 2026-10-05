/*
 *  arms.ts
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  The matrix, written out as a named list rather than generated from a cross
 *
 * MIT License
 *
 * Copyright (c) 2026 Stue Hunter
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 */

// The matrix, written out as a named list rather than generated from a cross
// product. Ten arms, each isolating one variable against the same baseline, is
// reviewable; a 5x4x2 cross product is 40 runs of which most vary two things at
// once and answer nothing.

import type { BenchConfig, RenderMode } from './bench'

export interface Arm {
  name: string
  /** What this arm is here to decide. */
  question: string
  cfg: Partial<BenchConfig>
  renderMode?: RenderMode
}

/** 192 kHz with 256-frame callbacks is the worst case §35 has to survive:
 * 750 audio callbacks a second. */
export const BASE: BenchConfig = {
  transport: 'channel',
  encoding: 'manual',
  meterHz: 60,
  waveHz: 30,
  positionHz: 10,
  coalesce: true,
  captureRate: 192000,
  callbackFrames: 256,
  bucketFrames: 1024,
  durationSecs: 20,
}

export const ARMS: Arm[] = [
  {
    name: 'control-raf',
    question:
      'Neither traffic nor drawing: pins the bare requestAnimationFrame rate, which turns out not to be vsync.',
    cfg: { meterHz: 0.001, waveHz: 0.001, positionHz: 0.001 },
    renderMode: 'none',
  },
  {
    name: 'control-idle',
    question: 'Frame pacing with no traffic: the compositor and canvas baseline every other arm is read against.',
    // 0.001 Hz means a period longer than any run here, so nothing is ever
    // sent. An earlier version used 1 Hz, which at 192 kHz batched 187 buckets
    // into one 4.6 kB waveform message per second - not an idle control at all.
    cfg: { meterHz: 0.001, waveHz: 0.001, positionHz: 0.001 },
  },
  {
    name: 'control-nodraw',
    question: 'Full traffic, nothing drawn: separates transport cost from canvas cost.',
    cfg: {},
    renderMode: 'none',
  },
  {
    name: 'channel-serde',
    question: 'Baseline: D6 without the "pre-serialized" clause.',
    cfg: { transport: 'channel', encoding: 'serde' },
  },
  {
    name: 'channel-manual',
    question: 'D6 as written.',
    cfg: { transport: 'channel', encoding: 'manual' },
  },
  {
    name: 'channel-raw',
    question: 'Does binary beat JSON, given tauri inflates sub-1 KiB Raw to a decimal array?',
    cfg: { transport: 'channel', encoding: 'raw' },
  },
  {
    name: 'event-serde',
    question: 'D6 says "not the event bus". Cost of being wrong about that.',
    cfg: { transport: 'event', encoding: 'serde' },
  },
  {
    name: 'event-manual',
    question: 'Separates the transport from the encoding in the event-bus arm.',
    cfg: { transport: 'event', encoding: 'manual' },
  },
  {
    name: 'render-naive',
    question: 'Full waveform redraw every frame - the cost D6 does not mention.',
    cfg: {},
    renderMode: 'naive',
  },
  {
    name: 'render-worker',
    question: 'Does the waveform need OffscreenCanvas in a worker?',
    cfg: {},
    renderMode: 'worker',
  },
  {
    name: 'render-react-dom',
    question: 'Cost of driving the meter through React state instead of canvas.',
    cfg: {},
    renderMode: 'react-dom',
  },
  {
    name: 'uncoalesced',
    question: 'Is "coalesce to <=60 Hz in Rust" load-bearing, at 750 Hz?',
    cfg: { coalesce: false },
  },
  {
    name: 'uncoalesced-raw',
    question: 'If coalescing is skipped, does binary framing rescue it?',
    cfg: { coalesce: false, encoding: 'raw' },
  },
]

/**
 * The soak. Not part of the matrix: it answers a different question - whether
 * the configuration the matrix recommends still holds after half an hour,
 * which is the shortest run that resembles one side of a record.
 *
 * Deliberately the *recommended* configuration rather than the most stressful
 * one. A soak of a setting nobody will ship tells you nothing about what will
 * ship.
 */
export const SOAK: Arm[] = [
  {
    name: 'soak-recommended',
    question: 'Does the recommended configuration hold for 30 minutes without drift or growth?',
    cfg: { transport: 'channel', encoding: 'manual', coalesce: true },
    renderMode: 'worker',
  },
]

export function armConfig(arm: Arm, durationSecs: number): BenchConfig {
  return { ...BASE, ...arm.cfg, durationSecs }
}
