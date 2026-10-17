# Development notes

## Build

```sh
scripts/dev cargo build --release --workspace
```

`scripts/dev` runs cargo in a rootless podman container (Debian 13, Rust 1.95). The host has no `-dev` packages, and the container gives every developer the same toolchain and libraries.

## sherpa-onnx libraries

KoeInput links sherpa-onnx statically from an upstream prebuilt archive.
The default archive also contains the text-to-speech libraries espeak-ng and ucd-tools (GPL-3.0-or-later), which would put GPL code into our binaries.
KoeInput does not use text to speech, so every build uses the upstream no-TTS archive instead.

- `packaging/sherpa-onnx.env` pins the archive: file name, URL, size, SHA-256 and the top directory inside it.
- `scripts/sherpa-onnx` downloads the archive once into `work/cache/sherpa-onnx/`, checks its size and SHA-256, extracts it and prints the `lib/` path. It refuses a file that does not match the pin.
  Without network, give it a copy you downloaded elsewhere: `scripts/sherpa-onnx --from-file <archive>`.
- `scripts/dev` runs `scripts/sherpa-onnx` before every container run and sets `SHERPA_ONNX_LIB_DIR` to that `lib/` directory. The `sherpa-onnx-sys` build script then uses it and downloads nothing.
- That build script always links `espeak-ng`, `piper_phonemize` and `ucd`. The no-TTS archive has none of them, so `scripts/sherpa-onnx` adds three empty archives with these names.
- `scripts/check-no-gpl` fails when a binary contains espeak-ng, ucd-tools or piper-phonemize symbols. It reads the symbol table, so run it on binaries that are not stripped:

  ```sh
  scripts/dev scripts/check-no-gpl target/release/koeinput target/release/koeinput-daemon
  ```

Older builds downloaded the default archive into `target/sherpa-onnx-prebuilt/`. Nothing uses it now, so you can delete it.

To update the pin (the archive version must match the `sherpa-onnx` crate version):

1. Change `sherpa-onnx = "=X.Y.Z"` in `crates/core/Cargo.toml`.
2. Put the new no-TTS archive's file name, URL, size, SHA-256 and top directory (`tar -tjf <archive> | head -1`) into `packaging/sherpa-onnx.env`. Upstream ships no checksum file; compare your `sha256sum` with the `digest` of the asset in `https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/vX.Y.Z`.
3. Check the list of libraries that the new `sherpa-onnx-sys` `build.rs` links. Each one must be in the archive or get an empty stub in `scripts/sherpa-onnx`.
4. Build, run `scripts/check-no-gpl` on both binaries, and run the model tests.

Cargo rebuilds `sherpa-onnx-sys` when `SHERPA_ONNX_LIB_DIR` changes, not when files inside the directory change. The directory name contains the version, so a new pin triggers the rebuild.

## Test

```sh
scripts/dev cargo test --workspace
scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks
scripts/dev cargo clippy --workspace --all-targets -- -D warnings
```

- Unit tests run with the first command.
- The daemon's D-Bus tests need a private session bus, so they run under `dbus-run-session`.
- Model tests are opt-in: set `KOEINPUT_MODEL_TESTS=1` and `KOEINPUT_TEST_MODELS_DIR=<dir with installed models>`.
- The microphone path cannot be tested through `scripts/dev`: the build container has no PulseAudio socket. Test it by hand with `koeinput listen` in the outer dev container (see below).
- The capture test `records_from_default_source_when_enabled` (`KOEINPUT_AUDIO_TESTS=1`) is not runnable through `scripts/dev`: it needs a real sound server, and other tests set `PULSE_SERVER` for the whole process. Treat it as unsupported until a dedicated runner exists.

## Run the daemon in the dev container

The release binaries need `libpulse`, the session bus and the PulseAudio socket, so run them directly in the outer dev container (where you work), not inside the `scripts/dev` build container.

```sh
HOME=$PWD/work/home KOEINPUT_DATA_DIR=$PWD/data target/release/koeinput-daemon &
sleep 3
target/release/koeinput status
HOME=$PWD/work/home KOEINPUT_DATA_DIR=$PWD/data target/release/koeinput transcribe --partials <file.wav>...
kill %1
```

`status` should print `ready: yes` and `model: parakeet-ja`. `transcribe` runs the recognizer inside the CLI process, not through the daemon.

## Try dictation

With the daemon running, start `koeinput listen`:

1. Press Enter. It prints `[preparing]`, then `[recording]`.
2. Speak after `[recording]`. Partial text appears on the current line.
3. Press Enter to stop. The final text appears with the time it took after you stopped.

`q` + Enter quits. Ctrl-C cancels the current session and quits.

## Install a model

```sh
koeinput model install parakeet-ja
koeinput model install parakeet-ja --from-file <archive>
```

Use `--from-file` to install from an archive you already downloaded.

## Disk use

Each test binary links sherpa-onnx statically, so it is large (about 50 MB) even without debug info.
To keep `target/` small:

- Dependencies are built with line tables only (`[profile.dev.package."*"]` in `Cargo.toml`). Backtraces still show file and line; the workspace crates keep full debug info for gdb.
- Cargo never deletes old build variants. They pile up when features, dependencies or profile settings change.
  `scripts/dev` therefore checks the size of `target/` before every `cargo` command. Above 8 GB it runs `cargo clean --profile dev` and `cargo clean --release` first, which keeps the addon builds. The sherpa-onnx archive is in `work/cache/`, outside `target/`. The next build takes about two minutes longer.
  Change the limit with `KOEINPUT_TARGET_LIMIT_GB`. A full set of dev, test and release builds takes about 3.5 GB.
- A git worktree under `.worktrees/` has its own `target/`. Removing the worktree removes it.

## Privacy

Recognized text is never written to logs unless you set `KOEINPUT_LOG_TEXT=1`.
