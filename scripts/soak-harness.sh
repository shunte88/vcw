#!/usr/bin/env bash
#
#  soak-harness.sh
#
#  VCW - The Vinyl Capture Workstation
#  (c) 2026 Stue Hunter
#
#  The WP-17 soak harness: the legs of REQUIREMENTS 41, run by CI and by hand.
#
#  MIT License - see the header in any Rust source file for the full text.
#
# Usage:
#   scripts/soak-harness.sh short <dir>    every push: seconds per leg, metered
#   scripts/soak-harness.sh nightly <dir>  scheduled: the real-time long run
#
# There is one script rather than a list of steps in the workflow file so that
# the thing CI runs is the thing a person can run, on the machine where the
# failure happened. A harness only reachable through a YAML file is a harness
# nobody reproduces.
#
# Every leg ends in `vcw soak`, which exits non-zero when its own verdict fails,
# so the gates live in the binary and not here. What lives here is the choice of
# runs - and the reason each one is worth making.

set -u

mode=${1:?usage: soak-harness.sh short|nightly <dir>}
dir=${2:?usage: soak-harness.sh short|nightly <dir>}
vcw=${VCW:-./target/release/vcw}

mkdir -p "$dir"
status=0

# 48 kHz throughout, not 192. The rate a leg runs at decides how much disk it
# eats, and a 24/192 stereo capture is 1.15 MB/s: an hour of it is 4 GB, which
# is most of a CI runner's free space. The properties these legs check - loss,
# growth, WAL bound, byte fidelity - do not depend on the rate. The rates that
# do need checking are checked by the format tests, and 24/192 endurance is
# checked on real hardware, where there is a disk for it.
leg() {
    name=$1
    shift
    project="$dir/$name.vcw"
    rm -f "$project" "$project-wal" "$project-shm"
    printf '%-12s ' "$name"
    if "$vcw" soak "$project" --rate 48000 --every 0 "$@" >"$dir/$name.log" 2>&1; then
        echo "OK   $(grep -E '^  verdict' "$dir/$name.log" | sed 's/^  verdict *//')"
    else
        echo "FAILED"
        cat "$dir/$name.log"
        status=1
    fi
    # The project, not the log: a passing leg's 100 MB file has nothing left to
    # say, and a runner that filled its disk on leg three is a useless red.
    rm -f "$project" "$project-wal" "$project-shm"
}

# A small WAV, written here rather than committed. §41 wants file-backed
# capture covered on every push, and a CI runner has no vinyl corpus - but the
# property being checked is that whatever bytes the container holds come back
# out of the project unchanged, and synthesised bytes test that as well as
# recorded ones. The real rips are fed through $VCW_RIP below, where they exist.
#
# Deliberately not a canonical 44-byte header: it carries a LIST chunk in front
# of the audio, which is what a tagged rip looks like and what a reader that
# assumed offset 44 would get silently wrong.
synth_wav() {
    python3 - "$1" <<'PYEOF'
import struct, sys, random

rate, channels, bits = 48000, 2, 32
frames = rate * 3
block = channels * bits // 8
random.seed(17)

fmt = struct.pack("<HHIIHH", 1, channels, rate, rate * block, block, bits)
# A tagger's chunk, odd-length on purpose so the pad byte is exercised.
info = b"INFOISFT" + b"vcw soak harness"
audio = bytes(random.getrandbits(8) for _ in range(frames * block))

chunks = b"".join([
    b"fmt " + struct.pack("<I", len(fmt)) + fmt,
    b"LIST" + struct.pack("<I", len(info)) + info + (b"\0" if len(info) % 2 else b""),
    b"data" + struct.pack("<I", len(audio)) + audio,
])
with open(sys.argv[1], "wb") as out:
    out.write(b"RIFF" + struct.pack("<I", 4 + len(chunks)) + b"WAVE" + chunks)
PYEOF
}

case "$mode" in
short)
    # Metered pace, so these are throughput runs and not timing runs. The
    # commit budget is still gated, but a shared runner's tail is not evidence
    # about a real capture's tail - that is what `nightly` is for.

    # Zero loss and every byte recomputed. The one leg that would catch a
    # writer that dropped, reordered or misplaced a block.
    leg clean --minutes 0.25 --fast

    # The window redrawing while the capture runs, four times over. Proves the
    # readers get answers and the WAL stays inside its budget with them there.
    leg contention --minutes 0.25 --readers 4

    # R9's four shapes of device failure. A fault leg's pass condition is the
    # opposite of a clean leg's: the fault has to show up, and the damage has
    # to stop at it. `--vanish-after` is the one that found a defect - a device
    # that goes silent leaves all four counters at zero, and before WP-17 the
    # capture was filed as `finalised`.
    leg vanish --minutes 0.25 --fast --vanish-after 5
    leg unplug --minutes 0.25 --fast --unplug-after 5
    leg error --minutes 0.25 --fast --error-after 5
    leg starve --minutes 0.25 --fast --starve-after 5

    # File-backed capture: real container bytes in, the same bytes out, with
    # the rate, channel count and format taken from the file rather than from
    # the flags. The verifier re-reads the file, so a feeder that wrapped at the
    # wrong byte or skipped the wrong number of header bytes fails here.
    synth_wav "$dir/synth.wav"
    leg from-file --minutes 0.25 --fast --from-file "$dir/synth.wav"
    rm -f "$dir/synth.wav"

    # And the corpus, where there is one. Not in CI: /data2/source_rips is 62
    # real rips and tens of gigabytes, and a test that needs them cannot be the
    # test that runs on every push.
    if [ -n "${VCW_RIP:-}" ] && [ -r "${VCW_RIP:-}" ]; then
        leg from-rip --minutes 0.25 --fast --from-file "$VCW_RIP"
    else
        printf '%-12s %s\n' from-rip "skipped (set VCW_RIP to a WAV from the corpus)"
    fi
    ;;
nightly)
    # Real-time pace and an hour of it, which is the only way to see the things
    # a fast run cannot: memory over thousands of commits, the WAL over
    # hundreds of checkpoint attempts, and a real album side's worth of file.
    #
    # The timing numbers from this leg are worth reading and not worth gating
    # hard: a hosted runner shares its CPU and its disk, so a commit tail
    # measured here says as much about the neighbours as about the writer.
    # `--minutes 60` against a 250 ms budget leaves an order of magnitude of
    # headroom, which is why the budget can stay on: it takes a genuinely
    # pathological runner to breach it, and that is worth seeing too.
    leg hour --minutes 60

    # The same hour with the window open. The pairing is the point - run
    # against `hour` above, it is the only way to attribute a difference in
    # WAL peak or commit tail to the readers rather than to the machine.
    leg hour-contended --minutes 60 --readers 4
    ;;
*)
    echo "unknown mode: $mode" >&2
    exit 2
    ;;
esac

exit $status
