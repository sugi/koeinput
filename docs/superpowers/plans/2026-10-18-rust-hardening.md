# Rust Hardening (M3/M4 Follow-ups) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the mechanical review findings from M1 that `docs/followups.md` assigns to M3 and M4, so the daemon and CLI are safe to package.

**Architecture:** Small, local changes in the existing Rust workspace (`crates/core`, `crates/daemon`, `crates/cli`), each with tests. No new crates, no interface change on D-Bus. Runs in parallel with the M2 addon work, in its own worktree.

**Tech Stack:** Rust 1.95 (edition 2024), `ureq` 3, `toml`/`toml_edit`, `hound`, `zbus` 6. Builds through `scripts/dev`.

**Spec:** `docs/followups.md` (the "M3" and "M4" lists) is the requirement list; `docs/superpowers/specs/2026-10-17-m1-daemon-cli-design.md` is the design these changes must keep. Items under "Recognition output" in followups.md are **out of scope** here (they need M4 design).

## Global Constraints

- Never copy code from fcitx5-vinput (GPL-3.0).
- Every build and test runs through `scripts/dev` (for example `scripts/dev cargo test --workspace`, `scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks`, `scripts/dev cargo clippy --workspace --all-targets -- -D warnings`). Run `scripts/dev cargo fmt --all` before committing.
- Recognized text is never logged unless `KOEINPUT_LOG_TEXT=1`.
- Size units shown to users are binary: `MiB` (1024 × 1024 bytes), one decimal place.
- Comments: only for non-obvious context. No bare task numbers in comments.
- Commit messages in English, ending with `Co-Authored-By: <the model that wrote the commit> <noreply@anthropic.com>`.
- Every shell command starts with `cd <worktree> && `. No background processes. No commands that wait on stdin. Never run the opt-in microphone test.
- When an item is done, mark it in `docs/followups.md` by moving it under a "Done" heading at the end of the file with the date (2026-10-18).

## Review Focus

1. A catalog entry with an empty `id` or an `id` of `..` → the catalog fails to load; `koeinput model remove` can never delete the models directory itself or anything outside it. Test: Task 1 `catalog_rejects_unsafe_entries`.
2. A server that sends more bytes than the catalog size → the download stops at the cap with a clear error and leaves nothing installed. Test: Task 1 `download_over_catalog_size_fails`.
3. A failed reinstall of an installed model → the previously installed model is still there and usable. Test: Task 1 `failed_reinstall_keeps_old_install`.
4. A config with `partial_interval_ms = 0` → the daemon refuses it with a message naming the key, instead of spinning. Test: Task 2 `rejects_invalid_timing_values`.
5. `koeinput transcribe good.wav missing.wav good2.wav` → both good files are transcribed, the missing one is reported, exit status is non-zero. Test: Task 4 `transcribe_continues_after_bad_file`.

---

### Task 1: Model catalog validation and safer downloads

**Files:** `crates/core/src/models.rs`, `crates/cli/src/model_cmd.rs`, `docs/followups.md`

Requirements:

1. `Catalog::parse` validates every entry and fails with `ModelError::Parse` naming the entry and the field:
   - `id` and `archive_root` (when present): non-empty, a single normal path component (no `/`, not `.` or `..`, no NUL);
   - every `files` value: a relative path made only of normal components (no `..`, not absolute, not empty);
   - `format`: `file` or `tar.bz2`;
   - `sha256`: exactly 64 hex digits;
   - ids are unique.
   The repository catalog `data/models.toml` must still parse (existing test).
2. Downloads (`InstallSource::Url`):
   - never write more than `entry.size` bytes: read at most `size + 1` bytes and fail with `ModelError::Download("larger than the catalog size …")` if the extra byte arrives;
   - set a connect timeout of 15 s and a timeout of 30 s for receiving the response headers through the `ureq` 3 agent configuration. No timeout on the whole body (large models take minutes).
3. Errors reading a local archive (`InstallSource::File`, or local write errors) are `ModelError::Io`, not `Download`. Only errors from the network reader are `Download`.
4. Reinstall is atomic: when the model directory already exists, rename it to `.<id>.old` in the models directory, rename the staging directory into place, then remove the old one. If the second rename fails, rename the old one back. A leftover `.<id>.old` from a crash is removed at the start of the next install.
5. `koeinput model list` and the download progress use `MiB` with one decimal place.

Tests (in `models.rs` unless noted), written before the code:

- `catalog_rejects_unsafe_entries`: one parse per bad value (empty id, `..`, `a/b`, absolute `files` value, `../x` in `files`, unknown format, 63-hex sha256, duplicate id).
- `download_over_catalog_size_fails`: a tiny HTTP server on `127.0.0.1:0` in a test thread (std `TcpListener`, answer one request with `HTTP/1.1 200 OK`, `Content-Length` and a body longer than `size`) and an entry whose `url` points at it; install fails with `Download`, nothing is left in the models directory except nothing (no `.download`, `.staging` or model dir).
- `installs_from_url`: the same server with the right body and checksum installs the model (covers the URL path).
- `local_read_error_is_io`: `InstallSource::File` on a directory or a missing path gives `ModelError::Io`.
- `failed_reinstall_keeps_old_install`: install a model, then reinstall from an archive with a bad checksum; the old files are still there and `is_installed` is true.
- `reinstall_replaces_files`: install, add an extra file to the model dir, reinstall; the extra file is gone.
- `cleanup_after_extraction_failure`: an archive that fails during extraction (unsafe entry) leaves no `.staging` or `.download`.
- `archive_with_directory_entries_installs`: a tar.bz2 with explicit directory entries installs.
- In `model_cmd.rs` (or wherever the formatting lives): a unit test for the `MiB` formatter (`1572864` → `1.5 MiB`).

Commit: `Validate the model catalog and harden model downloads`.

### Task 2: Config writes and validation, recognizer and WAV edge cases

**Files:** `crates/core/src/config.rs`, `crates/core/src/recognizer.rs`, `crates/core/src/audio/file.rs`, `docs/followups.md`

Requirements:

1. `set_model` writes the config through a temporary file in the same directory and `rename`s it into place (create the directory first). The file keeps the user's comments (existing `toml_edit` behavior).
2. `Paths::from_vars` treats an empty value the same as an unset variable for every variable it reads (`HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `KOEINPUT_DATA_DIR`). `from_env` then needs no filtering of its own.
3. `load` validates timing values and fails with `ConfigError` naming the key and the allowed range:
   - `partial_interval_ms`: 100 to 10000;
   - `tail_ms`: 0 to 2000;
   - `min_length_ms`: 0 to 10000, and not more than `max_length_ms`;
   - `max_length_ms`: 1000 to 600000;
   - `threads`: 1 to 256.
   Check the actual field names in `Config` and validate every timing or count field it has; defaults must pass.
4. `SherpaOffline::decode`: when sherpa-onnx returns no result, return an error (`RecognizerError` or the type the trait uses) instead of empty text.
5. `audio/file.rs`: 24-bit integer and 32-bit float WAV files read correctly (add support if missing).

Tests, written before the code:

- `set_model_writes_atomically`: after `set_model`, no temporary file is left in the directory and the content is right; `set_model` on a path whose directory does not exist creates it.
- `empty_env_values_are_ignored`: `from_vars` with `XDG_CONFIG_HOME = ""` falls back to `HOME/.config`, and the same for the others.
- `rejects_invalid_timing_values`: `partial_interval_ms = 0`, `max_length_ms = 0`, `min_length_ms > max_length_ms`, `threads = 0` each fail, and the message contains the key name.
- `reads_24bit_and_float_wav`: write a short sine with `hound` in both formats, read them back, compare with the 16-bit reading within a small tolerance.
- The decode-without-result case only if the sherpa-onnx wrapper allows a test without a model; otherwise cover it by code review and say so in the report.

Commit: `Validate config timings and write the config atomically`.

### Task 3: Missing session and engine tests

**Files:** `crates/core/src/session.rs`, `crates/core/src/engine.rs` (tests, and fixes for bugs the tests find), `docs/followups.md`

Add the tests listed under "Session and engine tests" in followups.md, with the existing fakes (`FakeRecognizer`, `FakeVad`, generator sources):

- `recognizer_error_during_finalize`: the final decode fails → `Error` with code `model`, then state `error`, no `Final`.
- `recognizer_error_on_partial_tick`: a partial decode fails → the session reports it the way the design says (an `Error` with code `model` and state `error`); if the code does something else, decide against the M1 spec and fix the code or the test, and say which in the report.
- `max_length_reached_during_tail`: `Stop` arrives, and `max_length` is hit while the tail is still recording → exactly one `Final`, then `done`.
- `session_config_from_config`: milliseconds convert to samples at 16 kHz for every field.
- `vad_factory_failure`: the VAD factory returns an error → `Error` with code `model` (or the code the engine uses), state `error`, no panic.
- `no_audio_for_two_seconds`: a source that stops sending without closing → `Error` with code `audio` after about 2 s.

Each test must finish in under 5 s. Fix any bug the tests reveal in the smallest way, and describe it in the report.

Commit: `Cover session and engine error paths with tests`.

### Task 4: Daemon exit status, owner check and CLI robustness

**Files:** `crates/daemon/src/main.rs`, `crates/daemon/src/service.rs`, `crates/cli/src/transcribe.rs`, `crates/cli/src/listen.rs`, `crates/cli/tests/cli.rs`, `crates/daemon/tests/dbus.rs`, `docs/followups.md`

Requirements:

1. The daemon exits with a non-zero status (an `Err` from `main`, with a message such as `session bus connection closed`) when the `NameOwnerChanged` stream ends, so `Restart=on-failure` restarts it.
2. In `Start`, if the `name_has_owner` check fails, cancel the new session and log a warning. The microphone must not stay open for a client that may be gone. The `Start` call still returns the id (the client sees `cancelled`).
3. `koeinput transcribe FILE...`: an unreadable or invalid file prints an error line naming the file and the batch goes on; the exit status is non-zero if any file failed.
4. `koeinput listen`: every early return after a session was started cancels the session explicitly before returning.

Tests:

- `transcribe_continues_after_bad_file` (CLI test): with the fake engine used by the existing CLI tests, or with a missing file between two generated WAVs; the output has results for both good files, an error for the bad one, and the exit status is non-zero. If the existing CLI tests cannot transcribe without a model, test the batch logic as a unit test in `transcribe.rs` and say so.
- `daemon_exits_nonzero_when_bus_goes_away` (D-Bus test, optional if not practical): if the test bus cannot be closed from the test, cover this by code review and say so in the report.
- Existing D-Bus tests stay green: `scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks`.

Commit: `Restart the daemon on bus loss and keep CLI batches going`.
