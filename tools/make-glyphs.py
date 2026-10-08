#!/usr/bin/env python3
#
#  make-glyphs.py
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Draws the six VCW panel glyphs the supplied icon set did not cover.
#
#  MIT License - see the header in any Rust source file for the full text.
#
"""Author the six VCW panel glyphs that the supplied art did not cover.

Same box, same blue, same construction as capture.svg, play.svg and
globe-lines.svg: viewBox 0 0 73.768 73.768, geometry inside 5.5..68.3, and
open line art at one stroke weight rather than a filled silhouette. The first
pass drew them filled; held up against the supplied set that read as a
different icon family on the same row, which is the one thing these had to
avoid.

A stroke renders to alpha like any other paint, so wearing them as a
mask-image works unchanged - which is also why nothing here needs an
interior drawn in a second color.
"""
import math

BOX = 73.768
C = BOX / 2.0


def f(x):
    return f"{x:.4f}".rstrip("0").rstrip(".")


def rrect(x, y, w, h, r):
    """A rounded rectangle, clockwise from the top-left corner."""
    r = min(r, w / 2, h / 2)
    return (
        f"M {f(x + r)},{f(y)} H {f(x + w - r)} A {f(r)},{f(r)} 0 0 1 {f(x + w)},{f(y + r)} "
        f"V {f(y + h - r)} A {f(r)},{f(r)} 0 0 1 {f(x + w - r)},{f(y + h)} "
        f"H {f(x + r)} A {f(r)},{f(r)} 0 0 1 {f(x)},{f(y + h - r)} "
        f"V {f(y + r)} A {f(r)},{f(r)} 0 0 1 {f(x + r)},{f(y)} Z"
    )


def circle(cx, cy, r, sweep=1):
    """A full circle as two arcs. `sweep=0` reverses it, which cuts a hole."""
    return (
        f"M {f(cx - r)},{f(cy)} A {f(r)},{f(r)} 0 0 {sweep} {f(cx + r)},{f(cy)} "
        f"A {f(r)},{f(r)} 0 0 {sweep} {f(cx - r)},{f(cy)} Z"
    )


def gear(teeth=8, outer=30.0, root=22.0, hub=9.0, tooth=0.42):
    """A gear outline: `teeth` trapezoids on a circle, with a hub bored out.

    `tooth` is the fraction of each tooth's pitch that the tooth itself takes,
    so the flanks stay parallel-ish at any count instead of closing up.
    """
    step = 2 * math.pi / teeth
    half_tip = step * tooth / 2
    half_root = step * (1 - tooth) / 2
    points = []
    for n in range(teeth):
        mid = n * step - math.pi / 2
        points += [
            (outer, mid - half_tip),
            (outer, mid + half_tip),
            (root, mid + half_tip + half_root * 0.6),
            (root, mid + step - half_tip - half_root * 0.6),
        ]
    d = []
    for index, (radius, angle) in enumerate(points):
        x, y = C + radius * math.cos(angle), C + radius * math.sin(angle)
        d.append(f"{'M' if index == 0 else 'L'} {f(x)},{f(y)}")
    return " ".join(d) + " Z " + circle(C, C, hub, sweep=0)


def tag(hole=4.4):
    """A luggage tag: a rectangle with one corner cut off, and a hole in it."""
    left, right, top, bottom, cut = 7.5, 66.3, 15.0, 58.8, 17.0
    body = (
        f"M {f(left + cut)},{f(top)} H {f(right - 4)} "
        f"A 4,4 0 0 1 {f(right)},{f(top + 4)} V {f(bottom - 4)} "
        f"A 4,4 0 0 1 {f(right - 4)},{f(bottom)} H {f(left + cut)} "
        f"L {f(left)},{f((top + bottom) / 2)} Z"
    )
    return body + " " + circle(left + cut - 1.0, (top + bottom) / 2, hole)


def page():
    """A sheet with a folded corner, ruled with three lines."""
    left, right, top, bottom, fold = 13.0, 60.8, 7.5, 66.3, 14.0
    sheet = (
        f"M {f(left + 4)},{f(top)} H {f(right - fold)} L {f(right)},{f(top + fold)} "
        f"V {f(bottom - 4)} A 4,4 0 0 1 {f(right - 4)},{f(bottom)} "
        f"H {f(left + 4)} A 4,4 0 0 1 {f(left)},{f(bottom - 4)} "
        f"V {f(top + 4)} A 4,4 0 0 1 {f(left + 4)},{f(top)} Z"
    )
    # The dog-ear, drawn as the two edges that are not already the sheet's.
    fold = f"M {f(right - fold)},{f(top)} V {f(top + fold)} H {f(right)}"
    rules = " ".join(
        f"M {f(left + 7)},{f(top + 24 + n * 11)} h {f((right - left) - 14 - n * 9)}"
        for n in range(3)
    )
    return sheet + " " + fold + " " + rules


def shelf():
    """Record sleeves standing on a shelf, which is what the library lists."""
    spines = [(8.0, 36.0), (21.0, 44.0), (34.0, 31.0)]
    base = 55.0
    out = [f"M 5.5,{f(base + 7.0)} H 68.268"]
    out += [rrect(x, base - h, 11.0, h, 2.4) for x, h in spines]
    # One sleeve pulled half out and leaning, so the row reads as a shelf
    # somebody uses rather than three bars in a column.
    out.append(
        f"M 51.2,{f(base)} L 58.6,{f(base - 29.0)} "
        f"A 2.4,2.4 0 0 1 61.5,{f(base - 30.8)} L 65.1,{f(base - 29.9)} "
        f"A 2.4,2.4 0 0 1 66.9,{f(base - 27.0)} L 59.9,{f(base)} Z"
    )
    return " ".join(out)


def rows():
    """Three bars: a side's tracks, each a length of the capture."""
    widths = [52.0, 36.0, 44.0]
    # Pitch 16.5 against a 9.5 bar: at a 2.77 stroke a tighter gap closes up
    # into one stack of ovals, which is what the first attempt drew.
    return " ".join(
        rrect(9.9, 15.8 + n * 16.5, w, 9.5, 4.75) for n, w in enumerate(widths)
    )


def keyboard():
    """A keyboard, because Keys is the shortcut sheet and not a door key."""
    # Three keys and a spacebar, not ten: this is read at about 18 px on a
    # toolbar, and a full key field turns to mush well before it reads as a
    # keyboard. Three is enough to say which object it is.
    body = rrect(5.5, 16.0, 62.768, 41.768, 5.0)
    keys = [rrect(13.5 + col * 16.0, 25.0, 11.0, 10.0, 2.2) for col in range(3)]
    keys.append(rrect(18.0, 42.0, 37.768, 7.0, 2.8))
    return body + " " + " ".join(keys)


GLYPHS = {
    "library": ("Projects on a shelf", shelf()),
    "tracks": ("A side cut into tracks", rows()),
    "metadata": ("The release's own label", tag()),
    "settings": ("Preferences", gear()),
    "log": ("The event log", page()),
    "keys": ("The keyboard map", keyboard()),
}

TEMPLATE = """<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<!--
  {name}.svg

  VCW - The Vinyl Capture Workstation
  (c) 2026 Stue Hunter

  {purpose}. Worn as a mask-image, so only the shape is read.

  MIT License - see the header in any Rust source file for the full text.
-->
<svg xmlns="http://www.w3.org/2000/svg" width="800px" height="800px"
     viewBox="0 0 {box} {box}" version="1.1">
  <path fill="none" stroke="#4f8cc9" stroke-width="2.7663"
        stroke-linecap="round" stroke-linejoin="round"
        d="{d}" />
</svg>
"""

if __name__ == "__main__":
    import pathlib
    import sys

    into = pathlib.Path(sys.argv[1])
    for name, (purpose, d) in GLYPHS.items():
        into.joinpath(f"{name}.svg").write_text(
            TEMPLATE.format(name=name, purpose=purpose, box=f(BOX), d=d),
            encoding="utf-8",
        )
        print(name)
