# M3: Debian packaging

Status: approved for implementation, 2026-10-18 (the user asked for no review stop).

Part of [the overall design](2026-10-17-overall-design.md). Background decisions are in [decisions.md](../../decisions.md).
Builds on M1 (daemon and CLI), M2 (fcitx5 addon) and the Rust hardening branch.
The research behind this spec (2026-10-18) is summarized here; its raw notes and upstream files are kept in `work/m3-research/` (not in git).

## Goal

The user installs two Debian packages on the host, installs a model once, and dictates in every app from then on.
This is where daily use starts.

Done when:

- One source package `koeinput` builds two binary packages, `koeinput` and `fcitx5-koeinput`, fully offline, for Debian 13 (trixie, with trixie-backports for Rust) and for Debian testing (the user's host).
- Lintian on the binary packages reports nothing worse than the accepted tags listed under "Lintian".
- Both packages install, work and remove cleanly in fresh Debian 13 and Debian testing containers (automated).
- No GPL code is linked into the binaries (automated check).
- The host install steps are written down, with rollback, for the user to run.

Out of scope for M3: RPM, PKGBUILD, Ubuntu packages and CI (M5); output quality and the optional final-stage model (M4).

## 1. License: no GPL code in the binaries

The default sherpa-onnx prebuilt archive (`sherpa-onnx-v1.13.8-linux-x64-static-lib.tar.bz2`) links espeak-ng and ucd-tools (GPL-3.0-or-later) into both binaries, although KoeInput never uses text to speech.
Binaries built from it could only be shipped under GPL terms, with an offer of corresponding source for prebuilt libraries we do not have.

**Decided** (controller ruling, 2026-10-18): every build, development and package, uses the upstream **no-TTS** static archive:

| Item | Value |
|---|---|
| File | `sherpa-onnx-v1.13.8-linux-x64-static-no-tts-lib.tar.bz2` |
| URL | `https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-linux-x64-static-no-tts-lib.tar.bz2` |
| Size | 21,755,415 bytes |
| SHA-256 | `36f2ebd0b9aa09248ff6461a30dcb7edfe7eecf2deb67dfb0ceb3aaf4906051f` |

- `sherpa-onnx-sys` checks no checksum, so we pin and check the SHA-256 ourselves.
- The crate is pointed at the extracted `lib/` directory with `SHERPA_ONNX_LIB_DIR`.
  Its build script always asks for `espeak-ng`, `piper_phonemize` and `ucd`, so that directory also gets three empty stub archives (`ar rcs lib<name>.a`).
- A check fails the build if `espeak_`, `ucd_` or `piper::` symbols appear in a binary (`nm -C`).
- Everything left is permissive (Apache-2.0, MIT, BSD-2-Clause, BSD-3-Clause, BSL-1.0, ISC, Unicode-3.0, CDLA-Permissive-2.0) or file-level MPL-2.0 (Eigen, header-only inside the C++ libraries).
  Our code stays `MIT OR Apache-2.0`.
- The archive exists only for linux-x64, so the packages are `Architecture: amd64`.
  arm64 would need sherpa-onnx built from source; not planned.
- The crate version and the archive version must match (`sherpa-onnx = "=1.13.8"`). Upgrading means a new pin, new stubs check, new attribution.

## 2. Development builds

- `packaging/sherpa-onnx.env` holds the pin (file name, URL, size, SHA-256) in shell syntax.
- `scripts/sherpa-onnx` downloads the archive once into `work/cache/sherpa-onnx/`, checks size and SHA-256, extracts it, adds the stubs, and prints the `lib/` path. It accepts an existing local file (`--from-file`) for offline machines.
- `scripts/dev` sets `SHERPA_ONNX_LIB_DIR=/src/work/cache/sherpa-onnx/<stem>/lib` for every container run, after running `scripts/sherpa-onnx` if the directory is missing.
  `target/sherpa-onnx-prebuilt/` is no longer used.
- `scripts/check-no-gpl` runs the symbol check on given unstripped binaries (it refuses stripped ones, where it could prove nothing); the package build calls it before `dh_strip`.

## 3. Daemon start through D-Bus activation

With the packages, the daemon is started by D-Bus activation (`/usr/share/dbus-1/services/org.nemui.KoeInput.service`, delegated to the systemd user unit `koeinput.service`).
The unit has no `[Install]` section, so it is never enabled for all users, and debhelper (compat 13) generates no maintainer scripts for it.

Two changes keep the first dictation after login from failing while the model loads:

- **The addon starts the daemon early.** When the addon loads (at fcitx5 start), it calls `org.freedesktop.DBus.StartServiceByName("org.nemui.KoeInput", 0)` asynchronously (through a new `DaemonClient::activate()`) and ignores the result. A failure (no activation file, as in the M2 dev setup) is only logged.
- **The daemon lets `Start` wait for the model.** A `Start` that arrives while the model is still loading waits up to 4 s for the load to finish, then proceeds, or fails with `NoModel` "the model is still loading" as today. The wait never blocks other D-Bus calls. 4 s stays below the addon's 5 s `Start` timeout.

The daemon keeps requesting its bus name at start, before the model is loaded, so status queries answer at once.

## 4. Source package

Format `3.0 (quilt)`, debhelper compat 13, no dh-cargo (our crates are not in Debian).

| Tarball | Content |
|---|---|
| `koeinput_0.1.0.orig.tar.xz` | `git archive --prefix=koeinput-0.1.0/ HEAD` |
| `koeinput_0.1.0.orig-vendor.tar.xz` | `cargo vendor-filterer --platform=x86_64-unknown-linux-gnu --locked --versioned-dirs`, top directory `vendor/` (the Windows-only crates become empty stubs, which removes 100 MB and the only lintian errors) |
| `koeinput_0.1.0.orig-sherpaonnx.tar.bz2` | the pinned no-TTS archive, byte for byte, so its SHA-256 in the `.dsc` equals the upstream digest |

`dpkg-source` unpacks the components into `vendor/` and `sherpaonnx/`.
An orig tarball cannot change within one upstream version: a change of `Cargo.lock` or of the sherpa-onnx pin needs a new upstream version.

`scripts/make-orig` produces the three tarballs into `target/deb/` from a clean git tree (it refuses a dirty tree), and checks the sherpa-onnx SHA-256.
`cargo-vendor-filterer` (0.5.18, from crates.io) is installed in the Debian 13 development image.

### debian/

- `control`: source `koeinput`, `Section: utils`, `Maintainer: Tatsuki Sugiura <sugi@nemui.org>`, `Rules-Requires-Root: no`, `Build-Depends: debhelper-compat (= 13), cargo (>= 1.89), rustc (>= 1.89), pkgconf, libpulse-dev, cmake (>= 3.25), extra-cmake-modules, gettext, libfcitx5core-dev, libfcitx5config-dev, libfcitx5utils-dev, fcitx5-modules-dev, binutils`.
  - `koeinput`: `Architecture: amd64`, `Depends: default-dbus-session-bus | dbus-session-bus, ${misc:Depends}, ${shlibs:Depends}`, `Suggests: fcitx5-koeinput`.
  - `fcitx5-koeinput`: `Architecture: amd64`, `Depends: fcitx5, koeinput (= ${binary:Version}), ${misc:Depends}, ${shlibs:Depends}`.
  - Descriptions say that models are downloaded separately with `koeinput model install` and that recognition runs locally.
- `rules`: plain `dh $@ --buildsystem=cmake --sourcedirectory=addon`; cargo runs in the overrides with `--release --frozen --offline --workspace`, `CARGO_HOME` inside the build tree with a vendored-sources config, absolute `CARGO_TARGET_DIR` and `SHERPA_ONNX_LIB_DIR` (from `sherpaonnx/lib` plus stubs), `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` and `CARGO_PROFILE_RELEASE_STRIP=none` so `dh_strip` makes `-dbgsym` packages, `RUSTFLAGS=--remap-path-prefix=$(CURDIR)=/usr/src/koeinput-<version>`, jobs from `DEB_BUILD_OPTIONS=parallel`.
  - Tests during the build (unless `nocheck`): `cargo test` (core and CLI) and the addon's `controller` test. The D-Bus and headless fcitx5 tests stay in the development test runs.
  - The no-GPL check runs on both binaries after the build.
  - `dh_clean -XCargo.toml.orig` (otherwise `dh_clean` deletes the vendored `Cargo.toml.orig` files and cargo's vendor checksums fail).
- `koeinput.install`: `/usr/bin/koeinput`, `/usr/libexec/koeinput/koeinput-daemon`, `/usr/lib/systemd/user/koeinput.service`, `/usr/share/dbus-1/services/org.nemui.KoeInput.service`, `/usr/share/koeinput/models.toml`.
- `fcitx5-koeinput.install`: `usr/lib/*/fcitx5/libkoeinput.so`, `usr/share/fcitx5/addon/koeinput.conf`, `usr/share/locale/*/LC_MESSAGES/fcitx5-koeinput.mo`.
- `koeinput.manpages`: `koeinput.1`, generated during the build by a hidden CLI subcommand `koeinput manpage` (crate `clap_mangen`).
- `koeinput.docs`: `README` text for the package (short), the ONNX Runtime `ThirdPartyNotices.txt`.
- `copyright`: DEP-5, generated by `scripts/gen-copyright` (see below).
- `changelog`: `koeinput (0.1.0-1) unstable; urgency=medium`, first entry.
- `source/format`: `3.0 (quilt)`.

### Attribution

- `LICENSE-MIT` and `LICENSE-APACHE` at the repository root; `license = "MIT OR Apache-2.0"` in the workspace and every crate.
- `packaging/licenses/` holds the license texts of the components inside the no-TTS archive, fetched once at their pinned versions and committed, and ONNX Runtime 1.28.2's `ThirdPartyNotices.txt`.
- `scripts/gen-copyright` writes `debian/copyright` from `cargo metadata` (one `Files: vendor/<crate>-<version>/*` paragraph per vendored crate, with its license expression and authors), a `Files: sherpaonnx/*` paragraph listing every component with its license, our own `Files: *`, and the full text of every license used (texts in `/usr/share/common-licenses/` are referenced instead).
  `scripts/gen-copyright --check` fails if the committed file is out of date; the package build script runs it.
- MPL-2.0 asks that users are told where the Eigen source is: `debian/copyright` names the Eigen 5.0.1 source URL.
- Model licenses (parakeet-ja is CC-BY-4.0) are not part of the packages, because models are downloaded by the user; `koeinput model list` already shows each model's license.

### Lintian

`scripts/build-deb` runs lintian (`-EvIL +pedantic`) on the binary `.changes`.
Accepted tags: `initial-upload-closes-no-bugs` (we do not upload to Debian), `hardening-no-fortify-functions` (from the prebuilt C++ libraries), experimental `spelling-error-in-binary` from the prebuilt libraries, `no-homepage-field` and `debian-watch-file-is-missing` until the repository is public (M5).
Anything else is fixed.
Lintian on the source package takes over 20 minutes with the vendor tree; it runs only before releases.

## 5. Build environment

- `packaging/containers/deb-trixie.Containerfile`: Debian 13 with trixie-backports enabled, the Build-Depends (Rust from backports), `build-essential`, `devscripts`, `lintian`, `fakeroot`.
- `packaging/containers/deb-testing.Containerfile`: Debian testing with the same packages.
- `scripts/build-deb [trixie|testing]`: takes the tarballs from `target/deb/` (running `scripts/make-orig` first if needed), unpacks the source package, runs `dpkg-buildpackage -us -uc` in the container with `--network=none`, runs lintian, and leaves the results in `target/deb/<dist>/`.

## 6. Install test

`scripts/test-deb [trixie|testing]` runs in a fresh container (root inside rootless podman):

1. `apt-get install` both `.deb` files from `target/deb/<dist>/`.
2. The files are where section 4 says, `koeinput --version` and `koeinput model list` work, and `man koeinput` exists. The installed binaries are stripped, so the GPL symbol check runs in the package build, before `dh_strip`.
3. Under `dbus-run-session` (no systemd, so the bus uses the `Exec=` line), `koeinput status` activates the daemon, which reports `ready: no` with the install hint (no model installed).
4. fcitx5 loads the addon headless: `fcitx5 --disable=all --enable=keyboard,dbus,koeinput` under `dbus-run-session` logs that `koeinput` was loaded, and exits on `fcitx5-remote -e` or after a timeout.
5. `apt-get remove` both packages: the installed files are gone; installing again works.

## 7. Host install (by the user)

Written in `docs/development.md` (and later the README):

1. Stop a daemon started by hand in the dev container, if one runs.
2. `sudo apt install ./target/deb/testing/koeinput_0.1.0-1_amd64.deb ./target/deb/testing/fcitx5-koeinput_0.1.0-1_amd64.deb`.
3. Install the model once, from the archive already downloaded in the spike: `koeinput model install parakeet-ja --from-file work/models/_dl/sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8.tar.bz2` (the VAD model is downloaded with it), or without `--from-file` to download.
4. Restart fcitx5 (System Settings → Keyboard → Virtual Keyboard → None, then Fcitx 5). The addon starts the daemon; `koeinput status` shows `ready: yes`.
5. Rollback: `sudo apt remove fcitx5-koeinput koeinput` and restart fcitx5. Models stay in `~/.local/share/koeinput/models/` until removed with `koeinput model remove` or by hand.
6. Logs: `journalctl --user -u koeinput`.

## Binary size

Both binaries embed ONNX Runtime (about 36 MB and 33 MB stripped; 66 MiB installed, 19 MB `.deb`), because `koeinput transcribe` runs the recognizer in-process.
**Decided** (controller ruling): accept this for the first release. Routing `transcribe` through the daemon would halve it; revisit if size matters.

## Testing

- `scripts/check-no-gpl` on dev builds and package builds.
- The daemon's new `Start` wait: D-Bus tests with `KOEINPUT_TEST_LOAD_DELAY_MS` (a `Start` during a 1 s load succeeds after the load; during a 6 s load it fails with `NoModel` after about 4 s, and a `Ready` property read during the wait answers at once).
- The addon's early start: `DaemonClient::activate()` sends `StartServiceByName`. `daemonclient_test` checks it on a private bus whose service directory holds an activation file for the test daemon (written by `scripts/addon test`, passed to `dbus-run-session` through `XDG_DATA_DIRS`): after `activate()` the daemon's name appears without any `Start`. The addon calls `activate()` once in its constructor (checked by review).
- `koeinput manpage` prints roff that starts with `.TH KOEINPUT 1`.
- `scripts/gen-copyright --check` in the package build.
- `scripts/build-deb trixie`, `scripts/build-deb testing`, `scripts/test-deb trixie`, `scripts/test-deb testing`.

## Follow-ups closed by M3

From `docs/followups.md`: the license check of the prebuilt sherpa-onnx archive. (Catalog validation and the daemon exit status were closed by the Rust hardening branch.)
The overall design's addon path (`koeinput.so`) is corrected to `libkoeinput.so`.
