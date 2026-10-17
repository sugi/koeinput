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

## fcitx5 addon

The addon lives in `addon/`. It builds and tests in the build containers, never on the host.

```sh
scripts/addon build
scripts/addon test
KOEINPUT_DEV_IMAGE=bookworm scripts/addon test
```

- The first image is Debian 13 (fcitx5 5.1). The `bookworm` image is Debian 12 (fcitx5 5.0). The addon must build and pass in both.
- `scripts/addon test` builds the daemon with the `test-hooks` feature and runs the tests against it. Everything runs under `dbus-run-session` inside the container. Never run the tests on your own session bus.
- When the addon loads, it asks the bus to start the daemon (D-Bus activation), so the model is loaded by the first dictation. If the bus cannot start it, for example because no activation file is installed and you start the daemon by hand, the addon only logs a warning.
- `daemonclient_activation` checks that start request. It runs on its own private bus, which finds an activation file for the test daemon in `target/addon/test-services/` (written by `scripts/addon test`, passed through `XDG_DATA_DIRS`). The other tests run on a bus without that file: on a bus that can start the daemon, every call to it starts one, so they could neither test "no daemon" nor start their own daemons.
- The Debian 12 run skips `daemonclient`, `daemonclient_activation` and the daemon scenarios of the addon test, because the daemon is built only in the Debian 13 image.
- The addon test uses its own `dbus.conf`. The system one depends on the `keyboard` addon, which is built into the fcitx5 binary and cannot load in a test instance.
- `translation` checks that `addon/po/ja.po` compiles. To regenerate the catalog, run `scripts/dev sh -c 'xgettext --keyword=_ --keyword=N_ --language=C++ --from-code=UTF-8 --add-comments --package-name=fcitx5-koeinput -o addon/po/fcitx5-koeinput.pot addon/src/*.cpp addon/src/*.h'`.

### Compatibility with fcitx5 5.0

These points came up while making the addon build on 5.0.21:

- `ITestFrontend::sendKeyEvent` does not exist. The tests send keys through the input context.
- `fcitx-utils/dbus/variant.h` does not exist. The addon does not use it.
- Module headers are included by file name, for example `dbus_public.h`.
- The sd-event timer has a default accuracy of 250 ms. Timers can fire up to 250 ms late unless the accuracy is set.

### Settings

Open System Settings, then Input Method, then Configure add-ons, then KoeInput. You can also run `fcitx5-configtool`. The settings are:

- the hotkey (default Right Alt);
- tap mode: a short press keeps recording until the next press;
- the tap threshold in milliseconds;
- partial text: where the text you are still speaking appears. "In the app" (the default) puts it into the app's text field as underlined preedit, or into the fcitx5 panel when the app has no preedit. "In the panel" always uses the panel. "Off" shows only the status;
- apps that show partial text in the panel: program names (as fcitx5 sees them, case does not matter) that use the panel whatever the partial text setting says. Use it for apps that commit their own composition on blur, such as text-input-v3 clients (Chrome, Discord).

Configs from before the "Partial text" setting have the boolean `ShowPartial`. The addon reads `ShowPartial=False` as "Off" until the settings are saved again.

### Host check

The host check uses the Debian packages, so do it after they are built. The packaging milestone (M3) adds the steps to build and install them to this file.

Install and start:

```sh
sudo apt install ./koeinput_0.1.0-1_amd64.deb ./fcitx5-koeinput_0.1.0-1_amd64.deb
koeinput model install parakeet-ja
```

Restart fcitx5 so it loads the addon: open System Settings, then Keyboard, then Virtual Keyboard (Plasma 6; on Plasma 5 it is Input Devices, then Virtual Keyboard). Choose None, apply, then choose Fcitx 5 and apply again. The keyboard keeps typing while fcitx5 is down, so a broken addon never locks you out of a terminal.

Rollback:

```sh
sudo apt remove fcitx5-koeinput koeinput
```

Then restart fcitx5 the same way.

If Right Alt acts as AltGr on your keyboard layout, choose another hotkey in the settings.

For each app (WezTerm, Chrome, Firefox, Emacs, Discord), check:

1. Hold Right Alt and speak after "録音中" appears. The final text arrives shortly after you release the key.
2. Tap Right Alt, speak, and press it again.
3. Partial text appears while you speak.
4. Esc cancels the session.
5. Another key while recording cancels the session. A modifier key alone (Shift, Control) does not.
6. A password field does not start recording.
7. While Mozc is composing, Right Alt does nothing.
8. Click into another window during a session.
9. Change the hotkey in the settings and try the new one.
10. Stop the daemon during a session: `systemctl --user stop koeinput`. A notification says the daemon stopped; `systemctl --user start koeinput` brings it back.
11. With partial text showing, click into another window: nothing is committed. Try a GTK app such as Firefox, a Qt app, and Chrome and Discord (text-input-v3 clients may commit their own composition on blur).
12. Switch the input method with its hotkey during a session: note what happens to the status and the session.

For each app, note whether hold mode works and how the partial text looks. These notes feed M4.

## Privacy

Recognized text is never written to logs unless you set `KOEINPUT_LOG_TEXT=1`.
