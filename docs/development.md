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
```

Stop the daemon in that shell (Ctrl-C), or run `pkill -x koeinput-daemon`.

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

The host check uses the Debian packages. Install them first, as described in [Install on the host](#install-on-the-host).

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

Once, in any app: log out and in again, wait about 10 seconds, then dictate. The first dictation works without a "model not ready" notice, because the addon started the daemon when fcitx5 loaded it.

## Debian packages

One source package, `koeinput`, builds two binary packages:

- `koeinput`: the daemon, the `koeinput` command, the systemd user unit, the D-Bus service file, the model catalog and the man page;
- `fcitx5-koeinput`: the addon.

They are built for Debian 13 (trixie) and Debian testing, for amd64 only. Everything runs in podman containers.

```sh
scripts/build-deb testing      # or trixie
scripts/test-deb testing
```

- `scripts/make-orig` makes the three orig tarballs in `target/deb/` from a clean work tree:
  - `koeinput_0.1.0.orig.tar.xz`: `git archive` of HEAD, without `debian/`;
  - `koeinput_0.1.0.orig-vendor.tar.xz`: the crates of `Cargo.lock`, from `cargo vendor-filterer` (crates that x86_64 Linux does not build become empty stubs);
  - `koeinput_0.1.0.orig-sherpaonnx.tar.bz2`: the pinned sherpa-onnx archive, byte for byte.

  It keeps an existing tarball with the same content and stops at one with different content.
- `scripts/build-deb trixie|testing` builds the source and binary packages without network in the image from `packaging/containers/deb-<dist>.Containerfile`, then runs lintian. The packages, the source package, `build.log` and `lintian.txt` go to `target/deb/<dist>/`.
  - It runs `scripts/gen-copyright --check` first, and `scripts/make-orig` when a tarball is missing or out of date.
  - Files outside `debian/` come from HEAD through the orig tarball, so commit them first. `debian/` comes from the work tree, so you can try packaging changes before you commit them.
  - The package build runs `cargo test`, the addon's `controller` test and `scripts/check-no-gpl` (before the binaries are stripped).
  - Rust comes from the distribution. Debian 13 has rustc 1.85 and the workspace needs 1.89, so the trixie image takes Rust from trixie-backports.
- `scripts/test-deb trixie|testing` installs the two packages in a fresh `debian:<dist>` container. apt downloads the dependencies, so it needs the network. It checks:
  - the installed files, and which package installs each one;
  - `koeinput --version`, `koeinput model list` and `man koeinput`;
  - `koeinput status` under `dbus-run-session`: the bus starts the daemon through the D-Bus service file. With no model installed, the daemon reports `ready: no` and logs the install hint;
  - fcitx5 without a display (`fcitx5 --disable=all --enable=keyboard,dbus,koeinput`): it loads the addon, and the addon starts the daemon;
  - `apt-get remove` removes the files, and a second install works.

  The output is also saved in `target/deb/<dist>/test.log`. If fcitx5 cannot run without a display, the script prints a warning and still passes the other checks. The installed binaries are stripped, so the GPL symbol check runs in the package build instead.
- `scripts/gen-copyright` writes `debian/copyright`. With `--check` it only compares and fails when the file is out of date.

### The main tarball changes with every commit

`git archive` records the commit, so every commit changes `koeinput_0.1.0.orig.tar.xz`. build-deb then runs make-orig, and make-orig stops because the tarball differs. While a version is unpublished (0.1.0 now), delete its tarballs and build again:

```sh
rm target/deb/koeinput_0.1.0.orig*
scripts/build-deb testing
```

make-orig needs a clean work tree, so commit or set aside changes in `debian/` first.
Once a version is released, keep its tarballs. Any later build then needs a new upstream version, even for a change in `debian/` alone, which normally only raises the Debian revision (see [followups.md](followups.md)).

### New version

1. Raise `version` in `Cargo.toml` and the `koeinput-dbus-api` version in `crates/cli/Cargo.toml` and `crates/daemon/Cargo.toml`. Then update the lock file: `scripts/dev cargo update --workspace`.
2. Raise `VERSION` in `project()` in `addon/CMakeLists.txt`.
3. Add an entry at the top of `debian/changelog` (version `X.Y.Z-1`) in the same format as the existing one.
4. Commit, then run `scripts/build-deb` and `scripts/test-deb` for both distributions.

### New sherpa-onnx pin

After the steps in [sherpa-onnx libraries](#sherpa-onnx-libraries), the package needs these:

1. Fetch the license files of the components again, at the versions that the new sherpa-onnx pins in its `cmake/*.cmake` files, into `packaging/licenses/`. Update `packaging/licenses/README` (file, component, commit, source URL).
2. Update the `SHERPA` and `REPO_LICENSE_FILES` tables in `scripts/gen-copyright`: versions, components, copyright holders and the Eigen source URLs. Update the ONNX Runtime notices file name in `debian/koeinput.docs`.
3. Run `scripts/gen-copyright`. It stops with a message when a table names a file or crate that no longer exists.
4. Build with a new upstream version, because the sherpa-onnx tarball changes.
5. Check `debian/koeinput.lintian-overrides`. It lists spelling errors inside the prebuilt libraries; lintian reports overrides that are no longer used.

### Crates and debian/copyright

`scripts/gen-copyright` takes the license of each vendored crate from `cargo metadata`, and the copyright lines from the crate's license files. Its tables add what those files miss:

- `EXTRA_HOLDERS`: copyright holders that a crate's license files leave out;
- `BUNDLED`: files inside a crate under another license, such as bundled C code or tests taken from other projects;
- `LICENSE_TEXTS`: where each license text comes from.

After a change of `Cargo.lock`, run `scripts/gen-copyright` and read the diff of `debian/copyright`. build-deb fails when a vendored crate has no paragraph. A change of `Cargo.lock` also changes the vendor tarball, so after a release it needs a new upstream version.

## Arch Linux packages

`packaging/arch/PKGBUILD` is a split package. It builds `koeinput` and `fcitx5-koeinput` with the same contents as the Debian packages, and makepkg adds `koeinput-debug` with the debug symbols and sources. `koeinput-debug` is for local debugging only; a release uploads only the two user packages.

```sh
scripts/build-arch
scripts/test-arch
```

- The sources are the three orig tarballs of the Debian package, under the same names. `scripts/build-arch` runs `scripts/make-orig` when they are missing or older than HEAD, with the same rules as build-deb (see [The main tarball changes with every commit](#the-main-tarball-changes-with-every-commit)).
- The committed PKGBUILD pins the SHA-256 of the sherpa-onnx archive (the value in `packaging/sherpa-onnx.env`). The main and vendor tarballs and `copyright` have `SKIP`: the main tarball contains the PKGBUILD itself, and all three change with commits and `Cargo.lock`.
- `scripts/build-arch` builds in the image from `packaging/containers/arch.Containerfile` (`archlinux:latest` with the build tools and a build user, because makepkg refuses to run as root). In a container without network, it:
  1. copies the PKGBUILD from the work tree, the three tarballs and `debian/copyright` to the build user;
  2. checks that the PKGBUILD's sherpa-onnx source and sum are the ones in `packaging/sherpa-onnx.env`;
  3. fills in the real sums on that copy with `updpkgsums`;
  4. runs `makepkg -s --noconfirm`. All dependencies are in the image, so nothing is installed. If the PKGBUILD needs a package that the image does not have, the build stops and names it; add it to the Containerfile;
  5. runs `namcap` on the PKGBUILD and the packages.

  The packages, the PKGBUILD with the real sums, `build.log` and `namcap.txt` go to `target/arch/`. The script fails on namcap errors and warnings, except for the ones listed and explained in it.
- Files outside `packaging/arch/` come from HEAD through the main tarball, so commit them first. The PKGBUILD comes from the work tree.
- The package build runs `cargo test`, the addon's `controller` test and `scripts/check-no-gpl`, as the Debian build does. The tests use their own target directory, so they cannot change the binaries that go into the package. `scripts/check-no-gpl` runs on the built binaries and again at the end of `package_koeinput()` on the installed copies, before makepkg strips them.
- The PKGBUILD turns off makepkg's LTO. With LTO, the C code in the crates becomes GCC LTO bytecode, which Rust's linker (lld) cannot read.
- Arch is a rolling release, but the image is built only once for each version of the Containerfile. To build with current packages, remove the image: `podman rmi $(podman images -q localhost/koeinput-arch)`.
- The addon goes to `/usr/lib/fcitx5/`. The license texts go to `/usr/share/licenses/koeinput/` and `/usr/share/licenses/fcitx5-koeinput/`. The `koeinput` directory also has:
  - `copyright`: `debian/copyright`, with the copyright notices and licenses of the vendored crates and the sherpa-onnx archive. build-arch runs `scripts/gen-copyright --check` first, as build-deb does. The file is written for Debian: for some licenses it points to `/usr/share/common-licenses/`, which Arch does not have; Arch has these texts in `/usr/share/licenses/spdx/`;
  - `sherpa-onnx/`: the license texts of the sherpa-onnx archive;
  - `crates/`: the license files of the crates in the binaries.
- Arch has no `/usr/libexec`, so the daemon goes to `/usr/lib/koeinput/koeinput-daemon`. The PKGBUILD changes the path in the installed copies of the D-Bus service file and the user unit; the files in `data/` keep `/usr/libexec/koeinput/` for the other distributions.
- `scripts/test-arch` installs the two packages with `pacman -U` in a fresh `archlinux:latest` container and runs the same checks as test-deb, with `pacman -R` for the removal. pacman downloads the dependencies, so it needs the network. The image leaves out man pages and translations (`NoExtract` in `/etc/pacman.conf`), so the test removes those lines before it installs the packages. The output is also saved in `target/arch/test.log`.
  The pacman hooks print messages that are expected in a container: "Skipped: Current root is not booted.", "Failed to connect to audit log, ignoring", and the `journal-nocow.conf` line "Failed to resolve specifier: uninitialized /etc/ detected, skipping."

To install on an Arch Linux machine:

```sh
sudo pacman -U target/arch/koeinput-0.1.0-1-x86_64.pkg.tar.zst target/arch/fcitx5-koeinput-0.1.0-1-x86_64.pkg.tar.zst
```

Then install a model and restart fcitx5, as in [Install on the host](#install-on-the-host) from step 3.

For a new version, set `pkgver` in the PKGBUILD to the new upstream version and keep `pkgrel=1`. build-arch stops when `pkgver` differs from the upstream version in `debian/changelog`.

## Install on the host

Build and test the packages in the dev container first:

```sh
scripts/build-deb testing
scripts/test-deb testing
```

Then, in a terminal on the host, in the repository directory (`~/works/git/nemui/koeinput`):

0. Run `sudo apt update && sudo apt upgrade` first: the testing build needs the current libc6 and fcitx5. Or use the trixie build (`target/deb/trixie/`).

1. If you started `koeinput-daemon` by hand in the dev container, stop it in that shell (Ctrl-C), or run `pkill -x koeinput-daemon`. While it runs, it holds the D-Bus name and the installed daemon cannot start.
2. Install the two packages:

   ```sh
   sudo apt install ./target/deb/testing/koeinput_0.1.0-1_amd64.deb ./target/deb/testing/fcitx5-koeinput_0.1.0-1_amd64.deb
   ```

   apt may say that the download is performed unsandboxed as root. That is harmless for local files. The `-dbgsym` packages in the same directory hold debug symbols; install them only if you need backtraces.
3. Install the model once. To use the archive downloaded in the model spike:

   ```sh
   koeinput model install parakeet-ja --from-file work/models/_dl/sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8.tar.bz2
   ```

   The VAD model is downloaded with it. To download both, run `koeinput model install parakeet-ja` instead.

   If the daemon was already running when you installed the model, run `systemctl --user restart koeinput`. The daemon looks for the model only when it starts.
4. Restart fcitx5 so that it loads the addon. Open System Settings, then Keyboard, then Virtual Keyboard (Plasma 6; on Plasma 5 it is Input Devices, then Virtual Keyboard). Choose None and apply, then choose Fcitx 5 and apply again. The keyboard keeps typing while fcitx5 is down.
5. The addon starts the daemon. Check it:

   ```sh
   koeinput status
   ```

   It prints `ready: yes` and `model: parakeet-ja`. The model takes a few seconds to load; until then it prints `ready: no`. If it keeps printing `ready: no` together with the install hint, the model step was skipped (or the daemon needs the restart from step 3).

Then go through the [host check](#host-check).

### Upgrade

Nothing restarts by itself after an upgrade. Install the new packages the same way as in step 2, then:

```sh
sudo apt install ./target/deb/testing/koeinput_<version>_amd64.deb ./target/deb/testing/fcitx5-koeinput_<version>_amd64.deb
systemctl --user restart koeinput
```

Restart fcitx5 as in step 4 when fcitx5-koeinput changed. The running daemon and the loaded addon keep the old code until you restart them.

### Rollback

```sh
systemctl --user stop koeinput
sudo apt remove fcitx5-koeinput koeinput
```

Then restart fcitx5 as in step 4. Keys still type while fcitx5 is down, so a broken addon never locks you out of a terminal.

The models stay in `~/.local/share/koeinput/models/`. Remove them with `koeinput model remove <id>` before you remove the package, or delete the directory.

### Logs

```sh
journalctl --user -u koeinput
```

The daemon logs there when D-Bus starts it through systemd. It never logs recognized text.

The addon's messages are in fcitx5's output. When KWin starts fcitx5, look in the user journal: `journalctl --user -b | grep -i koeinput`.

## Privacy

Recognized text is never written to logs unless you set `KOEINPUT_LOG_TEXT=1`.
