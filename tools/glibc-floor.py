#!/usr/bin/env python3
#
#  glibc-floor.py
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  Hold the Linux artifacts to a stated glibc floor, and lower them to it.
#
#  MIT License - see the header in any Rust source file for the full text.
#
# Why this exists. The Linux bundles are built on ubuntu-latest, which is 24.04
# and ships glibc 2.39, and a binary built there refuses to start anywhere
# older:
#
#     ./usr/bin/vcw: /lib/aarch64-linux-gnu/libc.so.6: version `GLIBC_2.39'
#     not found
#
# Found on a Raspberry Pi 4 running Bookworm on 2026-10-18, which is glibc 2.36.
# What actually cost us everything from 2.35 to 2.39 was two symbols, both from
# Rust's std, both weak and undefined: `pidfd_spawnp` and `pidfd_getpid`. The
# code already copes with them being absent. The refusal does not come from the
# symbols at all; it comes from the `.gnu.version_r` record, where a version
# need with no VER_FLG_WEAK is fatal to the loader before `main` runs.
#
# So: drop the record, and clear the version index on the symbols that pointed
# at it. `patchelf --clear-symbol-version` is not enough on patchelf 0.14.3,
# which is what Bookworm has - it clears the symbol references and leaves the
# record, which is the thing the loader reads.
#
# The floor is stated once, below, as a number. A post-link patch that nobody
# checks is a fix that stops working silently the day a new glibc 2.39 symbol
# appears, so the same tool reads artifacts back and fails if anything is over
# the floor, and `--selftest` is the part of that a box with a newer glibc can
# still prove.
#
# Standard library only, same rule as verify-release.py: this runs in CI and on
# whatever box is holding a release at the time.

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# The floor, and the only place it is written down. 2.35 covers Ubuntu 22.04,
# Debian 12 and Raspberry Pi OS Bookworm, and it is what the code actually
# needs rather than what the runner happened to build against.
FLOOR = (2, 35)

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

    def dangling_versions(self):
        """Symbols left pointing at a version index that is not there.

        Unlinking a vernaux without clearing the symbols that referenced it
        leaves exactly this, and it is invisible to a version listing: the
        loader sizes its version table from the highest index it finds while
        walking, so a stale index reads past the end of it.
        """
        known = self.version_indexes()
        bad = []
        for s in self.sections:
            if s["type"] != SHT_GNU_VERSYM:
                continue
            dyn = self.sections[s["link"]]
            strtab = self.sections[dyn["link"]]["offset"]
            for i in range(s["size"] // 2):
                ndx = u16(self.b, s["offset"] + 2 * i) & 0x7FFF
                if ndx not in known:
                    so = dyn["offset"] + i * SYM_SIZE
                    bad.append("%s -> %d" % (self.cstr(strtab + u32(self.b, so)), ndx))
        return bad

    def drop(self, need):
        """Unlink one vernaux and unbind the symbols that pointed at it.

        Offsets inside the chain move as soon as anything is dropped, so the
        caller re-reads the file between drops rather than holding a list.
        """
        for sym in self.symbols_at(need.other):
            self.b[sym["versym"]:sym["versym"] + 2] = \
                VER_NDX_GLOBAL.to_bytes(2, "little")
        cnt = u16(self.b, need.vn + 2)
        if cnt <= 1:
            raise Refusal(
                "%s: %s is the only version needed from %s, and dropping the "
                "whole record needs DT_VERNEEDNUM adjusting too"
                % (self.path, need.name, need.file))
        step = u32(self.b, need.aux + 12)
        if need.prev is None:
            if not step:
                raise Refusal("%s: inconsistent vernaux chain" % self.path)
            self.b[need.vn + 8:need.vn + 12] = \
                (need.aux + step - need.vn).to_bytes(4, "little")
        else:
            onward = (need.aux + step - need.prev) if step else 0
            self.b[need.prev + 12:need.prev + 16] = onward.to_bytes(4, "little")
        self.b[need.vn + 2:need.vn + 4] = (cnt - 1).to_bytes(2, "little")

    def write(self):
        self.path.write_bytes(bytes(self.b))


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


def fix_elf(path):
    """Drop every over-floor need, or refuse and write nothing."""
    offenders = over_floor(path)
    if not offenders:
        print("ok   %s: nothing over %s" % (path, floor_version()))
        return
    # The precondition dropping a version record rests on, checked for all of
    # them before a byte is written: the program only runs without the symbol
    # if it was already prepared to. A strong reference dropped this way gives
    # a binary that starts and then dies at the first call.
    for need, syms in offenders:
        for s in syms:
            if not (s["weak"] and s["undefined"]):
                raise Refusal(
                    "%s: %s is needed by %s, which is %s and %s, not weak and "
                    "undefined. Dropping it would break the binary at the "
                    "first call." % (path, need.name, s["name"],
                                     "weak" if s["weak"] else "strong",
                                     "undefined" if s["undefined"] else "defined"))
    elf = Elf(path)
    dropped = []
    # One at a time, re-reading between them, because every offset in the
    # vernaux chain moves as soon as anything is unlinked. The progress check
    # is not decoration: a drop that quietly fails to unlink turns this into a
    # hang, which is the one failure a gate cannot report.
    while True:
        todo = [n for n in elf.needs() if n.over_floor()]
        if not todo:
            break
        going = todo[0]
        elf.drop(going)
        if len([n for n in elf.needs() if n.over_floor()]) >= len(todo):
            raise Refusal("%s: dropping %s did not remove it"
                          % (path, going.name))
        dropped.append(going.name)
    stale = elf.dangling_versions()
    if stale:
        raise Refusal("%s: %s left pointing at a version that is gone: %s"
                      % (path, "a symbol is" if len(stale) == 1 else
                         "symbols are", ", ".join(stale)))
    elf.write()
    print("fix  %s: dropped %s" % (path, ", ".join(dropped)))


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
# A dev box running glibc 2.43 cannot hold a locally built binary to a 2.35
# floor: everything it builds is over it by construction, so a leg that tried
# would only ever report the box. What it can prove is the tool: that the
# reader agrees with an independent oracle, that the fix refuses a strong
# reference without touching the file, and that it drops a weak one and the
# binary still runs. The release pipeline proves the rest, against a real deb.

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


def compile_fixture(tmp, name, weak):
    src = Path(tmp) / (name + ".c")
    # pidfd_getpid is one of the two real symbols, and it is versioned
    # GLIBC_2.39 on any glibc new enough to have it - which is any box that
    # needs this tool. The weak form is the shape Rust's std produces.
    src.write_text(
        ("extern int pidfd_getpid(int) __attribute__((weak));\n"
         "int main(void) { if (pidfd_getpid) pidfd_getpid(0); return 0; }\n")
        if weak else
        ("extern int pidfd_getpid(int);\n"
         "int main(void) { return pidfd_getpid(0); }\n"))
    out = Path(tmp) / name
    subprocess.run([need_tool("cc"), "-o", str(out), str(src)], check=True)
    if not any(n.name == "GLIBC_2.39" for n in Elf(out).needs()):
        raise Refusal(
            "the %s fixture did not come out needing GLIBC_2.39, so the "
            "selftest has nothing to prove against" % name)
    return out


def selftest():
    oracle = need_tool("ls")
    mine = {(n.file, n.name) for n in Elf(oracle).needs()}
    theirs = readelf_needs(oracle)
    if mine != theirs:
        raise Refusal("the reader disagrees with readelf -V on %s: %r vs %r"
                      % (oracle, sorted(mine), sorted(theirs)))
    print("ok   the reader agrees with readelf -V on %s (%d needs)"
          % (oracle, len(mine)))

    with tempfile.TemporaryDirectory() as tmp:
        strong = compile_fixture(tmp, "strong", weak=False)
        before = strong.read_bytes()
        try:
            fix_elf(strong)
        except Refusal as e:
            print("ok   --fix refused a strong reference: %s" % e)
        else:
            raise Refusal("--fix dropped a version a strong symbol needs")
        if strong.read_bytes() != before:
            raise Refusal("--fix wrote to the file it refused")
        print("ok   the file it refused is byte for byte unchanged")

        weak = compile_fixture(tmp, "weak", weak=True)
        if not over_floor(weak):
            raise Refusal("the weak fixture reads as under the floor")
        fix_elf(weak)
        if over_floor(weak):
            raise Refusal("GLIBC_2.39 survived --fix")
        if any(n.name == "GLIBC_2.39" for n in Elf(weak).needs()):
            raise Refusal("the GLIBC_2.39 record survived --fix")
        subprocess.run([str(weak)], check=True)
        print("ok   --fix dropped a weak reference and the binary still runs")
    print("selftest OK (floor %s)" % floor_version())
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("targets", nargs="*", metavar="PATH",
                    help="an ELF file, a .deb, or a directory of either")
    ap.add_argument("--fix", action="store_true",
                    help="lower the targets to the floor instead of reading them")
    ap.add_argument("--selftest", action="store_true",
                    help="prove the tool, on a box whose own glibc is too new")
    args = ap.parse_args(argv)
    try:
        if args.selftest:
            if args.targets:
                ap.error("--selftest takes no paths")
            return selftest()
        if not args.targets:
            ap.error("nothing to read")
        if args.fix:
            for t in args.targets:
                fix_elf(Path(t))
            return 0
        return check(args.targets)
    except Refusal as e:
        print("REFUSED: %s" % e, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
