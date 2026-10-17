# M2: minimal fcitx5 addon

Status: approved for implementation, 2026-10-18 (the user asked for no review stop).

Part of [the overall design](2026-10-17-overall-design.md). Background decisions are in [decisions.md](../../decisions.md).
The daemon and its D-Bus interface come from [M1](2026-10-17-m1-daemon-cli-design.md).

## Goal

Dictate into the focused text field of any app connected to fcitx5, while Mozc stays active.

Done when:

- The addon builds against fcitx5 5.1.12 (Debian 13) and compiles against 5.0.21 (Debian 12).
- Unit tests of the session logic pass, and a headless fcitx5 test drives the addon against the real daemon (built with `test-hooks`).
- A local `fcitx5-koeinput` `.deb` installs in a clean Debian 13 container.
- The user checks dictation on the host in WezTerm, Chrome, Firefox, Emacs and Discord (handed over as a checklist; the work does not wait for it).

Out of scope for M2:

- Debian source packaging, D-Bus activation and the daemon on the host as a service (M3).
- Choosing between preedit and panel per app, more display options, and output quality (M4).
- Undo, surrounding text and voice commands.

## User-visible behavior

### Hotkey

- Default: Right Alt (`Alt_R`). The user can change it, or add more keys, in the fcitx5 settings window.
- **Hold**: press starts recording; release stops it and the final text is committed.
- **Tap**: a press shorter than the tap threshold (default 300 ms) keeps recording after release. The next hotkey press stops it.
  The tap mode can be turned off in the settings. Then every release stops.
- Recording starts only if all of these hold. Otherwise the key passes through unchanged:
  - the input context has focus;
  - it is not a password field (`CapabilityFlag::Password`);
  - no composition is in progress: the preedit, the client preedit and the candidate list of the input panel are all empty.
    This is how "Mozc is composing" is detected; it works for any engine.

### While a session runs

| Event | Starting or recording | Finalizing (stop sent, waiting for the final text) |
|---|---|---|
| Esc | Cancel. Esc is swallowed. | Cancel. Esc is swallowed. |
| Another key press | Cancel. The key passes through. | The key passes through. The session goes on. |
| Focus moves out of the session's input context | Cancel. | Cancel. |
| The session's input context is destroyed | Cancel. | Cancel. |
| Hotkey press (tap mode) | Stop. | Ignored, swallowed. |

The release of a swallowed hotkey press is swallowed too, so apps never see half a key.

### Display

All display goes to the input context that started the session.

- Status, as the upper auxiliary text of the fcitx5 panel:
  - `準備中…` from the hotkey press until the daemon reports `recording`;
  - `録音中` while recording in hold mode, `録音中（もう一度押すと確定）` in tap mode;
  - `認識中…` after the stop.
  The "recording" status appears only when the daemon reports that audio arrives, so the user knows when to start speaking.
- Partial text: each `Partial` replaces the whole text.
  If the app supports preedit (`CapabilityFlag::Preedit`), it is shown as underlined client preedit.
  Otherwise it goes to the panel preedit.
  A setting turns partial text off.
- Final text: the display is cleared and the text is committed with `commitString`. An empty final text commits nothing.

### Errors

Errors end the session, clear the display and show a desktop notification through the fcitx5 `notifications` addon (an optional dependency; without it, the error is only logged).

| Situation | Message (Japanese UI text, English here) |
|---|---|
| The daemon is not on the bus (`ServiceUnknown`, `NameHasNoOwner`), or D-Bus activation failed (`org.freedesktop.DBus.Error.Spawn.*`) | The KoeInput daemon is not running. |
| `NoModel` | The daemon's message: still loading, or the install command. |
| `Busy` | Another dictation is running. |
| No reply in time (`Start` 5 s, `Stop` 3 s), or the session is still starting after 6 s (for example the call could not be sent) | The daemon does not respond. |
| No `Final` 10 s after `Stop` | The daemon does not respond. The session is cancelled. |
| `Error` signal (`audio`, `model`, `internal`) | Code-specific text plus the daemon's message. |
| The daemon leaves the bus during a session | The daemon stopped. |

Recognized text is never logged.

## Settings

Through fcitx5's own config system, so the options appear in the normal fcitx5 settings window (`fcitx5-configtool`, or the Input Method page in KDE System Settings).
Stored by fcitx5 in `~/.config/fcitx5/conf/koeinput.conf`.

| Option | Type | Default |
|---|---|---|
| Hotkey | key list, modifier-only and modifier-less keys allowed | `Alt_R` |
| Tap to keep recording | bool | true |
| Tap threshold (ms) | int, 100 to 1000 | 300 |
| Show partial results | bool | true |

Labels have Japanese translations (gettext domain `fcitx5-koeinput`).

## Structure

```
addon/
  CMakeLists.txt          project, C++20, -Wall -Wextra, CPack for the local .deb
  koeinput.conf.in        addon descriptor
  po/                     fcitx5-koeinput.pot, ja.po
  src/
    controller.h/.cpp     session logic, no fcitx5 types
    daemonclient.h/.cpp   asynchronous D-Bus client on fcitx5's D-Bus library
    config.h              FCITX_CONFIGURATION for the settings above
    koeinput.h/.cpp       AddonInstance: event watchers, display, timers, notifications
  test/
    controller_test.cpp   unit tests of the controller
    addon_test.cpp        headless fcitx5 with testfrontend and testim, against the real daemon
```

### Addon descriptor

`Category=Module`, `Type=SharedLibrary`, `Library=libkoeinput`, `OnDemand=False`, `Configurable=True`.
Dependencies: `core:5.0.0` and `dbus`. Optional dependency: `notifications`.

### controller

The session logic lives in one class that knows nothing about fcitx5 or D-Bus, so it can be unit-tested with fake time and a fake action sink.

Inputs (all with a monotonic time in milliseconds where it matters):

- `hotkeyPress(now, eligible, modifier)`, `hotkeyRelease(now)`, `escape()`, `otherKeyPress()`, `focusLost()`;
- `startReply(id)` or `startFailed(kind, message)`;
- `stateChanged(id, state)`, `partial(id, text)`, `finalText(id, text)`, `error(id, code, message)`;
- `stopFailed(id)`, `daemonLeft()`, `timerFired()`.

Key inputs return whether the key is swallowed.

Outputs, through an `Actions` interface implemented by the addon:
`callStart()`, `callStop(id)`, `callCancel(id)`, `showStatus(status)`, `showPartial(text)`, `clearDisplay()`, `commit(text)`, `notify(kind, detail)`, `startTimer(ms)`, `stopTimer()`.

Phases: `Idle`, `Starting` (Start sent, no reply yet), `Active` (session id known), `Finalizing` (Stop sent).

Rules that need care:

- **Signals before the Start reply.** The M1 daemon starts forwarding signals before it returns from `Start`, so `StateChanged` and even `Partial` can arrive before the session id.
  While `Starting`, the controller buffers signals (at most 64) and replays those with the right id when the reply arrives.
- **Stop or cancel before the reply.** A quick release, Esc or a focus change can come while `Starting`.
  The controller remembers it and sends `Stop` or `Cancel` as soon as the id is known.
  A pending cancel clears the display at once.
- **Auto-repeat and lost releases.** Some X11 input method paths do not forward key releases, so a press can arrive while the key still counts as down.
  - Modifier keys (such as the default `Alt_R`) never auto-repeat, so such a press always means the release was lost: it counts as a new press.
  - Other keys auto-repeat. The first repeat comes after the desktop's initial delay (500 to 660 ms by default on GNOME, KDE and X.org), then every few tens of milliseconds.
    A press within 1000 ms of the previous press event of that key is a repeat and is swallowed; after a longer gap the release was lost and the press counts as a new press.
- **The daemon ends the session by itself** (the 120 s limit, an audio error). The final text or the error is handled as usual, and the key release that follows is swallowed.
- **`Final` decides the commit.** The `done` state that follows on a separate signal only clears what is left.
  `cancelled` and `error` states end the session without a commit.
- After a cancel, signals for that session id are ignored.
- **Stray sessions.** The daemon sends session signals only to the client that started the session, so a `starting` or `recording` state, or a partial, for an id that is not the current session belongs to a session this addon lost track of (for example a `Start` reply that came after the 5 s timeout). The controller cancels such a session once, so the microphone does not stay open until the daemon's 120 s limit.

### daemonclient

A thin wrapper over the fcitx5 `dbus` module's bus (`IDBusModule::bus`):

- `start()`, `stop(id)`, `cancel(id)` with `Message::callAsync` and the timeouts above; results come back through one set of callbacks. Error replies are mapped to the kinds the controller knows (`NoDaemon`, `NoModel`, `Busy`, `Timeout`, `Other`) by their D-Bus error name. Cancel is fire-and-forget: its reply or error is ignored, because the user has already ended the session.
- Signals are accepted only from the daemon's current unique name, as reported by the service watcher.
- One match rule for the daemon's interface (`org.nemui.KoeInput`, `/org/nemui/KoeInput`, `org.nemui.KoeInput.Recognizer1`), dispatched by member name to typed callbacks.
  The daemon sends these signals only to the client that started the session (unicast).
  **To verify in the integration test**: that a match rule with the well-known service name matches signals sent from the daemon's unique name. If not, match on the unique name reported by the service watcher.
- A `ServiceWatcher` on `org.nemui.KoeInput`: an empty new owner means the daemon left.
- Every pending call and subscription is held as a slot object, so destroying the client cancels all callbacks.

No call is ever synchronous, and nothing polls.

### koeinput (the addon)

- A key event watcher at `EventWatcherPhase::PreInputMethod`. Hotkey press and release are matched with `Key::checkKeyList`, which handles both the press (`Alt_R`) and the release (`Alt+Alt_R`) of a modifier key.
  Swallowed keys are marked with `filterAndAccept()`.
- Watchers for focus out and input context destruction, filtered to the session's input context.
- The session's input context is held as a `TrackableObjectReference`, so a destroyed context is never touched.
- One timer (`EventLoop::addTimeEvent`) for the controller's watchdog.
- Status text and partial text as described under Display. Clearing resets only what the addon set (aux text, client preedit, panel preedit).
- Settings: `getConfig`, `setConfig` and `reloadConfig`, with `readAsIni` and `safeSaveAsIni` on `conf/koeinput.conf`. This small file read at load and on reload is the only file I/O.
- A log category `koeinput`.

Rules from the overall design apply: no blocking calls, no polling, no heavy work, no change to capability flags, commits only to the session's input context.

## Build and packaging for M2

- The Debian 13 build image gets CMake, extra-cmake-modules, gettext, dpkg-dev and the fcitx5 development and module packages (including the test modules `testfrontend`, `testim` and `testui`).
- A second image, `packaging/containers/dev-bookworm.Containerfile`, has the Debian 12 toolchain and fcitx5 5.0.21 development packages, without Rust.
  `scripts/dev` picks it with `KOEINPUT_DEV_IMAGE=bookworm`.
- `scripts/addon` wraps the common steps: `build`, `test` (unit tests, then the integration test against a `test-hooks` daemon) and `deb`.
  The build directory is `target/addon` (or `target/addon-bookworm`), already ignored by git.
- The local `.deb` comes from CPack: package `fcitx5-koeinput`, version `0.1.0~dev1`, library dependencies from `dpkg-shlibdeps`, plus `fcitx5`.
  M3 replaces it with a real Debian source package.

## Testing

- `controller_test`: plain C++ with a small assertion helper, a fake clock and a recording `Actions`. Covers hold, tap, the tap threshold, the tap mode turned off, repeats and lost releases, ineligible contexts, Esc and other keys in each phase, focus loss, early signals, stop and cancel before the reply, every start error kind, empty final text, error signals, the daemon leaving, the finalize watchdog, and a session that the daemon ends while the key is still down.
- `addon_test`: a real `fcitx::Instance` with `--disable=all --enable=testfrontend,testim,testui,dbus,koeinput`, under `dbus-run-session` in the build container.
  It starts `koeinput-daemon` (path from `KOEINPUT_TEST_DAEMON`; the test is skipped without it) with `KOEINPUT_TEST_FAKE=1`, a temporary `HOME`, and waits until the daemon's `Ready` property is true.
  The fake recognizer returns the number of whole seconds of audio, so a 1.5 s hold commits `1`.
  Commits are recorded with an `InputContextCommitString` watcher. Scenarios:
  - hold for about 1.5 s, release: commits `1`, and the hotkey press and release are both swallowed;
  - tap, wait about 1.5 s, press again: commits `1`;
  - a testim handler puts text into the client preedit for `a`; the hotkey then passes through and no session starts;
  - Esc while recording: no commit, and a following hold works again;
  - another key while recording: no commit, and that key is not swallowed;
  - the daemon is killed before a press: the press is swallowed and the next session after a daemon restart works.
- The bookworm image builds the addon and runs `controller_test`. The integration test is skipped there (no daemon built for Debian 12).
- Install check: the `.deb` installs into a clean `debian:trixie` container with `apt-get install ./…deb`; the files land in the fcitx5 addon and data directories and `ldd` resolves the library.
- Tests never touch the user's session bus: the build container does not mount it, and the tests run under `dbus-run-session`.

## Host check (by the user)

Written up in `docs/development.md` with exact commands:

1. Start the daemon (in the dev container, as in M1).
2. Install the `.deb` with `sudo apt install ./target/addon/fcitx5-koeinput_…deb` and restart fcitx5.
3. Rollback: `sudo apt remove fcitx5-koeinput` and restart fcitx5. Without fcitx5 the keyboard still types, so a broken addon never locks the user out of a terminal.
4. In WezTerm, Chrome, Firefox, Emacs and Discord: hold, tap, partial text, Esc, another key, a password field, and Mozc composing.
5. Note whether key releases arrive (hold mode works), and how each app shows the preedit. These notes feed M4.
