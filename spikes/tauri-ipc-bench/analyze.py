#!/usr/bin/env python3
#  analyze.py
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Spike S3 - collapses the per-arm JSON in .bench/s3 into the report tables.
#
# MIT License
#
# Copyright (c) 2026 Stue Hunter
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.
#

"""Collapses the per-arm JSON in .bench/s3 into the tables the spike report needs.

Kept as a script rather than done by hand so the numbers in
docs/spikes/S3-tauri-ipc.md can be regenerated from the raw reports.
"""
import json
import pathlib
import sys

ARM_ORDER = [
    "control-raf",
    "control-idle",
    "control-nodraw",
    "channel-serde",
    "channel-manual",
    "channel-raw",
    "event-serde",
    "event-manual",
    "render-naive",
    "render-worker",
    "render-react-dom",
    "uncoalesced",
    "uncoalesced-raw",
]


def load(d: pathlib.Path) -> dict:
    out = {}
    for p in d.glob("*.json"):
        out[p.stem] = json.loads(p.read_text())
    return out


def ms(us: float) -> str:
    return f"{us / 1000:.1f}"


def bucket(upper_us: float) -> str:
    """Renders a log-bucket upper edge as the range it actually represents.

    Printing a single number invites reading it as a percentile; `16-33 ms` is
    what the histogram knows, and at a 1 ms clock floor that is all it can know.
    """
    if upper_us <= 0:
        return "-"
    lo = upper_us / 2
    if upper_us <= 1000:
        # The clock cannot resolve below its own 1 ms floor, so every bucket at
        # or under 1 ms means the same thing: too fast to measure here.
        return "<1 ms (floor)"
    if upper_us < 2000:
        return "<=1 ms (floor)"
    return f"{lo / 1000:.0f}-{upper_us / 1000:.0f} ms"


def main() -> int:
    d = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".bench/s3")
    reports = load(d)
    if not reports:
        print(f"no reports in {d}", file=sys.stderr)
        return 1
    names = [n for n in ARM_ORDER if n in reports] + sorted(set(reports) - set(ARM_ORDER))

    print("### Frame pacing, draw cost and main-thread occupancy\n")
    print(
        "`rAF/s` is not a quality measure here: WebKitGTK does not pace "
        "`requestAnimationFrame` to vsync, so it varies by arm on a fixed "
        "60 Hz output (see `control-raf`). Occupancy is the comparable figure: "
        "mean draw duration x callbacks / wall clock, i.e. the share of the "
        "main thread the UI spends painting. Mean is used rather than a "
        "percentile because a millisecond-clamped clock makes each *sample* "
        "useless but leaves the *mean* unbiased: a 0.3 ms draw reads as 1 ms "
        "about 30% of the time and 0 ms otherwise.\n"
    )
    print(
        "| arm | render | rAF/s | mean draw | occupancy | >20ms gaps | >33ms | "
        "draw >=1ms | draw >=4ms |"
    )
    print("|---|---|--:|--:|--:|--:|--:|--:|--:|")
    for n in names:
        c = reports[n]["client"]
        k = c["counters"]
        fps = c["frames"] / c["elapsedSecs"]
        pct20 = 100 * c["jank20"] / max(1, c["frames"])
        occ = 100 * c["drawUs"]["meanUs"] * c["frames"] / (c["elapsedSecs"] * 1e6)
        print(
            f"| `{n}` | {c['renderMode']} | {fps:.1f} | "
            f"{c['drawUs']['meanUs'] / 1000:.2f} ms | {occ:.1f}% | "
            f"{c['jank20']} ({pct20:.1f}%) | {c['jank33']} | "
            f"{k.get('drawGe1ms', 0)} | {k.get('drawGe4ms', 0)} |"
        )

    print("\n### Send cost on the producer thread\n")
    print("| arm | transport | enc | meter/s | meter send p99 | wave send p99 | "
          "tick late p99 | producer CPU |")
    print("|---|---|---|--:|--:|--:|--:|--:|")
    for n in names:
        r = reports[n]
        s, cfg = r["send"], r["config"]
        secs = s["elapsedSecs"]
        # Time inside send/emit as a fraction of wall clock: what fraction of a
        # core the IPC costs the producer thread.
        busy = sum(
            s[k]["mean_us"] * s[k]["count"] for k in ("meterSend", "waveSend", "positionSend")
        )
        print(
            f"| `{n}` | {cfg['transport']} | {cfg['encoding']} | "
            f"{s['meterSent'] / secs:.0f} | {s['meterSend']['p99_us_upper']} us | "
            f"{s['waveSend']['p99_us_upper']} us | {s['tickLateness']['p99_us_upper']} us | "
            f"{100 * busy / (secs * 1e6):.2f}% |"
        )

    print("\n### Payload size, and the size tauri actually evals\n")
    print("| arm | enc | meter payload | meter in JS | wave payload | wave in JS | "
          "JS source kB/s |")
    print("|---|---|--:|--:|--:|--:|--:|")
    for n in names:
        r = reports[n]
        s, cfg = r["send"], r["config"]
        if s["meterSent"] == 0:
            continue
        secs = s["elapsedSecs"]
        mb = s["meterBytes"] / s["meterSent"]
        mw = s.get("meterWire", 0) / s["meterSent"]
        wb = s["waveBytes"] / max(1, s["waveSent"])
        ww = s.get("waveWire", 0) / max(1, s["waveSent"])
        tot = (
            s.get("meterWire", 0) + s.get("waveWire", 0) + s.get("positionWire", 0)
        ) / secs / 1000
        infl = f" ({mw / mb:.1f}x)" if mb and abs(mw - mb) > 1 else ""
        print(
            f"| `{n}` | {cfg['encoding']} | {mb:.0f} B | {mw:.0f} B{infl} | "
            f"{wb:.0f} B | {ww:.0f} B | {tot:.1f} |"
        )

    print("\n### Delivery latency (Rust clock, round trip) and receive-side cost\n")
    print("| arm | echo RTT mean | echo RTT p99 | echo RTT max | meter recv | "
          "recv/sent | seq gaps | handler p99 | RSS total |")
    print("|---|--:|--:|--:|--:|--:|--:|--:|--:|")
    for n in names:
        r = reports[n]
        c, s = r["client"], r["send"]
        e = r.get("echoRttUs", {})
        k = c["counters"]
        recv = k.get("meterRecv", 0)
        ratio = recv / max(1, s["meterSent"])
        if e.get("count", 0) == 0:
            rtt_mean = rtt_p99 = rtt_max = "n/a"
        else:
            rtt_mean = f"{ms(e['mean_us'])} ms"
            rtt_p99 = f"{ms(e['p99_us_upper'])} ms"
            rtt_max = f"{ms(e['max_us'])} ms"
        print(
            f"| `{n}` | {rtt_mean} | {rtt_p99} | "
            f"{rtt_max} | {recv} | {ratio:.3f} | "
            f"{k.get('meterSeqGaps', 0)} | {bucket(c['meterHandlerUs']['p99UsUpper'])} | "
            f"{r['memory']['total_rss_kib'] / 1024:.0f} MiB |"
        )

    soaks = [n for n in names if len(reports[n]["client"].get("timeline") or []) >= 2]
    if soaks:
        print("\n### Over time\n")
        for n in soaks:
            c = reports[n]["client"]
            tl = c["timeline"]
            print(f"\n#### `{n}` ({c['elapsedSecs'] / 60:.0f} min, {c['renderMode']})\n")
            print("| at | RSS | procs | rAF/s since last | jank>20ms since last | mean draw |")
            print("|--:|--:|--:|--:|--:|--:|")
            prev = None
            for t in tl:
                if prev is None:
                    rate, jank = "-", "-"
                else:
                    dt = t["atSecs"] - prev["atSecs"]
                    rate = f"{(t['frames'] - prev['frames']) / dt:.1f}"
                    jank = str(t["jank20"] - prev["jank20"])
                print(
                    f"| {t['atSecs'] / 60:.1f} min | {t['rssTotalKib'] / 1024:.0f} MiB | "
                    f"{t['processCount']} | {rate} | {jank} | "
                    f"{t['drawMeanUs'] / 1000:.2f} ms |"
                )
                prev = t
            growth = (tl[-1]["rssTotalKib"] - tl[0]["rssTotalKib"]) / 1024
            mins = (tl[-1]["atSecs"] - tl[0]["atSecs"]) / 60
            print(
                f"\nRSS moved {growth:+.0f} MiB over {mins:.0f} minutes "
                f"({growth / max(mins, 1e-9):+.1f} MiB/min)."
            )

    print("\n### Environment as measured\n")
    first = reports[names[0]]["client"]
    print(f"- `performance.now()` resolution: {first['clockResolutionMs']:.3f} ms")
    print(f"- send errors across all arms: "
          f"{sum(reports[n]['send']['sendErrors'] for n in names)}")
    for n in names:
        w = reports[n]["client"].get("worker")
        if w:
            print(f"- `{n}` worker: {json.dumps(w)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
