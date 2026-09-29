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
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                return;
            }
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

/// Recomputes every sample in the capture from the frame index stored in its
/// own block, and returns the frame count if all of them match.
///
/// Deliberately does not trust `captures.frames`, `sequence`, or the order rows
/// happen to come back in. A block that was written at the wrong offset, on the
/// wrong channel, or after a gap fails here rather than passing on its own
/// internal consistency.
fn audit(conn: &Connection, capture_id: i64) -> u64 {
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
        for i in 0..frames as u64 {
            let want = Simulated::expected_sample(start as u64 + i, channel as u16).to_le_bytes();
            let at = i as usize * WIDTH;
            assert_eq!(
                &samples[at..at + WIDTH],
                &want[..WIDTH],
                "channel {channel} frame {} is not what the device produced",
                start as u64 + i
            );
        }
        per_channel[ch] += frames as u64;
        expect_sequence[ch] += 1;
    }

    assert!(
        per_channel.iter().all(|f| *f == per_channel[0]),
        "channels hold different amounts of audio: {per_channel:?}"
    );
    per_channel[0]
}

/// One kill, start to finish. Returns the recovered frame count.
fn kill_at(dir: &Path, iteration: u64, after: Duration) -> u64 {
    let path = dir.join(format!("kill-{iteration}.vcw"));
    let mut child = start(&path);
    let (recording_since, progress) = wait_until_recording(&mut child);

    // Sleep in slices and keep reading, because one number on the writer's own
    // progress line is what makes the floor below hold on a busy machine: the
    // real-time factor, audio produced over clock elapsed. The simulated source
    // is paced by a clock it does not own, so on a host that is compiling the
    // rest of the gate it falls behind, and a floor that treats wall-clock
    // seconds as seconds of audio fails by a few milliseconds for reasons that
    // have nothing to do with recovery. That is not hypothetical: 3.255 s of
    // clock against 3.000 s of audio, on this host, with the gate running.
    let mut rtf: Option<f64> = None;
    let until = Instant::now() + after;
    while Instant::now() < until {
        let slice = (until - Instant::now()).min(Duration::from_millis(50));
        if let Ok(line) = progress.recv_timeout(slice)
            && let Some((_, after_rtf)) = line.split_once("rtf ")
        {
            rtf = after_rtf
                .split(',')
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
    // one of them is the sample the generator would have produced.
    let audited = audit(project.conn(), record.id);
    assert_eq!(audited, record.frames, "the row overstates what is stored");

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

    // Three: the floor, against the audio that existed rather than against the
    // clock. `ran_for * rtf` is how much the source had produced by the kill,
    // taking the pacing the writer itself last reported; rtf is clamped at 1.0
    // because a source cannot outrun real time and a rounded 1.00001 should not
    // buy the product any slack. On an idle host rtf is 0.9999-something and
    // this is the tight bound the fifty-kill run established. On a starved one
    // it relaxes by exactly the amount of audio that was never made, which is
    // the only part of the failure that was ever about the scheduler.
    //
    // `--every 1` and a kill no earlier than 1.6 s in mean a progress line
    // always lands. If one did not, this check would silently not run, and a
    // check that turns itself off is not a check.
    let pacing = rtf
        .expect(
            "the writer printed no progress line before it was killed, so the \
             pacing-corrected floor could not be applied",
        )
        .min(1.0);
    let produced = ran_for.as_secs_f64() * pacing;
    assert!(
        recovered_secs >= produced - block_secs,
        "recovered {recovered_secs:.3} s of the {produced:.3} s the source had \
         produced ({:.3} s of clock at rtf {pacing:.5}); the floor allows \
         {block_secs:.3} s of loss, one commit block",
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
    assert_eq!(audit(project.conn(), record.id), record.frames);
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
