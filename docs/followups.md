# Follow-ups from reviews

Small findings from the code reviews that were deliberately left for later.
Each item names the milestone it belongs to.

## M4 (quality)

Kept with a reason:

- Recordings longer than `final_full_max_ms` (30 s) still join the segment texts. There, a pause can still put a `。` in the middle of a sentence, and Silero's 20 s maximum speech length can still split a word. Partial text always comes from the segments. A higher limit would push the final latency under load past the 2 s target ([m4-results.md](m4-results.md)).
- Decoding a whole utterance, parakeet-ja puts no `。` inside the text, even where a sentence really ends (none of the 3 sentence ends inside the read sentences). The final-stage model `cohere-transcribe` placed all 3. This is the model's punctuation, not a rule we can fix after recognition.
- Model download: a download that stalls mid-body hangs `koeinput model install`. There is no idle timeout, and ureq 3 has no per-read timeout. Ctrl-C stops it, and the install is atomic, so a stalled download leaves no half-installed model.
- Addon and daemon: a `Stop` or `Cancel` that the addon sends in the few milliseconds between the daemon losing its bus name and the addon seeing `NameOwnerChanged` can make the bus start a new daemon (D-Bus activation). Such calls should carry the `NO_AUTO_START` flag, but fcitx5's D-Bus API has no portable way to set it, and the window is milliseconds.
- Addon and daemon: the addon ends a session when `Final` arrives, but the daemon frees the session only when it sends the `done` state right after. A hotkey press in that gap (normally under a millisecond) gets `Busy`. The fix would be for the daemon to free the session before it sends `Final` or `Error`, which changes the order clients see.

## M5 (formats and release)

- The main orig tarball is `git archive` of HEAD, so it changes with every commit. That is fine until the first release. After it, every new build, even a change in `debian/` alone, needs a new upstream version, unless the tarball is made from the release tag.
- Some vendored crates contain tests or examples taken from other projects, which are only in the source package and not in the binaries: crossbeam-channel's tests from Go (BSD-3-Clause) and its example under CC-BY-3.0, and ring's test data from BoringSSL and other test suites. `debian/copyright` has paragraphs for the crossbeam-channel files and lists ring's holders, but the vendor tree was not checked file by file. An upload to Debian would need that check, or would leave such files out with `Files-Excluded`.
- CI runs on every branch push and on pull requests, so a pull request from a branch of this repository runs twice. Drop one trigger if the Actions minutes of the private repository run short.
- `debian/control` lists `cargo (>= 1.89) | cargo-1.91`. sbuild and Launchpad use only the first alternative, so a Ubuntu 24.04 PPA build would need an Ubuntu-specific `debian/control`.
- The release workflow pulls base images through `mirror.gcr.io`, which can lag behind floating tags such as `debian:testing` and `archlinux:latest`.

## M2 (fcitx5 addon)

- `Final` is sent before the `done` state, but on separate signals. Base timing and commit logic on `Final` itself, not on the order in which a client happens to process the two.
- Without the frontend's repeat flag, the 1000 ms repeat window is refreshed by each matched repeat; the addon test covers the flagged path and a single unflagged repeat, not a long chain of unflagged repeats.
- Kept with a reason:
  - Rolling a bare key into a combination hotkey (press `space`, then `Control` while `space` is still down) is not recognized as the hotkey: the `space` press went to the app, so its repeats (now `Control+space`) and its release go there too. Rare, and the alternative would take the key away from the app in the middle of a press.
  - An input method switch key during a session is handled by fcitx5 before the addon: the session keeps running, which is acceptable.
  - `Stop` and `Cancel` are sent without `NO_AUTO_START`: fcitx5's D-Bus API has no portable way to set it, and the window is milliseconds.
- Key releases pair with their presses by keycode. Every press the addon sees is recorded under its keycode with what it became (hotkey, chord, Escape, another key) and whether it was swallowed. The auto-repeats of that keycode (flagged by the frontend, or a plain key within one second of its last press or repeat) and its release are swallowed or passed exactly as the press was, whatever their sym and modifiers (Control+space released as space, Alt_R released as Alt+Shift+Meta_R, a held space repeating as Alt+space after Alt_R went down); they are never classified again. A release without a recorded press is not ours. The controller hears a release only for the press it got. A press of a recorded keycode that is not a repeat (a modifier key, or a plain key after the second) means the release was lost: it is a new key and replaces the record. With several hotkeys, a second hotkey pressed while the first is down is a chord (swallowed, does nothing) only with evidence that the first is held: a modifier hotkey (Alt_R; AltGr and Mode_switch as Mod5) must show in the states of every later press, and a plain-key hotkey (Control+space) counts as held for one second after its last press or repeat. Lost releases happen on some X11 paths. What remains:
  - Two one-second windows after a lost release. The controller's: when the next hotkey press is a plain-key hotkey (Control+space) and comes within one second of the lost key's last press or repeat, whichever hotkey that was (a modifier hotkey's lost release opens it too), the controller treats it as a repeat: it does nothing, and its release completes the old press. After that second the next hotkey press is an ordinary press, which stops a hold session still running from the lost release instead of starting a new one. A new press of a modifier hotkey is always a new press. The glue's: on the lost key's own keycode, any press within one second of the key's last press or repeat is taken for its repeat and goes the way of the lost press. This holds for every plain (non-modifier) key, not only a plain-key hotkey: a bare space within a second after a lost Control+space release is swallowed, and a letter pressed again within a second after its release was lost goes to the app as a repeat and does not cancel a running session the way another key would.
  - A key held down from before a session keeps auto-repeating into the app during the session, because its repeats go the way of its press (passed, without cancelling). With partial text in the app, those keys reach the app next to the preedit. A client that confirms its composition on a key it does not handle could then commit the partial text, and the final text would be committed again after it. Not seen on the host yet; the "Apps that show partial text in the panel" list avoids it for such an app.
  - Frontends that report keycode 0 (none on the host) fall back to the older sym rules: a release pairs when its sym is the stored press's sym, when it is the modifier release of a configured modifier hotkey, or when it matches a configured hotkey. Their gaps: a release under another sym (Alt_R released with Shift held arrives as Meta_R) is not paired, so a hold session runs until the next hotkey press; after a lost release of the default Alt_R, a Control+Alt_R press within one second is stored as the hotkey and reaches the controller as a modifier press, so it starts or stops a dictation and its release is swallowed, and a Control+Alt_R press after that second goes to the app but its release is swallowed when Control goes up first (Alt+Alt_R is the hotkey's modifier release and the controller still holds the lost press); and during a held Control+space session with Alt_R as a second hotkey, the release of a Control+Alt_R that went to the app can stop the session when Control went up first.
  - The held evidence assumes the frontend's modifier bits: `keySymToStates` maps Hyper_L to Mod4 in fcitx5 5.1 but Mod3 in 5.0, and Mode_switch under `grp:switch` may not set Mod5. A wrong assumption loses a chord (the second hotkey becomes an ordinary hotkey press); it never leaves a stuck key.

## Done

### 2026-10-18

- Recognition output (M4), checked on the user's recordings ([m4-results.md](m4-results.md)):
  - The final text comes from one decode of the whole utterance (up to 30 s), so a pause no longer puts a `。` at the VAD segment boundary: 1 such `。` in the 25 read sentences before, 0 after; in free speech the `。` inside the text went from 4 to 1, and that one is in the only recording over 30 s.
  - Silero's 20 s maximum speech length no longer splits the final text of a recording up to 30 s (41 of the 42 recordings).
  - The filler rule does not remove text inside words in the outputs: it matched twice, both えっと at the start of a phrase in free speech, and nothing in the read sentences.

- Daemon and CLI follow-ups (M4):
  - A `Start` racing with the bus drop is refused: shutdown marks the state as shutting down under the same lock as the cancel.
  - `koeinput model use` errors name the config file; `set_model` writes through a dangling symlink to the target path.
  - `transcribe` failure lines no longer use a substring check: audio errors always start with the path and other errors get it added.
  - Before-ready D-Bus tests use a load gate file instead of a timing window; `koeinput model use ID --final` refuses the main model and the daemon reuses the main recognizer when both ids are equal.
  - `koeinput status` says "not loaded yet or failed" for a final model that is not ready; `model list` warns on an invalid config instead of failing; `final_full_max_ms = 0` skips the final model with a warning.
  - The dictionary reload skips files over 1 MiB; the man page documents `dict` and the file format.

- The license terms of the prebuilt sherpa-onnx archive: the default archive links GPL code (espeak-ng, ucd-tools), so every build now uses the no-TTS archive, checked by `scripts/check-no-gpl`. The licenses of what remains are in `debian/copyright`. (M3)
- Addon (M4): the "Partial text" setting (in the app, in the panel, off) and the list of apps that always use the panel, for text-input-v3 clients that commit their composition on blur. The old `ShowPartial=False` reads as off.
- Addon hotkeys (M4): `ISO_Level3_Shift` and `Mode_switch` count as modifiers (no cancel, no repeat window); a second configured hotkey pressed while the first is down is a chord that does nothing; with keycodes, every release goes the way of its own press (see the M2 item above for what remains), so a key on the hotkey's keycode after a lost release (Shift+Meta_R, Control+Alt_R) passes together with its release.
- `DaemonProcess` test helper (M4): kills the child on a ready-timeout abort, uses only async-signal-safe calls between fork and exec, and removes its temporary directory when the daemon stops.

- Session and engine tests: recognizer errors during finalize and on a partial tick, `max_length` reached during the tail, `SessionConfig::from_config`, VAD factory failure, and the 2 s no-audio timeout.
- Validate the model catalog when it is loaded: each `id` and `archive_root` must be a single plain path component, `files` values must be relative paths without `..`, `format` must be `file` or `tar.bz2`, and `sha256` must be 64 hex characters. Before this, an empty `id` made `koeinput model remove` delete the whole models directory. (M3)
- Model download (`crates/core/src/models.rs`) (M4):
  - Cap the download at the catalog size (the ureq reader is unlimited).
  - Set connect and response timeouts (not a total body timeout, which would break large models).
  - Replace an existing install atomically (move the old directory aside first).
  - Label local read errors as I/O errors, not download errors.
  - Add tests for cleanup after an extraction failure, directory entries, reinstall and the URL path.
- CLI: download progress and `model list` mix MB and MiB. (M4)
- Config (`crates/core/src/config.rs`) (M4):
  - `set_model` writes the config through a temporary file and renames it.
  - `Paths::from_vars` treats empty environment values as unset.
  - `load` validates `threads` and the timing values and names the key and range in the error.
- `SherpaOffline::decode` returns an error when sherpa-onnx gives no result. Tests for 24-bit and float WAV files added. (M4)
- Daemon exits with a non-zero status when the session bus connection closes, so `Restart=on-failure` restarts it. (M3)
- Daemon `Start` cancels the new session when the `name_has_owner` check fails. (M4)
- CLI (M4):
  - `koeinput transcribe` continues after an unreadable or invalid file and exits non-zero if any file failed.
  - `koeinput listen` cancels the session on every error return.
