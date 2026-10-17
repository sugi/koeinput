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

## M2 (fcitx5 addon)

- `Final` is sent before the `done` state, but on separate signals. Base timing and commit logic on `Final` itself, not on the order in which a client happens to process the two.

## Done

### 2026-10-18

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
