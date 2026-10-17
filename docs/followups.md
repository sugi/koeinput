# Follow-ups from M1 reviews

Small findings from the M1 code reviews that were deliberately left for later.
Each item names the milestone it belongs to.

## M3 (Debian packaging)

- Decide the daemon's exit status when the session bus goes away (`crates/daemon/src/main.rs`). It exits with 0 today, so `Restart=on-failure` does not restart it.
- Check the license terms of the prebuilt sherpa-onnx archive before shipping it inside packages.

## M4 (quality)

- Recognition output:
  - Segment-wise decoding puts `。` at VAD segment boundaries in the middle of a sentence.
  - Silero's 20 s maximum speech length splits long monologues mid-word.
  - Fillers are also removed inside longer words; judge this with real transcripts.
- Config (`crates/core/src/config.rs`):
  - Write the config through a temporary file and rename.
  - Filter empty environment values in `Paths::from_vars`, not only in `from_env`.
  - Validate timing values (`partial_interval_ms = 0`, `max_length_ms = 0`).
- Recognizer and audio:
  - `SherpaOffline::decode` turns a missing result into empty text instead of an error.
  - Add tests for 24-bit and float WAV files.
- Session and engine tests: recognizer errors during finalize and on a partial tick, `max_length` reached during the tail, `SessionConfig::from_config`, VAD factory failure, and the 2 s no-audio timeout.
- CLI:
  - `koeinput transcribe` stops the whole batch at the first bad file.
  - `koeinput listen` early returns on errors skip the explicit cancel (the daemon still cancels when the client leaves).
- Daemon: if the `name_has_owner` check fails, the session is kept (the error is only logged).

## M2 (fcitx5 addon)

- `Final` is sent before the `done` state, but on separate signals. Base timing and commit logic on `Final` itself, not on the order in which a client happens to process the two.

## Done

### 2026-10-18

- Validate the model catalog when it is loaded: each `id` and `archive_root` must be a single plain path component, `files` values must be relative paths without `..`, `format` must be `file` or `tar.bz2`, and `sha256` must be 64 hex characters. Before this, an empty `id` made `koeinput model remove` delete the whole models directory. (M3)
- Model download (`crates/core/src/models.rs`) (M4):
  - Cap the download at the catalog size (the ureq reader is unlimited).
  - Set connect and response timeouts (not a total body timeout, which would break large models).
  - Replace an existing install atomically (move the old directory aside first).
  - Label local read errors as I/O errors, not download errors.
  - Add tests for cleanup after an extraction failure, directory entries, reinstall and the URL path.
- CLI: download progress and `model list` mix MB and MiB. (M4)
