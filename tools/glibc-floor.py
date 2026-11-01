#!/usr/bin/env python3
#
#  glibc-floor.py
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Hold the Linux artifacts to the stated glibc floor.
#
#  MIT License - see the header in any Rust source file for the full text.
#
# Why this exists. A Linux binary carries a `.gnu.version_r` record for every
# glibc symbol version it was linked against, and the loader refuses to start
# it on an older glibc before `main` runs:
#
#     ./usr/bin/vcw: /lib/aarch64-linux-gnu/libc.so.6: version `GLIBC_2.39'
#     not found
#
# That is a requirement, not a defect, and the floor is set by the box that
# links the binary and by nothing else. So the floor is a decision: it is
# stated once, below, it is stated again in docs/COMPATIBILITY.md where a
# person reads it and in the `libc6` line tauri.conf.json puts in the deb's
# Depends, and this tool holds the built artifacts to it.
#
# It used to do more than hold them. Found on a Raspberry Pi 4 running
# Bookworm on 2026-10-18, the 2.39 records cost us every machine older than
# the runner over two weak symbols from Rust's std - `pidfd_spawnp` and
# `pidfd_getpid` - which the code already copes with being absent, so the
# records were patched out after linking and the floor came down to 2.35.
# **That is gone, by decision on 2026-11-01.** A build bent to fit one old
# install is a build nobody can read the requirement off, and the install it
# was bent for was a stale one. The requirement is published instead, and what
# this tool does now is make sure the artifacts and the package metadata agree
# with what was published.
#
# Standard library only, same rule as verify-release.py: this runs in CI and on
# whatever box is holding a release at the time.

import argparse
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# The floor, and the only place in the code it is written down. 2.39 is what
# the pinned ubuntu-24.04 bundle runner links against, and it covers the
# current Ubuntu LTS, Debian 13 and Raspberry Pi OS Trixie. Changing it is
# changing a published requirement: docs/COMPATIBILITY.md and the `libc6` line
# in app/src-tauri/tauri.conf.json say the same number to a person and to apt,
# and `--selftest` fails if the three disagree.
FLOOR = (2, 39)

SHT_DYNSYM = 11
SHT_GNU_VERDEF = 0x6FFFFFFD
SHT_GNU_VERNEED = 0x6FFFFFFE
SHT_GNU_VERSYM = 0x6FFFFFFF
STB_WEAK = 2
SHN_UNDEF = 0
VER_NDX_GLOBAL = 1
SYM_SIZE = 24


def floor_version():
    return "GLIBC_" + ".".join(str(n) for n in FLOOR)


def floor_depends():
    return "libc6 (>= %s)" % ".".join(str(n) for n in FLOOR)


class Refusal(Exception):
    """Something this tool will not guess at."""


class NotAnElf(Exception):
    pass


def u16(b, o):
    return int.from_bytes(b[o:o + 2], "little")


def u32(b, o):
    return int.from_bytes(b[o:o + 4], "little")


def u64(b, o):
    return int.from_bytes(b[o:o + 8], "little")


def parse_version(name):
    """GLIBC_2.39 -> (2, 39). Anything else -> None."""
    if not name.startswith("GLIBC_"):
        return None
    parts = name[len("GLIBC_"):].split(".")
    if not all(p.isdigit() for p in parts):
        return None
    return tuple(int(p) for p in parts)


class Need:
    """One vernaux: a version of one library the file says it needs."""

    def __init__(self, file, name, other, vn, aux, prev):
        self.file = file
        self.name = name
        self.other = other
        self.vn = vn
        self.aux = aux
        self.prev = prev

    def over_floor(self):
        v = parse_version(self.name)
        return v is not None and v > FLOOR


class Elf:
    def __init__(self, path):
        self.path = Path(path)
        self.b = bytearray(self.path.read_bytes())
        if self.b[:4] != b"\x7fELF":
            raise NotAnElf(str(path))
        # No 32-bit or big-endian reader here. Nothing VCW ships is either, and
        # a reader that skips what it cannot parse is a check that stops
        # checking without saying so.
        if self.b[4] != 2 or self.b[5] != 1:
            raise Refusal("%s: not a 64-bit little-endian ELF" % path)
        shoff, shent, shnum = u64(self.b, 0x28), u16(self.b, 0x3A), u16(self.b, 0x3C)
        self.sections = []
        for i in range(shnum):
            o = shoff + i * shent
            self.sections.append({
                "type": u32(self.b, o + 4),
                "offset": u64(self.b, o + 0x18),
                "size": u64(self.b, o + 0x20),
                "link": u32(self.b, o + 0x28),
                "info": u32(self.b, o + 0x2C),
            })

    def cstr(self, off):
        end = self.b.index(b"\0", off)
        return self.b[off:end].decode("utf-8", "replace")

    def needs(self):
        out = []
        for s in self.sections:
            if s["type"] != SHT_GNU_VERNEED:
                continue
            strtab = self.sections[s["link"]]["offset"]
            vn = s["offset"]
            for _ in range(s["info"]):
                cnt = u16(self.b, vn + 2)
                fname = self.cstr(strtab + u32(self.b, vn + 4))
                aux = vn + u32(self.b, vn + 8)
                prev = None
                for _ in range(cnt):
                    out.append(Need(
                        file=fname,
                        name=self.cstr(strtab + u32(self.b, aux + 8)),
                        other=u16(self.b, aux + 6),
                        vn=vn, aux=aux, prev=prev))
                    step = u32(self.b, aux + 12)
                    prev = aux
                    if not step:
                        break
                    aux += step
                step = u32(self.b, vn + 12)
                if not step:
                    break
                vn += step
        return out

    def symbols_at(self, other):
        """Every dynamic symbol bound to one version index."""
        out = []
        for s in self.sections:
            if s["type"] != SHT_GNU_VERSYM:
                continue
            dyn = self.sections[s["link"]]
            if dyn["type"] != SHT_DYNSYM:
                raise Refusal("%s: .gnu.version does not link to a dynsym" % self.path)
            strtab = self.sections[dyn["link"]]["offset"]
            for i in range(s["size"] // 2):
                vs = s["offset"] + 2 * i
                if u16(self.b, vs) & 0x7FFF != other:
                    continue
                so = dyn["offset"] + i * SYM_SIZE
                info = self.b[so + 4]
                out.append({
                    "name": self.cstr(strtab + u32(self.b, so)),
                    "weak": info >> 4 == STB_WEAK,
                    "undefined": u16(self.b, so + 6) == SHN_UNDEF,
                    "versym": vs,
                })
        return out

    def version_indexes(self):
        """Every version index this file still defines or needs."""
        known = {0, VER_NDX_GLOBAL}
        for n in self.needs():
            known.add(n.other)
        for s in self.sections:
            if s["type"] != SHT_GNU_VERDEF:
                continue
            vd = s["offset"]
            for _ in range(s["info"]):
                known.add(u16(self.b, vd + 4))
                step = u32(self.b, vd + 16)
                if not step:
                    break
                vd += step
        return known


def over_floor(path):
    """[(need, symbols)] for every glibc version this file needs over the floor."""
    elf = Elf(path)
    return [(n, elf.symbols_at(n.other)) for n in elf.needs() if n.over_floor()]


def describe(path, offenders):
    lines = []
    for need, syms in offenders:
        named = ", ".join(s["name"] for s in syms) or "no symbols"
        lines.append("  %s: %s from %s (%s)" % (path, need.name, need.file, named))
    return lines


def check_elf(path):
    offenders = over_floor(path)
    if not offenders:
        print("ok   %s: nothing over %s" % (path, floor_version()))
        return []
    return describe(path, offenders)


def elves_under(root):
    found = []
    for p in sorted(Path(root).rglob("*")):
        if not p.is_file() or p.is_symlink():
            continue
        with open(p, "rb") as fh:
            if fh.read(4) == b"\x7fELF":
                found.append(p)
    return found


def check_deb(path):
    """The thing a person actually downloads: its ELFs and its control file."""
    if not shutil.which("dpkg-deb"):
        raise Refusal("dpkg-deb is not installed, so %s cannot be read" % path)
    complaints = []
    depends = subprocess.run(
        ["dpkg-deb", "-f", str(path), "Depends"],
        check=True, capture_output=True, text=True).stdout
    fields = [d.strip() for d in depends.replace("\n", " ").split(",")]
    if floor_depends() not in fields:
        complaints.append(
            "  %s: Depends is %r, which does not declare %r. Without it apt "
            "installs happily and the failure is an unreadable loader error at "
            "first run." % (path, ", ".join(f for f in fields if f),
                            floor_depends()))
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(["dpkg-deb", "-x", str(path), tmp], check=True)
        payload = elves_under(tmp)
        if not payload:
            raise Refusal("%s: no ELF files inside, so there is nothing to check"
                          % path)
        for elf in payload:
            inside = "%s!%s" % (path, elf.relative_to(tmp))
            offenders = over_floor(elf)
            if offenders:
                complaints += describe(inside, offenders)
            else:
                print("ok   %s: nothing over %s" % (inside, floor_version()))
    if not complaints:
        print("ok   %s: Depends declares %s" % (path, floor_depends()))
    return complaints


def check(targets):
    complaints = []
    for t in targets:
        p = Path(t)
        if p.is_dir():
            for elf in elves_under(p):
                complaints += check_elf(elf)
        elif p.suffix == ".deb":
            complaints += check_deb(p)
        else:
            complaints += check_elf(p)
    if complaints:
        print("\nover the %s floor:" % floor_version())
        print("\n".join(complaints))
        return 1
    return 0


# --- the selftest ------------------------------------------------------------
#
# A box whose own glibc is over the floor cannot hold a locally built binary
# to it: everything it links is over it by construction, so a leg that tried
# would only ever report the box. What any box can prove is the tool, and
# since the floor stopped being patched in and became a published number,
# that is three things: that the ELF reader agrees with an independent oracle,
# that the floor comparison actually fires, and that the three places the
# requirement is written down say the same number. The release pipeline proves
# the rest, against a real deb.

def need_tool(name):
    path = shutil.which(name)
    if not path:
        raise Refusal("%s is not installed, so the selftest cannot run" % name)
    return path


def readelf_needs(path):
    """(file, version) pairs as readelf -V sees them."""
    out = subprocess.run([need_tool("readelf"), "-W", "-V", str(path)],
                         check=True, capture_output=True, text=True).stdout
    pairs, inside, lib = set(), False, None
    for line in out.splitlines():
        if line.startswith("Version needs section"):
            inside = True
            continue
        if inside and line.startswith("Version") and "section" in line:
            inside = False
        if not inside:
            continue
        if "File:" in line:
            lib = line.split("File:")[1].split()[0]
        elif "Name:" in line and lib:
            pairs.add((lib, line.split("Name:")[1].split()[0]))
    return pairs


def stated_floors():
    """The floor as each of the places that publish it states it."""
    root = Path(__file__).resolve().parent.parent
    found = {}

    conf = (root / "app/src-tauri/tauri.conf.json").read_text()
    m = re.search(r"libc6 \(>= (\d+)\.(\d+)\)", conf)
    if m:
        found["tauri.conf.json Depends"] = (int(m.group(1)), int(m.group(2)))

    doc = (root / "docs/COMPATIBILITY.md").read_text()
    m = re.search(r"glibc (\d+)\.(\d+) or newer", doc)
    if m:
        found["docs/COMPATIBILITY.md"] = (int(m.group(1)), int(m.group(2)))

    return found


def selftest():
    oracle = need_tool("ls")
    mine = {(n.file, n.name) for n in Elf(oracle).needs()}
    theirs = readelf_needs(oracle)
    if mine != theirs:
        raise Refusal("the reader disagrees with readelf -V on %s: %r vs %r"
                      % (oracle, sorted(mine), sorted(theirs)))
    print("ok   the reader agrees with readelf -V on %s (%d needs)"
          % (oracle, len(mine)))

    # The comparison, proved at both ends on a file that is certainly there.
    # No compiler and no particular glibc: the floor is moved around a known
    # set of needs instead, which is the one way to prove this on a box at or
    # under the floor as well as on one over it.
    # Only the glibc ones: `ls` also needs libselinux and libcap, whose
    # version names parse to nothing and are not the floor's business.
    versioned = [n for n in Elf(oracle).needs() if parse_version(n.name)]
    if not versioned:
        raise Refusal("%s needs no versioned glibc symbols, so the floor "
                      "comparison has nothing to fire on" % oracle)
    highest = max(parse_version(n.name) for n in versioned)
    global FLOOR
    kept = FLOOR
    try:
        FLOOR = (0, 0)
        if len(over_floor(oracle)) != len(versioned):
            raise Refusal("under a zero floor the reader reported %d of the %d "
                          "versioned needs %s has"
                          % (len(over_floor(oracle)), len(versioned), oracle))
        print("ok   a floor under everything reports all %d of %s's versioned needs"
              % (len(versioned), oracle))
        FLOOR = highest
        if over_floor(oracle):
            raise Refusal("at %s's own highest need (%d.%d) the reader still "
                          "reports something over the floor"
                          % (oracle, highest[0], highest[1]))
        print("ok   a floor at the highest of them (%d.%d) reports none"
              % (highest[0], highest[1]))
    finally:
        FLOOR = kept

    # And the number itself. The floor is a published requirement now rather
    # than something patched in, so the failure to guard against is the three
    # statements of it drifting apart: apt would install onto a machine the
    # binary cannot start on, and the document would tell a person the wrong
    # thing, and nothing would be red.
    stated = stated_floors()
    for where in ("tauri.conf.json Depends", "docs/COMPATIBILITY.md"):
        if where not in stated:
            raise Refusal("%s does not state the glibc floor at all, so "
                          "nothing holds it to %s" % (where, floor_version()))
    wrong = {k: v for k, v in stated.items() if v != FLOOR}
    if wrong:
        raise Refusal("the floor is %s here but %s" % (
            floor_version(),
            "; ".join("%s says %d.%d" % (k, v[0], v[1])
                      for k, v in sorted(wrong.items()))))
    print("ok   %s and %s both state %s"
          % ("tauri.conf.json", "docs/COMPATIBILITY.md", floor_version()))

    print("selftest OK (floor %s)" % floor_version())
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("targets", nargs="*", metavar="PATH",
                    help="an ELF file, a .deb, or a directory of either")
    ap.add_argument("--selftest", action="store_true",
                    help="prove the tool and the floor, on any box")
    args = ap.parse_args(argv)
    try:
        if args.selftest:
            if args.targets:
                ap.error("--selftest takes no paths")
            return selftest()
        if not args.targets:
            ap.error("nothing to read")
        return check(args.targets)
    except Refusal as e:
        print("REFUSED: %s" % e, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
