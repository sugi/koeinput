# Development notes

## Build

```sh
scripts/dev cargo build --release --workspace
```

`scripts/dev` runs cargo in a rootless podman container (Debian 13, Rust 1.95). The host has no `-dev` packages, and the container gives every developer the same toolchain and libraries.

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
  `scripts/dev` therefore checks the size of `target/` before every `cargo` command. Above 8 GB it runs `cargo clean --profile dev` and `cargo clean --release` first, which keeps the downloaded sherpa-onnx archive and the addon builds. The next build takes about two minutes longer.
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
- The Debian 12 run skips the `daemonclient` test and the daemon scenarios of the addon test, because the daemon is built only in the Debian 13 image.
- The addon test uses its own `dbus.conf`. The system one depends on the `keyboard` addon, which is built into the fcitx5 binary and cannot load in a test instance.
- `translation` checks that `addon/po/ja.po` compiles. To regenerate the catalog, run `scripts/dev sh -c 'xgettext --keyword=_ --language=C++ --from-code=UTF-8 --add-comments --package-name=fcitx5-koeinput -o addon/po/fcitx5-koeinput.pot addon/src/*.cpp addon/src/*.h'`.

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
- whether to show partial text while you speak.

### Host check

The packages come from the packaging milestone. Until they exist, build them as described in the install section below.

Install and start:

```sh
sudo apt install ./koeinput_0.1.0-1_amd64.deb ./fcitx5-koeinput_0.1.0-1_amd64.deb
koeinput model install parakeet-ja
```

Restart fcitx5 so it loads the addon: System Settings, Keyboard, Virtual Keyboard, choose None, apply, then choose Fcitx 5 and apply again. The keyboard keeps typing while fcitx5 is down, so a broken addon never locks you out of a terminal.

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
