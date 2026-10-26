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
- `tools/measure-m4` measures accuracy and latency on the evaluation recordings in `work/eval-data/`, also under CPU load. It is a developer tool, not packaged. How to run it and the last results: [m4-results.md](m4-results.md).
  - The modes `split600`, `split800` and `split1000` (8 threads, no load) and `load-split800` (8 threads, under load) set `final_split_silence_ms`. The older modes pin it to `0`, so they still measure one decode of the whole utterance.
  - The summary has two more columns: the sentence ends found in the read sentences, and the `？` inside free speech.
  - The script also writes `compare.md` in the output directory (mode 0600): the final text of every mode side by side. It holds recognized text, so never commit or quote it.
  - Results of the split modes: [punctuation-results.md](punctuation-results.md).

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

## Final text

The final text is decoded per chunk. The VAD segments are grouped into chunks: a gap of at least `final_split_silence_ms` (default 800) between two segments starts a new chunk, and `0` never splits.
Each chunk is decoded again as one piece, and the session closes it with `。`, or `？` after a polite question ending, unless the model already ended it with punctuation. Dictionary, fillers and punctuation rules run per chunk.
`final_full_max_ms` (default 30000, so 30 s) is a total budget for these decodes in one utterance. The latest chunks use it first. A chunk that does not fit in what is left joins the texts of its VAD segments instead, as the partial text does.
`final_full_max_ms = 0` in `~/.config/koeinput/config.toml` always joins the segments.
The measurements behind the defaults are in [m4-results.md](m4-results.md) and [punctuation-results.md](punctuation-results.md).

### A second model for the final text

A slower, more accurate model can decode the final text while the default model keeps producing the partial text:

```sh
koeinput model install cohere-transcribe
koeinput model use cohere-transcribe --final
systemctl --user restart koeinput
koeinput model use --final none     # back to one model
```

- `model use --final` writes `final_model` into the config file. It installs nothing, and it refuses the main model.
- The daemon loads the final-stage model after the main model. Dictation works as soon as the main model is loaded; until the second model is loaded too, the main model decodes the final text.
- The final-stage model decodes only whole chunks. Over the `final_full_max_ms` budget, the main model's segment texts are joined, because decoding every segment again with the slow model would delay the final text by seconds. With `final_full_max_ms = 0` the daemon does not load it at all.
- If it fails to load, the daemon logs the error to the journal and works with the main model only.
- `koeinput status` prints a `final model:` line with `ready`, or `not loaded yet or failed` and a pointer to the journal. `model list` marks it with `final model`.

### D-Bus properties

Interface `org.nemui.KoeInput.Recognizer1` on `/org/nemui/KoeInput` has these read-only properties:

| Property | Type | Meaning |
|---|---|---|
| `Ready` | b | The main model is loaded. |
| `Model` | s | The main model id. |
| `FinalModel` | s | The configured final-stage model id, empty when none. |
| `FinalModelReady` | b | The final-stage model is loaded. |
| `Version` | s | The daemon version. |

`FinalModel` and `FinalModelReady` were added in M4. Older clients ignore them, and `koeinput status` prints no final-model line for an older daemon that lacks them.
To read them by hand: `busctl --user introspect org.nemui.KoeInput /org/nemui/KoeInput`.

## Dictionary

The dictionary replaces known misrecognitions after recognition, with any model.

```sh
koeinput dict add ラスト Rust       # add, or replace the entry for ラスト
koeinput dict remove ラスト
koeinput dict list                  # wrong<TAB>right per line
```

- The file is `~/.config/koeinput/dictionary.tsv` (or the `dictionary` path in the config), one `wrong<TAB>right` per line. `dict add` and `dict remove` keep comments, blank lines and the order of entries, and write through a temporary file and a rename.
- `dict add` refuses an empty right form. An entry that deletes a word (empty right form) is made by editing the file by hand.
- The daemon reads the file again at the start of the next session after its modification time or size changed; no restart is needed. A file over 1 MiB is not reloaded. `koeinput transcribe` reads it at start.

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
- apps that show partial text in the panel: program names that use the panel whatever the partial text setting says. Case does not matter; spaces around a name are ignored. Use it for apps that commit their own composition on blur, such as text-input-v3 clients (Chrome, Discord). To find the name fcitx5 uses for an app, focus a text field in the app and run `fcitx5-diagnose`: the "Input Context" list in its output names the program of each context (the same list comes from `dbus-send --session --print-reply --dest=org.fcitx.Fcitx5 /controller org.fcitx.Fcitx.Controller1.DebugInfo`). Apps without a program name follow the main setting. The per-app list works only when fcitx5 knows the app's name; some fcitx5 versions under KDE Wayland do not report it, and then the app follows the main setting, so use the main "In the panel" setting if the list has no effect.
- apps that show only the end of partial text, and characters shown at the end (10 to 1000, default 30): terminals draw preedit on one row and cut it off at the window edge, so a long partial text hides its end. For a session in a listed app, a partial text longer than the set length shows as `…` followed by its last characters, in the app and in the panel alike; the `…` counts toward the length. The final text is always committed in full. The list uses the same matching rules as the panel list (case does not matter, spaces around a name and empty entries are ignored, a context without a program name is never listed) and is empty by default. WezTerm's name is `org.wezfurlong.wezterm`; find other names the same way as for the panel list.

Configs from before the "Partial text" setting have the boolean `ShowPartial`. The addon reads `ShowPartial=False` as "Off"; the next save from the settings writes `PartialText=Off`.

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

## Packages

Every format has the same two packages:

- `koeinput`: the daemon, the `koeinput` command, the systemd user unit, the D-Bus service file, the model catalog and the man page;
- `fcitx5-koeinput`: the addon.

They are built for amd64 only. Everything runs in podman containers. Each format has a build script and a test script:

| Target | Build and test | Output |
|---|---|---|
| Debian 13, Debian testing and unstable, Ubuntu 24.04, Ubuntu 26.04 | `scripts/build-deb <dist>`, `scripts/test-deb <dist>` with `trixie`, `forky`, `noble` or `resolute`; `scripts/test-deb sid` tests the forky packages on sid | `target/deb/<dist>/` |
| Fedora 44 | `scripts/build-rpm fedora44`, `scripts/test-rpm fedora44` | `target/rpm/fedora44/` |
| Arch Linux | `scripts/build-arch`, `scripts/test-arch` | `target/arch/` |

This section describes what the formats share. The sections after it describe what is particular to each format.

### Orig tarballs

All formats build from the three orig tarballs of the Debian source package, under the same names. `scripts/make-orig` makes them in `target/deb/` from a clean work tree:

- `koeinput_0.1.0.orig.tar.xz`: `git archive` of HEAD, without `debian/`;
- `koeinput_0.1.0.orig-vendor.tar.xz`: the crates of `Cargo.lock`, from `cargo vendor-filterer` (crates that x86_64 Linux does not build become empty stubs);
- `koeinput_0.1.0.orig-sherpaonnx.tar.bz2`: the pinned sherpa-onnx archive, byte for byte.

make-orig keeps an existing tarball with the same content and stops at one with different content. Every build script runs it when a tarball is missing or older than HEAD.
The files in the main tarball come from HEAD, so commit them before you build. Each format section says which packaging files come from the work tree instead.

### The main tarball changes with every commit

`git archive` records the commit, so every commit changes `koeinput_0.1.0.orig.tar.xz`. The build script then runs make-orig, and make-orig stops because the tarball differs. While a version is unpublished (0.1.0 now), delete its tarballs and build again:

```sh
rm target/deb/koeinput_0.1.0.orig*
scripts/build-deb forky
```

make-orig needs a clean work tree, so commit or set aside the packaging changes first.
Once a version is released, keep its tarballs. Any later build then needs a new upstream version, even for a change in `debian/` alone, which normally only raises the Debian revision (see [followups.md](followups.md)).

### Checks in every build

Before the build container starts, each build script:

- runs `scripts/gen-copyright --check`. All formats ship `debian/copyright` as the copyright file of `koeinput`;
- runs `scripts/check-vendor-copyright` on the vendor tarball. It fails when a vendored crate has no `Files: vendor/<crate>/*` paragraph in `debian/copyright`, or when a paragraph names a crate that is not in the tarball;
- checks the sherpa-onnx tarball against the SHA-256 in `packaging/sherpa-onnx.env`.

The package build then runs without network. It:

- runs `cargo test` and the addon's `controller` test. `cargo test` uses its own target directory, so it cannot rebuild the binaries that go into the package;
- runs `scripts/check-no-gpl` on the files that go into the package, before they are stripped:
  - Debian and Ubuntu: on the built binaries, and again on the installed files in `debian/koeinput/` just before `dh_strip`;
  - Fedora: on the built binaries in `%build`. `%install` copies them unchanged, and rpmbuild strips them after `%install`;
  - Arch: on the built binaries, and again at the end of `package_koeinput()` on the installed files, before makepkg strips them.

  The installed binaries are stripped, so the install tests cannot run this check.

### Debug packages

Each format also builds packages with debug symbols:

- Debian: `koeinput-dbgsym` and `fcitx5-koeinput-dbgsym` (`.deb`);
- Ubuntu: the same packages, named `*.ddeb`;
- Fedora: `koeinput-debuginfo`, `fcitx5-koeinput-debuginfo` and `koeinput-debugsource`;
- Arch: `koeinput-debug`.

They are for local debugging. A release uploads only the two user packages per target.

### Documentation in the packages

`koeinput` installs `README.md`, `README.ja.md` and `models.md` (the model attributions) in `/usr/share/doc/koeinput/`. Debian and Ubuntu compress the files over 4 kB there, so the READMEs are `README.md.gz` and `README.ja.md.gz`; `models.md` is smaller and stays plain. On Fedora, `fcitx5-koeinput` also has `README.md` in `/usr/share/doc/fcitx5-koeinput/`, because rpmlint warns about a package without documentation.

### Install tests

`scripts/lib/install-checks.sh` is the install test of all formats. Each test script runs it in a fresh container of the distribution. The package manager downloads the dependencies, so the test scripts need the network. The test checks:

- the installed files, and which package installs each one. The D-Bus service file and the user unit start the daemon at its installed path, the user unit is not enabled, the copyright file has the paragraphs of the vendored crates, and the ONNX Runtime notices are installed;
- `koeinput --version`, `koeinput model list` and `man koeinput`, as an ordinary user;
- `koeinput status` under `dbus-run-session`: the bus starts the daemon through the D-Bus service file. With no model installed, the daemon reports `ready: no` and logs the install hint;
- fcitx5 without a display (`fcitx5 --disable=all --enable=keyboard,dbus,koeinput`): it loads the addon, and the addon starts the daemon;
- the removal removes the files, and a second install works.

Each test script writes a short format file for `scripts/lib/install-checks.sh` with what differs between the formats: the package files, the paths (the daemon, the addon directory, the documentation and license files), the package manager commands, and the checks that only that format has. The comment at the top of `scripts/lib/install-checks.sh` lists the names. To add a check for all formats, add it to `scripts/lib/install-checks.sh`.

A failed check prints `FAIL:` and stops the test, and the test script exits with status 1. If fcitx5 cannot run without a display, the test prints a warning and still runs the other checks. `podman run --init` does not work here (no catatonit), so the test runs its own PID 1, which passes signals on and stops a test that hangs. The output also goes to `test.log` in the output directory.

### Crates and debian/copyright

`scripts/gen-copyright` writes `debian/copyright`. With `--check` it only compares and fails when the file is out of date.

It takes the license of each vendored crate from `cargo metadata`, and the copyright lines from the crate's license files. Its tables add what those files miss:

- `EXTRA_HOLDERS`: copyright holders that a crate's license files leave out;
- `BUNDLED`: files inside a crate under another license, such as bundled C code or tests taken from other projects;
- `LICENSE_TEXTS`: where each license text comes from.

After a change of `Cargo.lock`, run `scripts/gen-copyright` and read the diff of `debian/copyright`. The build scripts fail when a vendored crate has no paragraph (`scripts/check-vendor-copyright`). A change of `Cargo.lock` also changes the vendor tarball, so after a release it needs a new upstream version.

## Debian packages

`debian/` builds the packages for Debian 13 (trixie), Debian testing (forky), Ubuntu 24.04 (noble) and Ubuntu 26.04 (resolute), all from the same directory. The forky packages are also the packages for Debian unstable (sid). See [Packages](#packages) for what all formats share.

```sh
scripts/build-deb forky        # or trixie, noble, resolute
scripts/test-deb forky
scripts/test-deb sid           # the forky packages on sid
```

Build the packages for each release on its own: they depend on the library versions of the release they were built on. For example, a forky build needs glibc 2.43, because it links float functions of libm such as `asinf` at the symbol version `GLIBC_2.43`, and Debian 13 has glibc 2.41. A 26.04 build needs fcitx5 5.1.19 or newer.

sid has no build of its own. Its libraries are as new as those of testing or newer, so the forky packages install there. They can fail only while sid has a library with a new soname that has not reached testing yet.

- `scripts/build-deb trixie|forky|noble|resolute` builds the source and binary packages without network in the image from `packaging/containers/deb-<dist>.Containerfile`, then runs lintian. The packages, the source package, `build.log` and `lintian.txt` go to `target/deb/<dist>/`.
  - It runs the [checks in every build](#checks-in-every-build).
  - Files outside `debian/` come from HEAD through the orig tarball, so commit them first. `debian/` comes from the work tree, so you can try packaging changes before you commit them.
  - Rust comes from the distribution. The workspace needs 1.89:
    - Debian 13 has rustc 1.85, so the trixie image takes Rust from trixie-backports;
    - Ubuntu 24.04 has rustc 1.75, so the noble image installs `rustc-1.91` and `cargo-1.91` (noble-updates). They install to `/usr/lib/rust-1.91/bin`, which `debian/rules` puts first in `PATH` when it exists. `debian/control` accepts either `rustc (>= 1.89)` or `rustc-1.91` (and the same for cargo);
    - Debian forky and Ubuntu 26.04 have new enough Rust.

    `debian/rules` stops with a message when `rustc` is older than 1.89.
  - The trixie and Ubuntu builds get their own version: in the container, before the package build, the script adds a local entry to `debian/changelog` with `dch --local ~<dist> --distribution <dist>`. The packages are then `0.1.0-1~trixie1`, `0.1.0-1~noble1` and `0.1.0-1~resolute1`. The entry is dated one second after the committed top entry, so the build date does not depend on when it runs, and the entry is newer than the one below it, as lintian wants. The committed `debian/changelog` does not change, and the forky build keeps `0.1.0-1`.

    So every Debian and Ubuntu target has its own file names. `~` sorts before the end of the version, so `0.1.0-1~trixie1` is older than `0.1.0-1`: after an upgrade from Debian 13 to forky, apt replaces the trixie build with the forky build of the same KoeInput version.

    These builds print `dpkg-genchanges: warning: the current version (0.1.0-1~<dist>1) is earlier than the previous one (0.1.0-1)`. This is expected: the local `~<dist>1` entry sits above `-1` on purpose, and `~` sorts before the end of the version.
  - The [debug packages](#debug-packages) go to `target/deb/<dist>/` too.
  - lintian must report no errors and no warnings. Ubuntu 24.04 has an older lintian (2.117), which reports other spelling errors in the binaries than the newer versions that `debian/koeinput.lintian-overrides` was written for. So the noble build skips the `spelling-error-in-binary` check; the comment in `scripts/build-deb` gives the details. The 26.04 lintian output shows the experimental tag `X: spelling-error-in-binary wtH`: the word is bytes of machine code. It has no override, because the override would be unused on Debian, and it does not fail the build.
- `scripts/test-deb trixie|forky|sid|noble|resolute` runs the [install tests](#install-tests) in a fresh container: `debian:<dist>`, `ubuntu:24.04` or `ubuntu:26.04`. The Ubuntu container images leave out man pages, translations and most documentation, and replace `man` with a stub. The script undoes this first, so the files are installed as on a normal Ubuntu system. The output is also saved in `target/deb/<dist>/test.log`. `sid` installs the packages from `target/deb/forky/` in `debian:sid` and saves the output in `target/deb/forky/test-sid.log`.

### New version

1. Raise `version` in `Cargo.toml` and the `koeinput-dbus-api` version in `crates/cli/Cargo.toml` and `crates/daemon/Cargo.toml`. Then update the lock file: `scripts/dev cargo update --workspace`.
2. Raise `VERSION` in `project()` in `addon/CMakeLists.txt`.
3. Add an entry at the top of `debian/changelog` (version `X.Y.Z-1`) in the same format as the existing one.
4. Commit, then run `scripts/build-deb` and `scripts/test-deb` for each distribution.

### New sherpa-onnx pin

After the steps in [sherpa-onnx libraries](#sherpa-onnx-libraries), the package needs these:

1. Fetch the license files of the components again, at the versions that the new sherpa-onnx pins in its `cmake/*.cmake` files, into `packaging/licenses/`. Update `packaging/licenses/README` (file, component, commit, source URL).
2. Update the `SHERPA` and `REPO_LICENSE_FILES` tables in `scripts/gen-copyright`: versions, components, copyright holders and the Eigen source URLs. Update the ONNX Runtime notices file name in `debian/koeinput.docs`.
3. Run `scripts/gen-copyright`. It stops with a message when a table names a file or crate that no longer exists.
4. Build with a new upstream version, because the sherpa-onnx tarball changes.
5. Check `debian/koeinput.lintian-overrides`. It lists spelling errors inside the prebuilt libraries; lintian reports overrides that are no longer used.

## RPM

`packaging/rpm/koeinput.spec` builds the same two packages, `koeinput` and `fcitx5-koeinput`, for Fedora 44 on amd64. See [Packages](#packages) for what all formats share.

```sh
scripts/build-rpm fedora44
scripts/test-rpm fedora44
```

- `Source0` to `Source2` are the three orig tarballs, written as release URLs (`%{url}/releases/download/v%{version}/koeinput_0.1.0.orig.tar.xz` and so on). rpmbuild uses only the file names, so build-rpm links the files from `target/deb/`. A release must upload the three orig tarballs under these names.
- `Source3`, `koeinput-copyright`, is a copy of `debian/copyright`; the package installs it as `/usr/share/licenses/koeinput/copyright`.
- `%prep` checks the sherpa-onnx tarball against the SHA-256 in `packaging/sherpa-onnx.env` from the main tarball. It also checks the macros `sherpa_onnx_version` (against the archive name in that file) and `onnxruntime_version` (against the ONNX Runtime license file in `packaging/licenses/`), which give the `bundled(...)` versions.
- `scripts/build-rpm fedora44` runs `rpmbuild -ba` without network in the image from `packaging/containers/rpm-fedora44.Containerfile`, then rpmlint on the spec and all packages. It fails on rpmlint errors; warnings are only listed. The packages (with the [debug packages](#debug-packages)), the source RPM, `build.log` and `rpmlint.txt` go to `target/rpm/fedora44/`.
  - It runs the [checks in every build](#checks-in-every-build). The spec and `debian/copyright` come from the work tree; everything else comes from HEAD through the main tarball, so commit it first.
  - The build uses Fedora's compiler and Rust flags, plus `--remap-path-prefix`, so the build directory does not end up in the binaries.
- `scripts/test-rpm fedora44` runs the [install tests](#install-tests) with dnf in a fresh `fedora:44` container. It also runs `rpm -V` and checks that the packages have no install scripts (the user unit is never enabled). The Fedora image installs no documentation (`tsflags=nodocs`, which leaves out man pages too) and no translations other than en_US (`/etc/rpm/macros.image-language-conf`), so the test turns both off first, as on a desktop. The output is also saved in `target/rpm/fedora44/test.log`.
- The license files go to `/usr/share/licenses/koeinput/`: `LICENSE-MIT`, `LICENSE-APACHE`, `copyright` (a copy of `debian/copyright`, with the notices of all the code linked into the binaries) and the files of `packaging/licenses/`. `fcitx5-koeinput` has the two project licenses.
- The `License` tag of `koeinput` is the macro `koeinput_license` at the top of the spec: the licenses of the crates the binaries link and of the sherpa-onnx libraries. At the start of `%build`, the build compares it with `cargo tree` and fails when a linked crate has a license that the tag does not list (add the license to the macro then), when a linked crate has no license expression, or when cargo tree fails or lists nothing.
- For a new version, change `Version:` and add a `%changelog` entry; keep `Release: 1%{?dist}`. build-rpm stops when `Version:` differs from the upstream version in `debian/changelog`, because the orig tarballs are named after it. For a new sherpa-onnx pin, update `sherpa_onnx_version`, `onnxruntime_version` and the sherpa-onnx part of `koeinput_license`; `%prep` stops while the two versions do not match the pin.

## Arch Linux packages

`packaging/arch/PKGBUILD` is a split package. It builds `koeinput` and `fcitx5-koeinput` with the same contents as the Debian packages, and makepkg adds `koeinput-debug` (see [Debug packages](#debug-packages)). See [Packages](#packages) for what all formats share.

```sh
scripts/build-arch
scripts/test-arch
```

- The sources are the three [orig tarballs](#orig-tarballs) and `debian/copyright`.
- The committed PKGBUILD pins the SHA-256 of the sherpa-onnx archive (the value in `packaging/sherpa-onnx.env`). The main and vendor tarballs and `copyright` have `SKIP`: the main tarball contains the PKGBUILD itself, and all three change with commits and `Cargo.lock`.
- `scripts/build-arch` runs the [checks in every build](#checks-in-every-build), then builds in the image from `packaging/containers/arch.Containerfile` (`archlinux:latest` with the build tools and a build user, because makepkg refuses to run as root). In a container without network, it:
  1. copies the PKGBUILD from the work tree, the three tarballs and `debian/copyright` to the build user;
  2. checks that the PKGBUILD's sherpa-onnx source and sum are the ones in `packaging/sherpa-onnx.env`;
  3. fills in the real sums on that copy with `updpkgsums`;
  4. runs `makepkg -s --noconfirm`. All dependencies are in the image, so nothing is installed. If the PKGBUILD needs a package that the image does not have, the build stops and names it; add it to the Containerfile;
  5. runs `namcap` on the PKGBUILD and the packages.

  The packages, the PKGBUILD with the real sums, `build.log` and `namcap.txt` go to `target/arch/`. The script fails on namcap errors and warnings, except for the ones listed and explained in it.
- Files outside `packaging/arch/` come from HEAD through the main tarball, so commit them first. The PKGBUILD comes from the work tree.
- The PKGBUILD turns off makepkg's LTO. With LTO, the C code in the crates becomes GCC LTO bytecode, which Rust's linker (lld) cannot read.
- Arch is a rolling release, but the image is built only once for each version of the Containerfile. To build with current packages, remove the image: `podman rmi $(podman images -q localhost/koeinput-arch)`.
- The addon goes to `/usr/lib/fcitx5/`. The license texts go to `/usr/share/licenses/koeinput/` and `/usr/share/licenses/fcitx5-koeinput/`. The `koeinput` directory also has:
  - `copyright`: `debian/copyright`, with the copyright notices and licenses of the vendored crates and the sherpa-onnx archive. The file is written for Debian: for some licenses it points to `/usr/share/common-licenses/`, which Arch does not have; Arch has these texts in `/usr/share/licenses/spdx/`;
  - `sherpa-onnx/`: the license texts of the sherpa-onnx archive;
  - `crates/`: the license files of the crates in the binaries.
- Arch has no `/usr/libexec`, so the daemon goes to `/usr/lib/koeinput/koeinput-daemon`. The PKGBUILD changes the path in the installed copies of the D-Bus service file and the user unit; the files in `data/` keep `/usr/libexec/koeinput/` for the other distributions.
- `scripts/test-arch` runs the [install tests](#install-tests) with `pacman -U` in a fresh `archlinux:latest` container, with `pacman -R` for the removal. It also checks the versions that `pacman -Q` shows, that nothing is in `/usr/libexec/`, and that the license files of the crates are installed. The image leaves out man pages and translations (`NoExtract` in `/etc/pacman.conf`), so the test removes those lines before it installs the packages. The output is also saved in `target/arch/test.log`.
  The pacman hooks print messages that are expected in a container: "Skipped: Current root is not booted.", "Failed to connect to audit log, ignoring", and the `journal-nocow.conf` line "Failed to resolve specifier: uninitialized /etc/ detected, skipping."

To install on an Arch Linux machine:

```sh
sudo pacman -U target/arch/koeinput-0.1.0-1-x86_64.pkg.tar.zst target/arch/fcitx5-koeinput-0.1.0-1-x86_64.pkg.tar.zst
```

Then install a model and restart fcitx5, as in [Install on the host](#install-on-the-host) from step 3.

For a new version, set `pkgver` in the PKGBUILD to the new upstream version and keep `pkgrel=1`. build-arch stops when `pkgver` differs from the upstream version in `debian/changelog`.

## CI

GitHub Actions runs the two workflows in `.github/workflows/` on `ubuntu-24.04` runners, which have rootless podman.
The workflows call the scripts of this repository and use the same images, so you can run every step with the same command on your machine.

`ci.yml` runs on every push to a branch and on pull requests. Its jobs:

- **Image**: builds the `dev-trixie` and `dev-bookworm` images when their Containerfile has changed, and saves them in the Actions cache. The other jobs wait for it and load the images from the cache.
- **Rust**: `cargo fmt`, clippy, the tests, the D-Bus tests and `scripts/gen-copyright --check`. Then it fails if these commands changed a tracked file, for example an out-of-date `Cargo.lock`.
- **No GPL code in a release build**: a release build and `scripts/check-no-gpl` on both binaries. It is a job of its own because the release build shares no compiled code with the dev builds, so the two run at the same time.
- **Rust version in Cargo.toml**: `cargo check` with the `rust-version` of `Cargo.toml` (1.89). rustup in the dev image installs that toolchain for the run.
- **fcitx5 addon**: `scripts/addon test` in the Debian 13 and the Debian 12 image.

The same checks on your machine:

```sh
scripts/dev cargo fmt --all --check
scripts/dev cargo clippy --workspace --all-targets -- -D warnings
scripts/dev cargo test --workspace
scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks
scripts/gen-copyright --check
scripts/dev cargo build --release --workspace
scripts/dev scripts/check-no-gpl target/release/koeinput target/release/koeinput-daemon
scripts/dev sh -c 'rustup toolchain install 1.89 --profile minimal && cargo +1.89 check --workspace --all-targets'
scripts/addon test
KOEINPUT_DEV_IMAGE=bookworm scripts/addon test
```

`scripts/dev` removes its container after each command, so the 1.89 line downloads the toolchain each time.

`release.yml` builds the orig tarballs and the packages of all six targets, and makes a draft release on a tag. [release.md](release.md) describes it. To run it without a tag, start it by hand and download the packages (this needs `release.yml` on `main`):

```sh
gh workflow run release.yml --ref <branch>
gh run download <run-id>
```

The workflows cache two things:

- the `dev-trixie` and `dev-bookworm` images, as `podman save` archives. The cache key contains the same Containerfile hash as the image tag in the scripts, so a changed Containerfile gives a new entry. Only the Image job saves them; `release.yml` loads `dev-trixie` from there too. Loading an image is faster than building it: the `dev-trixie` build installs Rust and compiles `cargo-vendor-filterer`;
- the downloaded crates and the sherpa-onnx archive from `work/cache/`, keyed on `Cargo.lock` and `packaging/sherpa-onnx.env`. Only the Rust job of `ci.yml` saves this cache, because it runs every command that downloads crates. These files are about 60 MB, so the time saved is small; the main gain is fewer downloads from crates.io and GitHub.

Not cached:

- `target/`: a full build is about 3.5 GB, and a runner has about 14 GB of free disk;
- the package build images (`deb-*`, `rpm-fedora44`, `arch`): about 2 to 3 GB each. Together they would fill most of the 10 GB cache of the repository, and only release builds use them.

A run on a branch can use the caches of `main` and of its own branch. A run on a tag can use the caches of `main`.

## Install on the host

Build and test the packages in the dev container first:

```sh
scripts/build-deb forky
scripts/test-deb forky
```

Then, in a terminal on the host, in the repository directory (`~/works/git/nemui/koeinput`):

0. Run `sudo apt update && sudo apt upgrade` first: the forky build needs the current libc6 and fcitx5. On Debian 13, use the trixie build instead (`target/deb/trixie/`, version `0.1.0-1~trixie1`).

1. If you started `koeinput-daemon` by hand in the dev container, stop it in that shell (Ctrl-C), or run `pkill -x koeinput-daemon`. While it runs, it holds the D-Bus name and the installed daemon cannot start.
2. Install the two packages:

   ```sh
   sudo apt install ./target/deb/forky/koeinput_0.1.0-1_amd64.deb ./target/deb/forky/fcitx5-koeinput_0.1.0-1_amd64.deb
   ```

   apt may say that the download is performed unsandboxed as root. That is harmless for local files. The `-dbgsym` packages in the same directory hold debug symbols; install them only if you need backtraces.
3. Install the model once. To use the archive downloaded in the model spike:

   ```sh
   koeinput model install parakeet-ja --from-file work/models/_dl/sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8.tar.bz2
   ```

   The VAD model is downloaded with it. To download both, run `koeinput model install parakeet-ja` instead.

   If the daemon was already running without a model, it loads the model you installed at the next dictation. Restart it (`systemctl --user restart koeinput`) only if `koeinput status` still says `ready: no` after one try. Changing the model with `koeinput model use` still needs a restart, because the reload uses the model that was configured when the daemon started.
4. Restart fcitx5 so that it loads the addon. Open System Settings, then Keyboard, then Virtual Keyboard (Plasma 6; on Plasma 5 it is Input Devices, then Virtual Keyboard). Choose None and apply, then choose Fcitx 5 and apply again. The keyboard keeps typing while fcitx5 is down.
5. The addon starts the daemon. Check it:

   ```sh
   koeinput status
   ```

   It prints `ready: yes` and `model: parakeet-ja`. The model takes a few seconds to load; until then it prints `ready: no`. If it keeps printing `ready: no` together with the install hint, the model step was skipped.

Then go through the [host check](#host-check).

### Upgrade

Nothing restarts by itself after an upgrade. Install the new packages the same way as in step 2. Unreleased builds keep the same version, and apt reports "already the newest version" and installs nothing for the same version, so add `--reinstall`:

```sh
sudo apt install --reinstall ./target/deb/forky/koeinput_0.1.0-1_amd64.deb ./target/deb/forky/fcitx5-koeinput_0.1.0-1_amd64.deb
systemctl --user restart koeinput
```

For a released version with a new number, the command is the same without `--reinstall`:

```sh
sudo apt install ./target/deb/forky/koeinput_<version>_amd64.deb ./target/deb/forky/fcitx5-koeinput_<version>_amd64.deb
systemctl --user restart koeinput
```

Restart fcitx5 as in step 4 when fcitx5-koeinput changed. The running daemon and the loaded addon keep the old code until you restart them.

### Rollback

```sh
systemctl --user stop koeinput
sudo apt remove fcitx5-koeinput koeinput
```

If you install an older build by hand, first remove `final_model` and `final_full_max_ms` from `~/.config/koeinput/config.toml`, because older builds refuse unknown settings.

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

## Model catalog maintenance

The archives' own README files were not compared with the upstream model cards when `docs/models.md` was written.
If a model page and `data/models.toml` differ, the upstream page is right: fix the catalog and `docs/models.md`.
