# M1: daemon and CLI

Status: draft for review, 2026-10-17.

Part of [the overall design](2026-10-17-overall-design.md). Background decisions are in [decisions.md](../../decisions.md).

## Goal

Dictation works end to end from a terminal, without the fcitx5 addon:
`koeinput listen` records from the microphone, shows partial results while the user speaks, and prints the final text.

Done when:

- `koeinput listen` works in the dev container against the host's session bus and microphone.
- On a few test utterances, the final result arrives within 2 s (p90) of stopping, and partial results update at least once per second.
- No text comes out for silence.
- Unit and D-Bus tests pass in the build container.

Out of scope for M1: the fcitx5 addon, Debian packaging, Qwen3-ASR hotwords, a second final-stage model, and tuning under CPU load.

## Workspace

```
Cargo.toml                 workspace
crates/core/               package koeinput-core (library)
crates/daemon/             package koeinput-daemon (binary koeinput-daemon)
crates/cli/                package koeinput-cli (binary koeinput)
data/models.toml           model catalog
data/koeinput.service      systemd user unit (installed in M3)
data/org.nemui.KoeInput.service   D-Bus activation file (installed in M3)
packaging/containers/dev-trixie.Containerfile   build container
```

## koeinput-core

Each module has one job and can be tested alone.

| Module | Job |
|---|---|
| `audio::capture` | Opens a PulseAudio record stream (16 kHz, mono, float32, fragment about 20 ms) on the configured source, or the default source. Runs in its own thread and sends chunks over a channel. The first chunk means the device is live. |
| `audio::file` | Reads a WAV file and resamples it to 16 kHz mono. Used by `transcribe` and by tests, so files and the microphone go through the same session code. |
| `vad` | Wraps the sherpa-onnx Silero VAD. Reports finished speech segments and where the current, unfinished segment starts. |
| `recognizer` | A `Recognizer` trait: samples in, text out. One implementation, `SherpaOffline`, covers the model kinds in the catalog: NeMo CTC (parakeet-ja), transducer (Zipformer), SenseVoice, Qwen3-ASR and Cohere. Tests use a fake. |
| `session` | The session state machine and the partial-result scheduler (see below). Knows nothing about D-Bus. |
| `postprocess` | Output clean-up: strip model artefacts (such as `<asr_text>` and `language Japanese`), apply the user dictionary, remove unambiguous fillers, remove stray spaces between Japanese characters. |
| `config` | Reads `~/.config/koeinput/config.toml` and fills in defaults. |
| `models` | Reads the catalog. Installs a model: download, SHA-256 check, safe extraction (rejects paths that leave the target directory). Lists, removes and selects models. |

### Session behavior

States: `starting` → `recording` → `finalizing` → `done`, or `cancelled` / `error` at any point.

1. `Start`: open the capture stream. State `starting`.
2. First audio chunk arrives: state `recording`.
3. Every 0.5 s while recording:
   - Feed new audio to the VAD.
   - Segments the VAD has finished are decoded once and their text is fixed.
   - The unfinished segment (from its speech start to now) is decoded again.
   - Send `Partial` = fixed text + current text, if it changed.
   - If the previous decode is still running, skip the tick. Ticks never queue up.
4. `Stop`: keep recording 0.25 s more, then close the stream. State `finalizing`.
   Decode what is left of the last segment, join all segment texts, clean them up and send `Final`. State `done`.
5. `Cancel`: close the stream, drop everything, state `cancelled`. No `Final`.

Edge cases:

- Recording shorter than 0.5 s, or no speech found: `Final` with empty text.
- Recording longer than 120 s: stop automatically, as if `Stop` was called.
- The microphone fails to open, or stops sending audio for 2 s: state `error` with code `audio`.

Defaults (all in the config file): partial interval 500 ms, tail 250 ms, minimum length 500 ms, maximum length 120 s, 8 threads (the fastest setting for the ONNX models in the spike).

## koeinput-daemon

- Async D-Bus service with `zbus` on a `tokio` runtime.
  Capture runs in its own thread. Decoding runs in one worker thread that owns the recognizer, because decoding is CPU-bound.
- Loads the configured model at startup. Until it is loaded, `Ready` is false.
  The model stays in memory; there is no idle exit.
- Logs go to stderr, so they reach the user journal under systemd.
  Recognized text is never logged unless `KOEINPUT_LOG_TEXT=1` is set.

### D-Bus interface

Bus name `org.nemui.KoeInput`, object `/org/nemui/KoeInput`, interface `org.nemui.KoeInput.Recognizer1`.

| Member | Kind | Description |
|---|---|---|
| `Start(a{sv} options) → u session` | method | Starts a session. `options` is empty in M1 and reserved for later. |
| `Stop(u session)` | method | Ends recording and produces the final result. |
| `Cancel(u session)` | method | Drops the session. |
| `StateChanged(u session, s state)` | signal | One of the states above. |
| `Partial(u session, s text)` | signal | Whole partial text so far. |
| `Final(u session, s text)` | signal | Final text, possibly empty. |
| `Error(u session, s code, s message)` | signal | Codes: `audio`, `model`, `internal`. |
| `Ready` | property `b` | Model loaded. |
| `Model` | property `s` | Model id. |
| `Version` | property `s` | Daemon version. |

Rules:

- Signals go only to the client that called `Start` (a unicast signal with a destination).
- One session at a time. `Start` during a session fails with `org.nemui.KoeInput.Error.Busy`.
- If the client that started the session leaves the bus, the session is cancelled.
- `Start` with no installed model fails with `org.nemui.KoeInput.Error.NoModel`. The message names the command to install one.
- `Stop` or `Cancel` with an unknown id fails with `org.nemui.KoeInput.Error.NoSuchSession`.

## koeinput (CLI)

| Command | What it does |
|---|---|
| `koeinput listen` | Talks to the daemon. Enter starts, Enter stops, Ctrl-C cancels. Shows the state, rewrites the partial text on one line, then prints the final text and the time from stop to final. |
| `koeinput transcribe FILE...` | Runs the same session code in-process, without the daemon. Prints the final text and timings. `--partials` also prints the partial timeline. |
| `koeinput status` | Shows the daemon's `Ready`, `Model` and `Version`. |
| `koeinput model list` | Lists catalog models and which are installed. |
| `koeinput model install ID` | Downloads, checks and extracts a model. The Silero VAD model is installed with the first model. |
| `koeinput model remove ID` | Deletes an installed model. |
| `koeinput model use ID` | Sets the model in the config. The daemon picks it up on restart. |

## Files

| Path | Content |
|---|---|
| `~/.config/koeinput/config.toml` | Model id, source, threads, timing settings, dictionary path |
| `~/.config/koeinput/dictionary.tsv` | User dictionary: `wrong form<TAB>right form`, one pair per line |
| `~/.local/share/koeinput/models/<id>/` | Installed models |
| `/usr/share/koeinput/models.toml` | Catalog. During development, `KOEINPUT_DATA_DIR` points at the repository's `data/`. |

The catalog starts with parakeet-ja (default), Zipformer, SenseVoice 2024, Qwen3-ASR 0.6B, Cohere Transcribe and Silero VAD.
Each entry has the URL, SHA-256, size, license and model kind. Values come from the spike's `work/models/manifest.json`.

## Build and development environment

- This container has no root access and no development packages. Builds run in a rootless podman container instead: `packaging/containers/dev-trixie.Containerfile`.
  It is based on Debian 13 and has the Rust toolchain, `pkg-config`, `libpulse-dev` and `dbus` (for tests).
  M2 and M3 add the fcitx5 and packaging tools to the same image.
- A script, `scripts/dev`, runs a command in that container with the repository mounted and the cargo cache kept between runs.
- The built binaries run directly in this container. It has `libpulse.so.0` and shares the host's session bus and PulseAudio socket.
  The image and this container are both Debian 13, so the C library matches.
- The `sherpa-onnx` crate links sherpa-onnx statically. On the first build it downloads the prebuilt archive; the archive is kept in a cache directory and passed with `SHERPA_ONNX_ARCHIVE_DIR` afterwards.

## Testing

- Unit tests in koeinput-core:
  - the scheduler, with a fake recognizer, a fake VAD and a fake clock: tick timing, skipped ticks, fixed segments, tail, minimum and maximum length, cancel
  - clean-up rules and the dictionary
  - config defaults and parsing
  - catalog parsing, SHA-256 checks and safe extraction (including a hostile archive)
- D-Bus tests under `dbus-run-session`: the daemon with a fake recognizer and a WAV file as the audio source. They check unicast signals, the `Busy` error, `NoSuchSession`, and cancel when the client leaves.
- Model tests (opt-in with `KOEINPUT_MODEL_TESTS=1`): `transcribe` on the sample WAVs shipped inside the model archives. Nothing from the user's own recordings goes into the repository.
- The microphone path is tested by hand with `koeinput listen`.

## To verify during M1

- The sherpa-onnx Rust wrapper (1.13.8) has no VAD `flush`. Workaround: feed trailing silence, or decode the unfinished segment from its start position, which the scheduler tracks anyway.
- How `zbus` 6 sends a signal with a destination.
- That `pipewire-pulse` honours the small fragment size through the simple API. `parec --latency-msec` worked in the spike.
- License terms of the prebuilt sherpa-onnx archive, for the packages in M3.
