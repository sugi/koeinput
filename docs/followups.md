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

## M2 (fcitx5 addon)

- `Final` is sent before the `done` state, but on separate signals. Base timing and commit logic on `Final` itself, not on the order in which a client happens to process the two.
- Two configured hotkeys pressed together: the release of the second can pass through without its press, because the addon remembers one press at a time.
- `ISO_Level3_Shift` (AltGr) is not `Key::isModifier()`, so pressing it during a session counts as another key and cancels.
- Without the frontend's repeat flag, the 1000 ms repeat window is refreshed by each matched repeat; the addon test covers the flagged path and a single unflagged repeat, not a long chain of unflagged repeats.
- Rolling a bare key into a combination hotkey (press `space`, then `Control` while `space` is still down) is not recognized as the hotkey.
- Clients using text-input-v3 (Chrome, Discord) may commit their own composition on blur even though the client preedit is marked `DontCommit`. Input for the per-app display choice in M4.
- An input method switch key during a session is handled by fcitx5 before the addon: the session keeps running while the engine change may wipe the status.
- The `DaemonProcess` test helper: a ready-timeout abort leaves the daemon child running; the child uses stdio between fork and exec; the temporary directories are not removed.
- After a lost release of a modifier hotkey, the stored keycode is not cleared by a later press of another key on the same physical key (for example Shift+Meta_R), so that key's release is swallowed.
- Frontends that send keycode 0 cannot pair a release such as Alt+Shift+Meta_R with a stateless Alt_R press.

## Done

### 2026-10-18

- Daemon and CLI follow-ups (M4):
  - A `Start` racing with the bus drop is refused: shutdown marks the state as shutting down under the same lock as the cancel.
  - `koeinput model use` errors name the config file; `set_model` writes through a dangling symlink to the target path.
  - `transcribe` failure lines no longer use a substring check: audio errors always start with the path and other errors get it added.
  - Before-ready D-Bus tests use a load gate file instead of a timing window; `koeinput model use ID --final` refuses the main model and the daemon reuses the main recognizer when both ids are equal.
  - `koeinput status` says "not loaded yet or failed" for a final model that is not ready; `model list` warns on an invalid config instead of failing; `final_full_max_ms = 0` skips the final model with a warning.
  - The dictionary reload skips files over 1 MiB; the man page documents `dict` and the file format.

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
