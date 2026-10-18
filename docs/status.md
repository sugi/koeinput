# Project status and handoff

Last updated: 2026-10-18.

This file is the starting point for a new working session.
It says where the project stands, what comes next, and how the working environment is set up.
Decisions themselves live in [decisions.md](decisions.md).

## Documents

| File | Content |
|---|---|
| [decisions.md](decisions.md) | Every decision so far, with labels Decided / Recommended / To verify / Open |
| [superpowers/specs/2026-10-17-overall-design.md](superpowers/specs/2026-10-17-overall-design.md) | Architecture, packaging plan, milestones M0–M5 |
| [superpowers/specs/2026-10-17-m1-daemon-cli-design.md](superpowers/specs/2026-10-17-m1-daemon-cli-design.md) | M1 spec (daemon and CLI) |
| [superpowers/plans/2026-10-17-m1-daemon-cli.md](superpowers/plans/2026-10-17-m1-daemon-cli.md) | M1 implementation plan (executed) |
| [superpowers/specs/2026-10-18-m2-addon-design.md](superpowers/specs/2026-10-18-m2-addon-design.md) | M2 spec (fcitx5 addon) |
| [superpowers/specs/2026-10-18-m3-debian-packaging-design.md](superpowers/specs/2026-10-18-m3-debian-packaging-design.md) | M3 spec (Debian packaging) |
| [superpowers/specs/2026-10-18-m4-quality-design.md](superpowers/specs/2026-10-18-m4-quality-design.md) | M4 spec (quality) |
| [superpowers/specs/2026-10-18-m5-release-design.md](superpowers/specs/2026-10-18-m5-release-design.md) | M5 spec (more formats, CI, release) |
| [release.md](release.md) | How to cut a release |
| [models.md](models.md) | Origin and license of every catalog model |
| [spike-results.md](spike-results.md) | Model comparison on the user's own recordings (M0) |
| [m4-results.md](m4-results.md) | Measurement of the M4 changes and of latency under CPU load |
| [development.md](development.md) | How to build, test, package and run, and how to install on the host |
| [followups.md](followups.md) | Review findings deliberately left for later milestones |

## Where we are

| Milestone | State |
|---|---|
| M0. Model spike | Done. Default model: parakeet-ja (int8, sherpa-onnx). |
| M1. Daemon and CLI | Implemented and merged into `main` (merge commit `dc1a3bf`). One check left, see below. |
| M2. Minimal fcitx5 addon | Merged into `main` (merge commit `fe20500`). Tests pass in the Debian 13 and Debian 12 images. The host check is pending; it uses the M3 packages. |
| M3. Debian packaging | Merged into `main` (merge commit `68abc8e`). The packages build, pass lintian and pass the install test for Debian 13 and Debian testing. The host install and the host check are pending for the user, see below. |
| M4. Quality | Merged into `main` (merge commit `4f55bc4`). The defaults are measured on the user's recordings. See below. |
| M5. RPM, PKGBUILD, CI, release | Merged into `main`. Packages for six targets build and pass the install test; CI runs on GitHub. No release has been made. See below. |

### Left over from M1

- **The microphone check has not been done.** It needs the user to speak:
  start the daemon in the dev container and run `koeinput listen` (commands in [development.md](development.md) and below).
  Check that the final text arrives within 2 s (p90) of pressing Enter to stop, and that silence gives no text.
- Verified so far without the microphone:
  - all tests pass (core 56, CLI 4, D-Bus 9);
  - clippy is clean and rustfmt has been applied;
  - with the daemon on the host session bus, `status` reports the model as ready;
  - `transcribe` on the user's recordings produced text, with the final result ready at the end of the audio;
  - `listen` works against the test-hooks fake daemon.

### Next for the user

1. Host install and host check: build `scripts/build-deb testing` and `scripts/test-deb testing` in the dev container, or download the `packages-testing` artifact of a `release.yml` run. Install on the host, install the model and restart fcitx5 as in [development.md](development.md#install-on-the-host). If the M3 packages are installed, use `apt install --reinstall` (same version). Then go through the [host check](development.md#host-check) in WezTerm, Chrome, Firefox, Emacs and Discord.
2. The microphone check left over from M1 (below).
3. Decide when to make the repository public and when to tag the first release ([release.md](release.md)).

## M5: More formats, CI and release

Spec: [M5 spec](superpowers/specs/2026-10-18-m5-release-design.md).

- Packages for Debian 13, Debian testing, Ubuntu 24.04, Ubuntu 26.04 (`.deb`), Fedora 44 (`.rpm`) and Arch Linux, all built offline from the same three orig tarballs. Each format runs `scripts/check-no-gpl` on the files that go into the package, ships `debian/copyright`, the license texts and `models.md`, and passes the same install test in a fresh container (`scripts/lib/install-checks.sh`). See [development.md](development.md#packages).
- `README.md` and `README.ja.md` for users; [models.md](models.md) with the CC-BY-4.0 attribution of the default model; [release.md](release.md) with the release steps.
- The private GitHub repository `sugi/koeinput`. `ci.yml` runs on every push and pull request; `release.yml` builds and tests all six targets on a `v*` tag or by hand, and makes a draft release only on a tag. No tag has been pushed.

## M4: Quality

Spec: [M4 spec](superpowers/specs/2026-10-18-m4-quality-design.md). Decisions are in [decisions.md](decisions.md#quality-m4), numbers in [m4-results.md](m4-results.md).

- The final text comes from one decode of the whole utterance (up to `final_full_max_ms`, 30 s). On the user's recordings this removed the `。` at pause points, kept the CER (27.8% against 27.7%) and kept the final p90 under 1 s, also under build load.
- An optional final-stage model: `koeinput model use cohere-transcribe --final`. It is more accurate (CER 23.2%) but slower (final p90 2.2 s, 3.9 s under load) and needs 4.4 GiB, so it stays off by default; the user decides.
- `koeinput dict add/remove/list`; the daemon reloads the dictionary at the next session.
- The addon's "Partial text" setting (in the app, in the panel, off) and a list of apps that always use the panel. Key releases pair with their presses by keycode.
- `threads` stays at 8: the fastest with and without load.
- Follow-ups closed or kept with reasons in [followups.md](followups.md). The host check still decides which apps need the panel list.
- `tools/measure-m4` repeats the measurement (see [m4-results.md](m4-results.md#how-to-run-it-again)).

## M3: Debian packaging

Spec: [M3 spec](superpowers/specs/2026-10-18-m3-debian-packaging-design.md). Decisions are in [decisions.md](decisions.md#debian-packages).

- Every build uses the upstream no-TTS sherpa-onnx archive, so no GPL code is in the binaries; the package build checks it.
- One source package builds `koeinput` and `fcitx5-koeinput` offline, for amd64. Scripts: `scripts/make-orig`, `scripts/build-deb`, `scripts/test-deb`, `scripts/gen-copyright` (see [development.md](development.md#debian-packages)).
- D-Bus starts the daemon on demand through the systemd user unit; the addon asks for it when fcitx5 loads the addon, and a `Start` waits up to 4 s for a loading model.
- `scripts/test-deb` installs both packages in fresh Debian 13 and Debian testing containers: files, `koeinput` command, man page, D-Bus activation, the addon in a headless fcitx5 (which starts the daemon), removal and a second install.

## M2: the minimal fcitx5 addon

The user answered the open questions on 2026-10-18 (recorded in decisions.md):
Right Alt as the default hotkey with a settings window, hold and tap both supported, no start while Mozc is composing, and partial results as preedit already in M2.
License `MIT OR Apache-2.0`; a private GitHub repository `sugi/koeinput` in M5.

The user asked for all milestones to be implemented in order without stopping for spec reviews or host checks.
Host checks are collected and handed to the user.

Facts already checked for M2 (2026-10-17):

- Debian 13 (the build container's base) ships everything the addon build needs (fcitx5 5.1.12):
  - `libfcitx5core-dev` and `fcitx5-modules-dev`, which includes `fcitx-module/dbus/dbus_public.h` and `notifications_public.h`;
  - the test modules `testfrontend`, `testim` and `testui` (in `fcitx5-modules`), which allow automated addon tests in a headless fcitx5;
  - `extra-cmake-modules` 6.13.0.
- Debian 12 has fcitx5 5.0.21, so a Debian 12 container can check that the addon compiles against the 5.0 API.
- During M2 the daemon can run by hand in the dev container. It shares the host session bus, so an addon on the host can reach it. Installing the daemon on the host is M3.
- The user must install the test `.deb` on the host (root needed) and restart fcitx5. Have a rollback ready: remove the package and restart fcitx5, because an addon crash stops all typing.
- Only the host can show whether key release events arrive through KWin, how each app renders preedit, and whether the addon loads in the host's newer fcitx5.
- Target apps for the final check: WezTerm, Chrome, Firefox, Emacs, Discord.

## Working environment

### Machines

- **Host:** Debian testing (kernel `7.2.9+deb14`), KDE Plasma on Wayland, fcitx5 with Mozc, PipeWire.
  CPU Intel Core i9-12900T (no NPU), iGPU UHD 770, 125 GiB RAM. Default microphone: DJI Wireless Mic Rx (USB).
- **Dev container** (where Claude Code runs): Debian 13, user `sugi` (uid 1000), no root, no -dev packages, no `/dev/dri`.
  - It shares only three sockets with the host: the session bus (`/run/user/1000/bus`), PulseAudio (`/run/user/1000/pulse/native`, `PULSE_SERVER` is set) and `wayland-0`. There is no PipeWire native socket, which is why capture uses the PulseAudio API.
  - It is started with `--rm`, so everything outside bind mounts disappears when it stops, including `/tmp`.
  - Bind-mounted from the host, so it survives (checked with `findmnt` on 2026-10-18):
    - the repository `/home/sugi/works/git/nemui/koeinput`, including `work/` and `target/`;
    - the podman storage `~/.local/share/containers` (the build image and its layers);
    - `~/.cargo`, `~/.rustup` and `~/.claude`.
  - The container ID changes on every start. To get a shell from a host terminal, find it with `docker ps`, then run `docker exec -it -u sugi -w /home/sugi/works/git/nemui/koeinput <container> bash`.

### Building

- Rootless podman works inside the dev container. `scripts/dev <command>` runs any command in the Debian 13 build image (`packaging/containers/dev-trixie.Containerfile`, Rust 1.95.0, libpulse-dev, dbus) with the repository at `/src` and `nice -n 10`.
- The Debian packages build in their own images (`packaging/containers/deb-trixie.Containerfile` and `deb-testing.Containerfile`) through `scripts/build-deb`, and `scripts/test-deb` tests them in fresh `debian:trixie` and `debian:testing` containers.
- `podman run --init` does not work here (no `catatonit`). `scripts/build-deb` uses tini from its image, and `scripts/test-deb` runs its own PID 1.
- Binaries built there run directly in the dev container (same glibc).
- D-Bus tests: `scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks`.

### The gitignored `work/` directory

| Path | Content |
|---|---|
| `work/eval-data/` | The user's own recordings and references (`references.jsonl`, `audio/`, `rejected/`, `extra/`). Never commit; never quote transcripts in docs. |
| `work/models/` | Spike models (13 GB) and their original archives in `_dl/` |
| `work/models-test/` | Relative symlinks `parakeet-ja` and `silero-vad` for opt-in model tests (`KOEINPUT_MODEL_TESTS=1 KOEINPUT_TEST_MODELS_DIR=/src/work/models-test`) |
| `work/home/` | A `HOME` with parakeet-ja, silero-vad and cohere-transcribe installed through `koeinput model install`; used to run the daemon by hand and by `tools/measure-m4` |
| `work/spike/`, `work/tools/`, `work/results/` | Throwaway spike harness (`run.py`, `score.py`, `transcribe.py`), the recorder and reference tools, benchmark results (`real1/summary.md`, and `m4/` from `tools/measure-m4`) |
| `work/cache/` | Cargo registry and git caches for the build container |

### Running the daemon and dictating by hand

In the dev container:

```
scripts/dev cargo build --release --workspace
HOME=$PWD/work/home KOEINPUT_DATA_DIR=$PWD/data target/release/koeinput-daemon 2>/tmp/koeinput-daemon.log &
target/release/koeinput status
target/release/koeinput listen    # Enter, speak after [recording], Enter; q to quit
kill %1
```

`koeinput listen` reads the keyboard, so run it from a real terminal (the `docker exec -it` shell above).

## How the work is done

- The user's global instructions are in `~/.claude/CLAUDE.md`. In short:
  - superpowers process: brainstorming, then spec, then plan, then subagent-driven implementation, with a fresh reviewer per task and a final review on the most capable model;
  - merge, not rebase;
  - `nice -n 10` for heavy jobs;
  - documents in English and plain language;
  - commit trailers name the model that actually wrote the commit.
- **Answer in Japanese.** The user often dictates by voice, so expect recognition errors in messages, for example "V インプット" for vinput, "5K" for "5系" (5.x) and "KI" for the short name of KoeInput.
- The user prefers moderate evaluation over strict measurement ("models can be changed later") and wants decisions written to files.
- Never copy code from fcitx5-vinput (GPL-3.0). Its design ideas are summarized in decisions.md.
- Implementer subagents must start every shell command with `cd /home/sugi/works/git/nemui/koeinput && `, must not leave background processes, and must not run commands that wait on stdin.
- Never run the opt-in microphone test or `koeinput listen` against the real microphone on the user's behalf. Microphone checks belong to the user.
