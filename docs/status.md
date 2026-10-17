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
| [spike-results.md](spike-results.md) | Model comparison on the user's own recordings (M0) |
| [development.md](development.md) | How to build, test and run |
| [followups.md](followups.md) | Review findings deliberately left for M2–M4 |

## Where we are

| Milestone | State |
|---|---|
| M0. Model spike | Done. Default model: parakeet-ja (int8, sherpa-onnx). |
| M1. Daemon and CLI | Implemented and merged into `main` (merge commit `dc1a3bf`). One check left, see below. |
| M2. Minimal fcitx5 addon | Next. Design not started. |
| M3. Debian packaging | Not started. |
| M4. Quality | Not started. |
| M5. RPM, PKGBUILD, CI, release | Not started. |

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

## Next: M2 design

Start with the brainstorming process (questions, then a written spec, then a plan).
Three questions are open for the user:

1. **Hotkey.** A modifier-only key such as Right Alt, or 無変換 / 変換, which Mozc also uses.
   It will be changeable in the fcitx5 settings window either way.
2. **Mozc composing when the hotkey is pressed.** The user accepts either "do not start recording" or "leave the composition as it is".
   Recommendation: do not start.
3. **Partial results in M2.** The overall design puts the partial display in M4.
   The user wants to see text while speaking, so the recommendation is a simple version in M2 (partial text as preedit), with the display location refined in M4.

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

Later decisions that are still open: the project license (before the first release), and the repository host and CI (M5; the repository has no remote yet).

## Working environment

### Machines

- **Host:** Debian testing (kernel `7.2.9+deb14`), KDE Plasma on Wayland, fcitx5 with Mozc, PipeWire.
  CPU Intel Core i9-12900T (no NPU), iGPU UHD 770, 125 GiB RAM. Default microphone: DJI Wireless Mic Rx (USB).
- **Dev container** (where Claude Code runs): Debian 13, user `sugi` (uid 1000), no root, no -dev packages, no `/dev/dri`.
  - It shares only three sockets with the host: the session bus (`/run/user/1000/bus`), PulseAudio (`/run/user/1000/pulse/native`, `PULSE_SERVER` is set) and `wayland-0`. There is no PipeWire native socket, which is why capture uses the PulseAudio API.
  - It is started with `--rm`, so everything outside bind mounts disappears when it stops, including `/tmp`.
  - Bind-mounted from the host, so it survives (checked with `findmnt` on 2026-10-18):
    - the repository `/home/sugi/works/git/nemui/voice-flood`, including `work/` and `target/`;
    - the podman storage `~/.local/share/containers` (the build image and its layers);
    - `~/.cargo`, `~/.rustup` and `~/.claude`.
  - The container ID changes on every start. To get a shell from a host terminal, find it with `docker ps`, then run `docker exec -it -u sugi -w /home/sugi/works/git/nemui/voice-flood <container> bash`.

### Building

- Rootless podman works inside the dev container. `scripts/dev <command>` runs any command in the Debian 13 build image (`packaging/containers/dev-trixie.Containerfile`, Rust 1.95.0, libpulse-dev, dbus) with the repository at `/src` and `nice -n 10`.
- Binaries built there run directly in the dev container (same glibc).
- D-Bus tests: `scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks`.

### The gitignored `work/` directory

| Path | Content |
|---|---|
| `work/eval-data/` | The user's own recordings and references (`references.jsonl`, `audio/`, `rejected/`, `extra/`). Never commit; never quote transcripts in docs. |
| `work/models/` | Spike models (13 GB) and their original archives in `_dl/` |
| `work/models-test/` | Relative symlinks `parakeet-ja` and `silero-vad` for opt-in model tests (`KOEINPUT_MODEL_TESTS=1 KOEINPUT_TEST_MODELS_DIR=/src/work/models-test`) |
| `work/home/` | A `HOME` with parakeet-ja and silero-vad installed through `koeinput model install`; used to run the daemon by hand |
| `work/spike/`, `work/tools/`, `work/results/` | Throwaway spike harness (`run.py`, `score.py`, `transcribe.py`), the recorder and reference tools, benchmark results (`real1/summary.md`) |
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
- Implementer subagents must start every shell command with `cd /home/sugi/works/git/nemui/voice-flood && `, must not leave background processes, and must not run commands that wait on stdin.
- Never run the opt-in microphone test or `koeinput listen` against the real microphone on the user's behalf. Microphone checks belong to the user.
