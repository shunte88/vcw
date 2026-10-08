#!/usr/bin/env python3
#
#  tidy-glyph.py
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Strips Inkscape editor state out of a shipped glyph and gives it a header.
#
#  MIT License - see the header in any Rust source file for the full text.
#
"""Strip Inkscape's editor state out of a shipped glyph and give it a header.

The art arrives as Inkscape saves: a `sodipodi:namedview` holding the window
size and the zoom at the moment it was written, a `<defs>` with nothing in
it, and per-element `style` strings carrying two dozen properties that do not
apply to a path. None of it draws anything, and since `app.css` masks these
files straight out of `assets/`, Vite inlines whatever is in them into the
stylesheet as a data URI - so the editor state was shipping to users.

Geometry is not touched: every `d`, `transform` and paint property survives.
"""
import pathlib
import re
import sys

# What a path is actually painted with. Everything else in an Inkscape style
# string is editor state or a default.
KEEP = {
    "fill",
    "fill-rule",
    "fill-opacity",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-dasharray",
    "stroke-opacity",
    "opacity",
}

DROP_ELEMENTS = ("sodipodi:namedview", "metadata")
DROP_ATTRS = re.compile(
    r'\s+(?:sodipodi|inkscape):[\w-]+="[^"]*"'
    r'|\s+xmlns:(?:sodipodi|inkscape|svg)="[^"]*"'
    r'|\s+id="[^"]*"'
)

HEADER = """<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<!--
  {name}.svg

  VCW - The Vinyl Capture Workstation
  (c) 2026 Stue Hunter

  {purpose}. Worn as a mask-image, so only the shape is read.
  Adapted from SVG Repo (www.svgrepo.com) - see THIRD-PARTY-NOTICES.md.

  MIT License - see the header in any Rust source file for the full text.
-->
"""


def style(match):
    """Keeps the declarations that paint, drops the rest."""
    kept = []
    for part in match.group(1).split(";"):
        name, _, value = part.partition(":")
        if name.strip() in KEEP:
            kept.append(f"{name.strip()}:{value.strip()}")
    return f' style="{";".join(kept)}"' if kept else ""


def tidy(text, name, purpose):
    for element in DROP_ELEMENTS:
        text = re.sub(rf"<{element}\b.*?(?:/>|</{element}>)", "", text, flags=re.S)
    text = re.sub(r"<defs\b[^>]*/>|<defs\b[^>]*>\s*</defs>", "", text)
    text = DROP_ATTRS.sub("", text)
    text = re.sub(r' style="([^"]*)"', style, text)
    text = re.sub(r"^<\?xml[^>]*\?>\s*", "", text)
    text = re.sub(r"^<!--.*?-->\s*", "", text, flags=re.S)
    text = re.sub(r"\n{3,}", "\n\n", text.strip())
    return HEADER.format(name=name, purpose=purpose) + text + "\n"


if __name__ == "__main__":
    path = pathlib.Path(sys.argv[1])
    before = path.read_text(encoding="utf-8")
    after = tidy(before, path.stem, sys.argv[2])
    path.write_text(after, encoding="utf-8")
    print(f"{path.name}: {len(before)} -> {len(after)} bytes")
