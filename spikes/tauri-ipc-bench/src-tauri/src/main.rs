/*
 *  main.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  S3 - Tauri 2 IPC throughput.
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

//! S3 - Tauri 2 IPC throughput.
//!
//! D6 proposes "Tauri 2 channels (not the event bus) for meter and waveform
//! deltas; pre-serialized compact payloads; coalesce to <=60 Hz in Rust", and
//! marks it *validate in S3*. Reading tauri 2.11's `src/ipc/channel.rs` and
//! `src/event/mod.rs` first turned up two things that make the naive version of
//! that experiment the wrong one:
//!
//! 1. **Both transports go through `webview.eval()`** for small payloads -
//!    channels via a direct callback, the event bus via a listener-dispatch
//!    wrapper. So the channel-vs-event question is about JS dispatch layers,
//!    not about a fundamentally different pipe.
//! 2. **Small `Raw` payloads are inflated, not compacted.** Under
//!    `MAX_RAW_DIRECT_EXECUTE_THRESHOLD` (1024 bytes) tauri renders the bytes
//!    as a *decimal JSON array* and evals `new Uint8Array([...]).buffer`, so a
//!    30-byte meter frame becomes well over 100 characters of JavaScript source.
//!    Above the threshold it switches to a `fetch` round-trip instead.
//!
//! So the matrix here is transport x encoding, and `raw` is in it as a
//! hypothesis to be disproved rather than an assumed win. The thresholds
//! upstream were tuned on WebView2 and macOS; this spike is the WebKitGTK
//! number, which nobody has measured.

mod metrics;
mod payload;
mod producer;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use metrics::{HistSummary, Memory};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    /// `Channel::send`, i.e. a direct JS callback invocation.
    Channel,
    /// `Webview::emit`, i.e. the event bus with listener dispatch.
    Event,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encoding {
    /// Let tauri's `T: Serialize` blanket impl serialize each frame.
    Serde,
    /// Hand-built compact JSON into a reused buffer - D6's "pre-serialized".
    Manual,
    /// Fixed-layout little-endian bytes. Not available on the event bus.
    Raw,
}

#[derive(Deserialize, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchConfig {
    pub transport: Transport,
    pub encoding: Encoding,
    /// Rate the UI is fed at. D6 wants this capped at 60.
    pub meter_hz: f64,
    pub wave_hz: f64,
    pub position_hz: f64,
    /// When false, meter frames go out at the raw worker rate
    /// (`captureRate / callbackFrames`) with no coalescing - 750 Hz at
    /// 192 kHz with 256-frame callbacks. This is the arm that tests whether
    /// D6's "coalesce in Rust" is load-bearing or just tidy.
    pub coalesce: bool,
    pub capture_rate: u32,
    pub callback_frames: u32,
    /// Frames per level-0 waveform bucket. 1024 gives 187.5 buckets/s at 192 kHz.
    pub bucket_frames: u32,
    pub duration_secs: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SendStats {
    /// How long the producer thread spent inside `send`/`emit`. If this is not
    /// ~zero, the meter worker is being blocked by the UI, which §37 forbids.
    pub meter_send: HistSummary,
    pub wave_send: HistSummary,
    pub position_send: HistSummary,
    /// How late each tick fired against its schedule - the producer's own
    /// jitter, so UI-side jitter can be attributed correctly.
    pub tick_lateness: HistSummary,
    pub meter_sent: u64,
    pub wave_sent: u64,
    pub position_sent: u64,
    pub meter_bytes: u64,
    pub wave_bytes: u64,
    pub position_bytes: u64,
    /// Payload size as it appears in the evaluated JavaScript. Differs from
    /// `*_bytes` only for the `raw` encoding, where tauri renders the bytes as
    /// a decimal array.
    pub meter_wire: u64,
    pub wave_wire: u64,
    pub position_wire: u64,
    pub send_errors: u64,
    pub elapsed_secs: f64,
}

/// What the webview measured. Handed back through a command rather than scraped
/// from the console, so a headless run can still produce a report.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClientStats(pub serde_json::Value);

#[derive(Default)]
pub struct Bench {
    running: AtomicBool,
    stop: AtomicBool,
    last: Mutex<Option<(BenchConfig, SendStats)>>,
    client: Mutex<Option<ClientStats>>,
    /// Round trip Rust -> webview -> Rust, timed entirely on the Rust clock.
    /// One-way latency is not measurable here: `performance.now()` is clamped
    /// to milliseconds in WebKit, so a cross-clock estimate has an error bar
    /// wider than the quantity. This is an upper bound with real microsecond
    /// resolution, which is worth more than a precise-looking guess.
    rtt: Mutex<metrics::Hist>,
}

/// Called by the webview when it receives a frame, echoing back the timestamp
/// that frame carried.
#[tauri::command]
fn echo(sent_us: u64, bench: State<'_, Bench>, epoch: State<'_, producer::Epoch>) {
    let now = epoch.elapsed_us();
    bench
        .rtt
        .lock()
        .unwrap()
        .record(now.saturating_sub(sent_us));
}

#[tauri::command]
fn memory() -> Memory {
    metrics::memory()
}

/// Round-trip probe. The webview uses the minimum of many of these to estimate
/// the Rust/JS clock offset, since one-way latency is otherwise unmeasurable
/// across two clocks.
#[tauri::command]
fn now_us(state: State<'_, producer::Epoch>) -> u64 {
    state.elapsed_us()
}

#[tauri::command]
fn stop_bench(state: State<'_, Bench>) -> Option<serde_json::Value> {
    state.stop.store(true, Ordering::Relaxed);
    let last = state.last.lock().unwrap();
    last.as_ref().map(|(cfg, stats)| {
        serde_json::json!({ "config": cfg, "send": stats, "memory": metrics::memory() })
    })
}

#[tauri::command]
fn submit_client_stats(stats: ClientStats, state: State<'_, Bench>) {
    *state.client.lock().unwrap() = Some(stats);
}

/// Writes the merged Rust-side and webview-side report to `.bench/s3/`.
#[tauri::command]
fn write_report(name: String, state: State<'_, Bench>) -> Result<String, String> {
    let dir = std::path::Path::new(".bench/s3");
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let last = state.last.lock().unwrap();
    let (cfg, send) = last.as_ref().ok_or("no run recorded yet")?;
    let report = serde_json::json!({
        "spike": "S3",
        "name": name,
        "config": cfg,
        "send": send,
        "client": state.client.lock().unwrap().clone(),
        "echoRttUs": state.rtt.lock().unwrap().summary(),
        "memory": metrics::memory(),
    });
    // Sanitized so a transport/encoding label can be used as a file name.
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let path = dir.join(format!("{safe}.json"));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Unattended-run instructions, read from the environment so the same binary
/// can be driven by hand or by a script. The matrix is 10 arms and the soak is
/// 30 minutes; neither is something to sit and click through.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AutoRun {
    /// `matrix`, or a single arm name from `src/arms.ts`.
    pub what: String,
    pub secs: f64,
    /// Whether to close the window when the run finishes.
    pub then_exit: bool,
}

#[tauri::command]
fn autorun() -> Option<AutoRun> {
    let what = std::env::var("S3_AUTORUN").ok()?;
    Some(AutoRun {
        secs: std::env::var("S3_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20.0),
        then_exit: std::env::var("S3_KEEP_OPEN").is_err(),
        what,
    })
}

/// The webview has no console this binary can read, and an unattended run that
/// fails silently is worse than no run at all. Everything the frontend wants to
/// say about its own progress comes back through here.
#[tauri::command]
fn log(msg: String) {
    eprintln!("[webview] {msg}");
}

#[tauri::command]
fn finish(app: tauri::AppHandle, code: i32) {
    app.exit(code);
}

/// Reports the webview's own view of itself back through `log`, for the case
/// where the bundle never gets far enough to report anything. Without this, a
/// frontend that fails to load is indistinguishable from one that loads and
/// does nothing.
fn probe_webview(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        for i in 0..5 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let Some(w) = app.webview_windows().values().next().cloned() else {
                eprintln!("[probe {i}] no webview window exists");
                continue;
            };
            if i == 0 {
                // Did `generate_context!` actually embed the frontend? A blank
                // page and an empty asset table look identical from the
                // outside.
                let r = app.asset_resolver();
                for k in ["index.html", "/index.html"] {
                    match r.get(k.into()) {
                        Some(a) => {
                            eprintln!("[probe] asset {k}: {} bytes {}", a.bytes.len(), a.mime_type)
                        }
                        None => eprintln!("[probe] asset {k}: absent"),
                    }
                }
                eprintln!("[probe] window url: {:?}", w.url());
            }
            // `fetch` to a throwaway local server is the one reporting channel
            // that depends on neither `invoke` nor the bundle. If nothing
            // arrives, JavaScript is not running at all.
            let js = format!(
                r#"
                  (function () {{
                    var m = 'i={i} readyState=' + document.readyState
                      + ' boot=' + !!window.__s3boot
                      + ' scripts=' + document.scripts.length
                      + ' internals=' + !!window.__TAURI_INTERNALS__
                      + ' url=' + location.href
                      + ' bodylen=' + (document.body ? document.body.innerHTML.length : -1)
                      + ' errs=' + JSON.stringify(window.__s3err || null);
                    fetch('http://127.0.0.1:8099/?' + encodeURIComponent(m));
                  }})()
                "#
            );
            if let Err(e) = w.eval(&js) {
                eprintln!("[probe {i}] eval failed: {e}");
            }
        }
    });
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            if std::env::var("S3_PROBE").is_ok() {
                probe_webview(app.handle());
            }
            Ok(())
        })
        .manage(Bench::default())
        .manage(producer::Epoch::new())
        .invoke_handler(tauri::generate_handler![
            producer::start_bench,
            stop_bench,
            submit_client_stats,
            write_report,
            memory,
            now_us,
            autorun,
            finish,
            log,
            echo,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the S3 bench app");
}
