# Vinyl Capture Workstation
## Requirements and Architecture
**Status:** Initial Requirements  
**Project Type:** Open Source  
**Core:** Rust  
**UI:** TypeScript / React  
**Audio I/O:** CPAL  
**Fingerprinting:** chromaprint-next  
**Project Storage:** SQLite, modeled on Audacity AUP4/AUP3 principles  
**Primary Platforms:** Linux, Windows, macOS  
**Future Platform:** Android
## 1. Purpose
The project is a complete cross-platform workstation for capturing, identifying, editing, cataloging, and exporting vinyl recordings.
It evolves VRipr from an Audacity companion into a self-contained vinyl recording environment while retaining and extending VRipr's metadata, track-detection, tagging, and export capabilities.
The application owns the complete workflow:
`Audio Device -> Capture -> Analysis -> Identification -> Editing -> Metadata -> Track Splitting -> Tagged Masters`
Audacity is not required.
The core principle is:
> Capture the source accurately once. Everything after capture is non-destructive.
## 2. Architectural Rule
All application functionality other than presentation shall be implemented in Rust.
TypeScript/React is strictly a view and interaction layer.
The frontend shall contain no:
Required: audio processing; capture or playback logic; waveform analysis; fingerprinting; track detection; metadata processing; project persistence; export processing; authoritative application state.
Rust owns the application. React displays and controls it.
## 3. Primary Goals
The application shall:
1. Capture stereo audio directly from a selected audio interface.
2. Preserve incoming PCM without alteration wherever hardware and OS permit.
3. Provide live input monitoring and VU metering.
4. Display the waveform progressively during recording.
5. Provide bit-perfect/native playback where supported.
6. Automatically detect probable track boundaries.
7. Generate Chromaprint fingerprints during capture using `chromaprint-next`.
8. Attempt to identify tracks progressively using AcoustID/MusicBrainz.
9. Retrieve release metadata from MusicBrainz and Discogs.
10. Combine fingerprint, timing, metadata, and signal evidence.
11. Allow manual metadata search and correction.
12. Allow track boundaries to be visually edited.
13. Export individual tracks in lossless and compressed formats.
14. Embed metadata and cover artwork.
15. Persist the complete recording and editing session in a single SQLite project.
16. Recover safely from application or system failure.
17. Operate without an external audio editor or fingerprint executable.
## 4. Design Principles
### 4.1 Non-destructive
Original captured PCM is immutable.
Track markers, metadata, edits, fades, gain changes, and future processing instructions are project data and never modify the original capture.
### 4.2 Capture First
Nothing may compromise the real-time capture path.
Network access, fingerprint lookup, waveform construction, UI updates, metadata operations, and export run asynchronously.
If analysis falls behind, analysis waits. Recording does not.
### 4.3 Local First
Recording shall not depend on Internet access or external services.
Discogs, MusicBrainz, and AcoustID enhance a recording but are never prerequisites for capture or editing.
### 4.4 Recoverable
Captured audio is committed incrementally.
A UI crash, network failure, metadata failure, or application termination must not destroy an otherwise valid recording.
### 4.5 Cross-platform Rust Core
Platform-specific behavior shall be isolated behind Rust interfaces.
The core engine must remain independently testable and usable without the graphical frontend.
## 5. High-Level Architecture
```text
React / TypeScript
        |
        | commands / events / view models
        v
+------------------------------------------------+
|                  Rust Core                     |
|                                                |
| Application Controller / State                 |
| Project / SQLite / Recovery                    |
| Metadata / Identification / Evidence Resolver  |
| Fingerprinting / chromaprint-next              |
| Signal Analysis / Waveform / Metering          |
| Capture / Playback / Device Management         |
| Export / Encoding / Tagging                    |
+-----------------------+------------------------+
                        |
                       CPAL
                        |
       ALSA/PipeWire | CoreAudio | WASAPI | AAudio
```
A Tauri 2 shell is the preferred initial application container, provided the Rust core remains independent of Tauri.
## 6. Proposed Cargo Workspace
```text
crates/
    audio/
        capture
        playback
        devices
        buffers
    signal/
        meter
        waveform
        silence
        spectral
        hmm
    fingerprint/
        chromaprint-next
        acoustid
    identify/
        evidence
        resolver
        candidate
        confidence
    metadata/
        discogs
        musicbrainz
        genres
        artwork
    project/
        sqlite
        session
        disc
        side
        track
        persistence
        recovery
    export/
        splitter
        encoder
        tagging
    core/
        engine
        commands
        events
        state
app/
    src-tauri/
    ui/
```
## 7. Audio Device Management
The application shall enumerate available input and output devices and expose, where available:
Required: device name and identifier; host/backend; input/output capability; channel count; sample formats; sample rates; buffer sizes; default configuration.
Capture and playback devices may be selected independently.
Preferred devices shall persist between sessions.
Device removal or configuration change shall be handled without corrupting the project.
## 8. Capture Configuration
The application shall support hardware-provided sample rates including:
44.1, 48, 88.2, 96, 176.4, and 192 kHz.
Supported PCM representations shall include, where hardware permits:
Required: 16-bit integer; 24-bit integer; 32-bit integer; 32-bit floating point.
The UI shall expose only configurations supported by the selected device.
A sensible archival default is 24-bit / 96 kHz but shall not be imposed.
## 9. Bit-Perfect Capture
Bit-perfect has an explicit meaning.
The application shall attempt to capture samples exactly as supplied by the selected audio device without:
Required: resampling; gain adjustment; normalization; DSP; format conversion; channel mixing; software volume modification.
The application shall report requested and negotiated configurations and whether known conversion exists in the active path.
Capture modes may include:
```rust
enum CaptureMode {
    Shared,
    Native,
    Exclusive,
}
```
The application must never claim bit-perfect operation solely because CPAL is in use.
## 10. Real-Time Capture Pipeline
The CPAL callback shall perform the minimum possible work.
It shall not perform filesystem I/O, networking, encoding, fingerprint lookup, database transactions, UI communication, or large allocation.
```text
CPAL callback
     |
     v
Bounded / lock-free PCM distribution
     |
     +--> SQLite audio writer
     +--> Meter worker
     +--> Waveform worker
     +--> Track detector
     +--> Fingerprint worker
```
Capture has absolute priority.
Overruns, underruns, dropped frames, and stream errors shall be counted and persisted.
## 11. Recording State
The transport shall provide:
`RECORD | PAUSE | RESUME | STOP`
Recording follows an explicit state machine:
`Idle -> Armed -> Recording <-> Paused -> Stopped`
Invalid transitions shall be impossible.
Stopping finalizes the current capture but does not close the project.
## 12. Project File Format
The project shall use a **single-file SQLite project database**, following the architectural principles of Audacity's AUP3/AUP4 project storage.
The preferred extension is project-specific and shall be selected before implementation.
The design target is **AUP4-like**, while retaining the proven SQLite storage and recovery characteristics introduced by AUP3.
The application shall not depend upon Audacity itself.
Binary compatibility with Audacity AUP4 is desirable only if practical and documented. The primary requirement is architectural compatibility: a unified SQLite project containing audio blocks and complete project state.
The project database shall contain:
Required: project configuration; source PCM audio blocks; capture/session records; tracks and clips; disc and side topology; waveform summaries; track markers; fingerprint data; identification evidence; metadata; artwork or artwork references; edit instructions; export settings; application state required for recovery; schema and project format versions.
The project shall be self-contained and movable as a single file.
## 13. SQLite Audio Capture
PCM audio shall be written directly into SQLite during capture using a block-oriented mechanism inspired by AUP3/AUP4.
Audio shall be divided into manageable immutable blocks rather than accumulated into one in-memory recording.
Conceptually:
```text
audio_blocks
    block_id
    capture_id
    channel
    sequence
    sample_count
    sample_format
    sample_rate
    pcm_blob
    checksum
```
The exact schema will be benchmarked before stabilization.
Blocks shall be written sequentially and referenced by the logical project timeline.
Audio blocks already committed shall never be rewritten merely because a marker or edit changes.
## 14. SQLite Transaction Strategy
SQLite writes must never occur on the CPAL callback.
The callback feeds a bounded PCM buffer.
A dedicated Rust capture writer consumes PCM and performs batched SQLite transactions.
The database shall use an appropriate journaling mode, expected initially to be WAL, subject to capture benchmarks.
Transaction size shall balance:
Required: sustained write throughput; recovery granularity; WAL growth; database contention; storage latency.
Database checkpoints must not interrupt real-time capture.
## 15. Project Recovery
The project database shall maintain recoverable state continuously.
On abnormal termination the next launch shall detect unfinished sessions and offer recovery.
Recovery shall reconstruct the recording from committed audio blocks and persisted project state.
The database shall support integrity checking and recovery diagnostics.
WAL/SHM handling shall be treated as part of the project lifecycle while a project is open.
Clean shutdown shall checkpoint and leave the project in a consistent state.
## 16. Project Versioning
The database shall contain explicit:
Required: schema version; application project-format version; creation version; last-written version.
Schema migrations shall be transactional.
Newer application versions should upgrade older projects without destroying the original.
Backward compatibility policy shall be documented before the first stable release.
## 17. Live Metering
Stereo meters shall operate during monitoring and recording.
Required measurements:
Required: instantaneous peak; RMS; peak hold; clipping indicator.
Future measurements may include LUFS and true peak.
The UI should update approximately 30-60 times per second using Rust-generated snapshots.
## 18. Recording Level Setup
Pre-record monitoring shall allow analog gain to be set before capture.
Clipping shall latch visually.
The application shall not digitally alter incoming levels while operating in native/bit-perfect capture mode.
## 19. Waveform Generation
Waveform data shall appear progressively during recording.
Raw PCM shall never be streamed wholesale to React.
Rust shall create multi-resolution waveform summaries suitable for the current zoom level.
Waveform summaries may be persisted in SQLite and regenerated from source PCM when required.
## 20. Waveform Editor
The editor shall support:
Required: horizontal zoom and pan; playhead and time ruler; stereo waveform; track regions; candidate and confirmed boundaries; draggable markers; selections; audition; zoom to selection/track; keyboard navigation.
Markers shall retain provenance and confidence.
## 21. Playback
Playback shall use the Rust audio engine and CPAL.
Required operations:
`PLAY | PAUSE | STOP | SEEK | SKIP FORWARD | SKIP BACK`
Playback shall support:
Required: complete capture; selected region; individual track; boundary audition.
Native/bit-perfect playback should be available where supported.
## 22. Track Detection
Existing VRipr detection approaches shall be migrated and refactored into the signal crate:
Required: RMS energy; spectral flatness; adaptive HMM.
Live analysis creates provisional markers.
A post-capture pass may refine them using the complete recording.
Detection may incorporate expected track count, release durations, fingerprints, side topology, and confirmed markers.
## 23. Evidence Model
Analysis subsystems shall publish observations rather than directly modifying tracks.
```rust
enum AudioObservation {
    Level(LevelObservation),
    Silence(SilenceObservation),
    Boundary(BoundaryObservation),
    Fingerprint(FingerprintObservation),
    Identification(IdentificationObservation),
    Clip(ClipObservation),
}
```
An evidence resolver combines observations into project decisions.
## 24. Track Boundary Evidence
A boundary shall include position, confidence, provenance, and supporting evidence.
Potential sources:
Required: silence; spectral change; HMM; fingerprint transition; metadata duration; release topology; user confirmation.
User-confirmed/locked boundaries shall not be moved by automatic analysis.
## 25. Fingerprinting
Fingerprinting shall be implemented entirely in Rust using `chromaprint-next`.
No external Chromaprint executable, subprocess, temporary WAV, or non-Rust fingerprint engine shall be required.
PCM shall be fed asynchronously from the capture pipeline to the fingerprint worker.
Candidate regions shall be fingerprinted progressively rather than repeatedly fingerprinting the entire recording.
## 26. Progressive Identification
Fingerprint results shall be resolved through AcoustID and MusicBrainz.
Identification is evidence-based rather than a single-match decision.
Multiple identified tracks may constrain the likely:
Required: artist; album; release; side; track position; pressing.
Preselected release metadata may conversely constrain fingerprint interpretation.
Automatic identification shall never silently replace user-confirmed metadata.
## 27. Identification Engine
Fingerprinting answers:
> What recording does this audio resemble?
Identification answers:
> Given all current evidence, what release, side, and track are being recorded?
The `identify` crate shall therefore remain separate from `fingerprint`.
## 28. Metadata Providers
The metadata layer shall provide common Rust provider interfaces.
Initial providers:
Required: Discogs; MusicBrainz; AcoustID.
Search criteria may include:
Required: artist; album; catalog number; barcode; label; year; country; provider release ID; fingerprint evidence.
Results should expose sufficient information to distinguish vinyl pressings.
## 29. Vinyl Data Model
Vinyl topology shall be first-class.
```text
Project
  Release
  Disc 1
    Side A
      Capture
      A1
      A2
    Side B
      Capture
      B1
      B2
  Disc 2
    Side C
    Side D
```
Multi-disc releases shall be supported.
VRipr's alpha track numbering shall be retained.
## 30. Recording Workflow
Typical workflow:
`New Project -> Select Device -> Select Format -> Monitor Level -> Optional Release Search -> Record Side -> Live Analysis -> Stop -> Flip -> Record Next Side -> Refine Boundaries -> Confirm Metadata -> Export`
Metadata lookup may occur before, during, or after capture.
## 31. Track Editing
Users shall be able to:
Required: add/delete/move boundaries; split/merge tracks; rename tracks; renumber tracks; assign side/disc; lock/unlock boundaries.
All edits are non-destructive SQLite project records.
## 32. Metadata and Artwork
The project shall retain:
Required: artist; album artist; album; title; track/disc/side numbers; year; genre/style; label; catalog number; composer; comments; artwork; MusicBrainz identifiers; Discogs release ID; fingerprint/identification references.
VRipr genre normalization shall be retained.
## 33. Export
Initial export formats:
Required: FLAC; WAV; MP3; OGG.
Potential later formats:
Required: ALAC; AAC; Opus.
Export operates from immutable source blocks plus project edit instructions.
Export shall support metadata, artwork, and configurable naming templates.
## 34. Frontend Responsibilities
React/TypeScript shall provide:
Required: project browser; capture workspace; transport controls; meters; waveform display; track editor; metadata browser; export UI; settings.
React owns only ephemeral view state such as zoom, scroll position, open panels, and temporary form state.
## 35. Rust/UI Communication
Commands perform state-changing operations.
Events expose asynchronous state.
Example commands:
`start_recording`, `pause_recording`, `stop_recording`, `play`, `seek`, `move_marker`, `search_metadata`, `select_release`, `export`.
Example events:
`meter-update`, `waveform-update`, `recording-position`, `track-detected`, `fingerprint-match`, `capture-warning`, `export-progress`.
High-frequency PCM shall never cross the Rust/JavaScript boundary.
## 36. Background Workers
The Rust application may use dedicated threads and Tokio as appropriate.
Principal workers:
Required: audio real-time callback; SQLite capture writer; playback engine; meter worker; waveform worker; track detector; fingerprint worker; identification resolver; metadata worker; project/recovery worker; export worker.
Task failure shall be isolated wherever possible.
## 37. Performance Requirements
The application shall target:
Required: zero sample loss under normal operation; responsive UI during capture; sub-second waveform latency; real-time-feeling meters; fingerprint/metadata isolation from capture; multi-hour high-resolution projects; bounded memory use; waveform rendering independent of total sample count.
## 38. Capture Integrity
Each capture shall persist diagnostics including:
Required: device; backend; sample format; sample rate; channels; duration; frame count; overruns; underruns; dropped frames; stream errors.
Completed audio blocks or captures should support checksums for archival verification.
## 39. Settings
Settings shall cover:
**Audio:** input/output, backend, rate, format, buffer size, capture mode.
**Recording:** default location, transaction/block size, recovery behavior.
**Detection:** algorithm, thresholds, minimum silence, minimum track length.
**Metadata:** Discogs, MusicBrainz, AcoustID configuration and genre mapping.
**Export:** format, codec options, output path, naming template.
Credentials shall not be stored in project files.
## 40. Network Behavior
The application remains fully usable offline.
Network operations shall support:
Required: timeouts; cancellation; rate limits; caching; conservative retries; provider identification requirements.
No network operation may execute on the real-time audio thread.
## 41. Testing
Testing shall include:
Required: Rust unit tests; metadata fixtures; known-audio boundary fixtures; simulated/file-backed capture; SQLite recovery tests; schema migration tests; multi-hour capture tests; memory growth tests; dropped-frame tests; database contention tests; WAL/checkpoint stress tests; cross-platform device tests.
## 42. Logging and Diagnostics
Structured Rust logging shall use appropriate levels.
Routine audio callbacks shall not log.
Diagnostic bundles should report application version, OS, backend, device configuration, project/database integrity, and capture errors without including recorded audio.
## 43. Accessibility and Keyboard Control
Core recording/editing operations shall be keyboard accessible.
Suggested defaults:
`Space` Play/Pause  
`R` Record  
`S` Stop  
`M` Add marker  
`Delete` Delete selected marker  
`Left/Right` Seek  
`Ctrl+S` Save/checkpoint  
`Ctrl+E` Export
## 44. MVP
The MVP shall include:
Required: device enumeration; stereo CPAL capture; selectable rate/format; SQLite block-based audio storage; project recovery; VU meters; CPAL playback/seek; progressive waveform; marker editing; migrated VRipr track detection; Discogs support; MusicBrainz support; FLAC/WAV export; metadata/artwork tagging.
## 45. Phase 2
Add:
Required: `chromaprint-next`; AcoustID; progressive identification; evidence resolver; metadata-assisted boundaries; album/release inference; MP3/OGG export; advanced capture diagnostics.
## 46. Phase 3
Add:
Required: non-destructive processing; playback equalization curves (§51); click detection/removal; optional normalization; advanced archival metadata; improved multi-disc workflow; remote interface (§52); Android support; plugin/provider architecture.
## 47. Initial Engineering Spike
Before significant UI development, create a Rust `vinyl-audio-test` utility that:
1. lists devices
2. lists supported formats
3. opens the requested stream
4. creates a SQLite project
5. captures PCM into SQLite blocks
6. calculates live peak/RMS
7. records capture diagnostics
8. plays captured audio from SQLite
9. verifies block checksums
10. simulates interruption and recovery
11. reports requested vs negotiated audio format
12. reports overruns, underruns, and dropped frames
Run the same spike on Linux, Windows, and macOS.
## 48. SQLite Capture Spike
Before freezing the project schema, benchmark:
Required: PCM block sizes; transaction batch sizes; WAL behavior; checkpoint strategies; sustained 24/96 and 24/192 stereo capture; simultaneous waveform/fingerprint reads; crash recovery; database growth; compaction/vacuum behavior; project copy/backup behavior.
Capture reliability takes precedence over database elegance.
## 49. Open Project Format
Although inspired by AUP4/AUP3, this project's schema shall be openly documented.
The project format shall be usable by third-party tools without requiring the GUI.
A Rust project crate shall expose supported APIs for reading, validating, recovering, and migrating project files.
## 50. North Star
The finished workflow should be:
`Connect -> Select Album -> Set Level -> Drop Needle -> Record -> Flip -> Record -> Review Suggested Tracks -> Correct if Needed -> Export`
The internals can be sophisticated.
The user's experience should not be.

One bit I particularly want to prototype early is **24/192 stereo → bounded buffer → batched SQLite BLOB writes → simultaneous analysis reads**. If that stays rock-solid under deliberate abuse and simulated crashes, we've got the foundation nailed.

## 51. Playback Equalization
Numbered after §50 so that no existing section is renumbered: the numbers are cited
from source files, tests and the delivery plan.

RIAA has been the standard only since 1954. Records cut before it, and many 78s after
it, were cut to the issuing label's own curve, and played back through a RIAA stage
they are wrong in both the bass and the treble.

The application shall support selectable playback equalization.
Equalization shall be a **non-destructive stored decision**, held with the project's
other edit instructions and applied on playback, render and export.
Equalization shall never be applied in the capture path, and §9 continues to govern
capture unchanged: a captured block is what the device supplied.

Every capture shall record the equalization already applied by the hardware upstream
of it:
```rust
enum CaptureEq {
    Flat,
    Riaa,
    Unknown,
}
```
`Unknown` shall be the recorded value for imported projects and for any capture whose
provenance was not stated.
This field shall be recorded from the first release that supports capture, ahead of the
processing chain that consumes it: a capture whose provenance is unknown cannot afterwards
be correctly re-equalized, and the field cannot be recovered later.

Curves shall include:
Required: flat (none); RIAA; Columbia LP; Decca FFRR; EMI; HMV; AES; NAB/NARTB; Teldec.
The application shall additionally accept a user-defined curve stated in the same terms.
Each curve is defined by a bass turnover, a treble rolloff, and where it has one a bass
shelf. The IEC amendment's rumble filter shall be selectable independently of the curve.

Every curve's parameters shall carry a citation to the published source they came from.
The implemented response shall be measured against that source and shall be within
**±0.5 dB from 20 Hz to 20 kHz**, at every sample rate §8 requires.
A curve that is approximately right is worse than none, because a listener cannot hear
that it is approximately right.

Where a capture is `Riaa` and a different curve is selected, the application shall apply
the inverse of RIAA before the chosen curve, and shall state that doing so amplifies
noise.
Where a capture is `Unknown`, the application shall require the operator to state the
provenance rather than assume one.

A curve shall be selectable per side, because a box set may mix them, and overridable
per track.
The curve in force shall be visible wherever audio is auditioned, shall be recorded in
the project, and shall be written into exported metadata, because an archival file whose
equalization is unrecorded is one that cannot be reproduced.
## 52. Remote Interface
Numbered after §51 for the reason §51 is numbered after §50: the numbers are cited from
source files, tests and the delivery plan, and renumbering to tidy the order is how a
citation silently starts pointing at the wrong requirement.

A turntable attached to a headless machine is a first-class deployment and not a
workaround. The desktop shell requires a WebKit and GTK stack measured at 135 linked
packages and 259 MB; the command line requires ALSA and libc. On a small or
RAM-resident host the second is available and the first is not, which today leaves that
operator with no interface beyond the command line.

The application shall provide a `vcw serve` verb that offers the existing frontend over
HTTP to a browser on another machine.

`serve` shall expose the same command and event surface §35 defines, and shall add no
command the desktop shell does not have. The frontend shall remain one codebase: the
transport is a detail behind the single module that today issues every command and
subscribes to the single event stream, and a second implementation of any decision
behind that boundary is forbidden by §2 and remains forbidden here.

`serve` shall bind to loopback unless an address is stated, and shall refuse to bind to
a non-loopback address without an authentication secret supplied by the environment
(§39). The command surface reads and writes arbitrary paths, opens audio devices and
spawns exports; an unauthenticated listener offering it is a remote shell and shall not
be reachable by accident.

Where the desktop shell opens a native file chooser, `serve` shall offer a path browser
rooted at a directory stated when it starts, and shall refuse a path outside it.

Audio auditioned through `serve` is rendered by the host, not by the browser, because
the host is the machine the audio hardware is attached to. The interface shall say so
rather than appear to have failed.

`serve` shall be a packaging-time feature that can be built out, and shall be off unless
the verb is invoked. A capture-only install shall not be listening on anything.
