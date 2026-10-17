# Overall design and implementation plan

Status: draft for review, 2026-10-17.

This document builds on [decisions.md](../../decisions.md).
It sets the architecture, the main technical choices, packaging and the order of work.
Each milestone below gets its own detailed spec and implementation plan before coding starts.

Labels follow decisions.md: **Decided**, **Recommended**, **To verify**, **Open**.

**Decided**: the project is called **KoeInput** (声インプット), short form **KI**.
Packages `koeinput` and `fcitx5-koeinput`, command `koeinput`, D-Bus name `org.nemui.KoeInput`.

## 1. First release

**Decided** by the user:

- Japanese only at first. Other languages can come later.
- Ship as a Debian package first.
  Also support RPM and other formats where they fit.
- Model choice does not need strict evaluation now. Models can be changed later, so the design must make that easy.

In scope for the first release:

- Push-to-talk dictation into the focused text field through fcitx5, with Mozc still active.
- Partial results while speaking, then the final result committed on key release.
- Local recognition on CPU with one model at a time.
- Output clean-up: removal of model artefacts, a user replacement dictionary, removal of unambiguous fillers.
- A CLI for testing, model install and status.
- Packages: `.deb` first, then `.rpm` and an Arch `PKGBUILD`.

Out of scope for the first release:

- Languages other than Japanese, GPU or NPU inference, and LLM post-processing.
- An evaluation UI. The spike scripts stay as developer tools.
- Undo of the last commit, and voice commands.

## 2. Architecture

```
          key events                 session D-Bus                 PipeWire
 [app] <-> [fcitx5 + addon] <------------------------> [daemon] <----------- [microphone]
                                                          |
 [terminal] <-> [CLI] <-----------------------------------+   sherpa-onnx (CPU)
```

| Component | Language | Job |
|---|---|---|
| addon | C++ | fcitx5 module. Hotkey, recording indicator, partial display, committing the final text. Nothing else. |
| daemon | Rust | Audio capture, VAD, recognition, partial-result scheduling, output clean-up, dictionary, model loading, D-Bus service. |
| CLI | Rust | Talks to the daemon (listen, status), transcribes WAV files, installs and selects models. |
| core | Rust library | Audio pipeline, VAD, recognizer backends and clean-up, shared by the daemon, the CLI and developer tools. |

Rules that keep the addon safe inside fcitx5 (from the vinput review in decisions.md):

- Only asynchronous D-Bus calls. No polling. State comes from daemon signals.
- No file I/O and no heavy work on the fcitx5 event loop.
- Do not change input context capability flags.
- Commit only to the input context that started the session, and only if it still exists.

## 3. Main technical choices

| Area | Choice | Status | Reason |
|---|---|---|---|
| Daemon and CLI language | Rust | Recommended | Memory safety for a resident process. The recognition runtime has an official crate. |
| Recognition runtime | sherpa-onnx, official `sherpa-onnx` crate | Recommended | One runtime covers parakeet-ja, Qwen3-ASR, Cohere, SenseVoice and Zipformer. Switching models is a config change. |
| whisper.cpp | Not in the first release | Recommended | On this CPU it takes about 2 to 6 s per utterance, and in one test it silently dropped whole sentences. It can be added later as a second backend. |
| Default model | parakeet-tdt_ctc-0.6b-ja (int8) | Recommended | Fast (RTF about 0.03), few dropped sentences in tests. Weak points: punctuation and English words. Confirm with the spike summary. |
| Optional final-stage model | Qwen3-ASR 0.6B | Recommended, later milestone | Better punctuation and hotword support, but too slow for partial results. |
| Audio capture | PulseAudio API (`libpulse-simple-binding`), served by `pipewire-pulse` on PipeWire desktops | Recommended | Works on PipeWire and PulseAudio desktops alike. The dev container only has the PulseAudio socket, not the native PipeWire socket. Small fragment size (about 20 ms) is required. |
| D-Bus | `zbus` | Recommended | Pure Rust, async, supports signals sent to one client. |
| Addon build | CMake with the fcitx5 development packages | Decided | The standard way to build fcitx5 modules. |
| Addon settings | fcitx5's own config system | Recommended | The hotkey and display options then appear in the normal fcitx5 settings window. |
| Daemon settings | TOML file in `~/.config/koeinput/` | Recommended | Model, microphone, thread count, dictionary path. |
| Models | Downloaded by the CLI into `~/.local/share/koeinput/models/`, checked against SHA-256 | Recommended | Models are large and have different licenses (parakeet-ja is CC-BY-4.0 and needs attribution). They do not go into packages. |

How the `sherpa-onnx` crate links (checked in `sherpa-onnx-sys` 1.13.8):

- By default it links statically and downloads a prebuilt archive from GitHub at build time.
- `SHERPA_ONNX_ARCHIVE_DIR` points it at an archive that was downloaded beforehand. Package builds use this so they work offline, with the archive pinned by checksum.
- `SHERPA_ONNX_LIB_DIR` points it at a sherpa-onnx built from source.
  Debian 13 ships ONNX Runtime 1.21 (`libonnxruntime-dev`), but not sherpa-onnx.

## 4. D-Bus interface (draft)


- Bus name `org.nemui.KoeInput`, object `/org/nemui/KoeInput`, interface `org.nemui.KoeInput.Recognizer1`.
- Methods: `Start(a{sv} options) → u session`, `Stop(u session)`, `Cancel(u session)`.
- Signals, sent only to the client that started the session: `StateChanged(u session, s state)`, `Partial(u session, s text)`, `Final(u session, s text)`, `Error(u session, s code, s message)`.
- States: `starting`, `recording` (sent when the first audio arrives), `finalizing`, `done`, `cancelled`, `error`.
- Properties: `Ready` (model loaded), `Model`, `Version`.

The M1 spec fixes the details.

## 5. Session flow

1. The user presses the hotkey. The addon calls `Start`.
   If Mozc is composing, the addon either does not start or leaves the composition as it is (the M2 spec picks one).
2. The addon shows "preparing". The daemon opens the capture stream with about 20 ms latency.
3. When the first audio arrives, the daemon sends `recording` and the addon shows the recording indicator.
4. While the user speaks, the daemon decodes the current VAD segment again every 0.5 s and sends `Partial`.
   Finished segments stay fixed.
5. The user releases the key and the addon calls `Stop`.
   The daemon records about 0.25 s more, so the last syllable is not cut off.
6. The daemon decodes the last segment, joins all segments, cleans up the text and sends `Final`.
   The addon commits it to the original input context, if that context still exists.
7. Esc, focus out or another key pressed during recording calls `Cancel`.
   Recordings shorter than 0.5 s and recordings with no speech produce no text.

Output clean-up, in this order:

1. Remove model artefacts, such as tags that leak into the text.
2. Apply the user replacement dictionary (wrong form → right form).
3. Remove unambiguous fillers (えーと, えっと, あのー). Do not touch words that are also normal words (あの, まあ).
4. Remove stray spaces between Japanese characters.

## 6. Repository layout

```
Cargo.toml            Rust workspace
crates/core/          audio, VAD, recognizer backends, clean-up
crates/daemon/        D-Bus service binary
crates/cli/           CLI binary
addon/                fcitx5 module (C++, CMake)
data/                 systemd user unit, D-Bus service file, default config, model catalog
debian/               Debian packaging
packaging/rpm/        RPM spec
packaging/arch/       PKGBUILD
tools/                developer scripts (evaluation), not packaged
docs/
```

Evaluation data and models stay under `work/`, which git ignores.

## 7. Packaging

### Formats

| Format | Plan | Reason |
|---|---|---|
| `.deb` | First | The user's system is Debian. |
| `.rpm` | After the `.deb` | Fedora and openSUSE ship fcitx5. |
| Arch `PKGBUILD` | After the `.rpm` | Small effort once the build works. |
| Flatpak | No | The addon must be loaded by the host fcitx5. A Flatpak would only help users who run fcitx5 itself as a Flatpak. |
| Snap, AppImage | No | They cannot install an fcitx5 addon or a systemd user unit into the system. |

### Package split (Debian)

One source package builds two binary packages:

- `koeinput`: daemon, CLI, systemd user unit, D-Bus service file, model catalog.
- `fcitx5-koeinput`: the addon. It depends on `koeinput` and on fcitx5.

Installed files:

- `/usr/bin/koeinput` and `/usr/libexec/koeinput/koeinput-daemon`
- `/usr/lib/systemd/user/koeinput.service` and `/usr/share/dbus-1/services/org.nemui.KoeInput.service` (D-Bus activation through systemd)
- `/usr/lib/<multiarch>/fcitx5/koeinput.so` and `/usr/share/fcitx5/addon/koeinput.conf`
- `/usr/share/koeinput/models.toml` (model catalog: URL, SHA-256, size, license)

Installing to system paths also removes the open question about how fcitx5 finds addons in user directories.

### Build rules

- Package builds run offline:
  - Rust crates come from `cargo vendor`.
  - The sherpa-onnx archive is pinned by checksum and passed with `SHERPA_ONNX_ARCHIVE_DIR`.
- The addon supports every fcitx5 5.x (see decisions.md). The core library soname has been `libfcitx5core7` from 5.0.5 to 5.1.23.
  Packages are still built per distribution release, as usual, and the package dependency comes from the libraries actually used.
  Targets: Debian 13 and Debian testing (the user's host) first, then Ubuntu LTS, then Fedora.
- `postinst` does not download anything. After installation, the user runs `koeinput model install`.
  If no model is installed, the addon shows a notification with that command.
- The RPM spec and PKGBUILD follow the same build steps.
- **Open**: the repository host and CI service. Until then, builds run locally in containers.

## 8. Milestones

Each milestone starts with its own spec and implementation plan.

| Milestone | Deliverable | Done when |
|---|---|---|
| M0. Spike summary | Short results document; default model fixed | The current benchmark run is summarized. No further measurement. |
| M1. Daemon and CLI | Rust workspace; capture, VAD, sherpa-onnx backend, partial scheduling, D-Bus service, WAV file input; `koeinput listen` prints partial and final text in a terminal | Dictation works end to end from the CLI on the host session bus. Tests pass on WAV fixtures. |
| M2. Minimal addon | fcitx5 module: hotkey, indicator, final commit; installed on the host with a local `.deb` built in the Debian 13 container | Dictation works in WezTerm, Chrome, Firefox, Emacs and Discord. |
| M3. Debian packaging | Proper source package with two binary packages, offline build, model install command, lintian mostly clean | Clean install on a fresh Debian 13 container and on the host. Daily use starts here. |
| M4. Quality | Partial display (inline preedit or panel), output clean-up, user dictionary, optional final-stage model, latency under CPU load | The user's daily pain points are fixed. |
| M5. More formats and release | RPM spec, PKGBUILD, CI, release steps, README and install guide, license and model attribution | Packages build for every target from one tag. |

No separate tests are needed before M2:

- Committing Mozc's composition is not required.
- Key release is detected the same way vinput does, and M2 confirms it works.
- The host's fcitx5 version does not matter within 5.x.

## 9. Testing

- core: unit tests for VAD segmentation, the partial scheduler, clean-up rules and the dictionary.
- daemon: integration tests that feed WAV fixtures through the file input. D-Bus tests run under `dbus-run-session`.
  Fixtures use public sample audio only. The user's own recordings never go into the repository.
- addon: keep the logic small. **To verify**: use fcitx5's test frontend modules for automated tests.
  Otherwise, use a manual checklist for the five main apps.
- packages: lintian, install and remove in a clean container, then a smoke test (`koeinput transcribe` on a fixture).
- Models: the spike scripts under `tools/` can rerun the benchmark when a new model is tried.

## 10. Risks

| Risk | Effect | Plan |
|---|---|---|
| fcitx5 changes its binary interface within 5.x | The addon fails to load after an fcitx5 upgrade | CI builds against the oldest and newest 5.x. The package depends on the soname, so a new soname blocks the install. |
| sherpa-onnx prebuilt archive changes or disappears | Builds break | Pin by checksum and keep a local copy. Building from source against Debian's ONNX Runtime is the long-term option. |
| Inline preedit renders badly in terminals or Emacs | Partial results are hard to read | Decide in M4. Option to show partial results in the fcitx5 panel instead. |
| Latency rises when the CPU is busy | The final result arrives late | Measure in M4. Tune the thread count and process priority. |

## 11. Open items for the user

- License of this project. Needed before the first release.
- Repository host and CI. Needed for M5.
