# Follow-ups from M1 reviews

Small findings from the M1 code reviews that were deliberately left for later.
Each item names the milestone it belongs to.

## M3 (Debian packaging)

- Check the license terms of the prebuilt sherpa-onnx archive before shipping it inside packages.

## M4 (quality)

- Recognition output:
  - Segment-wise decoding puts `。` at VAD segment boundaries in the middle of a sentence.
  - Silero's 20 s maximum speech length splits long monologues mid-word.
  - Fillers are also removed inside longer words; judge this with real transcripts.

- Model download: a download that stalls mid-body hangs `koeinput model install`. There is no idle timeout, and ureq 3 has no per-read timeout.
- Daemon: a `Start` that arrives at the exact moment the bus drops can store a session after the shutdown cancel; mark the shared state as shutting down under the same lock so such a `Start` is refused.
- Config: `koeinput model use` errors do not name the config file; `set_model` on a dangling symlink replaces the link with a regular file.
- CLI: `transcribe` decides whether to add the file name by a substring check, which can drop the name when a very short path appears inside an unrelated message.

## M2 (fcitx5 addon)

- `Final` is sent before the `done` state, but on separate signals. Base timing and commit logic on `Final` itself, not on the order in which a client happens to process the two.
- Without the frontend's repeat flag, the 1000 ms repeat window is refreshed by each matched repeat; the addon test covers the flagged path and a single unflagged repeat, not a long chain of unflagged repeats.
- Kept with a reason:
  - Rolling a bare key into a combination hotkey (press `space`, then `Control` while `space` is still down) is not recognized as the hotkey: rare, and it needs key-state tracking of every key.
  - An input method switch key during a session is handled by fcitx5 before the addon: the session keeps running, which is acceptable.
  - `Stop` and `Cancel` are sent without `NO_AUTO_START`: fcitx5's D-Bus API has no portable way to set it, and the window is milliseconds.
- Key releases pair with their presses by keycode. Every press the addon sees is recorded under its keycode, and the release of that keycode is swallowed or passed exactly as the press was, whatever its sym and modifiers (Control+space released as space, Alt_R released as Alt+Shift+Meta_R); a release without a recorded press is not ours. The controller hears a release only for the press it got. A new press of a recorded keycode is the key's auto-repeat (flagged by the frontend, or a plain key within one second of its last press), otherwise its release was lost and the press is a new key. With several hotkeys, a second hotkey pressed while the first is down is a chord (swallowed, does nothing) only with evidence that the first is held: a modifier hotkey (Alt_R; AltGr and Mode_switch as Mod5) must show in the states of every later press, and a plain-key hotkey (Control+space) counts as held for one second after its last press or repeat. Lost releases happen on some X11 paths. What remains:
  - After a lost release of a plain-key hotkey, the controller treats a hotkey press within one second of the lost key's last press or repeat as a repeat: it does nothing, and its release completes the old press. After that second the next hotkey press is an ordinary press, which stops a hold session still running from the lost release instead of starting a new one. Modifier hotkeys never auto-repeat, so a new press of one is always a new press.
  - Frontends that report keycode 0 (none on the host) fall back to the older sym rules: a release pairs when its sym is the stored press's sym, when it is the modifier release of a configured modifier hotkey, or when it matches a configured hotkey. Their gaps: a release under another sym (Alt_R released with Shift held arrives as Meta_R) is not paired, so a hold session runs until the next hotkey press; after a lost Alt_R release, Control+Alt_R within one second is swallowed as a repeat together with its release; and during a held Control+space session with Alt_R as a second hotkey, the release of a Control+Alt_R that went to the app can stop the session when Control went up first.
  - The held evidence assumes the frontend's modifier bits: `keySymToStates` maps Hyper_L to Mod4 in fcitx5 5.1 but Mod3 in 5.0, and Mode_switch under `grp:switch` may not set Mod5. A wrong assumption loses a chord (the second hotkey becomes an ordinary hotkey press); it never leaves a stuck key.

## Done

### 2026-10-18

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
