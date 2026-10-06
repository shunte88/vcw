<p align="center">
  <img src="assets/vcw-readme.webp" alt="VCW - The Vinyl Capture Workstation" width="320"/>
</p>

# VCW - The Vinyl Capture Workstation

**Capture the album. Export tagged tracks. Preserve the signal.**

VCW helps listeners preserve and catalog their vinyl collections, with a complete desktop workflow and reusable services for those building their own tools and interfaces.

Built around audiophile-quality, bit-perfect input and output, VCW takes you from recording the complete album to exporting individual tracks with metadata, cover art, and filenames you choose. Check your levels, capture the recording, identify the release, review the track boundaries, and export, all in one app.

Keep the complete album in one project, including double and triple albums. Recording and reviewing it as a whole makes it easier to maintain a consistent setup across the release, without dividing the work into separate side projects.

VCW separates the tracks. It cannot settle the argument about which pressing sounds better.

**Alpha release · [User guide](docs/USER-GUIDE.md) · [Development status](docs/STATUS.md) · [Releases](https://github.com/shunte88/vcw/releases) · [Report a problem](https://github.com/shunte88/vcw/issues)**

## What VCW does

- **Captures without hidden conversion.** Record at supported rates from 44.1 to 192 kHz, using the sample formats your device and VCW support. Inspect the requested and negotiated configurations and the capture diagnostics.
- **Shows levels before recording.** Arm the input and check the meters before the stylus goes down.
- **Preserves the original audio.** Adjust track boundaries and metadata without rewriting the captured samples.
- **Helps identify the release.** Look up catalog details through Discogs and MusicBrainz, with AcoustID audio identification when needed. Review and correct the result before exporting.
- **Suggests track boundaries.** Check detection results on the waveform, listen around transitions, and adjust or lock boundaries yourself.
- **Exports a tagged collection.** Write supported WAV, FLAC, MP3, or Ogg Vorbis files, with artist, album, track information, genres, artwork, and custom naming. Preview the export paths before writing.
- **Recovers interrupted recordings.** Audio is committed throughout capture; recovery reports what was saved and can finalize the recoverable recording.
- **Imports existing work.** Bring supported Audacity `.aup3` and `.aup4` projects into the same review and export workflow.

## Bit-perfect input and output

Signal preservation is a core requirement, backed by sample comparisons, hardware checks, and independent export validation.

**At capture**, VCW preserves the samples delivered by the recording device without automatic resampling, dithering, or hidden gain. It reports the format requested and the format negotiated. Where OS verification is available, it checks the hardware configuration too: an audio API reporting success is not enough to establish a bit-perfect path. A path that cannot be confirmed is reported as unconfirmed.

**In the project**, track and metadata edits leave the original capture unchanged. Tests compare the stored audio before and after editing, including checks that detect an altered sample.

**At lossless export**, supported, unprocessed WAV and FLAC exports preserve the exact sample sequence within the selected track boundaries. File headers, tags, and compression change the file's representation; they do not change those audio samples. Export tests compare selected samples against the recording, and independent tools validate the resulting files and tags.

**At playback**, VCW uses the capture's native rate rather than silently resampling for an incompatible output device. The development evidence includes byte-level playback and seek checks alongside measurements on real hardware.

MP3 and Ogg Vorbis are optional **lossy listening copies**, not bit-perfect archival exports. Any deliberately applied processing also changes the output samples; keep the original capture and use an unprocessed lossless export when sample preservation is the goal.

See the [development status and test evidence](docs/STATUS.md) for the measured results and current platform coverage.

## Alpha status and format limits

VCW is moving from development into its first alpha release. Recording, track review, metadata, recovery, and export are implemented; hardware validation is still expanding.

The build and packaging targets are:

| Target | Package formats |
| --- | --- |
| Linux x86_64 | DEB, AppImage |
| Linux aarch64 | DEB, AppImage |
| Windows x86_64 | MSI |
| macOS aarch64 | App, DMG |

Capture-path verification has been performed on Linux x86_64. Builds and packages for other targets do not yet establish equivalent recording-device validation. Check the [current status](docs/STATUS.md) and the notes accompanying your build.

Choose your intended export format **before recording**:

| Export | Preserves samples? | Current limits |
| --- | --- | --- |
| WAV | Yes, without processing | Supports VCW's stored sample formats, including Float32; approximately 4 GiB per file under RIFF. |
| FLAC | Yes, without processing | Integer audio up to 32-bit at any rate VCW records. A Float32 capture needs a width chosen for it first, in Settings > Export. |
| MP3 | No: lossy | Supported rates up to 48 kHz; mono or stereo. |
| Ogg Vorbis | No: lossy | Accepts VCW's supported capture rates, including 192 kHz, and Float32 audio. |

VCW refuses unsupported combinations instead of silently reducing bit depth or resampling. FLAC is an integer codec, so deciding how to bring a Float32 capture down to integers is a judgment about headroom: VCW asks rather than guesses. Settings > Export chooses the width (24-bit or 32-bit), whether to dither, and how much headroom to leave; until one is chosen a Float32 capture is refused by name and leaves as lossless WAV instead.

## Get started

Check [Releases](https://github.com/shunte88/vcw/releases) for published alpha installers and their release notes. If a package is not available for your target, use the source-build instructions below. The desktop application is **VCW**; the command-line tool is **`vcw`**.

The desktop workflow follows the complete recording:

1. **Connect and configure.** Connect your turntable through the appropriate phono stage and recording interface. Select the input device, rate, and sample format. State whether the incoming signal already has RIAA equalization.
2. **Set levels.** Arm recording and check the live meters. Set the input level before starting the capture, then inspect the negotiated format and verification status.
3. **Capture the album.** Keep the complete recording in one `.vcw` project, including every disc. Maintain the recording setup as you turn or change records.
4. **Identify and review.** Look up the release, check its track list and artwork, then audition and adjust the suggested boundaries. Confirm the metadata you want to keep.
5. **Plan and export.** Choose the destination, format, and naming template. Review the planned filenames, then export the tagged tracks.

The [user guide](docs/USER-GUIDE.md) covers the controls, keyboard shortcuts, metadata setup, and export options. The meters show the live input; the desktop waveform becomes available after capture stops.

Capture, local editing, and export do not require online lookup. Online metadata and identification need internet access and the relevant credentials:

- `VCW_DISCOGS_TOKEN` for Discogs.
- `VCW_ACOUSTID_KEY` for AcoustID.
- `VCW_CONTACT` for the contact information used in metadata-service requests.

MusicBrainz lookup does not require an API key. Credentials come from environment variables, not project files. `vcw metadata credentials` reports which are configured without printing their values.

## CLI and services

The desktop app provides the complete workflow. VCW's service-based approach also supports command-line use, automation, and alternative interfaces.

The UI and CLI can work alongside one another: direct a recording in the app while using the CLI for metadata lookup, assignment, or export. They use the same core capabilities; you do not have to choose one interface for the entire process.

For an existing project named `album.vcw`:

```sh
vcw doctor
vcw metadata search --artist "Kraftwerk" --album "Trans-Europe Express"
vcw release album.vcw set --artist "Kraftwerk" --album "Trans-Europe Express" --year 1977
vcw tracks album.vcw set 1 --title "Europe Endless"
vcw export album.vcw --into ./exports --format wav --dry-run
vcw export album.vcw --into ./exports --format wav
```

Use `vcw --help` and each command's `--help` for options. Supported commands offer `--json` for integration. `vcw doctor` checks the environment; `vcw devices` and `vcw formats` inspect recording devices and configurations.

For custom tools and interfaces, start with the [Rust API](docs/PROJECT-API.md), [project schema](docs/SCHEMA.md), and [command/event contract](crates/contract/). The [user guide](docs/USER-GUIDE.md) contains fuller CLI examples.

## Recovery and project care

A `.vcw` project holds the recording, boundaries, metadata, and artwork. Capture commits audio in 250 ms blocks. An interruption can lose the uncommitted block and audio still buffered by the recording device; the exact loss depends on the capture configuration.

Recovery reports first and applies changes only when requested:

```sh
vcw recover album.vcw
vcw recover album.vcw --apply
```

Close a project before copying it for backup. SQLite creates temporary `-wal` and `-shm` files while the project is open; they may contain recording data that has not yet been folded into the main file. Read the [recovery and backup guidance](docs/USER-GUIDE.md#when-something-goes-wrong-mid-capture) before handling an interrupted recording.

Allow plenty of storage: stereo 24-bit/96 kHz PCM alone uses about 346 MB per ten minutes, before project overhead. Higher rates, wider stored formats, and multi-disc recordings need more.

To report a problem, open an [issue](https://github.com/shunte88/vcw/issues) with the version, OS, device, requested format, and steps to reproduce it. A diagnostic bundle helps:

```sh
vcw bundle album.vcw --out bundle.json
```

The bundle excludes recorded audio, credentials, and track and album titles. Review it before attaching it. See the [troubleshooting guide](docs/USER-GUIDE.md#when-you-need-to-report-a-problem) for logging and integrity checks.

## Build from source

The core and CLI require Rust 1.90 or newer and a native build toolchain. SQLite is bundled. On Linux, install `libasound2-dev` for audio support.

From the repository root:

```sh
cargo build --release -p vcw-cli
cargo test --workspace
```

The CLI is `target/release/vcw` (`vcw.exe` on Windows). Add its directory to your PATH to use the bare `vcw` commands above, or invoke it by its path.

### Desktop app on Linux

Also install Node.js 22, pnpm 10, and these development packages: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, and `librsvg2-dev`.

From the repository root, stage the CLI and build the frontend before launching the desktop app:

```sh
bash tools/stage-cli.sh
cd app/ui
pnpm install --frozen-lockfile
pnpm build
cd ../src-tauri
cargo run
```

The desktop app has a separate Cargo workspace, so the root build does not build it. For Windows/macOS build setup and installer packaging, consult the target-specific steps in the [build workflow](.github/workflows/ci.yml).

## Project and direction

VCW is the successor to [VRipr](https://github.com/shunte88/vripr), which analyzed recordings made in Audacity. VCW handles recording itself and can import supported Audacity projects.

Two weeks ago, VCW was a requirements document. Since then, the repository has seen almost 1,000 clone operations from 199 unique cloners, without any promotion from me. Seeing that interest while the tool is still taking shape means a lot. Those figures describe the roughly two-week development period leading up to this alpha introduction.

Our goal is to make VCW the de facto tool for vinyl capture across platforms: a complete workstation for listeners preserving and cataloging their collections, with reusable services for people building their own workflows and interfaces.

For development detail, see the [requirements](REQUIREMENTS.md), [delivery plan](PROJECT_PLAN.md), [architecture decisions](docs/adr/), and [spike findings](docs/spikes/).

## License and support

VCW's own source is [MIT licensed](LICENSE). Distributed binaries also include third-party components, including LGPL-2.1-or-later fingerprinting code and, in builds with MP3 enabled, LGPL-3.0 MP3 encoding components. MP3 and Ogg Vorbis are enabled by default. See [third-party notices and relinking information](THIRD-PARTY-NOTICES.md), the app's About dialog, or `vcw doctor` for component information.

If VCW helps power your project, a link back is appreciated:

> Built on [VCW - The Vinyl Capture Workstation](https://github.com/shunte88/vcw), © 2026 Stue Hunter. MIT licensed.

Bug reports, hardware test results, and useful pull requests are welcome. You can also [buy me a coffee](https://www.buymeacoffee.com/shunte88) or browse [Team Badger merchandise](https://www.zazzle.com/team_badger_t_shirt-235604841593837420).
