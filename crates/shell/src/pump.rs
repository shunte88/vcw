/*
 *  pump.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  One thread per bus, turning core events into webview events (§35, D6).
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

//! One thread per bus, turning core events into webview events (§35, D6).
//!
//! The whole of §35's event half is this file. A [`vcw_core::Events`] stream is
//! blocking and a webview cannot block, so a thread sits between them: it takes
//! events off the bus, converts each to a [`Wire`], and emits it.
//!
//! One event name, `vcw://event`, for every kind. The alternative - a name per
//! kind, so a meter component subscribes only to meters - was measured in S3
//! and is not worth it: the cost of a 60 Hz meter update is drawing it on the
//! main thread, not delivering it, and the boundary itself was free at every
//! rate tried. One name keeps `Wire`'s discriminated union usable as it was
//! designed, with a single `switch (event.kind)` at the top of the frontend.
//!
//! Nothing is coalesced. S3 tried it and found sending every frame cheaper than
//! deciding not to: a 16 ms window and a dirty flag cost more in bookkeeping
//! than the sends they saved, and the frontend has `requestAnimationFrame` for
//! exactly this. Nothing is packed into a binary payload either - the same
//! spike found `InvokeResponseBody::Raw` a pessimisation for frames this small,
//! because the JavaScript side then has to decode what `JSON.parse` would have
//! given it for free.

use vcw_contract::event::Wire;
use vcw_core::Events;

use crate::host::Hosted;

/// Forwards a bus to whatever is hosting, until it closes.
///
/// Spawns and returns immediately. The thread ends when the stream does, which
/// for the engine's bus means [`vcw_core::Event::Closed`] and for the shell's
/// means the last sender was dropped.
pub fn forward(host: &Hosted, events: Events, label: &'static str) {
    let host = host.clone();
    let name = format!("vcw-pump-{label}");
    let spawned = std::thread::Builder::new()
        .name(name)
        .spawn(move || run(&host, &events));
    if let Err(error) = spawned {
        // A thread that will not spawn is not recoverable, but it is also not a
        // reason to take the window down: the commands still work, and a
        // frontend with no events can still poll. Said out loud on stderr
        // because there is no other channel left to say it on.
        eprintln!("vcw: could not start the {label} event pump: {error}");
    }
}

/// The thread body.
fn run(host: &Hosted, events: &Events) {
    while let Some(event) = events.next() {
        let wire = Wire::from(&event);
        // A host that can no longer deliver is a window that has closed. There
        // is nothing useful to do about it and nothing left to report it to,
        // so the pump stops rather than reading a bus on nobody's behalf.
        if !host.emit(&wire) {
            return;
        }
        if wire.is_last() {
            return;
        }
    }
}
