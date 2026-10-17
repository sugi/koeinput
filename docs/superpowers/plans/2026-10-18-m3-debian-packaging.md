# M3: Debian Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A source package `koeinput` that builds the binary packages `koeinput` and `fcitx5-koeinput` offline for Debian 13 and Debian testing, free of GPL code, installable and removable cleanly, with the host install steps written down.

**Architecture:** The sherpa-onnx no-TTS static archive is pinned and used by every build through `SHERPA_ONNX_LIB_DIR`. A maintainer script makes three orig tarballs (git archive, filtered vendor tree, the pinned archive). `debian/rules` drives the CMake addon with plain `dh` and runs cargo offline in overrides. Separate podman images build the packages per distribution; a test script installs them in fresh containers. The daemon waits briefly for the model on `Start`, and the addon starts the daemon early through D-Bus activation.

**Tech Stack:** debhelper compat 13, dpkg-source 3.0 (quilt) with component tarballs, cargo-vendor-filterer 0.5.18, clap_mangen, rustc ≥ 1.89 (trixie-backports / testing), CMake, lintian, podman.

**Spec:** `docs/superpowers/specs/2026-10-18-m3-debian-packaging-design.md` (read it first). Research notes: `work/m3-research/m3-research.md` (not in git; tested `debian/rules`, `control` and `.install` files are in `work/m3-research/skel/debian/`).

## Global Constraints

- Never copy code from fcitx5-vinput (GPL-3.0).
- No GPL code in any binary: every build uses `sherpa-onnx-v1.13.8-linux-x64-static-no-tts-lib.tar.bz2`, SHA-256 `36f2ebd0b9aa09248ff6461a30dcb7edfe7eecf2deb67dfb0ceb3aaf4906051f`, size 21,755,415 bytes, URL `https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-linux-x64-static-no-tts-lib.tar.bz2`, through `SHERPA_ONNX_LIB_DIR` plus empty stubs `libespeak-ng.a`, `libpiper_phonemize.a`, `libucd.a`.
- Builds and tests run in podman containers (`scripts/dev`, `scripts/addon`, `scripts/build-deb`, `scripts/test-deb`), never on the bare dev container, never against the user's session bus. Heavy commands under `nice -n 10`.
- Package version `0.1.0-1`; maintainer `Tatsuki Sugiura <sugi@nemui.org>`; `Architecture: amd64`; debhelper compat 13; source format `3.0 (quilt)`.
- Installed paths: `/usr/bin/koeinput`, `/usr/libexec/koeinput/koeinput-daemon`, `/usr/lib/systemd/user/koeinput.service`, `/usr/share/dbus-1/services/org.nemui.KoeInput.service`, `/usr/share/koeinput/models.toml`, `/usr/lib/x86_64-linux-gnu/fcitx5/libkoeinput.so`, `/usr/share/fcitx5/addon/koeinput.conf`, `/usr/share/locale/ja/LC_MESSAGES/fcitx5-koeinput.mo`, `/usr/share/man/man1/koeinput.1.gz`.
- License `MIT OR Apache-2.0` for our code.
- The daemon's `Start` waits at most 4 s for a loading model.
- Recognized text is never logged.
- Comments only for non-obvious context; no bare task numbers in comments.
- Commit messages in English, ending with `Co-Authored-By: <the model that wrote the commit> <noreply@anthropic.com>`.
- Every shell command starts with `cd <your worktree or checkout> && `. No background processes. No commands that wait on stdin. Never run `koeinput listen` or the microphone test.

## Review Focus

1. A stale or tampered sherpa-onnx download → `scripts/sherpa-onnx` refuses it (size or SHA-256 mismatch) and nothing builds against it. Test: Task 1 `scripts/sherpa-onnx --from-file <corrupted copy>` fails.
2. A build that silently falls back to the default (TTS) archive → `scripts/check-no-gpl` fails the package build. Test: Task 1 RED step on the old binaries, Task 4 package build.
3. The first dictation right after login while the model is loading → `Start` waits and succeeds instead of failing. Test: Task 2 `start_waits_for_a_loading_model`.
4. `apt remove` leaves nothing behind that breaks a reinstall. Test: Task 5 install test (install, remove, install again).
5. `debian/copyright` drifts from `Cargo.lock` after a dependency change → `scripts/gen-copyright --check` fails the package build. Test: Task 4.

---

### Task 1: Pin the no-TTS sherpa-onnx archive, check for GPL symbols, add license files

Runs in parallel with Tasks 2 and 3 (own worktree).

**Files:** create `packaging/sherpa-onnx.env`, `scripts/sherpa-onnx`, `scripts/check-no-gpl`, `LICENSE-MIT`, `LICENSE-APACHE`; modify `scripts/dev`, `Cargo.toml` (workspace `license`), `docs/development.md`, `.gitignore` if needed. Do not edit `crates/*/Cargo.toml` (Task 2 adds `license.workspace = true`).

**Interfaces produced:** `scripts/sherpa-onnx [--from-file PATH]` prints the absolute `lib/` path on stdout (outer-container path); `scripts/check-no-gpl BINARY...` exits 0 when clean, 1 with the offending symbol names otherwise; `packaging/sherpa-onnx.env` variables `SHERPA_ONNX_FILE`, `SHERPA_ONNX_URL`, `SHERPA_ONNX_SIZE`, `SHERPA_ONNX_SHA256`, `SHERPA_ONNX_STEM` (directory name inside the archive).

- [ ] **Step 1 (RED):** build the current release binaries (`scripts/dev cargo build --release --workspace`) and write `scripts/check-no-gpl` (POSIX sh, uses `nm -C --defined-only`, fails on any of `espeak_`, `ucd_`, `piper::`). Run it on `target/release/koeinput` and `target/release/koeinput-daemon`: it must FAIL and list espeak symbols. `nm` exists in the build image (binutils); run the check through `scripts/dev` so it works on the dev container too.
- [ ] **Step 2:** `packaging/sherpa-onnx.env` with the values from the Global Constraints (`SHERPA_ONNX_STEM=sherpa-onnx-v1.13.8-linux-x64-static-no-tts-lib`; confirm the stem with `tar -tjf`).
- [ ] **Step 3:** `scripts/sherpa-onnx`: sources the env file; cache dir `$root/work/cache/sherpa-onnx/`; if `$cache/$STEM/lib/.ready` exists, print the path and exit; otherwise download with `curl -fL --retry 3` to a temporary file (or copy `--from-file`), check size and `sha256sum`, refuse on mismatch (delete the temp file, exit 1 with a message naming expected and actual), extract into a temporary directory, add the three stub archives with `ar rcs`, move into place atomically, touch `.ready`. `ar` and `tar` run in the build container if missing outside (`scripts/dev` or a plain `podman run` of the dev image); keep it simple and robust.
- [ ] **Step 4:** `scripts/dev`: before `podman run`, resolve the lib dir with `scripts/sherpa-onnx` (quietly, once; skip when `KOEINPUT_DEV_IMAGE=bookworm`, which has no Rust) and pass `-e SHERPA_ONNX_LIB_DIR=/src/work/cache/sherpa-onnx/<stem>/lib`. The auto-clean added earlier stays.
- [ ] **Step 5 (GREEN):** `scripts/dev cargo clean --release` then `scripts/dev cargo build --release --workspace`; `scripts/check-no-gpl` passes on both binaries. Run `scripts/dev cargo test --workspace` and the opt-in model test (`KOEINPUT_MODEL_TESTS=1 KOEINPUT_TEST_MODELS_DIR=/src/work/models-test scripts/dev cargo test -p koeinput-core --release`, or the debug equivalent the M1 docs use) to show recognition still works.
- [ ] **Step 6:** Review Focus 1: copy the archive, flip one byte, run `scripts/sherpa-onnx --from-file` on the copy in a scratch cache (`KOEINPUT_SHERPA_CACHE` override or a temporary `work/cache` path): it must refuse.
- [ ] **Step 7:** `LICENSE-MIT` (MIT text with `Copyright (c) 2026 Tatsuki Sugiura`) and `LICENSE-APACHE` (the Apache-2.0 text; copy the canonical text from a vendored crate's `LICENSE-APACHE` in `work/cache/cargo-registry`, for example `anyhow-1.0.104/LICENSE-APACHE`, and check it is the full unmodified license). Workspace `Cargo.toml`: `license = "MIT OR Apache-2.0"` under `[workspace.package]`.
- [ ] **Step 8:** `docs/development.md`: a short "sherpa-onnx libraries" section (why no-TTS, where the cache is, how to update the pin).
- [ ] **Step 9:** Commit "Build against the no-TTS sherpa-onnx archive and check for GPL code".

### Task 2: Daemon waits for a loading model; man page; crate license fields

Runs in parallel with Tasks 1 and 3 (own worktree).

**Files:** `crates/daemon/src/main.rs`, `crates/daemon/src/service.rs`, `crates/daemon/tests/dbus.rs`, `crates/cli/src/main.rs`, `crates/cli/Cargo.toml`, `crates/cli/tests/cli.rs`, `crates/{core,dbus-api,daemon}/Cargo.toml` (license line only), `Cargo.lock`.

- [ ] **Step 1 (tests first):** in `crates/daemon/tests/dbus.rs`:
  - `start_waits_for_a_loading_model`: daemon with `KOEINPUT_TEST_LOAD_DELAY_MS=1000`; call `Start` at once (do not wait for Ready); it succeeds (returns an id) after about 1 s; cancel it.
  - `start_gives_up_after_four_seconds`: `KOEINPUT_TEST_LOAD_DELAY_MS=6000`; `Start` fails with `org.nemui.KoeInput.Error.NoModel` and "still loading" after 3.5–5 s.
  - `ready_answers_during_a_waiting_start`: `KOEINPUT_TEST_LOAD_DELAY_MS=3000`; spawn a `Start` call, then read `Ready` on another proxy within 500 ms: it answers `false` at once (the wait blocks nothing else).
  Run them; the first two fail today.
- [ ] **Step 2:** implement: a readiness notifier in `Shared` (for example a `tokio::sync::watch` channel or `Notify` updated by `load_model`); in `start`, while `Readiness::Loading`, release the lock and wait for the change with `tokio::time::timeout(4 s)`; then re-check. Keep the existing `Busy` and owner logic. Update the existing tests that relied on an immediate "still loading" if needed (the loops in `dbus.rs` that retry on "still loading" stay valid).
- [ ] **Step 3:** `koeinput manpage` — a hidden subcommand (`#[command(hide = true)]`) that prints the roff page from `clap_mangen::Man::new(Cli::command())`. Add `clap_mangen` (latest compatible with clap 4.6) to `crates/cli/Cargo.toml`, update `Cargo.lock` (`scripts/dev cargo build` with network). Test in `crates/cli/tests/cli.rs`: output starts with `.TH koeinput 1` (match what clap_mangen emits for the name, case as produced) and mentions `model`.
- [ ] **Step 4:** add `license.workspace = true` to every crate's `[package]`.
- [ ] **Step 5:** run `scripts/dev cargo test --workspace`, `scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks`, clippy, fmt. Commit "Let Start wait for a loading model and add a man page".

### Task 3: Addon starts the daemon early

Runs in parallel with Tasks 1 and 2 (own worktree).

**Files:** `addon/src/daemonclient.h/.cpp`, `addon/src/koeinput.cpp`, `addon/test/daemonclient_test.cpp`, `scripts/addon`.

- [ ] **Step 1:** `scripts/addon test` writes an activation file for the test daemon before running ctest: `target/addon/test-services/dbus-1/services/org.nemui.KoeInput.service` with
  ```ini
  [D-BUS Service]
  Name=org.nemui.KoeInput
  Exec=/usr/bin/env KOEINPUT_TEST_FAKE=1 HOME=/tmp/koeinput-activated XDG_CONFIG_HOME=/tmp/koeinput-activated/config XDG_DATA_HOME=/tmp/koeinput-activated/data /src/target/debug/koeinput-daemon
  ```
  and runs `XDG_DATA_DIRS=/src/target/addon/test-services:/usr/local/share:/usr/share dbus-run-session -- ctest ...` (trixie only; bookworm unchanged). Check that the session bus config honours `XDG_DATA_DIRS` for service directories (`<standard_session_servicedirs/>`); if it does not, use `dbus-run-session --config-file` with a small config that adds `<servicedir>`.
- [ ] **Step 2 (test first):** `daemonclient_test` gains a first step "activation": with no daemon running, `client.activate()`; within 3 s the service watcher reports an owner (record it through a test hook or a second watcher in the test); then stop that daemon (look up its pid with `org.freedesktop.DBus.GetConnectionUnixProcessID` and `kill`, wait for the name to go) before the existing steps start their own daemons. Without `activate()` implemented the step fails.
- [ ] **Step 3:** `DaemonClient::activate()`: `bus_.createMethodCall("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "StartServiceByName")` with `kBusName` and `uint32_t{0}`; `callAsync` with `kStartTimeoutUsec`, a reply callback that only logs failures through an optional callback or ignores them (keep the slot in a member `activateCall_`). Call it once at the end of `KoeInputAddon`'s constructor.
- [ ] **Step 4:** `scripts/addon test` and `KOEINPUT_DEV_IMAGE=bookworm scripts/addon test` green. Commit "Start the daemon when the addon loads".

### Task 4: Debian source package and build scripts

After Tasks 1–3 are merged into `m3-packaging`. Main checkout on that branch (or a worktree if Task 5 work overlaps; it does not).

**Files:** `debian/{control,rules,changelog,copyright,source/format,koeinput.install,fcitx5-koeinput.install,koeinput.manpages,koeinput.docs}`, `packaging/containers/deb-trixie.Containerfile`, `packaging/containers/deb-testing.Containerfile`, `packaging/containers/dev-trixie.Containerfile` (add `python3` and `cargo-vendor-filterer`), `packaging/licenses/*`, `scripts/make-orig`, `scripts/build-deb`, `scripts/gen-copyright`.

- [ ] **Step 1: images.** `dev-trixie` gets `python3` (apt) and `RUN cargo install --locked cargo-vendor-filterer --version 0.5.18`. `deb-trixie`: `debian:trixie`, add `deb http://deb.debian.org/debian trixie-backports main`, install `build-essential devscripts lintian fakeroot debhelper pkgconf libpulse-dev cmake extra-cmake-modules gettext libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev fcitx5-modules-dev binutils` and `cargo rustc` from `-t trixie-backports`. `deb-testing`: `debian:testing` with the same list from testing. Check `rustc --version` ≥ 1.89 in both.
- [ ] **Step 2: license texts.** `packaging/licenses/`: copy `work/m3-research/upstream/ORT-ThirdPartyNotices.txt` as `onnxruntime-1.28.2-ThirdPartyNotices.txt` and `hclust-LICENSE`; fetch at the pinned versions (see the research report section 2 table) the LICENSE/COPYING files of sherpa-onnx (Apache-2.0, `LICENSE` at v1.13.8), kaldi-native-fbank v1.22.3, kaldi-decoder v0.3.0, csukuangfj/openfst, simple-sentencepiece v0.7, kissfft, nlohmann/json 3.12.0, Eigen 5.0.1 (`COPYING.MPL2` and `COPYING.README`) with `curl` from `raw.githubusercontent.com` / the upstream tags, and a `README` listing each file with its source URL and commit. Commit them.
- [ ] **Step 3: `scripts/gen-copyright`** (Python 3, stdlib only, run in the dev image): reads `cargo metadata --format-version 1 --locked --filter-platform x86_64-unknown-linux-gnu`, takes every package that is not a workspace member and that is in the vendored set (use `cargo tree -e normal,build` reachability or simply all non-workspace packages from the filtered vendor tree), writes DEP-5: header (`Upstream-Name: koeinput`, `Source:` repository URL placeholder `https://github.com/sugi/koeinput`), `Files: *` (`Copyright: 2026 Tatsuki Sugiura`, `License: MIT or Apache-2.0`), `Files: vendor/<name>-<version>/*` per crate (`Copyright:` from `authors` or "<name> contributors", `License:` from the SPDX expression converted to DEP-5 syntax: `OR` → `or`, `AND` → `and`, `WITH` kept), `Files: sherpaonnx/*` with one paragraph listing every component and license (from the research table, no-TTS set) and the Eigen source URL `https://gitlab.com/libeigen/eigen/-/archive/5.0.1/eigen-5.0.1.tar.gz`, then a standalone `License:` paragraph per license short name (Apache-2.0, MPL-2.0, BSL-1.0 reference `/usr/share/common-licenses/<name>`; others get the full text from `packaging/licenses/` or from a vendored crate's license file). `--check` compares with the committed `debian/copyright` and exits 1 with a diff.
- [ ] **Step 4: `debian/`** from `work/m3-research/skel/debian/` adapted to the spec (section 4): `control` (with descriptions), `rules` (spec section 4; add `RUSTFLAGS` remap, parallel jobs, `scripts/check-no-gpl target/release/koeinput target/release/koeinput-daemon` after the cargo build, `target/release/koeinput manpage > debian/koeinput.1` in the build, run `addon` ctest `controller` only in `dh_auto_test`), the `.install` files, `koeinput.manpages` (`debian/koeinput.1`), `koeinput.docs` (`packaging/licenses/onnxruntime-1.28.2-ThirdPartyNotices.txt`), `changelog`, `source/format`.
- [ ] **Step 5: `scripts/make-orig`**: refuse a dirty tree; read the version from `debian/changelog` (upstream part); `git archive --format=tar --prefix=koeinput-$V/ HEAD | xz -T0 > target/deb/koeinput_$V.orig.tar.xz`; in the dev image: `cargo vendor-filterer --platform=x86_64-unknown-linux-gnu --locked --versioned-dirs` into a temp dir, tar with top directory `vendor/` to `koeinput_$V.orig-vendor.tar.xz` (use `--sort=name --mtime=@<commit time> --owner=0 --group=0 --numeric-owner` for reproducibility); copy the pinned archive (from `scripts/sherpa-onnx`'s cache, verify SHA-256 again) to `koeinput_$V.orig-sherpaonnx.tar.bz2`. Skip a tarball whose file already exists with the same content (orig tarballs must not change within a version: if it exists and differs, stop with an error).
- [ ] **Step 6: `scripts/build-deb [trixie|testing]`**: run `scripts/gen-copyright --check`; `scripts/make-orig` if tarballs are missing; in `deb-<dist>` with `--network=none` and the target dir mounted: `dpkg-source -x` of a source package made with `dpkg-source -b` (or unpack the orig tarballs plus the `debian/` dir), `dpkg-buildpackage -us -uc -b` (also `-S` once for the `.dsc`), `lintian -EvIL +pedantic --suppress-tags initial-upload-closes-no-bugs,no-homepage-field,debian-watch-file-is-missing,spelling-error-in-binary *.changes`; fail on any `E:` or `W:` tag; copy `.deb`, `.changes`, `.buildinfo`, lintian output to `target/deb/<dist>/`. Use `podman run --init` so time limits work.
- [ ] **Step 7:** run `scripts/build-deb trixie` and `scripts/build-deb testing`; fix until both pass. Record lintian output in the report. Commit "Add the Debian source package and build scripts".

### Task 5: Install test, host install steps and documents

After Task 4.

**Files:** `scripts/test-deb`, `docs/development.md`, `docs/status.md`, `docs/decisions.md`, `docs/superpowers/specs/2026-10-17-overall-design.md` (addon file name `libkoeinput.so`), `docs/followups.md`.

- [ ] **Step 1: `scripts/test-deb [trixie|testing]`** implementing spec section 6 in a fresh `debian:<dist>` container (`podman run --rm --init`): install with `apt-get install -y ./*.deb` (network allowed for dependencies), the file checks, `koeinput --version`, `koeinput model list`, `man -w koeinput` (install `man-db` in the test container), `dbus-run-session -- koeinput status` (expect `ready: no` and the install hint; allow up to 10 s for activation), headless fcitx5 addon load (`dbus-run-session -- sh -c 'fcitx5 --disable=all --enable=keyboard,dbus,koeinput --verbose="*=5" & sleep 5; fcitx5-remote -e || true'` and grep the log for koeinput being loaded; skip with a warning if fcitx5 cannot start headless), `apt-get remove -y koeinput fcitx5-koeinput` and check the files are gone.
- [ ] **Step 2:** run it for both distributions; both pass.
- [ ] **Step 3: documents.** `docs/development.md`: "Debian packages" section (make-orig, build-deb, test-deb, how to bump the version, how to update the sherpa-onnx pin and the license texts) and "Install on the host" (spec section 7, exact commands, rollback, logs). `docs/status.md`: M3 done, host install pending for the user. `docs/decisions.md`: no-TTS archive (license), amd64 only, binary size accepted, distribution rustc for packages, compat 13. Overall design: `libkoeinput.so`. `docs/followups.md`: close the sherpa-onnx license item.
- [ ] **Step 4:** commit "Test the packages in fresh containers and document the host install".
