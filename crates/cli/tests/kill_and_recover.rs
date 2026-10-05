/*
 *  kill_and_recover.rs
 *
 *  VCW - The Vinyl Capture Workstation
 *  (c) 2026 Stue Hunter
 *
 *  WP-06's exit criterion: kill a capture at a random point, recover, verify.
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

//! WP-06's exit criterion: kill a capture at a random point, recover, verify.
//!
//! # Why this cannot be an in-process test
//!
//! Dropping a writer, which is what the unit tests do, reproduces a crash's
//! *database* state and not its *filesystem* state: `sqlite3_close` still runs,
//! and SQLite checkpoints and deletes the `-wal` and `-shm` when the last
//! connection to a file goes. A capture that was really killed leaves a hot log
//! nobody folded back, and replaying it is half of what recovery has to survive.
//! So the subject here is a real child process, killed with a real signal.
//!
//! # What is actually proven
//!
//! The child is `vcw soak`, whose source is deterministic: every sample is a
//! pure function of its frame and channel index. After the kill, every byte in
//! the project is recomputed from the frame index *stored in its own block* and
//! compared. That turns "recovery produced a plausible frame count" into
//! "recovery produced exactly the audio the device had delivered, at the offsets
//! it delivered them, and not one sample that was invented".
//!
//! # What is not proven
//!
//! `SIGKILL` ends a process; it does not cut power. The page cache survives, so
//! this exercises SQLite's crash recovery and not the storage stack's. Against
//! that, `synchronous=FULL` means every commit was fsynced before it returned,
//! so the difference should be nothing - but "should be" is the honest phrasing,
//! and closing the gap needs either real power cuts or a fault-injecting
//! filesystem. S2 has both on its open list.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use rusqlite::Connection;
use vcw_audio::source::Simulated;
use vcw_project::persistence::Config;
use vcw_project::recovery::{self, Sidecars};
use vcw_project::{Options, Project, session, validate};
use vcw_types::CaptureState;

const VCW: &str = env!("CARGO_BIN_EXE_vcw");
const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
/// `--format s32`: four bytes a sample, and the width the generator produces.
const WIDTH: usize = 4;
/// The soak's ring. Deliberately four blocks deep, so that if any of it were
/// being lost on a crash the shortfall would be unmissable.
const RING_MILLIS: u64 = 1_000;
/// D3's block, which is the commit granularity and so the whole of the loss.
const BLOCK_MILLIS: u64 = 250;

/// Starts a capture that will run far longer than we intend to let it.
fn start(path: &Path) -> Child {
    Command::new(VCW)
        .args([
            "soak",
            &path.display().to_string(),
            "--rate",
            &RATE.to_string(),
            "--channels",
            &CHANNELS.to_string(),
            "--format",
            "s32",
            "--minutes",
            "10",
            "--ring-millis",
            &RING_MILLIS.to_string(),
            "--every",
            "1",
            "--no-verify",
            // Off, and explicitly. `vcw soak` defaults to a 32 MiB growth gate
            // and refuses to start where resident memory cannot be read, which
            // is every platform without a procfs - so on Windows and macOS all
            // four tests here died before the first frame with
            // `this platform cannot report resident memory`. That refusal is
            // right: §41's gate must not read as a pass where it was never
            // applied. It is also nothing to do with recovery, which is what
            // this file is about.
            "--max-growth-mib",
            "0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn vcw soak")
}

/// How long to wait for a writer to say it is capturing before giving up on it.
///
/// Generous because the only thing it is protecting against is a hang: every
/// real answer, including a child that dies on startup, arrives as EOF long
/// before this. A plain `cargo test` has no timeout of its own, so without this
/// a writer that neither prints nor exits would hold a CI job until the job's
/// own limit.
const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// Blocks until the writer announces that it is capturing, and returns the
/// instant the announcement arrived.
///
/// # Why the clock cannot start at `spawn`
///
/// It used to, with 750 ms of allowance for start-up "for a Pi booting this off
/// an SD card". On a cold `ubuntu-24.04-arm` runner the real figure was 1.9 s,
/// so the test charged process start-up to recovery loss and reported
/// `recovered only 0.750 s of a 2.626 s capture`. On `macos-latest` the kill
/// landed before the writer had created the schema, so there was no hot log and
/// the assertion blamed the writer for it. One unmeasured term, two platforms,
/// three failures, and none of them reproducible on the dev box - where
/// start-up is tens of milliseconds and 750 ms looked like ample slack.
///
/// A measured signal has no such calibration. `vcw soak` prints its banner once
/// `persistence::spawn` has returned, which is to say once the capture is
/// already running, so the instant this returns is a little *after* the first
/// frame rather than before it. [`kill_at`] bounds the consequence in both
/// directions rather than pretending the gap is zero.
fn wait_until_recording(child: &mut Child) -> (Instant, mpsc::Receiver<String>) {
    let stdout = child.stdout.take().expect("stdout was piped");
    let (lines, arriving) = mpsc::channel();
    // Drains for the child's whole life, and keeps draining after the receiver
    // has gone rather than returning. Returning drops the `BufReader`, which
    // closes the read end of the pipe, and the writer prints a progress line
    // every second: the next one then fails, and `println!` panics on a failed
    // write, so the child died with
    // `failed printing to stdout: Broken pipe (os error 32)` and the test
    // blamed it for exiting on its own. A caller that wants only the banner
    // (`recovery_reports_before_it_writes` does) drops the receiver
    // immediately, which made that a race against the writer's own timer - won
    // on this host, lost on a loaded runner.
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = lines.send(line);
        }
    });

    loop {
        // Bound before the match so the receiver is not still borrowed by the
        // scrutinee when an arm hands it to the caller.
        let next = arriving.recv_timeout(READY_TIMEOUT);
        match next {
            Ok(line) if line.starts_with("soaking ") => return (Instant::now(), arriving),
            Ok(_) => {}
            // Disconnected means the child closed stdout, which for this binary
            // means it exited; Timeout means it is wedged. Neither is a hot log
            // to recover, and its stderr is the only thing that can say which.
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "the writer never reported that it was capturing:\n{}",
                    stderr_of(child)
                );
            }
        }
    }
}

/// Everything the child put on stderr. Only call it once the child is gone.
fn stderr_of(child: &mut Child) -> String {
    let mut text = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut text);
    }
    if text.trim().is_empty() {
        "(nothing on stderr)".to_owned()
    } else {
        text
    }
}

/// Panics unless the writer is still running, quoting it if it is not.
///
/// A child that died on its own is otherwise indistinguishable from one that was
/// killed: `kill` succeeds on a corpse and `status.success()` is false either
/// way. That is why macOS reported
/// `a killed writer should leave a hot log, found Sidecars { wal_bytes: 0 }`
/// instead of whatever the writer had to say for itself - the stderr was piped
/// and then thrown away. Asking first, and reading it, turns a wrong accusation
/// into a message.
fn still_running(child: &mut Child) {
    if let Some(status) = child.try_wait().expect("try_wait") {
        panic!(
            "the writer exited on its own with {status} instead of capturing \
             until it was killed:\n{}",
            stderr_of(child)
        );
    }
}

/// Nanosecond jitter as a seed. No dependency, and reproducibility is not the
/// point: the suite is looking for a kill point that breaks something, so a
/// different set of points every run is worth more than a fixed one.
fn seeded(n: u64) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(12_345);
    let mut x = nanos ^ (n.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x
}

/// Runs `vcw recover` as the operator would, and returns its stdout.
fn recover_cli(path: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(VCW)
        .arg("recover")
        .arg(path)
        .args(args)
        .output()
        .expect("run vcw recover");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// A span of audio the device never delivered, as found in the stored stream.
///
/// `at` is the stored frame the gap sits in front of: stored frame `at` holds
/// the sample the source produced for frame `at + frames`, and everything after
/// it is shifted by the same amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Gap {
    at: u64,
    frames: u64,
}

/// How far ahead of itself the stored stream may be found before the search
/// gives up. Five seconds is longer than any capture this file takes, so the
/// bound never decides an answer - it only stops the search on audio that is
/// not the source's at any offset.
const MAX_GAP_FRAMES: u64 = RATE as u64 * 5;

/// Frames that have to agree before a shift is believed. Every sample is a
/// 32-bit function of its own frame index, so one frame matching by chance is
/// already a one-in-four-billion coincidence; sixteen makes it unarguable, and
/// stops a gap being "found" at the first delta that happens to line up.
const RESYNC_WINDOW: u64 = 16;

/// Recomputes every sample in the capture from the frame index stored in its
/// own block, and returns the frame count together with the device-side gaps
/// that had to be allowed for.
///
/// Deliberately does not trust `captures.frames`, `sequence`, or the order rows
/// happen to come back in. A block written at the wrong offset, on the wrong
/// channel, or out of sequence fails here rather than passing on its own
/// internal consistency.
///
/// # Why a mismatch is not immediately a failure
///
/// The whole method rests on stored frame *n* holding the sample the source
/// produced for frame *n*, and one lost callback breaks that for the rest of the
/// run: the simulated source is paced by a clock it does not own, and when the
/// ring fills, a whole callback is discarded while the source's frame index
/// moves on. `Pace::Fast` says so in as many words, and it is the same
/// mechanism that makes `soak --starve-after` verify only up to the fault.
/// A busy CI runner reaches it without being asked: this is what
/// `channel 0 frame 48000 is not what the device produced` was, one ring's
/// worth into the run, on a runner compiling three other jobs.
///
/// Comparing past that point reports a mismatch on every remaining frame and
/// says nothing new, so the audit does what the product's verifier does and
/// re-synchronizes instead - but it has to *find* the shift rather than be told
/// it, which is the stronger claim: the audio after a gap is still exactly the
/// audio the source produced, at a named offset, so nothing was invented and
/// nothing was moved. A mismatch that no gap explains is still a failure, and
/// the gaps themselves are returned for the caller to hold the device to.
fn audit(conn: &Connection, capture_id: i64) -> (u64, Vec<Gap>) {
    let mut stmt = conn
        .prepare(
            "SELECT b.channel, b.sequence, b.start_frame, b.frame_count, s.samples
               FROM capture_blocks b JOIN sampleblocks s ON s.blockid = b.blockid
              WHERE b.capture_id = ?1 ORDER BY b.channel, b.sequence",
        )
        .expect("prepare");
    let rows = stmt
        .query_map([capture_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Vec<u8>>(4)?,
            ))
        })
        .expect("query");

    let mut per_channel = vec![0u64; CHANNELS as usize];
    let mut expect_sequence = vec![0i64; CHANNELS as usize];
    // Frames the device dropped before this channel's current position, and
    // where. Per channel and not global, because a callback carries every
    // channel: a real device-side gap therefore has to show up in all of them,
    // at the same stored frame and the same size, and that agreement is checked
    // rather than assumed.
    let mut shift = vec![0u64; CHANNELS as usize];
    let mut gaps: Vec<Vec<Gap>> = vec![Vec::new(); CHANNELS as usize];
    for row in rows {
        let (channel, sequence, start, frames, samples) = row.expect("row");
        assert!(
            (0..i64::from(CHANNELS)).contains(&channel),
            "block claims channel {channel}"
        );
        let ch = channel as usize;
        assert_eq!(
            sequence, expect_sequence[ch],
            "channel {channel} jumps from sequence {} to {sequence}",
            expect_sequence[ch]
        );
        assert_eq!(
            start as u64, per_channel[ch],
            "channel {channel} sequence {sequence} starts at {start}, expected {}",
            per_channel[ch]
        );
        assert_eq!(
            samples.len(),
            frames as usize * WIDTH,
            "channel {channel} sequence {sequence} declares {frames} frames \
             but holds {} bytes",
            samples.len()
        );
        let mut i = 0u64;
        while i < frames as u64 {
            let stored = start as u64 + i;
            let at = i as usize * WIDTH;
            let got = &samples[at..at + WIDTH];
            let want = Simulated::expected_sample(stored + shift[ch], channel as u16).to_le_bytes();
            if got == &want[..WIDTH] {
                i += 1;
                continue;
            }
            let Some(delta) = resync(&samples, i, frames as u64, start as u64, shift[ch], channel)
            else {
                panic!(
                    "channel {channel} frame {stored} is not what the device produced: \
                     stored {got:02X?}, the source's frame {} is {:02X?}, and no gap of \
                     up to {MAX_GAP_FRAMES} frames puts the source's audio here either",
                    stored + shift[ch],
                    &want[..WIDTH]
                );
            };
            gaps[ch].push(Gap {
                at: stored,
                frames: delta,
            });
            shift[ch] += delta;
        }
        per_channel[ch] += frames as u64;
        expect_sequence[ch] += 1;
    }

    assert!(
        per_channel.iter().all(|f| *f == per_channel[0]),
        "channels hold different amounts of audio: {per_channel:?}"
    );
    assert!(
        gaps.iter().all(|found| *found == gaps[0]),
        "the channels disagree about where the device dropped audio, which no \
         overrun can produce: {gaps:?}"
    );
    (per_channel[0], gaps.swap_remove(0))
}

/// The gap that puts the source's audio back under the stored frames, or `None`
/// if no gap does.
///
/// Searched rather than derived, because the counters say how much was dropped
/// and never where. `RESYNC_WINDOW` frames have to agree, so the answer is the
/// real shift and not the first delta that lines up on one sample.
fn resync(
    samples: &[u8],
    i: u64,
    frames: u64,
    start: u64,
    shift: u64,
    channel: i64,
) -> Option<u64> {
    let window = RESYNC_WINDOW.min(frames - i);
    (1..=MAX_GAP_FRAMES).find(|delta| {
        (0..window).all(|j| {
            let at = (i + j) as usize * WIDTH;
            let want = Simulated::expected_sample(start + i + j + shift + delta, channel as u16)
                .to_le_bytes();
            samples[at..at + WIDTH] == want[..WIDTH]
        })
    })
}

/// One kill, start to finish. Returns the recovered frame count.
fn kill_at(dir: &Path, iteration: u64, after: Duration) -> u64 {
    let path = dir.join(format!("kill-{iteration}.vcw"));
    let mut child = start(&path);
    let (recording_since, progress) = wait_until_recording(&mut child);

    // Sleep in slices and keep reading, because one number on the writer's own
    // progress line is what the floor below is built from: the audio it has
    // *committed*, which `Progress::frames` counts and this line prints as
    // "N.N s written". It is the only statement about this capture that is not
    // an estimate, and the floor needs one - the simulated source is paced by a
    // clock it does not own, so on a host that is compiling the rest of the gate
    // it falls behind, and a floor that treats wall-clock seconds as seconds of
    // audio fails for reasons that have nothing to do with recovery.
    let mut committed: Option<f64> = None;
    let until = Instant::now() + after;
    while Instant::now() < until {
        let slice = (until - Instant::now()).min(Duration::from_millis(50));
        if let Ok(line) = progress.recv_timeout(slice)
            && let Some((before, _)) = line.split_once(" s written")
        {
            committed = before
                .rsplit(' ')
                .next()
                .and_then(|number| number.parse().ok());
        }
    }
    still_running(&mut child);

    // SIGKILL on Unix, TerminateProcess on Windows. Either way the process gets
    // no chance to close the database, which is the condition under test.
    child.kill().expect("kill");
    let ran_for = recording_since.elapsed();
    let status = child.wait().expect("wait");
    assert!(!status.success(), "the child was supposed to be killed");

    // Before opening anything: connecting is what replays the log away.
    let sidecars = Sidecars::inspect(&path);
    assert!(
        sidecars.log_left_behind(),
        "a killed writer should leave a hot log, found {sidecars:?}"
    );

    let (ok, output) = recover_cli(&path, &["--apply", "--verify"]);
    assert!(ok, "vcw recover failed:\n{output}");
    assert!(
        output.contains("state recovered"),
        "recover did not report applying anything:\n{output}"
    );

    let project = Project::open(&path).expect("reopen");
    let unfinished = recovery::survey(project.conn()).expect("survey");
    assert!(
        unfinished.is_empty(),
        "still unfinished after recovery: {unfinished:?}"
    );

    let captures = session::all(project.conn()).expect("captures");
    assert_eq!(captures.len(), 1);
    let record = &captures[0];
    assert_eq!(record.state, CaptureState::Recovered);
    assert!(record.finished_at.is_some());
    assert!(!record.needs_recovery());

    // The frames the row claims are the frames that are really there, and every
    // one of them is the sample the generator would have produced - allowing
    // for audio the device never delivered, which `audit` has to find rather
    // than be told about.
    let (audited, gaps) = audit(project.conn(), record.id);
    assert_eq!(audited, record.frames, "the row overstates what is stored");
    let lost: u64 = gaps.iter().map(|gap| gap.frames).sum();

    let report = validate(
        &project,
        Options {
            verify_checksums: true,
        },
    )
    .expect("validate");
    assert!(report.is_clean(), "{:?}", report.findings);

    // Loss is bounded by commit granularity: across fifty kills on an idle host
    // every recovered length came back an exact multiple of the 250 ms block
    // with a 1000 ms ring in play the whole time, so the ring is not part of
    // the loss. A writer that keeps up drains it before the crash matters,
    // which is what S1 concluded.
    //
    // Three checks, because they can each carry a different part of that.
    let recovered_secs = audited as f64 / f64::from(RATE);
    let block_secs = BLOCK_MILLIS as f64 / 1_000.0;
    let ring_secs = RING_MILLIS as f64 / 1_000.0;
    let block_frames = u64::from(RATE) * BLOCK_MILLIS / 1_000;

    // A gap belongs to the device and not to recovery, so it is reported rather
    // than tolerated silently, and one thing still has to hold: §15 says a
    // capture that lost audio must not read as a flawless one, which is WP-17's
    // finding in the one place that can still produce it - the counters reach
    // the row on the writer's own timer, and this writer was killed. A gap
    // inside the last commit block is exempt for exactly that reason; anything
    // earlier had time to be noted.
    if !gaps.is_empty() {
        println!(
            "  the device dropped {lost} frames in {} gap(s): {gaps:?}",
            gaps.len()
        );
        //
        // `is_clean` rather than `dropped_frames`, because the two counters
        // divide the loss honestly and only one of them can count it: a ring
        // overrun discards a callback we were handed, so its frames are known
        // and counted, while a device that delivers an empty callback has told
        // us nothing about what it skipped. `soak --starve-after 1.0` proves
        // exactly that - the audit finds a 480-frame gap at frame 48000 against
        // `Diagnostics { underruns: 1, dropped_frames: 0 }` - and §15's claim
        // is the same in both cases: not a flawless capture.
        //
        // The allowance is the writer's own diagnostics interval, taken from
        // the config rather than written down here, plus the commit block. Both
        // terms are earned: the counters reach the row on that timer, so a
        // capture killed less than an interval after a fault genuinely has four
        // zeros in it, which `--starve-after 1.0` with a kill at 1.76 s
        // produces. That is the documented cost of not fsyncing a counter row
        // eight times a second, and `capture_diagnostics.updated_at` is what
        // recovery reads to report the staleness rather than hide it.
        let interval = u64::from(Config::default().diagnostics_millis);
        let noticed_by = block_frames + u64::from(RATE) * interval / 1_000;
        if gaps.iter().any(|gap| gap.at + noticed_by < audited) {
            assert!(
                !record.diagnostics.is_clean(),
                "the device dropped {lost} frames more than {interval} ms before \
                 the kill and the row calls the capture clean: {:?}",
                record.diagnostics
            );
        }
    }

    // One: the timing-free one, and the one that actually says "commit
    // granularity". A block is committed whole or not at all, and a killed
    // writer never gets to flush the part-filled one, so what survives is a
    // whole number of blocks however loaded the machine was. A recovery that
    // truncated to the nearest anything else, or that kept a partial block it
    // had not committed, fails here and cannot hide in a tolerance.
    assert_eq!(
        audited % block_frames,
        0,
        "recovered {audited} frames, which is not a whole number of \
         {block_frames}-frame commit blocks",
    );

    // Two: the ceiling, which is two-sided because the announcement and the
    // first frame are not the same instant. The writer's ring is already
    // filling while the project is created and the banner comes after that, so
    // the stored audio can begin up to a ring's worth *before* this clock
    // started. That is a check on overstatement, not on loss.
    assert!(
        recovered_secs <= ran_for.as_secs_f64() + ring_secs + block_secs,
        "recovered {recovered_secs:.3} s from a capture that ran {:.3} s, \
         which is more than the ring can account for",
        ran_for.as_secs_f64()
    );

    // Three: the floor, and the one thing here that is not an estimate. The
    // writer prints the audio it has committed, so recovery is required to come
    // back with all of it: anything less is audio that reached the database and
    // was then truncated away, which is the failure this whole file exists to
    // rule out. Nothing is subtracted for the block that was in flight at the
    // kill, because an uncommitted block was never announced.
    //
    // The earlier form was `ran_for * rtf - block_secs`, and it was unsound in
    // both directions. `rtf` is committed audio over the *soak's* elapsed time,
    // which starts at the project rather than at the banner `ran_for` measures
    // from, so the two were an apples-to-oranges ratio: on this host it read
    // 0.749 on a run that recovered 1.750 s of 1.750 s, a floor 0.44 s below
    // where it belonged. And on the one random kill point in six where the
    // startup offset happened to be small it read 0.9985, putting the floor at
    // 1.5004 s against a recovery of 1.500 s - a failure by 0.4 ms on a block
    // that had filled a hair before the kill and was genuinely still
    // committing. A bound that tight on an estimate that loose is two bugs that
    // canceled.
    //
    // What is given up with the estimate is tightness: the line lands once a
    // second, so a kill can be up to that much past the last announcement. The
    // *magnitude* of crash loss is not claimed here at all - check one claims
    // the shape of it, and `recovery_survives_twenty_kills` is the sign-off run.
    //
    // `--every 1` and a kill no earlier than 1.6 s in mean a progress line
    // always lands. If one did not, this check would silently not run, and a
    // check that turns itself off is not a check.
    let announced = committed.expect(
        "the writer printed no progress line before it was killed, so there is \
         no committed figure to hold recovery to",
    );
    // Audio the device dropped never reached the writer, so it is not in the
    // figure the writer printed and nothing is owed for it here. It is reported
    // because a run that lost frames is the run this floor is read on.
    let lost_secs = lost as f64 / f64::from(RATE);
    // One decimal on the line, so the truth is within 50 ms of it and the floor
    // takes the low end. This is the only tolerance in the check and it is the
    // print format, not a fudge.
    assert!(
        recovered_secs >= announced - 0.05,
        "recovered {recovered_secs:.3} s, but the writer had already announced \
         {announced:.1} s committed before it was killed {:.3} s in, with \
         {lost_secs:.3} s dropped by the device",
        ran_for.as_secs_f64()
    );

    project.close().expect("close");
    audited
}

#[test]
fn a_capture_killed_at_a_random_point_recovers_every_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut recovered = Vec::new();
    for i in 0..6 {
        // Between 1.6 s and 4.0 s: past the first few commits, and different
        // every run so the kill lands in a different part of the write cycle.
        let after = Duration::from_millis(1_600 + seeded(i) % 2_400);
        recovered.push((after, kill_at(dir.path(), i, after)));
    }
    // Not an assertion so much as a record: if this ever fails, the numbers are
    // what someone will want to see.
    println!("kill points and recovered frames: {recovered:?}");
    assert!(recovered.iter().all(|(_, frames)| *frames > 0));
}

#[test]
fn a_capture_killed_before_it_committed_anything_still_recovers() {
    // The other end of the range. Killed inside the first block, the project
    // holds a session row and no audio at all, and recovery still has to close
    // it rather than leaving a file that asks about it on every launch.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("instant.vcw");
    // Killed the instant the writer says it is capturing, rather than 120 ms
    // after the process was asked to exist. The old form could not tell a
    // too-fast machine from a too-slow one, so it guarded itself with
    // `if !path.exists() { return }` - and a runner that had not finished
    // creating the project inside 120 ms did not fail here, it passed here
    // having asserted nothing at all. The banner is printed after
    // `Project::create` returns and one commit block before the first block is
    // written, which is the window this test wants and now always gets.
    let mut child = start(&path);
    let _ = wait_until_recording(&mut child);
    still_running(&mut child);
    child.kill().expect("kill");
    let _ = child.wait();

    assert!(
        path.exists(),
        "the writer announced a capture without leaving a project behind"
    );

    let (ok, output) = recover_cli(&path, &["--apply", "--verify"]);
    assert!(ok, "vcw recover failed:\n{output}");

    let project = Project::open(&path).expect("reopen");
    assert!(recovery::survey(project.conn()).expect("survey").is_empty());
    let report = validate(
        &project,
        Options {
            verify_checksums: true,
        },
    )
    .expect("validate");
    assert!(report.is_clean(), "{:?}", report.findings);
}

#[test]
fn recovery_reports_before_it_writes() {
    // §15 says detect and *offer*. A dry run has to leave the project exactly
    // as it found it, including still asking about it.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("offered.vcw");
    let mut child = start(&path);
    wait_until_recording(&mut child);
    std::thread::sleep(Duration::from_millis(1_800));
    still_running(&mut child);
    child.kill().expect("kill");
    let _ = child.wait();

    let before = Sidecars::inspect(&path);
    assert!(before.log_left_behind(), "no hot log to dry-run against");

    let (ok, output) = recover_cli(&path, &[]);
    assert!(ok, "the dry run failed:\n{output}");
    assert!(
        output.contains("nothing written"),
        "a dry run must say so:\n{output}"
    );

    // Pinning the honest half of that claim. Nothing was written to the
    // database, but the hot log is gone, because opening a SQLite file replays
    // it and closing folds it in. A dry run is a report, not a snapshot, and
    // this asserts it so nobody can quietly start believing otherwise.
    let after = Sidecars::inspect(&path);
    assert!(
        !after.present(),
        "the dry run left sidecars behind, so this comment is now wrong: {after:?}"
    );
    assert!(
        !output.contains("state recovered"),
        "a dry run must not apply anything:\n{output}"
    );

    let project = Project::open(&path).expect("reopen");
    let still = recovery::survey(project.conn()).expect("survey");
    assert_eq!(still.len(), 1, "the dry run consumed the capture");
    assert_eq!(still[0].state, CaptureState::Recording);
    drop(project);

    // And the counters the writer persisted on its timer are there, which is
    // the whole reason for that timer: without it a killed capture reads as
    // four zeros, which is the spelling of a flawless one.
    let project = Project::open(&path).expect("reopen");
    let assessment = &recovery::survey(project.conn()).expect("survey")[0];
    assert!(
        assessment.diagnostics_at >= assessment.started_at,
        "the counters were never written during the capture"
    );
}

#[test]
fn stranded_audio_is_not_discarded_without_being_asked() {
    // D4 through the CLI: --apply refuses to lose a block, --repair is how an
    // operator says they accept it.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("stranded.vcw");
    let mut child = start(&path);
    wait_until_recording(&mut child);
    std::thread::sleep(Duration::from_millis(2_000));
    still_running(&mut child);
    child.kill().expect("kill");
    let _ = child.wait();

    {
        let conn = Connection::open(&path).expect("open");
        let last: i64 = conn
            .query_row(
                "SELECT blockid FROM capture_blocks WHERE channel = 1
                 ORDER BY sequence DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .expect("blockid");
        conn.execute("DELETE FROM capture_blocks WHERE blockid = ?1", [last])
            .expect("delete");
        conn.execute("DELETE FROM sampleblocks WHERE blockid = ?1", [last])
            .expect("delete");
    }

    let (ok, output) = recover_cli(&path, &["--apply"]);
    assert!(!ok, "--apply should have refused:\n{output}");

    let (ok, output) = recover_cli(&path, &["--repair", "--verify"]);
    assert!(ok, "--repair failed:\n{output}");
    assert!(output.contains("block(s) removed"), "{output}");

    let project = Project::open(&path).expect("reopen");
    assert!(recovery::survey(project.conn()).expect("survey").is_empty());
    let record = &session::all(project.conn()).expect("all")[0];
    // A repair removes the stranded block, so what is left is a shorter run of
    // the same audio: the audit must still recompute every byte of it. Any gap
    // is reported rather than asserted against - this is a real-time source, so
    // whether the ring overran is a fact about the machine, and the claim being
    // made here is that the audio either side of one is still the source's.
    let (audited, gaps) = audit(project.conn(), record.id);
    assert_eq!(audited, record.frames);
    if !gaps.is_empty() {
        println!("  the device dropped audio during the run: {gaps:?}");
    }
}

/// The stress version. Twenty kills rather than six, spread over a wider range
/// of offsets. Ignored by default because it takes about a minute; run it with
/// `cargo test -p vcw-cli -- --ignored` before calling recovery done on a new
/// platform.
#[test]
#[ignore = "takes about a minute; the platform sign-off run"]
fn recovery_survives_twenty_kills() {
    let dir = tempfile::tempdir().expect("tempdir");
    for i in 0..20 {
        let after = Duration::from_millis(400 + seeded(i + 100) % 4_600);
        let frames = kill_at(dir.path(), i, after);
        println!("kill {i} after {after:?}: {frames} frames recovered");
    }
}
