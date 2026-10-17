# M1: Daemon and CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dictation works end to end from a terminal: `koeinput listen` records from the microphone through the `koeinput-daemon` D-Bus service, shows partial results and prints the final text.

**Architecture:** A Rust workspace. `koeinput-core` holds all logic (config, clean-up, model catalog, audio, VAD, recognizer, a pure session state machine and an engine that runs one session on a worker thread). `koeinput-daemon` exposes the engine on the session bus with `zbus`. `koeinput` (CLI) talks to the daemon, transcribes files in-process and manages models. `koeinput-dbus-api` holds the D-Bus names and the client proxy shared by the CLI and the daemon tests.

**Tech Stack:** Rust 1.95.0 (edition 2024), `sherpa-onnx` 1.13.8 (static), `libpulse-simple-binding`, `zbus` 6 on `tokio`, `crossbeam-channel`, `hound`, `clap`, `toml`/`toml_edit`, `regex`, `sha2`, `tar`, `bzip2`, `ureq`. Builds run in a rootless podman container (Debian 13).

**Spec:** `docs/superpowers/specs/2026-10-17-m1-daemon-cli-design.md` (read it first). Background: `docs/decisions.md`, `docs/spike-results.md`.

## Global Constraints

- Never copy code from fcitx5-vinput (GPL-3.0). Do not read its source. Write everything from scratch.
- Every build and test runs through `scripts/dev` (Task 1), never on the bare container: it has no development packages and no root.
- All CPU-heavy commands run under `nice -n 10` (`scripts/dev` does this).
- Internal audio format everywhere: 16 kHz, mono, `f32` samples in [-1, 1]. Constant `koeinput_core::SAMPLE_RATE: u32 = 16000`.
- D-Bus: bus name `org.nemui.KoeInput`, object `/org/nemui/KoeInput`, interface `org.nemui.KoeInput.Recognizer1`, error prefix `org.nemui.KoeInput.Error`.
- Recognized text is never logged unless the environment variable `KOEINPUT_LOG_TEXT=1` is set.
- The user's own recordings (`work/eval-data/`) never go into git. Tests use generated audio, or WAVs from model archives at run time behind `KOEINPUT_MODEL_TESTS=1`.
- Paths: config `~/.config/koeinput/config.toml` (honour `XDG_CONFIG_HOME`), dictionary `~/.config/koeinput/dictionary.tsv`, models `~/.local/share/koeinput/models/<id>/` (honour `XDG_DATA_HOME`), catalog `/usr/share/koeinput/models.toml`, overridden by `KOEINPUT_DATA_DIR` (directory containing `models.toml`).
- Session defaults: partial interval 500 ms, tail 250 ms, minimum length 500 ms, maximum length 120 s, 8 threads.
- Default model id `parakeet-ja`. VAD model id `silero-vad`.
- Comments: only for non-obvious context. No bare task numbers in comments.
- Commit messages in English, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. Stop pressed before any audio arrives (device still waking up) → the session ends with an empty `Final` and `done`, and nothing hangs. Test: Task 7 `stop_before_audio_gives_empty_final`, Task 9 `stop_while_starting_finishes`.
2. Sound server unreachable or source name wrong → `Error` with code `audio` within a few seconds, and the engine/daemon stays usable. Test: Task 9 `unreachable_sound_server_reports_audio_error` (runs in the build container, which has no sound server).
3. Dictionary file with a UTF-8 BOM, CRLF line endings, comments and malformed lines → valid pairs load, malformed lines are reported as warnings, the daemon still starts. Test: Task 3 `dictionary_tolerates_bom_crlf_comments_and_bad_lines`.
4. Model download interrupted or checksum mismatch → no directory that later counts as "installed". Test: Task 4 `checksum_mismatch_leaves_nothing_installed`.
5. The client that started a session disappears (CLI killed) → the session is cancelled, the microphone closes, and the next `Start` works. Test: Task 11 `owner_disconnect_cancels_session`.

---

## File Structure

```
Cargo.toml                                  workspace
.gitignore                                  add /target/
scripts/dev                                 run a command in the build container
packaging/containers/dev-trixie.Containerfile
data/models.toml                            model catalog
data/koeinput.service                       systemd user unit (installed in M3)
data/org.nemui.KoeInput.service             D-Bus activation file (installed in M3)
crates/core/Cargo.toml
crates/core/src/lib.rs                      module list, SAMPLE_RATE
crates/core/src/config.rs                   Config, Paths, load, set_model
crates/core/src/postprocess.rs              PostProcessor, dictionary loading
crates/core/src/models.rs                   Catalog, ModelEntry, install/remove/list
crates/core/src/audio/mod.rs
crates/core/src/audio/file.rs               read_wav_16k_mono
crates/core/src/audio/capture.rs            PulseAudio capture thread
crates/core/src/recognizer.rs               Recognizer trait, SherpaOffline
crates/core/src/vad.rs                      Vad trait, Segment, SileroVad
crates/core/src/session.rs                  Session state machine (pure)
crates/core/src/engine.rs                   Engine, Source, SessionHandle
crates/core/src/testing.rs                  FakeRecognizer, FakeVad (feature test-hooks or cfg(test))
crates/dbus-api/Cargo.toml
crates/dbus-api/src/lib.rs                  names + Recognizer1Proxy
crates/daemon/Cargo.toml
crates/daemon/src/main.rs                   startup, model loading
crates/daemon/src/service.rs                Recognizer1 interface
crates/daemon/tests/dbus.rs                 D-Bus integration tests
crates/cli/Cargo.toml
crates/cli/src/main.rs                      clap commands
crates/cli/src/listen.rs                    listen command
crates/cli/src/model_cmd.rs                 model subcommands
crates/cli/src/transcribe.rs                transcribe command
```

---

### Task 1: Build container, workspace skeleton and native-library link check

**Files:**
- Create: `packaging/containers/dev-trixie.Containerfile`
- Create: `scripts/dev`
- Create: `Cargo.toml`
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`
- Create: `crates/dbus-api/Cargo.toml`, `crates/dbus-api/src/lib.rs`
- Create: `crates/daemon/Cargo.toml`, `crates/daemon/src/main.rs`
- Create: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`
- Modify: `.gitignore`

**Interfaces:**
- Produces: `scripts/dev <command...>` runs any command in the build container at `/src`; crate names `koeinput-core`, `koeinput-dbus-api`, `koeinput-daemon` (binary `koeinput-daemon`), `koeinput-cli` (binary `koeinput`); `koeinput_core::SAMPLE_RATE: u32`.

- [ ] **Step 1: Write the Containerfile**

`packaging/containers/dev-trixie.Containerfile`:

```dockerfile
FROM docker.io/library/debian:trixie

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git \
      libpulse-dev dbus \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
      --default-toolchain 1.95.0 --component clippy,rustfmt \
 && chmod -R a+rwX /usr/local/cargo /usr/local/rustup
```

- [ ] **Step 2: Write `scripts/dev`**

```sh
#!/bin/sh
# Runs a command in the Debian 13 build container with the repository at /src.
# The outer dev container has no root and no -dev packages, so all builds go through here.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
containerfile="$root/packaging/containers/dev-trixie.Containerfile"
tag=$(sha256sum "$containerfile" | cut -c1-12)
image="localhost/koeinput-dev:$tag"
if ! podman image exists "$image"; then
  nice -n 10 podman build -t "$image" -f "$containerfile" "$root/packaging/containers" >&2
fi
mkdir -p "$root/work/cache/cargo-registry" "$root/work/cache/cargo-git"
exec nice -n 10 podman run --rm -i --userns=keep-id \
  -v "$root:/src" -w /src \
  -v "$root/work/cache/cargo-registry:/usr/local/cargo/registry" \
  -v "$root/work/cache/cargo-git:/usr/local/cargo/git" \
  -e CARGO_TARGET_DIR=/src/target \
  -e KOEINPUT_MODEL_TESTS -e KOEINPUT_TEST_MODELS_DIR -e KOEINPUT_AUDIO_TESTS \
  "$image" "$@"
```

Run: `chmod +x scripts/dev`

- [ ] **Step 3: Write the workspace and crate manifests**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/core", "crates/dbus-api", "crates/daemon", "crates/cli"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.89"
publish = false
```

`crates/core/Cargo.toml`:

```toml
[package]
name = "koeinput-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[features]
test-hooks = []

[dependencies]
```

`crates/dbus-api/Cargo.toml`, `crates/daemon/Cargo.toml` and `crates/cli/Cargo.toml` follow the same pattern with names `koeinput-dbus-api`, `koeinput-daemon` and `koeinput-cli`. In `crates/cli/Cargo.toml` add:

```toml
[[bin]]
name = "koeinput"
path = "src/main.rs"
```

and in `crates/daemon/Cargo.toml`:

```toml
[[bin]]
name = "koeinput-daemon"
path = "src/main.rs"

[features]
test-hooks = ["koeinput-core/test-hooks"]
```

Add dependencies with cargo (inside the container so versions resolve):

```bash
scripts/dev cargo add -p koeinput-core sherpa-onnx@=1.13.8 libpulse-binding libpulse-simple-binding
scripts/dev cargo add -p koeinput-daemon koeinput-core --path crates/core
scripts/dev cargo add -p koeinput-cli koeinput-core --path crates/core
```

- [ ] **Step 4: Write the link-check test and stubs**

`crates/core/src/lib.rs`:

```rust
pub const SAMPLE_RATE: u32 = 16000;

#[cfg(test)]
mod link_check {
    #[test]
    fn sherpa_onnx_links() {
        let r = sherpa_onnx::LinearResampler::create(48000, 16000).expect("resampler");
        let out = r.resample(&vec![0.0f32; 4800], true);
        assert!((1500..=1700).contains(&out.len()), "got {}", out.len());
    }

    #[test]
    fn pulse_simple_links() {
        let spec = libpulse_binding::sample::Spec {
            format: libpulse_binding::sample::Format::F32le,
            channels: 1,
            rate: super::SAMPLE_RATE,
        };
        assert!(spec.is_valid());
        let _ = libpulse_simple_binding::Simple::new;
    }
}
```

`crates/dbus-api/src/lib.rs`: empty file for now. `crates/daemon/src/main.rs` and `crates/cli/src/main.rs`: `fn main() {}`.

`.gitignore` becomes:

```
/work/
/target/
```

- [ ] **Step 5: Build and run the tests**

Run: `scripts/dev cargo test --workspace`
Expected: the image builds on first use, sherpa-onnx downloads its prebuilt archive once, and both tests PASS. If `sherpa_onnx_links` fails to link, read `sherpa-onnx-sys` 1.13.8 `build.rs` (`SHERPA_ONNX_LIB_DIR`, `SHERPA_ONNX_ARCHIVE_DIR`) before changing anything.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore scripts/dev packaging crates
git commit -m "Add Rust workspace and podman build container" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Config and paths

**Files:**
- Create: `crates/core/src/config.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod config;`)

**Interfaces:**
- Produces:
  - `pub struct Config { pub model: Option<String>, pub source: Option<String>, pub threads: i32, pub partial_interval_ms: u64, pub tail_ms: u64, pub min_length_ms: u64, pub max_length_ms: u64, pub dictionary: Option<PathBuf> }` with `Default` (None, None, 8, 500, 250, 500, 120000, None), `Deserialize`, `#[serde(default, deny_unknown_fields)]`.
  - `pub struct Paths { pub config_file: PathBuf, pub dictionary_file: PathBuf, pub models_dir: PathBuf, pub catalog_file: PathBuf }` with `Paths::from_env() -> Paths` and `Paths::from_vars(get: impl Fn(&str) -> Option<String>) -> Paths`.
  - `pub fn load(path: &Path) -> Result<Config, ConfigError>`: missing file → `Config::default()`.
  - `pub fn set_model(path: &Path, model_id: &str) -> Result<(), ConfigError>`: keeps other keys and comments.
  - `pub enum ConfigError { Io(std::io::Error), Parse(String) }` implementing `std::error::Error` (use `thiserror`).

- [ ] **Step 1: Add dependencies**

```bash
scripts/dev cargo add -p koeinput-core serde --features derive
scripts/dev cargo add -p koeinput-core toml toml_edit thiserror
scripts/dev cargo add -p koeinput-core --dev tempfile
```

- [ ] **Step 2: Write the failing tests** (bottom of `config.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(&dir.path().join("none.toml")).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.threads, 8);
        assert_eq!(cfg.partial_interval_ms, 500);
        assert_eq!(cfg.tail_ms, 250);
        assert_eq!(cfg.min_length_ms, 500);
        assert_eq!(cfg.max_length_ms, 120_000);
    }

    #[test]
    fn values_override_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, "model = \"zipformer-ja\"\nthreads = 4\n").unwrap();
        let cfg = load(&p).unwrap();
        assert_eq!(cfg.model.as_deref(), Some("zipformer-ja"));
        assert_eq!(cfg.threads, 4);
        assert_eq!(cfg.tail_ms, 250);
    }

    #[test]
    fn unknown_key_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, "modle = \"x\"\n").unwrap();
        assert!(matches!(load(&p), Err(ConfigError::Parse(_))));
    }

    #[test]
    fn set_model_keeps_other_keys_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/c.toml");
        set_model(&p, "parakeet-ja").unwrap();
        assert_eq!(load(&p).unwrap().model.as_deref(), Some("parakeet-ja"));
        std::fs::write(&p, "# mine\nthreads = 4\nmodel = \"a\"\n").unwrap();
        set_model(&p, "b").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# mine"));
        assert!(text.contains("threads = 4"));
        assert_eq!(load(&p).unwrap().model.as_deref(), Some("b"));
    }

    #[test]
    fn paths_follow_xdg_and_data_dir_override() {
        let vars: HashMap<&str, &str> = [
            ("HOME", "/h"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("KOEINPUT_DATA_DIR", "/repo/data"),
        ]
        .into();
        let p = Paths::from_vars(|k| vars.get(k).map(|v| v.to_string()));
        assert_eq!(p.config_file, PathBuf::from("/cfg/koeinput/config.toml"));
        assert_eq!(p.dictionary_file, PathBuf::from("/cfg/koeinput/dictionary.tsv"));
        assert_eq!(p.models_dir, PathBuf::from("/h/.local/share/koeinput/models"));
        assert_eq!(p.catalog_file, PathBuf::from("/repo/data/models.toml"));
        let p = Paths::from_vars(|k| (k == "HOME").then(|| "/h".to_string()));
        assert_eq!(p.config_file, PathBuf::from("/h/.config/koeinput/config.toml"));
        assert_eq!(p.catalog_file, PathBuf::from("/usr/share/koeinput/models.toml"));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `scripts/dev cargo test -p koeinput-core config`
Expected: compile errors (items not defined).

- [ ] **Step 4: Implement**

```rust
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub model: Option<String>,
    pub source: Option<String>,
    pub threads: i32,
    pub partial_interval_ms: u64,
    pub tail_ms: u64,
    pub min_length_ms: u64,
    pub max_length_ms: u64,
    pub dictionary: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: None,
            source: None,
            threads: 8,
            partial_interval_ms: 500,
            tail_ms: 250,
            min_length_ms: 500,
            max_length_ms: 120_000,
            dictionary: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read or write config: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid config: {0}")]
    Parse(String),
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(|e| ConfigError::Parse(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn set_model(path: &Path, model_id: &str) -> Result<(), ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|e: toml_edit::TomlError| ConfigError::Parse(e.to_string()))?;
    doc["model"] = toml_edit::value(model_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, doc.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paths {
    pub config_file: PathBuf,
    pub dictionary_file: PathBuf,
    pub models_dir: PathBuf,
    pub catalog_file: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        Self::from_vars(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
    }

    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Self {
        let home = PathBuf::from(get("HOME").unwrap_or_else(|| "/".into()));
        let config_home = get("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(home.join(".config"));
        let data_home = get("XDG_DATA_HOME").map(PathBuf::from).unwrap_or(home.join(".local/share"));
        let catalog_dir = get("KOEINPUT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/share/koeinput"));
        let config_dir = config_home.join("koeinput");
        Self {
            config_file: config_dir.join("config.toml"),
            dictionary_file: config_dir.join("dictionary.tsv"),
            models_dir: data_home.join("koeinput/models"),
            catalog_file: catalog_dir.join("models.toml"),
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `scripts/dev cargo test -p koeinput-core config`
Expected: 5 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "Add config file and path handling" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Output clean-up and user dictionary

**Files:**
- Create: `crates/core/src/postprocess.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod postprocess;`)

**Interfaces:**
- Produces:
  - `pub struct PostProcessor` with `PostProcessor::new(dict: Vec<(String, String)>) -> Self`, `PostProcessor::empty() -> Self`, `fn apply(&self, text: &str) -> String`.
  - `pub struct DictionaryLoad { pub pairs: Vec<(String, String)>, pub warnings: Vec<String> }`.
  - `pub fn load_dictionary(path: &Path) -> std::io::Result<DictionaryLoad>`: missing file → empty pairs, no warnings.
  - `pub fn join_segments(parts: &[String]) -> String`: concatenates, inserting one space only when both sides of a boundary are ASCII alphanumeric.

Rules, applied in this order by `apply`:
1. Remove artefacts: the regex `(?:language\s+[A-Za-z]+\s*)?</?asr_text>` and SenseVoice-style tags `<\|[^|>]*\|>`.
2. Dictionary: one left-to-right pass; at each position the longest matching key wins; replaced text is not scanned again.
3. Fillers: remove `えーっと`, `えーと`, `えっと`, `えー`, `あのー`, `うーん`, `んー`, `そのー` (longest first) together with one directly following `、`, `,` or space. Never remove `あの` or `まあ`.
4. Whitespace: remove ASCII spaces and U+3000 when the characters on both sides are Japanese (Hiragana, Katakana incl. ー, CJK ideographs, or full-width punctuation U+3000–U+303F, U+FF00–U+FFEF). Trim both ends.

- [ ] **Step 1: Add dependency**

```bash
scripts/dev cargo add -p koeinput-core regex
```

- [ ] **Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn pp(pairs: &[(&str, &str)]) -> PostProcessor {
        PostProcessor::new(pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect())
    }

    #[test]
    fn strips_qwen_and_sensevoice_artefacts() {
        let p = PostProcessor::empty();
        assert_eq!(p.apply("language Japanese<asr_text>こんにちは"), "こんにちは");
        assert_eq!(p.apply("前の文。language Japanese<asr_text>次の文"), "前の文。次の文");
        assert_eq!(p.apply("<|ja|><|NEUTRAL|>テスト"), "テスト");
    }

    #[test]
    fn dictionary_longest_match_single_pass() {
        let p = pp(&[("ウィスパー", "Whisper"), ("ウィスパーCPP", "whisper.cpp"), ("Whisper", "X")]);
        assert_eq!(p.apply("ウィスパーCPPとウィスパー"), "whisper.cppとWhisper");
    }

    #[test]
    fn removes_unambiguous_fillers_only() {
        let p = PostProcessor::empty();
        assert_eq!(p.apply("えーと、明日はえー晴れ"), "明日は晴れ");
        assert_eq!(p.apply("うーん、そうですね"), "そうですね");
        assert_eq!(p.apply("あの人はまあまあです"), "あの人はまあまあです");
    }

    #[test]
    fn removes_spaces_between_japanese_only() {
        let p = PostProcessor::empty();
        assert_eq!(p.apply(" 測る の は 次 の 項目 です。 "), "測るのは次の項目です。");
        assert_eq!(p.apply("git worktree を 使う"), "git worktree を使う");
        assert_eq!(p.apply("これは　全角"), "これは全角");
    }

    #[test]
    fn order_dictionary_before_fillers() {
        let p = pp(&[("えーと", "ETO")]);
        assert_eq!(p.apply("えーと"), "ETO");
    }

    #[test]
    fn join_inserts_space_only_between_latin() {
        let parts = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_segments(&parts(&["Rust", "cargo"])), "Rust cargo");
        assert_eq!(join_segments(&parts(&["日本語", "テスト"])), "日本語テスト");
        assert_eq!(join_segments(&parts(&["Rust", "で書く"])), "Rustで書く");
        assert_eq!(join_segments(&parts(&["", "あ", ""])), "あ");
    }

    #[test]
    fn dictionary_tolerates_bom_crlf_comments_and_bad_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("d.tsv");
        std::fs::write(&p, "\u{feff}# comment\r\nラスト\tRust\r\n\r\nno tab here\r\n\tempty key\r\nギット\tgit\n").unwrap();
        let d = load_dictionary(&p).unwrap();
        assert_eq!(d.pairs, vec![("ラスト".into(), "Rust".into()), ("ギット".into(), "git".into())]);
        assert_eq!(d.warnings.len(), 2);
        assert!(d.warnings[0].contains("line 4"), "{:?}", d.warnings);
        assert!(load_dictionary(&dir.path().join("missing.tsv")).unwrap().pairs.is_empty());
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `scripts/dev cargo test -p koeinput-core postprocess`
Expected: compile errors.

- [ ] **Step 4: Implement**

```rust
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static ARTEFACT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:language\s+[A-Za-z]+\s*)?</?asr_text>|<\|[^|>]*\|>").unwrap());

const FILLERS: [&str; 8] = ["えーっと", "えーと", "えっと", "あのー", "そのー", "うーん", "えー", "んー"];

pub struct PostProcessor {
    dict: Vec<(String, String)>,
}

impl PostProcessor {
    pub fn new(mut dict: Vec<(String, String)>) -> Self {
        dict.retain(|(k, _)| !k.is_empty());
        dict.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Self { dict }
    }

    pub fn empty() -> Self {
        Self { dict: Vec::new() }
    }

    pub fn apply(&self, text: &str) -> String {
        let t = ARTEFACT.replace_all(text, "");
        let t = self.replace_dictionary(&t);
        let t = remove_fillers(&t);
        remove_japanese_spaces(&t)
    }

    fn replace_dictionary(&self, text: &str) -> String {
        if self.dict.is_empty() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        'outer: while !rest.is_empty() {
            for (k, v) in &self.dict {
                if rest.starts_with(k.as_str()) {
                    out.push_str(v);
                    rest = &rest[k.len()..];
                    continue 'outer;
                }
            }
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
        out
    }
}

fn remove_fillers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'outer: while !rest.is_empty() {
        for f in FILLERS {
            if let Some(after) = rest.strip_prefix(f) {
                rest = after
                    .strip_prefix('、')
                    .or_else(|| after.strip_prefix(','))
                    .or_else(|| after.strip_prefix(' '))
                    .unwrap_or(after);
                continue 'outer;
            }
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn is_japanese(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F | 0x3040..=0x309F | 0x30A0..=0x30FF | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

fn is_space(c: char) -> bool {
    c == ' ' || c == '\u{3000}'
}

// A space is dropped only when the nearest non-space characters on both sides are Japanese,
// so "git worktree を" keeps both of its spaces.
fn remove_japanese_spaces(text: &str) -> String {
    let chars: Vec<char> = text.trim().chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if is_space(c) {
            let prev = chars[..i].iter().rev().find(|c| !is_space(**c));
            let next = chars[i + 1..].iter().find(|c| !is_space(**c));
            if let (Some(p), Some(n)) = (prev, next) {
                if is_japanese(*p) && is_japanese(*n) {
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}
```

```rust
pub fn join_segments(parts: &[String]) -> String {
    let mut out = String::new();
    for p in parts.iter().filter(|p| !p.is_empty()) {
        if let (Some(l), Some(r)) = (out.chars().last(), p.chars().next()) {
            if l.is_ascii_alphanumeric() && r.is_ascii_alphanumeric() {
                out.push(' ');
            }
        }
        out.push_str(p);
    }
    out
}

pub struct DictionaryLoad {
    pub pairs: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

pub fn load_dictionary(path: &Path) -> std::io::Result<DictionaryLoad> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DictionaryLoad { pairs: Vec::new(), warnings: Vec::new() });
        }
        Err(e) => return Err(e),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut pairs = Vec::new();
    let mut warnings = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        match line.split_once('\t') {
            Some((k, v)) if !k.is_empty() => pairs.push((k.to_string(), v.to_string())),
            _ => warnings.push(format!("{}: line {}: expected \"wrong<TAB>right\"", path.display(), i + 1)),
        }
    }
    Ok(DictionaryLoad { pairs, warnings })
}
```

- [ ] **Step 5: Run to verify pass**

Run: `scripts/dev cargo test -p koeinput-core postprocess`
Expected: 7 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "Add output clean-up and user dictionary" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Model catalog and installation

**Files:**
- Create: `data/models.toml`
- Create: `crates/core/src/models.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod models;`)

**Interfaces:**
- Produces:
  - `pub struct Catalog { pub models: Vec<ModelEntry> }`, `Catalog::load(path: &Path) -> Result<Catalog, ModelError>`, `Catalog::parse(text: &str) -> Result<Catalog, ModelError>`, `fn get(&self, id: &str) -> Option<&ModelEntry>`, `fn default_model(&self) -> Option<&ModelEntry>`.
  - `pub struct ModelEntry { pub id: String, pub name: String, pub kind: String, pub url: String, pub sha256: String, pub size: u64, pub license: String, pub format: String /* "tar.bz2" | "file" */, pub archive_root: Option<String>, pub default: bool, pub files: BTreeMap<String, String> }` (serde, `default` field `#[serde(default)]`).
  - `fn dir(&self, models_dir: &Path) -> PathBuf` = `models_dir/<id>`; `fn file(&self, models_dir: &Path, key: &str) -> Result<PathBuf, ModelError>` (error if key unknown); `fn is_installed(&self, models_dir: &Path) -> bool` (dir exists and every file in `files` exists).
  - `pub enum InstallSource<'a> { Url, File(&'a Path) }`.
  - `pub fn install(entry: &ModelEntry, source: InstallSource, models_dir: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, ModelError>`.
  - `pub fn remove(entry: &ModelEntry, models_dir: &Path) -> Result<(), ModelError>`.
  - `pub enum ModelError { Io, Parse(String), Download(String), Checksum { expected: String, actual: String }, UnsafeArchive(String), MissingFile(String), UnknownModel(String) }` (thiserror).
  - Constant `pub const VAD_MODEL_ID: &str = "silero-vad";`, `pub const DEFAULT_MODEL_ID: &str = "parakeet-ja";`.

- [ ] **Step 1: Write `data/models.toml`**

```toml
# Model catalog. `files` maps roles to paths inside the installed model directory.
# Archives are extracted with `archive_root` stripped.

[[model]]
id = "parakeet-ja"
name = "NVIDIA Parakeet TDT-CTC 0.6B Japanese (int8)"
kind = "nemo_ctc"
default = true
url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8.tar.bz2"
sha256 = "4b0a800ef29f4f4c8667339bf6f60d5bfdc2852ddc9dc5741aea65b6f8d1306b"
size = 489389564
license = "CC-BY-4.0"
format = "tar.bz2"
archive_root = "sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8"
files = { model = "model.int8.onnx", tokens = "tokens.txt" }

[[model]]
id = "zipformer-ja"
name = "ReazonSpeech Zipformer (k2 v2, int8)"
kind = "transducer"
url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01.tar.bz2"
sha256 = "e0981d0d5d7b446d41010831b59091ebf57d2aa7b79980f67ca37af460b5842d"
size = 713097333
license = "Apache-2.0"
format = "tar.bz2"
archive_root = "sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01"
files = { encoder = "encoder-epoch-99-avg-1.int8.onnx", decoder = "decoder-epoch-99-avg-1.int8.onnx", joiner = "joiner-epoch-99-avg-1.int8.onnx", tokens = "tokens.txt" }

[[model]]
id = "sensevoice-2024"
name = "SenseVoice Small 2024-07-17 (int8)"
kind = "sense_voice"
url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17.tar.bz2"
sha256 = "7d1efa2138a65b0b488df37f8b89e3d91a60676e416f515b952358d83dfd347e"
size = 163002883
license = "FunASR Model License"
format = "tar.bz2"
archive_root = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17"
files = { model = "model.int8.onnx", tokens = "tokens.txt" }

[[model]]
id = "qwen3-asr-0.6b"
name = "Qwen3-ASR 0.6B (int8)"
kind = "qwen3"
url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2"
sha256 = "393f8a14e2f5fb96746aaab342997a40641001fbd5bf9592a080a8329178ee96"
size = 878702423
license = "Apache-2.0"
format = "tar.bz2"
archive_root = "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25"
files = { conv_frontend = "conv_frontend.onnx", encoder = "encoder.int8.onnx", decoder = "decoder.int8.onnx", tokenizer = "tokenizer" }

[[model]]
id = "cohere-transcribe"
name = "Cohere Transcribe 03-2026 (int8)"
kind = "cohere"
url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-cohere-transcribe-14-lang-int8-2026-04-01.tar.bz2"
sha256 = "bd582588d50685a795dcd2807ab77e11361b8312d96c53884682def45ab4206d"
size = 1699791751
license = "Apache-2.0"
format = "tar.bz2"
archive_root = "sherpa-onnx-cohere-transcribe-14-lang-int8-2026-04-01"
files = { encoder = "encoder.int8.onnx", decoder = "decoder.int8.onnx", tokens = "tokens.txt" }

[[model]]
id = "silero-vad"
name = "Silero VAD"
kind = "vad"
url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx"
sha256 = "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6"
size = 643854
license = "MIT"
format = "file"
files = { model = "silero_vad.onnx" }
```

- [ ] **Step 2: Add dependencies**

```bash
scripts/dev cargo add -p koeinput-core sha2 hex tar bzip2 ureq
```

- [ ] **Step 3: Write the failing tests**

Test archives are built in the test with `tar::Builder` and `bzip2::write::BzEncoder`. Use raw headers for hostile entries, because `tar::Builder` refuses `..` in normal use.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Write;

    fn bz_tar(entries: &[(&str, &[u8], tar::EntryType)]) -> Vec<u8> {
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        {
            let mut b = tar::Builder::new(&mut enc);
            for (path, data, kind) in entries {
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(*kind);
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                let name = path.as_bytes();
                h.as_old_mut().name[..name.len()].copy_from_slice(name);
                if *kind == tar::EntryType::Symlink {
                    h.set_link_name("/etc/passwd").unwrap();
                }
                h.set_cksum();
                b.append(&h, *data).unwrap();
            }
            b.finish().unwrap();
        }
        enc.finish().unwrap()
    }

    fn entry_for(bytes: &[u8], format: &str) -> ModelEntry {
        let mut files = BTreeMap::new();
        files.insert("model".to_string(), "m.onnx".to_string());
        ModelEntry {
            id: "t".into(), name: "t".into(), kind: "nemo_ctc".into(), url: "file:///none".into(),
            sha256: hex::encode(Sha256::digest(bytes)), size: bytes.len() as u64,
            license: "x".into(), format: format.into(), archive_root: Some("root".into()),
            default: false, files,
        }
    }

    fn write_tmp(dir: &Path, bytes: &[u8]) -> PathBuf {
        let p = dir.join("a.tar.bz2");
        std::fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn repository_catalog_parses_and_has_defaults() {
        let c = Catalog::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/models.toml"))).unwrap();
        assert_eq!(c.default_model().unwrap().id, DEFAULT_MODEL_ID);
        assert!(c.get(VAD_MODEL_ID).is_some());
        for m in &c.models {
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(!m.files.is_empty(), "{}", m.id);
        }
    }

    #[test]
    fn installs_archive_and_strips_root() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/m.onnx", b"onnx", tar::EntryType::Regular), ("root/extra/x.txt", b"x", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        let dir = install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(dir.join("m.onnx")).unwrap(), b"onnx");
        assert!(dir.join("extra/x.txt").exists());
        assert!(e.is_installed(&models));
    }

    #[test]
    fn checksum_mismatch_leaves_nothing_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/m.onnx", b"onnx", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let mut e = entry_for(&bytes, "tar.bz2");
        e.sha256 = "0".repeat(64);
        let models = tmp.path().join("models");
        assert!(matches!(install(&e, InstallSource::File(&src), &models, &mut |_, _| {}), Err(ModelError::Checksum { .. })));
        assert!(!e.is_installed(&models));
        assert!(!e.dir(&models).exists());
        let leftovers: Vec<_> = std::fs::read_dir(&models).map(|d| d.collect()).unwrap_or_default();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn rejects_parent_dir_absolute_and_symlink_entries() {
        for bad in [
            vec![("root/../evil", &b"x"[..], tar::EntryType::Regular)],
            vec![("/abs", &b"x"[..], tar::EntryType::Regular)],
            vec![("root/link", &b""[..], tar::EntryType::Symlink)],
            vec![("other/m.onnx", &b"x"[..], tar::EntryType::Regular)],
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let bytes = bz_tar(&bad);
            let src = write_tmp(tmp.path(), &bytes);
            let e = entry_for(&bytes, "tar.bz2");
            let models = tmp.path().join("models");
            let r = install(&e, InstallSource::File(&src), &models, &mut |_, _| {});
            assert!(matches!(r, Err(ModelError::UnsafeArchive(_))), "{bad:?}: {r:?}");
            assert!(!e.dir(&models).exists());
        }
    }

    #[test]
    fn missing_required_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/other.onnx", b"x", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        assert!(matches!(install(&e, InstallSource::File(&src), &models, &mut |_, _| {}), Err(ModelError::MissingFile(_))));
        assert!(!e.dir(&models).exists());
    }

    #[test]
    fn installs_single_file_and_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = b"vad-bytes";
        let src = tmp.path().join("silero_vad.onnx");
        std::fs::write(&src, bytes).unwrap();
        let mut e = entry_for(bytes, "file");
        e.files.insert("model".into(), "silero_vad.onnx".into());
        let models = tmp.path().join("models");
        install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        assert!(e.is_installed(&models));
        remove(&e, &models).unwrap();
        assert!(!e.dir(&models).exists());
    }
}
```

- [ ] **Step 4: Run to verify failure**

Run: `scripts/dev cargo test -p koeinput-core models`
Expected: compile errors.

- [ ] **Step 5: Implement**

Key points (write the full module):

```rust
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const VAD_MODEL_ID: &str = "silero-vad";
pub const DEFAULT_MODEL_ID: &str = "parakeet-ja";

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("invalid catalog: {0}")]
    Parse(String),
    #[error("download failed: {0}")]
    Download(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("unsafe archive entry: {0}")]
    UnsafeArchive(String),
    #[error("model file missing after install: {0}")]
    MissingFile(String),
    #[error("unknown model: {0}")]
    UnknownModel(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub license: String,
    pub format: String,
    pub archive_root: Option<String>,
    #[serde(default)]
    pub default: bool,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct Catalog {
    #[serde(rename = "model")]
    pub models: Vec<ModelEntry>,
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Self, ModelError> {
        toml::from_str(text).map_err(|e| ModelError::Parse(e.to_string()))
    }
    pub fn load(path: &Path) -> Result<Self, ModelError> {
        Self::parse(&std::fs::read_to_string(path)?)
    }
    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }
    pub fn default_model(&self) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.default)
    }
}

impl ModelEntry {
    pub fn dir(&self, models_dir: &Path) -> PathBuf {
        models_dir.join(&self.id)
    }
    pub fn file(&self, models_dir: &Path, key: &str) -> Result<PathBuf, ModelError> {
        self.files
            .get(key)
            .map(|f| self.dir(models_dir).join(f))
            .ok_or_else(|| ModelError::MissingFile(format!("{}: no file role {key}", self.id)))
    }
    pub fn is_installed(&self, models_dir: &Path) -> bool {
        let d = self.dir(models_dir);
        d.is_dir() && self.files.values().all(|f| d.join(f).exists())
    }
}

pub enum InstallSource<'a> {
    Url,
    File(&'a Path),
}
```

`install` algorithm:
1. `std::fs::create_dir_all(models_dir)`. Staging paths: `partial = models_dir/.<id>.download`, `staging = models_dir/.<id>.staging`. Remove leftovers of both first.
2. Copy the source into `partial` while hashing with `Sha256`, calling `progress(done, entry.size)` every 4 MiB. For `InstallSource::Url`, the reader is `ureq::get(&entry.url).call()` → `.into_body().into_reader()` (check the ureq 3 docs; map errors to `ModelError::Download`). For `File`, open the path.
3. Compare hex digest with `entry.sha256` (case-insensitive). On mismatch, delete `partial` and return `Checksum`.
4. Extract into `staging`:
   - `format == "file"`: `create_dir_all(staging)`, rename `partial` to `staging/<files.model>`.
   - `format == "tar.bz2"`: iterate `tar::Archive::new(BzDecoder::new(File::open(partial)?)).entries()?`. For each entry: reject anything that is not `Regular`, `Directory` or GNU long-name metadata (`EntryType::Regular | Directory` only after `entries()` resolves long names); take `entry.path()?`; reject if any component is `ParentDir`, `RootDir` or `Prefix`; require the first component to equal `archive_root` (reject otherwise); build `staging.join(rest)`; create parents; `entry.unpack(&target)`. Any rejection → `UnsafeArchive(path)`.
5. Check every `files` value exists under `staging`, else `MissingFile`.
6. Remove an existing `models_dir/<id>` and rename `staging` to it. On any error in steps 2–5, remove `partial` and `staging` before returning, so nothing half-installed remains.

`remove`: `std::fs::remove_dir_all(entry.dir(models_dir))`, treating `NotFound` as success.

- [ ] **Step 6: Run to verify pass**

Run: `scripts/dev cargo test -p koeinput-core models`
Expected: 6 tests PASS.

- [ ] **Step 7: Commit**

```bash
git add data/models.toml crates/core Cargo.lock
git commit -m "Add model catalog and safe model installation" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: WAV input and the recognizer

**Files:**
- Create: `crates/core/src/audio/mod.rs`, `crates/core/src/audio/file.rs`
- Create: `crates/core/src/recognizer.rs`
- Create: `crates/core/src/testing.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `ModelEntry`, `ModelEntry::file` (Task 4), `SAMPLE_RATE`.
- Produces:
  - `pub fn audio::file::read_wav_16k_mono(path: &Path) -> Result<Vec<f32>, AudioError>`; `pub enum audio::AudioError { Io(String), Format(String), Device(String) }` (thiserror).
  - `pub trait Recognizer: Send + Sync { fn decode(&self, samples: &[f32]) -> Result<String, RecognizerError>; }`
  - `pub struct SherpaOffline`, `SherpaOffline::new(entry: &ModelEntry, models_dir: &Path, threads: i32) -> Result<Self, RecognizerError>` implementing `Recognizer`.
  - `pub enum RecognizerError { UnsupportedKind(String), Model(String), Decode(String) }`.
  - `testing::FakeRecognizer::new(f: impl Fn(&[f32]) -> Result<String, RecognizerError> + Send + Sync + 'static)`; `testing::FakeRecognizer::calls(&self) -> usize`. Module compiled with `#[cfg(any(test, feature = "test-hooks"))]`.

Model kinds → sherpa-onnx config (`sherpa_onnx::OfflineRecognizerConfig`, verified fields in 1.13.8):

| kind | fields |
|---|---|
| `nemo_ctc` | `model_config.nemo_ctc.model = files.model`, `model_config.tokens = files.tokens` |
| `transducer` | `model_config.transducer.{encoder,decoder,joiner}`, `model_config.tokens` |
| `sense_voice` | `model_config.sense_voice.model`, `.language = Some("ja")`, `.use_itn = true`, `model_config.tokens` |
| `qwen3` | `model_config.qwen3_asr.{conv_frontend,encoder,decoder,tokenizer}` (keep its `Default` for lengths) |
| `cohere` | `model_config.cohere_transcribe.{encoder,decoder}`, `.language = Some("ja")`, `.use_punct = true`, `.use_itn = true`, `model_config.tokens` |

Always set `model_config.num_threads = threads`, `model_config.provider = Some("cpu".into())`, `decoding_method = Some("greedy_search".into())`. `OfflineModelConfig` derives `Default` with zeros, so `num_threads` must be set explicitly.

- [ ] **Step 1: Add dependency**

```bash
scripts/dev cargo add -p koeinput-core hound
```

- [ ] **Step 2: Write the failing tests**

In `audio/file.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, rate: u32, channels: u16, secs: f32) {
        let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let n = (rate as f32 * secs) as usize;
        for i in 0..n {
            let v = ((i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16;
            for _ in 0..channels {
                w.write_sample(v).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    #[test]
    fn passes_through_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        write_wav(&p, 16000, 1, 1.0);
        let s = read_wav_16k_mono(&p).unwrap();
        assert_eq!(s.len(), 16000);
        assert!(s.iter().all(|v| v.abs() <= 1.0));
    }

    #[test]
    fn downmixes_and_resamples_48k_stereo() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("b.wav");
        write_wav(&p, 48000, 2, 1.0);
        let s = read_wav_16k_mono(&p).unwrap();
        assert!((15800..=16200).contains(&s.len()), "{}", s.len());
    }

    #[test]
    fn missing_file_is_io_error() {
        assert!(matches!(read_wav_16k_mono(Path::new("/nonexistent.wav")), Err(AudioError::Io(_))));
    }
}
```

In `recognizer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Catalog;

    fn catalog() -> Catalog {
        Catalog::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/models.toml"))).unwrap()
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let mut e = catalog().get("parakeet-ja").unwrap().clone();
        e.kind = "whisper".into();
        assert!(matches!(SherpaOffline::new(&e, Path::new("/x"), 1), Err(RecognizerError::UnsupportedKind(_))));
    }

    #[test]
    fn missing_model_files_are_reported_before_loading() {
        let e = catalog().get("parakeet-ja").unwrap().clone();
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(SherpaOffline::new(&e, dir.path(), 1), Err(RecognizerError::Model(_))));
    }

    #[test]
    fn parakeet_transcribes_sample_when_enabled() {
        if std::env::var("KOEINPUT_MODEL_TESTS").as_deref() != Ok("1") {
            return;
        }
        let models = PathBuf::from(std::env::var("KOEINPUT_TEST_MODELS_DIR").expect("KOEINPUT_TEST_MODELS_DIR"));
        let e = catalog().get("parakeet-ja").unwrap().clone();
        let r = SherpaOffline::new(&e, &models, 8).unwrap();
        let wav = e.dir(&models).join("test_wavs/test_ja_1.wav");
        let samples = crate::audio::file::read_wav_16k_mono(&wav).unwrap();
        let text = r.decode(&samples).unwrap();
        assert!(text.chars().count() > 5, "{text}");
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `scripts/dev cargo test -p koeinput-core audio recognizer`
Expected: compile errors.

- [ ] **Step 4: Implement `audio/file.rs`**

```rust
use super::AudioError;
use crate::SAMPLE_RATE;
use std::path::Path;

pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>, AudioError> {
    let mut r = hound::WavReader::open(path).map_err(|e| match e {
        hound::Error::IoError(io) => AudioError::Io(format!("{}: {io}", path.display())),
        other => AudioError::Format(format!("{}: {other}", path.display())),
    })?;
    let spec = r.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>()
        }
    }
    .map_err(|e| AudioError::Format(e.to_string()))?;
    let ch = spec.channels.max(1) as usize;
    let mono: Vec<f32> = interleaved.chunks(ch).map(|c| c.iter().sum::<f32>() / ch as f32).collect();
    if spec.sample_rate == SAMPLE_RATE {
        return Ok(mono);
    }
    let rs = sherpa_onnx::LinearResampler::create(spec.sample_rate as i32, SAMPLE_RATE as i32)
        .ok_or_else(|| AudioError::Format(format!("cannot resample from {} Hz", spec.sample_rate)))?;
    Ok(rs.resample(&mono, true))
}
```

`audio/mod.rs`:

```rust
pub mod file;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("{0}")]
    Io(String),
    #[error("unsupported audio: {0}")]
    Format(String),
    #[error("audio device: {0}")]
    Device(String),
}
```

- [ ] **Step 5: Implement `recognizer.rs` and `testing.rs`**

```rust
use crate::models::ModelEntry;
use crate::SAMPLE_RATE;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum RecognizerError {
    #[error("unsupported model kind: {0}")]
    UnsupportedKind(String),
    #[error("model: {0}")]
    Model(String),
    #[error("decode: {0}")]
    Decode(String),
}

pub trait Recognizer: Send + Sync {
    fn decode(&self, samples: &[f32]) -> Result<String, RecognizerError>;
}

pub struct SherpaOffline {
    inner: sherpa_onnx::OfflineRecognizer,
}

impl SherpaOffline {
    pub fn new(entry: &ModelEntry, models_dir: &Path, threads: i32) -> Result<Self, RecognizerError> {
        let path = |key: &str| -> Result<Option<String>, RecognizerError> {
            let p = entry.file(models_dir, key).map_err(|e| RecognizerError::Model(e.to_string()))?;
            if !p.exists() {
                return Err(RecognizerError::Model(format!("{} is missing; run `koeinput model install {}`", p.display(), entry.id)));
            }
            Ok(Some(p.to_string_lossy().into_owned()))
        };
        let mut c = sherpa_onnx::OfflineRecognizerConfig::default();
        let m = &mut c.model_config;
        match entry.kind.as_str() {
            "nemo_ctc" => {
                m.nemo_ctc.model = path("model")?;
                m.tokens = path("tokens")?;
            }
            "transducer" => {
                m.transducer.encoder = path("encoder")?;
                m.transducer.decoder = path("decoder")?;
                m.transducer.joiner = path("joiner")?;
                m.tokens = path("tokens")?;
            }
            "sense_voice" => {
                m.sense_voice.model = path("model")?;
                m.sense_voice.language = Some("ja".into());
                m.sense_voice.use_itn = true;
                m.tokens = path("tokens")?;
            }
            "qwen3" => {
                m.qwen3_asr.conv_frontend = path("conv_frontend")?;
                m.qwen3_asr.encoder = path("encoder")?;
                m.qwen3_asr.decoder = path("decoder")?;
                m.qwen3_asr.tokenizer = path("tokenizer")?;
            }
            "cohere" => {
                m.cohere_transcribe.encoder = path("encoder")?;
                m.cohere_transcribe.decoder = path("decoder")?;
                m.cohere_transcribe.language = Some("ja".into());
                m.cohere_transcribe.use_punct = true;
                m.cohere_transcribe.use_itn = true;
                m.tokens = path("tokens")?;
            }
            other => return Err(RecognizerError::UnsupportedKind(other.to_string())),
        }
        m.num_threads = threads;
        m.provider = Some("cpu".into());
        c.decoding_method = Some("greedy_search".into());
        let inner = sherpa_onnx::OfflineRecognizer::create(&c)
            .ok_or_else(|| RecognizerError::Model(format!("sherpa-onnx could not load {}", entry.id)))?;
        Ok(Self { inner })
    }
}

impl Recognizer for SherpaOffline {
    fn decode(&self, samples: &[f32]) -> Result<String, RecognizerError> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let s = self.inner.create_stream();
        s.accept_waveform(SAMPLE_RATE as i32, samples);
        self.inner.decode(&s);
        Ok(s.get_result().map(|r| r.text.trim().to_string()).unwrap_or_default())
    }
}
```

`testing.rs` (FakeVad is added in Task 6):

```rust
use crate::recognizer::{Recognizer, RecognizerError};
use std::sync::atomic::{AtomicUsize, Ordering};

type DecodeFn = dyn Fn(&[f32]) -> Result<String, RecognizerError> + Send + Sync;

pub struct FakeRecognizer {
    f: Box<DecodeFn>,
    calls: AtomicUsize,
}

impl FakeRecognizer {
    pub fn new(f: impl Fn(&[f32]) -> Result<String, RecognizerError> + Send + Sync + 'static) -> Self {
        Self { f: Box::new(f), calls: AtomicUsize::new(0) }
    }
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Recognizer for FakeRecognizer {
    fn decode(&self, samples: &[f32]) -> Result<String, RecognizerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.f)(samples)
    }
}
```

`lib.rs` adds:

```rust
pub mod audio;
pub mod recognizer;
#[cfg(any(test, feature = "test-hooks"))]
pub mod testing;
```

- [ ] **Step 6: Run to verify pass**

Run: `scripts/dev cargo test -p koeinput-core audio recognizer`
Expected: 5 tests PASS (the model test returns early).
Then run the model test once with the spike's extracted model. In the outer container, create a directory that matches the catalog layout, using relative symlinks so they resolve inside the build container:
`mkdir -p work/models-test && ln -sfn ../models/sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8 work/models-test/parakeet-ja && ln -sfn ../models/silero-vad work/models-test/silero-vad`
Then run:
`KOEINPUT_MODEL_TESTS=1 KOEINPUT_TEST_MODELS_DIR=/src/work/models-test scripts/dev cargo test -p koeinput-core parakeet_transcribes`
Expected: PASS with a Japanese transcript longer than 5 characters.

- [ ] **Step 7: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "Add WAV input and sherpa-onnx recognizer" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Voice activity detection

**Files:**
- Create: `crates/core/src/vad.rs`
- Modify: `crates/core/src/testing.rs` (add `FakeVad`)
- Modify: `crates/core/src/lib.rs` (add `pub mod vad;`)

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct Segment { pub start: usize, pub end: usize }` (absolute sample indices since the session started, end exclusive).
  - `pub trait Vad: Send { fn accept(&mut self, samples: &[f32]); fn pop_finished(&mut self) -> Vec<Segment>; fn speech_start(&self) -> Option<usize>; fn finish(&mut self) -> Vec<Segment>; }` — `speech_start` is `Some(start)` while an unfinished segment is open; `finish` returns all remaining segments (including an open one, ended at the last real sample).
  - `pub struct SileroVad`, `SileroVad::new(model: &Path) -> Result<Self, VadError>`; `pub enum VadError { Model(String) }`.
  - `testing::FakeVad::new(segments: Vec<Segment>, min_silence: usize) -> Self` and `testing::FakeVad::always_speech() -> Self`.

Silero settings: threshold 0.5, min silence 0.4 s, min speech 0.25 s, window 512, max speech 20 s, sample rate 16000, 1 thread. The 1.13.8 Rust wrapper has no `flush`, so `finish` feeds 0.65 s of zeros (min silence + margin), drains segments and clamps each `end` to the real audio length. `speech_start` is estimated: when `detected()` turns true, the start is `fed - lookback` where lookback = min speech + 0.3 s, clamped to the end of the last finished segment.

- [ ] **Step 1: Write the failing tests**

In `testing.rs` tests module, and in `vad.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeVad;

    #[test]
    fn fake_vad_reports_open_and_finished_segments() {
        let mut v = FakeVad::new(vec![Segment { start: 100, end: 200 }, Segment { start: 400, end: 500 }], 50);
        v.accept(&[0.0; 150]);
        assert_eq!(v.speech_start(), Some(100));
        assert!(v.pop_finished().is_empty());
        v.accept(&[0.0; 100]);
        assert_eq!(v.pop_finished(), vec![Segment { start: 100, end: 200 }]);
        assert_eq!(v.speech_start(), None);
        v.accept(&[0.0; 200]);
        assert_eq!(v.speech_start(), Some(400));
        assert_eq!(v.finish(), vec![Segment { start: 400, end: 450 }]);
    }

    #[test]
    fn fake_vad_always_speech() {
        let mut v = FakeVad::always_speech();
        v.accept(&[0.0; 10]);
        assert_eq!(v.speech_start(), Some(0));
        assert_eq!(v.finish(), vec![Segment { start: 0, end: 10 }]);
    }

    fn sample_wav() -> Option<(Vec<f32>, std::path::PathBuf)> {
        if std::env::var("KOEINPUT_MODEL_TESTS").as_deref() != Ok("1") {
            return None;
        }
        let models = std::path::PathBuf::from(std::env::var("KOEINPUT_TEST_MODELS_DIR").unwrap());
        let wav = models.join("parakeet-ja/test_wavs/test_ja_1.wav");
        let speech = crate::audio::file::read_wav_16k_mono(&wav).unwrap();
        Some((speech, models.join("silero-vad/silero_vad.onnx")))
    }

    #[test]
    fn silero_finds_speech_between_silences_when_enabled() {
        let Some((speech, model)) = sample_wav() else { return };
        let mut audio = vec![0.0; 16000];
        audio.extend_from_slice(&speech);
        audio.extend(std::iter::repeat_n(0.0, 16000));
        let mut v = SileroVad::new(&model).unwrap();
        let mut segs = Vec::new();
        for chunk in audio.chunks(320) {
            v.accept(chunk);
            segs.extend(v.pop_finished());
        }
        segs.extend(v.finish());
        assert!(!segs.is_empty());
        assert!(segs.iter().all(|s| s.start < s.end && s.end <= audio.len()));
        assert!(segs[0].start >= 12000, "{segs:?}");
    }

    #[test]
    fn silero_finish_closes_segment_cut_mid_speech_when_enabled() {
        let Some((speech, model)) = sample_wav() else { return };
        let cut = &speech[..speech.len() / 2];
        let mut v = SileroVad::new(&model).unwrap();
        v.accept(cut);
        let mut segs = v.pop_finished();
        segs.extend(v.finish());
        assert!(segs.last().unwrap().end <= cut.len());
        assert!(segs.last().unwrap().end >= cut.len() - 16000);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `scripts/dev cargo test -p koeinput-core vad`
Expected: compile errors.

- [ ] **Step 3: Implement `FakeVad`** (in `testing.rs`)

```rust
use crate::vad::{Segment, Vad};

pub struct FakeVad {
    segments: Vec<Segment>,
    min_silence: usize,
    fed: usize,
    returned: usize,
    always: bool,
}

impl FakeVad {
    pub fn new(segments: Vec<Segment>, min_silence: usize) -> Self {
        Self { segments, min_silence, fed: 0, returned: 0, always: false }
    }
    pub fn always_speech() -> Self {
        Self { segments: Vec::new(), min_silence: 0, fed: 0, returned: 0, always: true }
    }
}

impl Vad for FakeVad {
    fn accept(&mut self, samples: &[f32]) {
        self.fed += samples.len();
    }
    fn pop_finished(&mut self) -> Vec<Segment> {
        let mut out = Vec::new();
        while let Some(s) = self.segments.get(self.returned) {
            if s.end + self.min_silence <= self.fed {
                out.push(*s);
                self.returned += 1;
            } else {
                break;
            }
        }
        out
    }
    fn speech_start(&self) -> Option<usize> {
        if self.always {
            return (self.fed > 0).then_some(0);
        }
        self.segments.get(self.returned).filter(|s| s.start < self.fed).map(|s| s.start)
    }
    fn finish(&mut self) -> Vec<Segment> {
        if self.always {
            return if self.fed > 0 { vec![Segment { start: 0, end: self.fed }] } else { Vec::new() };
        }
        let mut out = Vec::new();
        while let Some(s) = self.segments.get(self.returned) {
            if s.start >= self.fed {
                break;
            }
            out.push(Segment { start: s.start, end: s.end.min(self.fed) });
            self.returned += 1;
        }
        out
    }
}
```

- [ ] **Step 4: Implement `vad.rs`**

```rust
use crate::SAMPLE_RATE;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
}

pub trait Vad: Send {
    fn accept(&mut self, samples: &[f32]);
    fn pop_finished(&mut self) -> Vec<Segment>;
    fn speech_start(&self) -> Option<usize>;
    fn finish(&mut self) -> Vec<Segment>;
}

#[derive(Debug, thiserror::Error)]
pub enum VadError {
    #[error("VAD model: {0}")]
    Model(String),
}

const MIN_SILENCE_S: f32 = 0.4;
const MIN_SPEECH_S: f32 = 0.25;

pub struct SileroVad {
    vad: sherpa_onnx::VoiceActivityDetector,
    fed: usize,
    last_end: usize,
    open_start: Option<usize>,
}

impl SileroVad {
    pub fn new(model: &Path) -> Result<Self, VadError> {
        if !model.exists() {
            return Err(VadError::Model(format!("{} is missing; run `koeinput model install silero-vad`", model.display())));
        }
        let mut c = sherpa_onnx::VadModelConfig::default();
        c.silero_vad.model = Some(model.to_string_lossy().into_owned());
        c.silero_vad.threshold = 0.5;
        c.silero_vad.min_silence_duration = MIN_SILENCE_S;
        c.silero_vad.min_speech_duration = MIN_SPEECH_S;
        c.silero_vad.window_size = 512;
        c.silero_vad.max_speech_duration = 20.0;
        c.sample_rate = SAMPLE_RATE as i32;
        c.num_threads = 1;
        c.provider = Some("cpu".into());
        let vad = sherpa_onnx::VoiceActivityDetector::create(&c, 130.0)
            .ok_or_else(|| VadError::Model("sherpa-onnx could not load the VAD".into()))?;
        Ok(Self { vad, fed: 0, last_end: 0, open_start: None })
    }

    fn drain(&mut self, limit: usize) -> Vec<Segment> {
        let mut out = Vec::new();
        while let Some(seg) = self.vad.front() {
            let start = seg.start().max(0) as usize;
            let end = (start + seg.n().max(0) as usize).min(limit);
            if start < end {
                out.push(Segment { start, end });
                self.last_end = end;
            }
            self.vad.pop();
        }
        out
    }
}

impl Vad for SileroVad {
    fn accept(&mut self, samples: &[f32]) {
        self.vad.accept_waveform(samples);
        self.fed += samples.len();
        if self.vad.detected() {
            if self.open_start.is_none() {
                let lookback = ((MIN_SPEECH_S + 0.3) * SAMPLE_RATE as f32) as usize;
                self.open_start = Some(self.fed.saturating_sub(lookback).max(self.last_end));
            }
        } else {
            self.open_start = None;
        }
    }

    fn pop_finished(&mut self) -> Vec<Segment> {
        let out = self.drain(self.fed);
        if let (Some(open), Some(last)) = (self.open_start, out.last()) {
            if open < last.end {
                self.open_start = self.vad.detected().then_some(last.end);
            }
        }
        out
    }

    fn speech_start(&self) -> Option<usize> {
        self.open_start
    }

    fn finish(&mut self) -> Vec<Segment> {
        let real = self.fed;
        let pad = vec![0.0f32; ((MIN_SILENCE_S + 0.25) * SAMPLE_RATE as f32) as usize];
        self.vad.accept_waveform(&pad);
        let out = self.drain(real);
        self.open_start = None;
        out
    }
}
```

- [ ] **Step 5: Run to verify pass**

Run: `scripts/dev cargo test -p koeinput-core vad`
Expected: 4 tests PASS (Silero tests return early).
Then, using the `work/models-test/` directory from Task 5 (it already links `silero-vad`):
`KOEINPUT_MODEL_TESTS=1 KOEINPUT_TEST_MODELS_DIR=/src/work/models-test scripts/dev cargo test -p koeinput-core silero`
Expected: 2 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "Add Silero VAD wrapper and test fake" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Session state machine

**Files:**
- Create: `crates/core/src/session.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod session;`)

**Interfaces:**
- Consumes: `Recognizer` (Task 5), `Vad`, `Segment` (Task 6), `PostProcessor`, `join_segments` (Task 3), `Config` (Task 2).
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum State { Starting, Recording, Finalizing, Done, Cancelled, Error }` with `fn as_str(&self) -> &'static str` returning `"starting"`, `"recording"`, `"finalizing"`, `"done"`, `"cancelled"`, `"error"`.
  - `#[derive(Debug, Clone, PartialEq)] pub enum Event { State(State), Partial(String), Final(String), Error { code: String, message: String } }`.
  - `#[derive(Debug, Clone, Copy)] pub struct SessionConfig { pub partial_interval: usize, pub tail: usize, pub min_length: usize, pub max_length: usize }` in samples, with `SessionConfig::from_config(&Config) -> Self`.
  - `pub struct Session` with `Session::new(cfg: SessionConfig, rec: Arc<dyn Recognizer>, vad: Box<dyn Vad>, post: Arc<PostProcessor>) -> Self` and methods returning `Vec<Event>`: `push(&mut self, samples: &[f32])`, `stop(&mut self)`, `end_of_audio(&mut self)`, `cancel(&mut self)`; `abort(&mut self)` marks the session `Error` without events (the engine reports the cause itself); queries `fn wants_audio(&self) -> bool`, `fn is_finished(&self) -> bool`, `fn state(&self) -> State`.

Behaviour (time is measured in received samples, so file input and tests are deterministic):

- `Session::new` starts in `Starting` and emits nothing; the caller emits `State(Starting)`.
- First non-empty `push`: emit `State(Recording)`.
- Every `push`: append to the buffer, feed the VAD, decode each newly finished segment once (padded by 1600 samples each side, clamped) and store its text in order. Then, if `len >= next_tick`, decode the open segment (`speech_start()` minus 1600 padding, to the end) and set `next_tick = len + partial_interval`. Ticks never queue: one large push yields at most one tick. Emit `Partial(post.apply(join_segments(fixed + current)))` if it differs from the last partial and is not empty.
- `max_length`: when `len >= max_length`, behave as `stop` with zero tail and finalize immediately.
- `stop`: emit `State(Finalizing)`. If no audio was received yet, finalize at once. Otherwise set `stop_at = len + tail`; `wants_audio()` stays true until `len >= stop_at`, then finalize.
- `end_of_audio`: finalize (emit `State(Finalizing)` first if `stop` was not called).
- Finalize: decode the segments returned by `vad.finish()`, join all segment texts, and emit `Final(text)` then `State(Done)`. `Final("")` when total audio < `min_length` or there are no segments.
- `cancel`: emit `State(Cancelled)`; later calls return no events.
- A recognizer error emits `Error { code: "model", message }` then `State(Error)` and finishes the session.
- After the session finishes, every method returns an empty `Vec`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRecognizer, FakeVad};
    use crate::vad::Segment;

    const S: usize = 16000;

    fn cfg() -> SessionConfig {
        SessionConfig { partial_interval: S / 2, tail: S / 4, min_length: S / 2, max_length: 10 * S }
    }

    // Each test segment is filled with a distinct level; the fake maps the mean level to a word.
    fn rec() -> Arc<FakeRecognizer> {
        Arc::new(FakeRecognizer::new(|s| {
            let mean = s.iter().sum::<f32>() / s.len().max(1) as f32;
            Ok(if mean > 0.15 { "二".into() } else if mean > 0.05 { "一".into() } else { String::new() })
        }))
    }

    fn audio(parts: &[(usize, f32)]) -> Vec<f32> {
        parts.iter().flat_map(|(n, v)| std::iter::repeat_n(*v, *n)).collect()
    }

    fn session(r: Arc<FakeRecognizer>, segs: Vec<Segment>) -> Session {
        Session::new(cfg(), r, Box::new(FakeVad::new(segs, S / 4)), Arc::new(PostProcessor::empty()))
    }

    fn finals(ev: &[Event]) -> Vec<String> {
        ev.iter().filter_map(|e| if let Event::Final(t) = e { Some(t.clone()) } else { None }).collect()
    }

    #[test]
    fn first_audio_emits_recording_once() {
        let mut s = session(rec(), vec![]);
        assert_eq!(s.push(&[0.0; 320]), vec![Event::State(State::Recording)]);
        assert!(!s.push(&[0.0; 320]).contains(&Event::State(State::Recording)));
    }

    #[test]
    fn partials_follow_audio_time_and_never_queue() {
        let r = rec();
        let mut s = session(r.clone(), vec![Segment { start: 0, end: 4 * S }]);
        let mut ev = Vec::new();
        for chunk in audio(&[(4 * S, 0.1)]).chunks(320).take(S / 320) {
            ev.extend(s.push(chunk));
        }
        let before = r.calls();
        assert!(before <= 3, "{before} decodes in 1 s of audio");
        s.push(&audio(&[(2 * S, 0.1)]));
        assert_eq!(r.calls(), before + 1);
        assert!(ev.contains(&Event::Partial("一".into())));
    }

    #[test]
    fn finished_segments_are_fixed_and_ordered() {
        let mut s = session(rec(), vec![Segment { start: 0, end: S }, Segment { start: 2 * S, end: 3 * S }]);
        let mut ev = s.push(&audio(&[(S, 0.1), (S, 0.0), (S, 0.2), (S, 0.0)]));
        ev.extend(s.end_of_audio());
        assert_eq!(finals(&ev), vec!["一二".to_string()]);
        assert_eq!(ev.last(), Some(&Event::State(State::Done)));
    }

    #[test]
    fn stop_waits_for_tail() {
        let mut s = session(rec(), vec![Segment { start: 0, end: 2 * S }]);
        s.push(&audio(&[(S, 0.1)]));
        let ev = s.stop();
        assert_eq!(ev, vec![Event::State(State::Finalizing)]);
        assert!(s.wants_audio());
        assert!(finals(&s.push(&audio(&[(S / 8, 0.1)]))).is_empty());
        let ev = s.push(&audio(&[(S / 8, 0.1)]));
        assert_eq!(finals(&ev), vec!["一".to_string()]);
        assert!(!s.wants_audio());
        assert!(s.is_finished());
    }

    #[test]
    fn stop_before_audio_gives_empty_final() {
        let mut s = session(rec(), vec![]);
        let ev = s.stop();
        assert_eq!(ev, vec![Event::State(State::Finalizing), Event::Final(String::new()), Event::State(State::Done)]);
        assert!(s.is_finished());
        assert!(s.push(&[0.1; 320]).is_empty());
    }

    #[test]
    fn short_or_silent_recordings_give_empty_final() {
        let mut s = session(rec(), vec![Segment { start: 0, end: S / 4 }]);
        s.push(&audio(&[(S / 4, 0.1)]));
        assert_eq!(finals(&s.end_of_audio()), vec![String::new()]);
        let mut s = session(rec(), vec![]);
        s.push(&audio(&[(2 * S, 0.0)]));
        assert_eq!(finals(&s.end_of_audio()), vec![String::new()]);
    }

    #[test]
    fn cancel_emits_cancelled_and_no_final() {
        let mut s = session(rec(), vec![Segment { start: 0, end: 2 * S }]);
        s.push(&audio(&[(S, 0.1)]));
        assert_eq!(s.cancel(), vec![Event::State(State::Cancelled)]);
        assert!(s.end_of_audio().is_empty());
        assert!(s.is_finished());
    }

    #[test]
    fn max_length_finalizes_automatically() {
        let mut s = Session::new(
            SessionConfig { max_length: 2 * S, ..cfg() },
            rec(),
            Box::new(FakeVad::always_speech()),
            Arc::new(PostProcessor::empty()),
        );
        let ev = s.push(&audio(&[(3 * S, 0.1)]));
        assert_eq!(finals(&ev), vec!["一".to_string()]);
        assert!(s.is_finished());
    }

    #[test]
    fn recognizer_error_ends_session_with_error() {
        let r = Arc::new(FakeRecognizer::new(|_| Err(RecognizerError::Decode("boom".into()))));
        let mut s = session(r, vec![Segment { start: 0, end: S }]);
        let ev = s.push(&audio(&[(2 * S, 0.1)]));
        assert!(ev.iter().any(|e| matches!(e, Event::Error { code, .. } if code == "model")));
        assert_eq!(ev.last(), Some(&Event::State(State::Error)));
        assert!(s.is_finished());
    }

    #[test]
    fn abort_marks_finished_without_events() {
        let mut s = session(rec(), vec![]);
        s.push(&[0.1; 320]);
        s.abort();
        assert!(s.is_finished());
        assert_eq!(s.state(), State::Error);
        assert!(s.end_of_audio().is_empty());
    }

    #[test]
    fn final_text_is_post_processed() {
        let post = Arc::new(PostProcessor::new(vec![("一".into(), "壱".into())]));
        let mut s = Session::new(cfg(), rec(), Box::new(FakeVad::new(vec![Segment { start: 0, end: S }], S / 4)), post);
        s.push(&audio(&[(2 * S, 0.1)]));
        assert_eq!(finals(&s.end_of_audio()), vec!["壱".to_string()]);
    }
}
```

`RecognizerError` must be imported in the test module (`use crate::recognizer::RecognizerError;`).

- [ ] **Step 2: Run to verify failure**

Run: `scripts/dev cargo test -p koeinput-core session`
Expected: compile errors.

- [ ] **Step 3: Implement**

```rust
use crate::config::Config;
use crate::postprocess::{PostProcessor, join_segments};
use crate::recognizer::Recognizer;
use crate::vad::{Segment, Vad};
use crate::SAMPLE_RATE;
use std::sync::Arc;

const PAD: usize = 1600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State { Starting, Recording, Finalizing, Done, Cancelled, Error }

impl State {
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Recording => "recording",
            State::Finalizing => "finalizing",
            State::Done => "done",
            State::Cancelled => "cancelled",
            State::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    State(State),
    Partial(String),
    Final(String),
    Error { code: String, message: String },
}

#[derive(Debug, Clone, Copy)]
pub struct SessionConfig {
    pub partial_interval: usize,
    pub tail: usize,
    pub min_length: usize,
    pub max_length: usize,
}

impl SessionConfig {
    pub fn from_config(c: &Config) -> Self {
        let ms = |v: u64| (v as usize) * SAMPLE_RATE as usize / 1000;
        Self {
            partial_interval: ms(c.partial_interval_ms),
            tail: ms(c.tail_ms),
            min_length: ms(c.min_length_ms),
            max_length: ms(c.max_length_ms),
        }
    }
}

pub struct Session {
    cfg: SessionConfig,
    rec: Arc<dyn Recognizer>,
    vad: Box<dyn Vad>,
    post: Arc<PostProcessor>,
    audio: Vec<f32>,
    fixed: Vec<String>,
    last_partial: String,
    next_tick: usize,
    stop_at: Option<usize>,
    state: State,
}

impl Session {
    pub fn new(cfg: SessionConfig, rec: Arc<dyn Recognizer>, vad: Box<dyn Vad>, post: Arc<PostProcessor>) -> Self {
        Self { cfg, rec, vad, post, audio: Vec::new(), fixed: Vec::new(), last_partial: String::new(),
               next_tick: cfg.partial_interval, stop_at: None, state: State::Starting }
    }

    pub fn state(&self) -> State { self.state }

    pub fn is_finished(&self) -> bool {
        matches!(self.state, State::Done | State::Cancelled | State::Error)
    }

    pub fn wants_audio(&self) -> bool {
        !self.is_finished() && self.stop_at.is_none_or(|t| self.audio.len() < t)
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<Event> {
        let mut ev = Vec::new();
        if self.is_finished() || samples.is_empty() {
            return ev;
        }
        if self.state == State::Starting {
            self.state = State::Recording;
            ev.push(Event::State(State::Recording));
        }
        self.audio.extend_from_slice(samples);
        self.vad.accept(samples);
        if let Err(e) = self.fix_finished() {
            return self.fail(ev, e);
        }
        if self.audio.len() >= self.cfg.max_length {
            if self.stop_at.is_none() {
                self.state = State::Finalizing;
                ev.push(Event::State(State::Finalizing));
            }
            ev.extend(self.finalize());
            return ev;
        }
        if let Some(t) = self.stop_at {
            if self.audio.len() >= t {
                ev.extend(self.finalize());
            }
            return ev;
        }
        if self.audio.len() >= self.next_tick {
            self.next_tick = self.audio.len() + self.cfg.partial_interval;
            match self.partial_text() {
                Ok(text) if !text.is_empty() && text != self.last_partial => {
                    self.last_partial = text.clone();
                    ev.push(Event::Partial(text));
                }
                Ok(_) => {}
                Err(e) => return self.fail(ev, e),
            }
        }
        ev
    }

    pub fn stop(&mut self) -> Vec<Event> {
        if self.is_finished() || self.stop_at.is_some() {
            return Vec::new();
        }
        self.state = State::Finalizing;
        let mut ev = vec![Event::State(State::Finalizing)];
        if self.audio.is_empty() {
            ev.extend(self.finalize());
        } else {
            self.stop_at = Some(self.audio.len() + self.cfg.tail);
        }
        ev
    }

    pub fn end_of_audio(&mut self) -> Vec<Event> {
        if self.is_finished() {
            return Vec::new();
        }
        let mut ev = Vec::new();
        if self.state != State::Finalizing {
            self.state = State::Finalizing;
            ev.push(Event::State(State::Finalizing));
        }
        ev.extend(self.finalize());
        ev
    }

    pub fn cancel(&mut self) -> Vec<Event> {
        if self.is_finished() {
            return Vec::new();
        }
        self.state = State::Cancelled;
        vec![Event::State(State::Cancelled)]
    }

    pub fn abort(&mut self) {
        self.state = State::Error;
    }

    fn decode(&self, seg: Segment) -> Result<String, crate::recognizer::RecognizerError> {
        let s = seg.start.saturating_sub(PAD);
        let e = (seg.end + PAD).min(self.audio.len());
        self.rec.decode(&self.audio[s..e])
    }

    fn fix_finished(&mut self) -> Result<(), crate::recognizer::RecognizerError> {
        for seg in self.vad.pop_finished() {
            let t = self.decode(seg)?;
            self.fixed.push(t);
        }
        Ok(())
    }

    fn partial_text(&self) -> Result<String, crate::recognizer::RecognizerError> {
        let mut parts = self.fixed.clone();
        if let Some(start) = self.vad.speech_start() {
            parts.push(self.decode(Segment { start, end: self.audio.len() })?);
        }
        Ok(self.post.apply(&join_segments(&parts)))
    }

    fn finalize(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        for seg in self.vad.finish() {
            match self.decode(seg) {
                Ok(t) => self.fixed.push(t),
                Err(e) => return self.fail(ev, e),
            }
        }
        let text = if self.audio.len() < self.cfg.min_length {
            String::new()
        } else {
            self.post.apply(&join_segments(&self.fixed))
        };
        self.state = State::Done;
        ev.push(Event::Final(text));
        ev.push(Event::State(State::Done));
        ev
    }

    fn fail(&mut self, mut ev: Vec<Event>, e: crate::recognizer::RecognizerError) -> Vec<Event> {
        self.state = State::Error;
        ev.push(Event::Error { code: "model".into(), message: e.to_string() });
        ev.push(Event::State(State::Error));
        ev
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `scripts/dev cargo test -p koeinput-core session`
Expected: 11 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "Add session state machine with partial scheduling" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Microphone capture

**Files:**
- Create: `crates/core/src/audio/capture.rs`
- Modify: `crates/core/src/audio/mod.rs` (add `pub mod capture;`)

**Interfaces:**
- Consumes: `AudioError` (Task 5).
- Produces:
  - `pub enum CaptureMsg { Audio(Vec<f32>), Error(String), Closed }`.
  - `pub struct Capture` with `Capture::start(source: Option<String>, tx: crossbeam_channel::Sender<CaptureMsg>) -> Capture` and `fn stop(self)` (sets a stop flag and joins the thread). Dropping a `Capture` also stops it.

The thread opens `libpulse_simple_binding::Simple::new(None, "KoeInput", Direction::Record, source.as_deref(), "Dictation", &spec, None, Some(&attr))` with `spec = Spec { format: Format::F32le, channels: 1, rate: 16000 }` and `attr = BufferAttr { maxlength: u32::MAX, tlength: u32::MAX, prebuf: u32::MAX, minreq: u32::MAX, fragsize: 1280 }` (20 ms; the default fragment on this machine is 2 s). On open failure it sends `Error(msg)` and exits. Otherwise it reads 1280-byte blocks, converts with `f32::from_le_bytes`, sends `Audio`, and sends `Closed` when the stop flag is set or a read fails (after `Error` for a failed read).

- [ ] **Step 1: Add dependency**

```bash
scripts/dev cargo add -p koeinput-core crossbeam-channel
```

- [ ] **Step 2: Write the tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn unreachable_server_reports_error() {
        // SAFETY: tests in this module do not run concurrently with other env readers of PULSE_SERVER.
        unsafe { std::env::set_var("PULSE_SERVER", "unix:/nonexistent/koeinput-test") };
        let (tx, rx) = crossbeam_channel::unbounded();
        let c = Capture::start(None, tx);
        let msg = rx.recv_timeout(Duration::from_secs(5)).expect("a message");
        assert!(matches!(msg, CaptureMsg::Error(_)), "{msg:?}");
        c.stop();
    }

    #[test]
    fn records_from_default_source_when_enabled() {
        if std::env::var("KOEINPUT_AUDIO_TESTS").as_deref() != Ok("1") {
            return;
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        let c = Capture::start(None, tx);
        let mut chunks = 0;
        while chunks < 10 {
            match rx.recv_timeout(Duration::from_secs(3)).expect("audio") {
                CaptureMsg::Audio(a) => {
                    assert_eq!(a.len(), 320);
                    chunks += 1;
                }
                other => panic!("{other:?}"),
            }
        }
        c.stop();
    }
}
```

Derive `Debug` on `CaptureMsg`.

- [ ] **Step 3: Run to verify failure**

Run: `scripts/dev cargo test -p koeinput-core capture`
Expected: compile errors.

- [ ] **Step 4: Implement**

```rust
use crossbeam_channel::Sender;
use libpulse_binding::{def::BufferAttr, sample::{Format, Spec}, stream::Direction};
use libpulse_simple_binding::Simple;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

const CHUNK_BYTES: usize = 1280;

#[derive(Debug)]
pub enum CaptureMsg {
    Audio(Vec<f32>),
    Error(String),
    Closed,
}

pub struct Capture {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Capture {
    pub fn start(source: Option<String>, tx: Sender<CaptureMsg>) -> Capture {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::Builder::new()
            .name("koeinput-capture".into())
            .spawn(move || run(source, tx, flag))
            .expect("spawn capture thread");
        Capture { stop, handle: Some(handle) }
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(source: Option<String>, tx: Sender<CaptureMsg>, stop: Arc<AtomicBool>) {
    let spec = Spec { format: Format::F32le, channels: 1, rate: crate::SAMPLE_RATE };
    let attr = BufferAttr { maxlength: u32::MAX, tlength: u32::MAX, prebuf: u32::MAX, minreq: u32::MAX, fragsize: CHUNK_BYTES as u32 };
    let s = match Simple::new(None, "KoeInput", Direction::Record, source.as_deref(), "Dictation", &spec, None, Some(&attr)) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(CaptureMsg::Error(format!("cannot open microphone: {e}")));
            return;
        }
    };
    let mut buf = [0u8; CHUNK_BYTES];
    while !stop.load(Ordering::SeqCst) {
        if let Err(e) = s.read(&mut buf) {
            let _ = tx.send(CaptureMsg::Error(format!("microphone read failed: {e}")));
            break;
        }
        let samples = buf.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        if tx.send(CaptureMsg::Audio(samples)).is_err() {
            break;
        }
    }
    let _ = tx.send(CaptureMsg::Closed);
}
```

- [ ] **Step 5: Run to verify pass**

Run: `scripts/dev cargo test -p koeinput-core capture`
Expected: 2 tests PASS (the microphone test returns early; the build container has no sound server, so the first test exercises the failure path).

- [ ] **Step 6: Commit**

```bash
git add crates/core Cargo.lock
git commit -m "Add PulseAudio microphone capture" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Engine (runs a session on a worker thread)

**Files:**
- Create: `crates/core/src/engine.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod engine;`)

**Interfaces:**
- Consumes: `Session`, `SessionConfig`, `Event`, `State` (Task 7); `Capture`, `CaptureMsg` (Task 8); `read_wav_16k_mono` (Task 5); `Vad` (Task 6).
- Produces:
  - `pub enum Source { Microphone(Option<String>), File(PathBuf), Samples(Vec<f32>) }`, plus `#[cfg(any(test, feature = "test-hooks"))] Source::Generator { level: f32 }` (sends 20 ms chunks every 20 ms until stopped).
  - `pub type VadFactory = Arc<dyn Fn() -> Result<Box<dyn Vad>, String> + Send + Sync>`.
  - `pub struct Engine { pub cfg: SessionConfig, pub rec: Arc<dyn Recognizer>, pub vad: VadFactory, pub post: Arc<PostProcessor> }` with `fn start(&self, source: Source) -> SessionHandle`.
  - `pub struct SessionHandle { pub events: crossbeam_channel::Receiver<Event>, cmd: Sender<Command> }` with `fn stop(&self)`, `fn cancel(&self)` (ignore send errors after the worker ended).

Worker loop:
1. Send `State(Starting)`. Create the VAD; on error send `Error{code:"model"}` and `State(Error)` and return.
2. `Samples`/`File`: push the audio in 320-sample chunks (stop early if the session stops wanting audio), then `end_of_audio`. File read errors → `Error{code:"audio"}` + `State(Error)`.
3. `Microphone`: start `Capture`. Loop with `crossbeam_channel::select!` over the command channel and the capture channel, with a 2 s timeout:
   - `Audio` → `session.push`; when `!session.wants_audio()` → stop capture, `end_of_audio` if not finished.
   - `Error(msg)` or `Closed` before the session finished → if `stop` was requested, `end_of_audio`; otherwise `Error{code:"audio", message}` + `State(Error)`.
   - `Command::Stop` → `session.stop()`; `Command::Cancel` → stop capture, `session.cancel()`.
   - Timeout while not finished → `Error{code:"audio", message:"no audio for 2 s"}` + `State(Error)`.
4. Forward every event to `events`; return when the session is finished.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRecognizer, FakeVad};
    use std::time::Duration;

    fn engine() -> Engine {
        Engine {
            cfg: SessionConfig { partial_interval: 8000, tail: 4000, min_length: 8000, max_length: 160_000 },
            rec: Arc::new(FakeRecognizer::new(|s| Ok(format!("{}", s.len() / 16000)))),
            vad: Arc::new(|| Ok(Box::new(FakeVad::always_speech()) as Box<dyn Vad>)),
            post: Arc::new(PostProcessor::empty()),
        }
    }

    fn collect(h: &SessionHandle) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(e) = h.events.recv_timeout(Duration::from_secs(10)) {
            let done = matches!(e, Event::State(State::Done | State::Cancelled | State::Error));
            out.push(e);
            if done {
                break;
            }
        }
        out
    }

    #[test]
    fn samples_source_runs_to_final() {
        let h = engine().start(Source::Samples(vec![0.1; 3 * 16000]));
        let ev = collect(&h);
        assert_eq!(ev.first(), Some(&Event::State(State::Starting)));
        assert!(ev.contains(&Event::State(State::Recording)));
        assert!(ev.iter().any(|e| matches!(e, Event::Partial(_))));
        assert!(ev.contains(&Event::Final("3".into())), "{ev:?}");
        assert_eq!(ev.last(), Some(&Event::State(State::Done)));
    }

    #[test]
    fn generator_source_stops_and_cancels() {
        let h = engine().start(Source::Generator { level: 0.1 });
        std::thread::sleep(Duration::from_millis(700));
        h.stop();
        let ev = collect(&h);
        assert!(ev.iter().any(|e| matches!(e, Event::Final(_))), "{ev:?}");
        let h = engine().start(Source::Generator { level: 0.1 });
        std::thread::sleep(Duration::from_millis(300));
        h.cancel();
        assert_eq!(collect(&h).last(), Some(&Event::State(State::Cancelled)));
    }

    #[test]
    fn stop_while_starting_finishes() {
        let h = engine().start(Source::Generator { level: 0.1 });
        h.stop();
        let ev = collect(&h);
        assert_eq!(ev.last(), Some(&Event::State(State::Done)), "{ev:?}");
    }

    #[test]
    fn unreachable_sound_server_reports_audio_error() {
        // SAFETY: see the capture tests; PULSE_SERVER is only read by capture threads.
        unsafe { std::env::set_var("PULSE_SERVER", "unix:/nonexistent/koeinput-test") };
        let h = engine().start(Source::Microphone(None));
        let ev = collect(&h);
        assert!(ev.iter().any(|e| matches!(e, Event::Error { code, .. } if code == "audio")), "{ev:?}");
        assert_eq!(ev.last(), Some(&Event::State(State::Error)));
    }

    #[test]
    fn missing_file_reports_audio_error() {
        let h = engine().start(Source::File("/nonexistent.wav".into()));
        let ev = collect(&h);
        assert!(ev.iter().any(|e| matches!(e, Event::Error { code, .. } if code == "audio")));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `scripts/dev cargo test -p koeinput-core engine`
Expected: compile errors.

- [ ] **Step 3: Implement**

```rust
use crate::audio::capture::{Capture, CaptureMsg};
use crate::postprocess::PostProcessor;
use crate::recognizer::Recognizer;
use crate::session::{Event, Session, SessionConfig, State};
use crate::vad::Vad;
use crossbeam_channel::{Receiver, Sender, select, unbounded};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const CHUNK: usize = 320;
const AUDIO_TIMEOUT: Duration = Duration::from_secs(2);

pub enum Source {
    Microphone(Option<String>),
    File(PathBuf),
    Samples(Vec<f32>),
    #[cfg(any(test, feature = "test-hooks"))]
    Generator { level: f32 },
}

pub type VadFactory = Arc<dyn Fn() -> Result<Box<dyn Vad>, String> + Send + Sync>;

#[derive(Clone)]
pub struct Engine {
    pub cfg: SessionConfig,
    pub rec: Arc<dyn Recognizer>,
    pub vad: VadFactory,
    pub post: Arc<PostProcessor>,
}

enum Command { Stop, Cancel }

pub struct SessionHandle {
    pub events: Receiver<Event>,
    cmd: Sender<Command>,
}

impl SessionHandle {
    pub fn stop(&self) { let _ = self.cmd.send(Command::Stop); }
    pub fn cancel(&self) { let _ = self.cmd.send(Command::Cancel); }
}

impl Engine {
    pub fn start(&self, source: Source) -> SessionHandle {
        let (ev_tx, ev_rx) = unbounded();
        let (cmd_tx, cmd_rx) = unbounded();
        let engine = self.clone();
        std::thread::Builder::new()
            .name("koeinput-session".into())
            .spawn(move || engine.run(source, ev_tx, cmd_rx))
            .expect("spawn session thread");
        SessionHandle { events: ev_rx, cmd: cmd_tx }
    }

    fn run(&self, source: Source, ev: Sender<Event>, cmd: Receiver<Command>) {
        let send = |events: Vec<Event>| for e in events { let _ = ev.send(e); };
        send(vec![Event::State(State::Starting)]);
        let vad = match (self.vad)() {
            Ok(v) => v,
            Err(m) => return send(fail("model", m)),
        };
        let mut session = Session::new(self.cfg, self.rec.clone(), vad, self.post.clone());
        let samples = match source {
            Source::Samples(s) => s,
            Source::File(p) => match crate::audio::file::read_wav_16k_mono(&p) {
                Ok(s) => s,
                Err(e) => return send(fail("audio", e.to_string())),
            },
            Source::Microphone(name) => return self.run_live(&mut session, LiveSource::Mic(name), &send, &cmd),
            #[cfg(any(test, feature = "test-hooks"))]
            Source::Generator { level } => return self.run_live(&mut session, LiveSource::Generator(level), &send, &cmd),
        };
        for chunk in samples.chunks(CHUNK) {
            if let Ok(Command::Cancel) = cmd.try_recv() {
                return send(session.cancel());
            }
            send(session.push(chunk));
            if !session.wants_audio() {
                break;
            }
        }
        send(session.end_of_audio());
    }
}

fn fail(code: &str, message: String) -> Vec<Event> {
    vec![Event::Error { code: code.into(), message }, Event::State(State::Error)]
}
```

Live sources (same file):

```rust
enum LiveSource {
    Mic(Option<String>),
    #[cfg(any(test, feature = "test-hooks"))]
    Generator(f32),
}

enum Running {
    Mic(Capture),
    #[cfg(any(test, feature = "test-hooks"))]
    Generator(Arc<std::sync::atomic::AtomicBool>),
}

impl Running {
    fn start(src: LiveSource, tx: Sender<CaptureMsg>) -> Running {
        match src {
            LiveSource::Mic(name) => Running::Mic(Capture::start(name, tx)),
            #[cfg(any(test, feature = "test-hooks"))]
            LiveSource::Generator(level) => {
                let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let flag = stop.clone();
                std::thread::spawn(move || {
                    while !flag.load(std::sync::atomic::Ordering::SeqCst) {
                        if tx.send(CaptureMsg::Audio(vec![level; CHUNK])).is_err() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    let _ = tx.send(CaptureMsg::Closed);
                });
                Running::Generator(stop)
            }
        }
    }

    fn stop(self) {
        match self {
            Running::Mic(c) => c.stop(),
            #[cfg(any(test, feature = "test-hooks"))]
            Running::Generator(flag) => flag.store(true, std::sync::atomic::Ordering::SeqCst),
        }
    }
}

fn fail_and_finish(session: &mut Session, code: &str, message: String) -> Vec<Event> {
    session.abort();
    fail(code, message)
}

impl Engine {
    fn run_live(&self, session: &mut Session, src: LiveSource, send: &dyn Fn(Vec<Event>), cmd: &Receiver<Command>) {
        let (cap_tx, cap_rx) = unbounded();
        let running = Running::start(src, cap_tx);
        let mut stop_requested = false;
        while !session.is_finished() {
            select! {
                recv(cmd) -> c => match c {
                    Ok(Command::Stop) => {
                        stop_requested = true;
                        send(session.stop());
                    }
                    Ok(Command::Cancel) | Err(_) => send(session.cancel()),
                },
                recv(cap_rx) -> m => match m {
                    Ok(CaptureMsg::Audio(a)) => {
                        send(session.push(&a));
                        if !session.wants_audio() && !session.is_finished() {
                            send(session.end_of_audio());
                        }
                    }
                    Ok(CaptureMsg::Error(msg)) => {
                        if stop_requested {
                            send(session.end_of_audio());
                        } else {
                            send(fail_and_finish(session, "audio", msg));
                        }
                    }
                    Ok(CaptureMsg::Closed) | Err(_) => {
                        if stop_requested {
                            send(session.end_of_audio());
                        } else {
                            send(fail_and_finish(session, "audio", "microphone closed".into()));
                        }
                    }
                },
                default(AUDIO_TIMEOUT) => send(fail_and_finish(session, "audio", "no audio for 2 s".into())),
            }
        }
        running.stop();
    }
}
```

In `run`, pass the closure as `&send` (it has type `impl Fn(Vec<Event>)`, which coerces to `&dyn Fn(Vec<Event>)`), and change the two live branches to `return self.run_live(&mut session, LiveSource::Mic(name), &send, &cmd)` and `return self.run_live(&mut session, LiveSource::Generator(level), &send, &cmd)`.

When `stop` arrives while still `Starting` (no audio yet), `Session::stop` finalizes immediately, so the loop ends because `is_finished()` is true.

- [ ] **Step 4: Run to verify pass**

Run: `scripts/dev cargo test -p koeinput-core engine session`
Expected: 5 engine tests and 11 session tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "Add engine that runs sessions on a worker thread" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: CLI: transcribe and model commands

**Files:**
- Create: `crates/core/src/build.rs` (helper that builds an `Engine` from config)
- Modify: `crates/core/src/lib.rs` (add `pub mod build;`)
- Modify: `crates/cli/src/main.rs`
- Create: `crates/cli/src/model_cmd.rs`, `crates/cli/src/transcribe.rs`
- Test: `crates/cli/tests/cli.rs`

**Interfaces:**
- Consumes: everything from Tasks 2–9.
- Produces:
  - `koeinput_core::build::load_engine(paths: &Paths, cfg: &Config) -> Result<(Engine, String /* model id */), BuildError>` — loads the catalog, picks `cfg.model` or the catalog default, fails with `BuildError::NoModel { id, hint }` if the model or `silero-vad` is not installed (hint: `koeinput model install <id>`), loads the dictionary (warnings go to `tracing::warn!`), builds `SherpaOffline` with `cfg.threads` and a VAD factory using `SileroVad::new`.
  - `pub enum BuildError { Catalog(String), UnknownModel(String), NoModel { id: String, hint: String }, Load(String) }` (thiserror; `NoModel` displays the hint).
  - CLI commands: `koeinput transcribe [--partials] FILE...`, `koeinput model list`, `koeinput model install ID [--from-file PATH]`, `koeinput model remove ID`, `koeinput model use ID`.

`model install` also installs `silero-vad` first if it is missing (from its URL), unless the ID is `silero-vad` itself. Progress prints `\r<id>: <done MB>/<total MB> MB` to stderr.

`transcribe` output, per file:

```
<file>
  <final text>
  audio 3.21 s, final 0.42 s after end of audio
```

With `--partials`, before the final text, one line per partial: `  [1.50 s] <partial text>` where the time is audio time at the moment the partial was produced. Compute audio time by counting samples pushed: run `Session` directly (not `Engine`) so the CLI controls pushing; measure "final after end of audio" as wall time from calling `end_of_audio()` to its return.

- [ ] **Step 1: Add dependencies**

```bash
scripts/dev cargo add -p koeinput-cli clap --features derive
scripts/dev cargo add -p koeinput-cli anyhow tracing tracing-subscriber
scripts/dev cargo add -p koeinput-core tracing
scripts/dev cargo add -p koeinput-cli --dev tempfile
```

- [ ] **Step 2: Write the failing CLI tests**

`crates/cli/tests/cli.rs`:

```rust
use std::process::Command;

fn koeinput(home: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_koeinput"));
    c.env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env("KOEINPUT_DATA_DIR", concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"));
    c
}

#[test]
fn model_list_shows_catalog_and_install_state() {
    let home = tempfile::tempdir().unwrap();
    let out = koeinput(home.path()).args(["model", "list"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("parakeet-ja"));
    assert!(text.contains("default"));
    assert!(text.contains("not installed"));
}

#[test]
fn model_use_writes_config() {
    let home = tempfile::tempdir().unwrap();
    let out = koeinput(home.path()).args(["model", "use", "zipformer-ja"]).output().unwrap();
    assert!(out.status.success());
    let cfg = std::fs::read_to_string(home.path().join(".config/koeinput/config.toml")).unwrap();
    assert!(cfg.contains("zipformer-ja"));
    let out = koeinput(home.path()).args(["model", "use", "nope"]).output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn transcribe_without_model_explains_how_to_install() {
    let home = tempfile::tempdir().unwrap();
    let out = koeinput(home.path()).args(["transcribe", "x.wav"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("koeinput model install parakeet-ja"));
}

#[test]
fn install_from_file_rejects_wrong_checksum() {
    let home = tempfile::tempdir().unwrap();
    let f = home.path().join("fake.onnx");
    std::fs::write(&f, b"not the vad").unwrap();
    let out = koeinput(home.path())
        .args(["model", "install", "silero-vad", "--from-file"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("checksum"));
}
```

- [ ] **Step 3: Run to verify failure**

Run: `scripts/dev cargo test -p koeinput-cli`
Expected: FAIL (commands not implemented).

- [ ] **Step 4: Implement `build.rs`, `main.rs`, `model_cmd.rs`, `transcribe.rs`**

`crates/core/src/build.rs`:

```rust
use crate::config::{Config, Paths};
use crate::engine::{Engine, VadFactory};
use crate::models::{Catalog, VAD_MODEL_ID};
use crate::postprocess::{PostProcessor, load_dictionary};
use crate::recognizer::SherpaOffline;
use crate::session::SessionConfig;
use crate::vad::{SileroVad, Vad};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("model catalog: {0}")]
    Catalog(String),
    #[error("unknown model: {0}")]
    UnknownModel(String),
    #[error("model {id} is not installed; run `{hint}`")]
    NoModel { id: String, hint: String },
    #[error("cannot load model: {0}")]
    Load(String),
}

pub fn load_engine(paths: &Paths, cfg: &Config) -> Result<(Engine, String), BuildError> {
    let catalog = Catalog::load(&paths.catalog_file).map_err(|e| BuildError::Catalog(e.to_string()))?;
    let entry = match &cfg.model {
        Some(id) => catalog.get(id).ok_or_else(|| BuildError::UnknownModel(id.clone()))?,
        None => catalog.default_model().ok_or_else(|| BuildError::Catalog("no default model".into()))?,
    };
    for e in [entry, catalog.get(VAD_MODEL_ID).ok_or_else(|| BuildError::Catalog("no silero-vad entry".into()))?] {
        if !e.is_installed(&paths.models_dir) {
            return Err(BuildError::NoModel { id: e.id.clone(), hint: format!("koeinput model install {}", entry.id) });
        }
    }
    let dict_path = cfg.dictionary.clone().unwrap_or_else(|| paths.dictionary_file.clone());
    let dict = load_dictionary(&dict_path).map_err(|e| BuildError::Load(e.to_string()))?;
    for w in &dict.warnings {
        tracing::warn!("{w}");
    }
    let rec = SherpaOffline::new(entry, &paths.models_dir, cfg.threads).map_err(|e| BuildError::Load(e.to_string()))?;
    let vad_model = catalog.get(VAD_MODEL_ID).unwrap().file(&paths.models_dir, "model").map_err(|e| BuildError::Load(e.to_string()))?;
    let vad: VadFactory = Arc::new(move || SileroVad::new(&vad_model).map(|v| Box::new(v) as Box<dyn Vad>).map_err(|e| e.to_string()));
    let engine = Engine {
        cfg: SessionConfig::from_config(cfg),
        rec: Arc::new(rec),
        vad,
        post: Arc::new(PostProcessor::new(dict.pairs)),
    };
    Ok((engine, entry.id.clone()))
}
```

`crates/cli/src/main.rs`:

```rust
mod listen;
mod model_cmd;
mod transcribe;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "koeinput", version, about = "Japanese voice input (KoeInput)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Dictate through the daemon: Enter starts, Enter stops, Ctrl-C cancels.
    Listen,
    /// Show the daemon's status.
    Status,
    /// Transcribe WAV files in-process, without the daemon.
    Transcribe {
        #[arg(long)]
        partials: bool,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Manage recognition models.
    Model {
        #[command(subcommand)]
        cmd: ModelCmd,
    },
}

#[derive(Subcommand)]
enum ModelCmd {
    List,
    Install {
        id: String,
        #[arg(long)]
        from_file: Option<PathBuf>,
    },
    Remove { id: String },
    Use { id: String },
}

fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
    ).init();
    let cli = Cli::parse();
    let paths = koeinput_core::config::Paths::from_env();
    let result = match cli.cmd {
        Cmd::Listen => listen::run(),
        Cmd::Status => listen::status(),
        Cmd::Transcribe { partials, files } => transcribe::run(&paths, partials, &files),
        Cmd::Model { cmd } => match cmd {
            ModelCmd::List => model_cmd::list(&paths),
            ModelCmd::Install { id, from_file } => model_cmd::install(&paths, &id, from_file.as_deref()),
            ModelCmd::Remove { id } => model_cmd::remove(&paths, &id),
            ModelCmd::Use { id } => model_cmd::use_model(&paths, &id),
        },
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("koeinput: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
```

`env-filter` needs `scripts/dev cargo add -p koeinput-cli tracing-subscriber --features env-filter`.

For this task, `listen.rs` contains only stubs that return `anyhow::bail!("not implemented yet")` for `run` and `status`; Task 12 replaces them.

`model_cmd.rs`:

```rust
use anyhow::{Context, Result, bail};
use koeinput_core::config::{Paths, set_model};
use koeinput_core::models::{Catalog, InstallSource, ModelEntry, VAD_MODEL_ID, install as install_model, remove as remove_model};
use std::path::Path;

fn catalog(paths: &Paths) -> Result<Catalog> {
    Catalog::load(&paths.catalog_file).with_context(|| format!("reading {}", paths.catalog_file.display()))
}

fn entry<'a>(c: &'a Catalog, id: &str) -> Result<&'a ModelEntry> {
    c.get(id).with_context(|| format!("unknown model {id}; see `koeinput model list`"))
}

pub fn list(paths: &Paths) -> Result<()> {
    let c = catalog(paths)?;
    for m in &c.models {
        let state = if m.is_installed(&paths.models_dir) { "installed" } else { "not installed" };
        let default = if m.default { ", default" } else { "" };
        println!("{:<20} {:>7.0} MB  {}{}  {} [{}]", m.id, m.size as f64 / 1e6, state, default, m.name, m.license);
    }
    Ok(())
}

fn install_one(paths: &Paths, m: &ModelEntry, file: Option<&Path>) -> Result<()> {
    let source = match file { Some(f) => InstallSource::File(f), None => InstallSource::Url };
    let mut last = 0u64;
    install_model(m, source, &paths.models_dir, &mut |done, total| {
        if done - last >= 16 << 20 || done == total {
            last = done;
            eprint!("\r{}: {} / {} MB", m.id, done >> 20, total >> 20);
        }
    })
    .with_context(|| format!("installing {}", m.id))?;
    eprintln!("\r{}: installed in {}", m.id, m.dir(&paths.models_dir).display());
    Ok(())
}

pub fn install(paths: &Paths, id: &str, from_file: Option<&Path>) -> Result<()> {
    let c = catalog(paths)?;
    let m = entry(&c, id)?;
    if id != VAD_MODEL_ID {
        let vad = entry(&c, VAD_MODEL_ID)?;
        if !vad.is_installed(&paths.models_dir) {
            install_one(paths, vad, None)?;
        }
    }
    install_one(paths, m, from_file)
}

pub fn remove(paths: &Paths, id: &str) -> Result<()> {
    let c = catalog(paths)?;
    remove_model(entry(&c, id)?, &paths.models_dir)?;
    Ok(())
}

pub fn use_model(paths: &Paths, id: &str) -> Result<()> {
    let c = catalog(paths)?;
    let m = entry(&c, id)?;
    if m.kind == "vad" {
        bail!("{id} is a VAD model, not a recognizer");
    }
    set_model(&paths.config_file, id)?;
    println!("model set to {id}; restart the daemon to use it (systemctl --user restart koeinput)");
    Ok(())
}
```

`transcribe.rs`:

```rust
use anyhow::{Result, anyhow};
use koeinput_core::build::load_engine;
use koeinput_core::config::{Paths, load};
use koeinput_core::session::{Event, Session};
use koeinput_core::SAMPLE_RATE;
use std::path::PathBuf;
use std::time::Instant;

pub fn run(paths: &Paths, partials: bool, files: &[PathBuf]) -> Result<()> {
    let cfg = load(&paths.config_file)?;
    let (engine, _) = load_engine(paths, &cfg).map_err(|e| anyhow!("{e}"))?;
    for f in files {
        let samples = koeinput_core::audio::file::read_wav_16k_mono(f)?;
        let vad = (engine.vad)().map_err(|e| anyhow!(e))?;
        let mut s = Session::new(engine.cfg, engine.rec.clone(), vad, engine.post.clone());
        println!("{}", f.display());
        let mut pushed = 0usize;
        let mut final_text = String::new();
        let mut handle = |events: Vec<Event>, at: usize, final_text: &mut String| -> Result<()> {
            for e in events {
                match e {
                    Event::Partial(t) if partials => println!("  [{:.2} s] {t}", at as f64 / SAMPLE_RATE as f64),
                    Event::Final(t) => *final_text = t,
                    Event::Error { code, message } => return Err(anyhow!("{code}: {message}")),
                    _ => {}
                }
            }
            Ok(())
        };
        for chunk in samples.chunks(320) {
            pushed += chunk.len();
            handle(s.push(chunk), pushed, &mut final_text)?;
        }
        let t = Instant::now();
        let ev = s.end_of_audio();
        let after = t.elapsed();
        handle(ev, pushed, &mut final_text)?;
        println!("  {final_text}");
        println!("  audio {:.2} s, final {:.2} s after end of audio", pushed as f64 / SAMPLE_RATE as f64, after.as_secs_f64());
    }
    Ok(())
}
```

- [ ] **Step 5: Run to verify pass**

Run: `scripts/dev cargo test -p koeinput-cli`
Expected: 4 tests PASS.

- [ ] **Step 6: Manual check with the real model**

Install from the spike's downloaded archives (no network needed for parakeet):

```bash
scripts/dev sh -c 'HOME=/src/work/home KOEINPUT_DATA_DIR=/src/data target/debug/koeinput model install parakeet-ja --from-file work/models/_dl/sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8.tar.bz2'
scripts/dev sh -c 'HOME=/src/work/home KOEINPUT_DATA_DIR=/src/data target/debug/koeinput transcribe --partials work/models/sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8/test_wavs/test_ja_1.wav'
```

`silero-vad` downloads from its URL (640 KB) during the first command.
Expected: partial lines, a Japanese final text, and `final ... after end of audio` under 1 s.

- [ ] **Step 7: Commit**

```bash
git add crates Cargo.lock
git commit -m "Add CLI transcribe and model commands" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: D-Bus API crate and daemon

**Files:**
- Modify: `crates/dbus-api/src/lib.rs`, `crates/dbus-api/Cargo.toml`
- Create: `crates/daemon/src/service.rs`
- Modify: `crates/daemon/src/main.rs`, `crates/daemon/Cargo.toml`
- Create: `crates/daemon/tests/dbus.rs`
- Create: `data/koeinput.service`, `data/org.nemui.KoeInput.service`

**Interfaces:**
- Consumes: `Engine`, `Source`, `SessionHandle`, `Event`, `State` (Task 9), `load_engine`, `BuildError` (Task 10).
- Produces:
  - `koeinput_dbus_api::{BUS_NAME, OBJECT_PATH, INTERFACE, ERROR_PREFIX}` = `"org.nemui.KoeInput"`, `"/org/nemui/KoeInput"`, `"org.nemui.KoeInput.Recognizer1"`, `"org.nemui.KoeInput.Error"`.
  - `koeinput_dbus_api::Recognizer1Proxy` (zbus proxy) with `start(options: HashMap<&str, zbus::zvariant::Value<'_>>) -> u32`, `stop(session: u32)`, `cancel(session: u32)`, properties `ready() -> bool`, `model() -> String`, `version() -> String`, signals `state_changed(session: u32, state: String)`, `partial(session: u32, text: String)`, `final_text(session: u32, text: String)` (D-Bus name `Final`), `error(session: u32, code: String, message: String)`.
  - Daemon binary `koeinput-daemon`. With the `test-hooks` feature and `KOEINPUT_TEST_FAKE=1`, it skips model loading and uses `FakeRecognizer` (text = whole seconds of audio, e.g. `"2"`), `FakeVad::always_speech()` and `Source::Generator { level: 0.1 }` instead of the microphone. With `KOEINPUT_TEST_NO_MODEL=1` it starts with `Ready = false`.

Daemon behaviour (from the spec):
- Own `org.nemui.KoeInput`; serve `Recognizer1` at `/org/nemui/KoeInput`.
- Load the engine with `load_engine` in `tokio::task::spawn_blocking`; until done, `Ready = false`. On `BuildError::NoModel`, stay up with `Ready = false` and remember the hint.
- `Start`: if a session is active → `Busy`; if not ready → `NoModel(hint)` (or `NoModel("model is still loading")`); else take the caller's unique name from the message header, allocate the next id (starting at 1), call `engine.start(Source::Microphone(cfg.source))`, store `{id, owner, handle}`, and spawn the forwarder; return the id.
- Forwarder: a `spawn_blocking` loop reading `handle.events`; for each event, `rt.block_on` emits the matching signal with `SignalEmitter::new(&conn, OBJECT_PATH)?.set_destination(owner.clone().into())`. On `Done`, `Cancelled` or `Error` state, clear the current session if its id matches, then stop.
- `Stop`/`Cancel` with an id that is not the current one → `NoSuchSession`.
- Watch `NameOwnerChanged` through `zbus::fdo::DBusProxy`: when the current owner's name loses its owner, call `handle.cancel()`.
- Log state changes and errors with `tracing`; log text only when `KOEINPUT_LOG_TEXT=1`.

- [ ] **Step 1: Add dependencies**

```bash
scripts/dev cargo add -p koeinput-dbus-api zbus@6 --no-default-features --features tokio,proxy
scripts/dev cargo add -p koeinput-daemon zbus@6 --no-default-features --features tokio,proxy,service
scripts/dev cargo add -p koeinput-daemon tokio --features rt-multi-thread,macros,sync,time
scripts/dev cargo add -p koeinput-daemon koeinput-dbus-api --path crates/dbus-api
scripts/dev cargo add -p koeinput-daemon tracing tracing-subscriber futures-util thiserror
scripts/dev cargo add -p koeinput-daemon --dev tokio --features rt-multi-thread,macros,time
scripts/dev cargo add -p koeinput-daemon --dev futures-util
scripts/dev cargo add -p koeinput-cli koeinput-dbus-api --path crates/dbus-api
scripts/dev cargo add -p koeinput-cli zbus@6 --no-default-features --features tokio,proxy
scripts/dev cargo add -p koeinput-cli tokio --features rt-multi-thread,macros,io-std,io-util,signal,time
scripts/dev cargo add -p koeinput-cli futures-util
```

If a feature name differs in zbus 6, check `zbus-6.0.0/Cargo.toml` `[features]` (features `tokio`, `proxy`, `service` exist there).

- [ ] **Step 2: Write `koeinput-dbus-api`**

```rust
use std::collections::HashMap;
use zbus::proxy;
use zbus::zvariant::Value;

pub const BUS_NAME: &str = "org.nemui.KoeInput";
pub const OBJECT_PATH: &str = "/org/nemui/KoeInput";
pub const INTERFACE: &str = "org.nemui.KoeInput.Recognizer1";
pub const ERROR_PREFIX: &str = "org.nemui.KoeInput.Error";

#[proxy(
    interface = "org.nemui.KoeInput.Recognizer1",
    default_service = "org.nemui.KoeInput",
    default_path = "/org/nemui/KoeInput"
)]
pub trait Recognizer1 {
    fn start(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<u32>;
    fn stop(&self, session: u32) -> zbus::Result<()>;
    fn cancel(&self, session: u32) -> zbus::Result<()>;

    #[zbus(property)]
    fn ready(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn model(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    fn state_changed(&self, session: u32, state: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn partial(&self, session: u32, text: String) -> zbus::Result<()>;
    #[zbus(signal, name = "Final")]
    fn final_text(&self, session: u32, text: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn error(&self, session: u32, code: String, message: String) -> zbus::Result<()>;
}
```

- [ ] **Step 3: Write the failing D-Bus tests**

`crates/daemon/tests/dbus.rs` (run under `dbus-run-session`; each test starts its own daemon and waits for the name):

```rust
#![cfg(feature = "test-hooks")]

use futures_util::StreamExt;
use koeinput_dbus_api::{BUS_NAME, Recognizer1Proxy};
use std::collections::HashMap;
use std::process::{Child, Command};
use std::time::Duration;
use tokio::time::timeout;

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn start_daemon(extra: &[(&str, &str)]) -> (Daemon, zbus::Connection) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_koeinput-daemon"));
    cmd.env("KOEINPUT_TEST_FAKE", "1");
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let child = Daemon(cmd.spawn().unwrap());
    let conn = zbus::Connection::session().await.unwrap();
    let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
    for _ in 0..100 {
        if dbus.name_has_owner(BUS_NAME.try_into().unwrap()).await.unwrap() {
            return (child, conn);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("daemon did not own {BUS_NAME}");
}

async fn wait_ready(p: &Recognizer1Proxy<'_>) {
    for _ in 0..100 {
        if p.ready().await.unwrap() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("not ready");
}

#[tokio::test(flavor = "multi_thread")]
async fn start_stop_delivers_partial_and_final_to_caller_only() {
    let (_d, conn) = start_daemon(&[]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;

    let other = zbus::Connection::session().await.unwrap();
    let op = Recognizer1Proxy::new(&other).await.unwrap();
    let mut other_finals = op.receive_final_text().await.unwrap();

    let mut states = p.receive_state_changed().await.unwrap();
    let mut finals = p.receive_final_text().await.unwrap();
    let id = p.start(HashMap::new()).await.unwrap();
    let mut seen = Vec::new();
    while let Ok(Some(s)) = timeout(Duration::from_secs(5), states.next()).await {
        let a = s.args().unwrap();
        assert_eq!(a.session, id);
        seen.push(a.state.clone());
        if a.state == "recording" {
            break;
        }
    }
    assert_eq!(seen, vec!["starting".to_string(), "recording".to_string()]);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    p.stop(id).await.unwrap();
    let f = timeout(Duration::from_secs(5), finals.next()).await.unwrap().unwrap();
    let a = f.args().unwrap();
    assert_eq!(a.session, id);
    assert!(!a.text.is_empty());
    assert!(timeout(Duration::from_millis(500), other_finals.next()).await.is_err(), "signal leaked to another client");
}

#[tokio::test(flavor = "multi_thread")]
async fn second_start_is_busy_and_unknown_ids_are_rejected() {
    let (_d, conn) = start_daemon(&[]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    let id = p.start(HashMap::new()).await.unwrap();
    let err = p.start(HashMap::new()).await.unwrap_err();
    assert!(err.to_string().contains("org.nemui.KoeInput.Error.Busy"), "{err}");
    let err = p.stop(id + 100).await.unwrap_err();
    assert!(err.to_string().contains("org.nemui.KoeInput.Error.NoSuchSession"), "{err}");
    p.cancel(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn owner_disconnect_cancels_session() {
    let (_d, conn) = start_daemon(&[]).await;
    {
        let first = zbus::Connection::session().await.unwrap();
        let p1 = Recognizer1Proxy::new(&first).await.unwrap();
        wait_ready(&p1).await;
        p1.start(HashMap::new()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    let mut ok = false;
    for _ in 0..40 {
        match p.start(HashMap::new()).await {
            Ok(id) => {
                p.cancel(id).await.unwrap();
                ok = true;
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    assert!(ok, "session of the disconnected client was not cancelled");
}

#[tokio::test(flavor = "multi_thread")]
async fn start_without_model_explains_install() {
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_NO_MODEL", "1")]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    assert!(!p.ready().await.unwrap());
    let mut msg = String::new();
    for _ in 0..40 {
        msg = p.start(HashMap::new()).await.unwrap_err().to_string();
        if !msg.contains("still loading") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(msg.contains("org.nemui.KoeInput.Error.NoModel"), "{msg}");
    assert!(msg.contains("koeinput model install"), "{msg}");
}
```

- [ ] **Step 4: Run to verify failure**

Run: `scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks`
Expected: FAIL (daemon does not own the name).

- [ ] **Step 5: Implement `service.rs`**

```rust
use koeinput_core::engine::{Engine, SessionHandle, Source};
use koeinput_core::session::{Event, State};
use koeinput_dbus_api::OBJECT_PATH;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::message::Header;
use zbus::names::{BusName, UniqueName};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, interface};

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.nemui.KoeInput.Error")]
pub enum KoeError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Busy(String),
    NoModel(String),
    NoSuchSession(String),
}

pub enum Readiness {
    Loading,
    Ready(Arc<Engine>),
    Missing(String),
}

pub struct Current {
    pub id: u32,
    pub owner: UniqueName<'static>,
    pub handle: Arc<SessionHandle>,
}

pub struct Shared {
    pub readiness: Readiness,
    pub current: Option<Current>,
    pub next_id: u32,
    pub model: String,
    pub source: Option<String>,
    pub test_source: bool,
}

pub struct Recognizer1 {
    pub shared: Arc<Mutex<Shared>>,
}

#[interface(name = "org.nemui.KoeInput.Recognizer1")]
impl Recognizer1 {
    async fn start(
        &self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> Result<u32, KoeError> {
        let owner = hdr.sender().ok_or_else(|| KoeError::NoSuchSession("no sender".into()))?.to_owned();
        let mut s = self.shared.lock().unwrap();
        if s.current.is_some() {
            return Err(KoeError::Busy("a session is already running".into()));
        }
        let engine = match &s.readiness {
            Readiness::Ready(e) => e.clone(),
            Readiness::Loading => return Err(KoeError::NoModel("the model is still loading".into())),
            Readiness::Missing(hint) => return Err(KoeError::NoModel(hint.clone())),
        };
        s.next_id += 1;
        let id = s.next_id;
        let source = if s.test_source { test_source() } else { Source::Microphone(s.source.clone()) };
        let handle = Arc::new(engine.start(source));
        s.current = Some(Current { id, owner: owner.clone(), handle: handle.clone() });
        drop(s);
        spawn_forwarder(conn.clone(), self.shared.clone(), id, owner, handle);
        Ok(id)
    }

    async fn stop(&self, session: u32) -> Result<(), KoeError> {
        self.with_session(session, |h| h.stop())
    }

    async fn cancel(&self, session: u32) -> Result<(), KoeError> {
        self.with_session(session, |h| h.cancel())
    }

    #[zbus(property)]
    async fn ready(&self) -> bool {
        matches!(self.shared.lock().unwrap().readiness, Readiness::Ready(_))
    }

    #[zbus(property)]
    async fn model(&self) -> String {
        self.shared.lock().unwrap().model.clone()
    }

    #[zbus(property)]
    async fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    #[zbus(signal)]
    pub async fn state_changed(emitter: &SignalEmitter<'_>, session: u32, state: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn partial(emitter: &SignalEmitter<'_>, session: u32, text: &str) -> zbus::Result<()>;
    #[zbus(signal, name = "Final")]
    pub async fn final_text(emitter: &SignalEmitter<'_>, session: u32, text: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn error(emitter: &SignalEmitter<'_>, session: u32, code: &str, message: &str) -> zbus::Result<()>;
}

impl Recognizer1 {
    fn with_session(&self, id: u32, f: impl FnOnce(&SessionHandle)) -> Result<(), KoeError> {
        let s = self.shared.lock().unwrap();
        match &s.current {
            Some(c) if c.id == id => {
                f(&c.handle);
                Ok(())
            }
            _ => Err(KoeError::NoSuchSession(format!("no session {id}"))),
        }
    }
}

#[cfg(feature = "test-hooks")]
fn test_source() -> Source {
    Source::Generator { level: 0.1 }
}

#[cfg(not(feature = "test-hooks"))]
fn test_source() -> Source {
    unreachable!("test source requested without the test-hooks feature")
}

fn spawn_forwarder(conn: Connection, shared: Arc<Mutex<Shared>>, id: u32, owner: UniqueName<'static>, handle: Arc<SessionHandle>) {
    let rt = tokio::runtime::Handle::current();
    let log_text = std::env::var("KOEINPUT_LOG_TEXT").as_deref() == Ok("1");
    tokio::task::spawn_blocking(move || {
        let emitter = match SignalEmitter::new(&conn, OBJECT_PATH) {
            Ok(e) => e.set_destination(BusName::Unique(owner)),
            Err(e) => return tracing::error!("signal emitter: {e}"),
        };
        while let Ok(ev) = handle.events.recv() {
            let finished = matches!(ev, Event::State(State::Done | State::Cancelled | State::Error));
            let r = rt.block_on(async {
                match &ev {
                    Event::State(st) => {
                        tracing::info!(session = id, state = st.as_str());
                        Recognizer1::state_changed(&emitter, id, st.as_str()).await
                    }
                    Event::Partial(t) => Recognizer1::partial(&emitter, id, t).await,
                    Event::Final(t) => {
                        if log_text {
                            tracing::info!(session = id, text = %t, "final");
                        }
                        Recognizer1::final_text(&emitter, id, t).await
                    }
                    Event::Error { code, message } => {
                        tracing::warn!(session = id, code = %code, "{message}");
                        Recognizer1::error(&emitter, id, code, message).await
                    }
                }
            });
            if let Err(e) = r {
                tracing::warn!("emitting signal: {e}");
            }
            if finished {
                let mut s = shared.lock().unwrap();
                if s.current.as_ref().is_some_and(|c| c.id == id) {
                    s.current = None;
                }
                break;
            }
        }
    });
}
```

Emitting `Final` before the `done` state matters: the session emits `Final` then `State(Done)`, so clients see the text before the session is cleared.

- [ ] **Step 6: Implement `main.rs`**

```rust
mod service;

use futures_util::StreamExt;
use koeinput_core::build::{BuildError, load_engine};
use koeinput_core::config::{Paths, load};
use koeinput_dbus_api::{BUS_NAME, OBJECT_PATH};
use service::{Readiness, Recognizer1, Shared};
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
    ).init();
    let paths = Paths::from_env();
    let cfg = load(&paths.config_file)?;
    let test_fake = cfg!(feature = "test-hooks") && std::env::var("KOEINPUT_TEST_FAKE").as_deref() == Ok("1");
    let shared = Arc::new(Mutex::new(Shared {
        readiness: Readiness::Loading,
        current: None,
        next_id: 0,
        model: cfg.model.clone().unwrap_or_default(),
        source: cfg.source.clone(),
        test_source: test_fake,
    }));
    let conn = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, Recognizer1 { shared: shared.clone() })?
        .build()
        .await?;
    tracing::info!("serving {BUS_NAME}");

    let loader_shared = shared.clone();
    tokio::task::spawn_blocking(move || {
        let r = load_for(&paths, &cfg, test_fake);
        let mut s = loader_shared.lock().unwrap();
        match r {
            Ok((engine, id)) => {
                s.model = id;
                s.readiness = Readiness::Ready(Arc::new(engine));
                tracing::info!(model = %s.model, "model loaded");
            }
            Err(BuildError::NoModel { hint, .. }) => {
                tracing::warn!("no model installed; run `{hint}`");
                s.readiness = Readiness::Missing(format!("no model installed; run `{hint}`"));
            }
            Err(e) => {
                tracing::error!("{e}");
                s.readiness = Readiness::Missing(e.to_string());
            }
        }
    });

    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let mut changes = dbus.receive_name_owner_changed().await?;
    while let Some(sig) = changes.next().await {
        let Ok(args) = sig.args() else { continue };
        if args.new_owner().is_some() {
            continue;
        }
        let s = shared.lock().unwrap();
        if let Some(c) = &s.current {
            if args.name().as_str() == c.owner.as_str() {
                tracing::info!(session = c.id, "client left; cancelling");
                c.handle.cancel();
            }
        }
    }
    Ok(())
}

fn load_for(paths: &Paths, cfg: &koeinput_core::config::Config, test_fake: bool) -> Result<(koeinput_core::engine::Engine, String), BuildError> {
    #[cfg(feature = "test-hooks")]
    if test_fake {
        if std::env::var("KOEINPUT_TEST_NO_MODEL").as_deref() == Ok("1") {
            return Err(BuildError::NoModel { id: "parakeet-ja".into(), hint: "koeinput model install parakeet-ja".into() });
        }
        use koeinput_core::testing::{FakeRecognizer, FakeVad};
        let engine = koeinput_core::engine::Engine {
            cfg: koeinput_core::session::SessionConfig::from_config(cfg),
            rec: Arc::new(FakeRecognizer::new(|s| Ok(format!("{}", s.len() / 16000)))),
            vad: Arc::new(|| Ok(Box::new(FakeVad::always_speech()) as Box<dyn koeinput_core::vad::Vad>)),
            post: Arc::new(koeinput_core::postprocess::PostProcessor::empty()),
        };
        return Ok((engine, "fake".into()));
    }
    let _ = test_fake;
    load_engine(paths, cfg)
}
```

Add `anyhow` to the daemon (`scripts/dev cargo add -p koeinput-daemon anyhow` and `tracing-subscriber --features env-filter`).

- [ ] **Step 7: Write the service files**

`data/koeinput.service`:

```ini
[Unit]
Description=KoeInput speech recognition daemon

[Service]
Type=dbus
BusName=org.nemui.KoeInput
ExecStart=/usr/libexec/koeinput/koeinput-daemon
Restart=on-failure
```

`data/org.nemui.KoeInput.service`:

```ini
[D-BUS Service]
Name=org.nemui.KoeInput
Exec=/usr/libexec/koeinput/koeinput-daemon
SystemdService=koeinput.service
```

- [ ] **Step 8: Run to verify pass**

Run: `scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks`
Expected: 4 tests PASS.
Also run: `scripts/dev cargo build -p koeinput-daemon` (without `test-hooks`) to make sure the release configuration compiles.

- [ ] **Step 9: Commit**

```bash
git add crates data Cargo.lock
git commit -m "Add D-Bus daemon with unicast session signals" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: CLI listen and status, end-to-end check

**Files:**
- Modify: `crates/cli/src/listen.rs`
- Create: `docs/development.md`

**Interfaces:**
- Consumes: `Recognizer1Proxy` (Task 11).
- Produces: `listen::run() -> anyhow::Result<()>`, `listen::status() -> anyhow::Result<()>` (both create their own tokio runtime).

`listen` flow:
1. Connect to the session bus; build the proxy. If `ready()` is false, print the daemon's start error hint: call `start` once and print the error message, then exit with failure.
2. Subscribe to `state_changed`, `partial`, `final_text` and `error` before the first `Start`.
3. Print `Enter: start / stop, Ctrl-C: cancel, q + Enter: quit`. Read stdin lines with `tokio::io::BufReader::lines`.
4. Enter while idle → `start`; print `[preparing]` immediately; on state `recording` print `[recording]`.
5. On `partial` for the current id → print `\r\x1b[K` + text (no newline).
6. Enter while recording → note `Instant::now()`, call `stop`.
7. On `final_text` → print `\r\x1b[K` + text + newline and `(final <x.xx> s after stop)`; return to idle.
8. On `error` → print it on its own line; return to idle.
9. Ctrl-C (`tokio::signal::ctrl_c`) → `cancel` the current session if any, then exit.

`status` prints `ready: yes|no`, `model: <id>`, `version: <v>`; if the daemon is not running, print `daemon not running (start it with: systemctl --user start koeinput, or run koeinput-daemon)` and fail.

- [ ] **Step 1: Implement `listen.rs`**

```rust
use anyhow::{Context, Result};
use futures_util::StreamExt;
use koeinput_dbus_api::Recognizer1Proxy;
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread().enable_all().build()?)
}

pub fn status() -> Result<()> {
    runtime()?.block_on(async {
        let conn = zbus::Connection::session().await.context("no session bus")?;
        let p = Recognizer1Proxy::new(&conn).await?;
        let ready = p.ready().await.context(
            "daemon not running (start it with: systemctl --user start koeinput, or run koeinput-daemon)",
        )?;
        println!("ready: {}", if ready { "yes" } else { "no" });
        println!("model: {}", p.model().await?);
        println!("version: {}", p.version().await?);
        Ok(())
    })
}

pub fn run() -> Result<()> {
    runtime()?.block_on(async {
        let conn = zbus::Connection::session().await.context("no session bus")?;
        let p = Recognizer1Proxy::new(&conn).await?;
        let ready = p.ready().await.context("daemon not running")?;
        if !ready {
            let err = p.start(HashMap::new()).await.err().map(|e| e.to_string()).unwrap_or_default();
            anyhow::bail!("daemon not ready: {err}");
        }
        let mut states = p.receive_state_changed().await?;
        let mut partials = p.receive_partial().await?;
        let mut finals = p.receive_final_text().await?;
        let mut errors = p.receive_error().await?;
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let mut current: Option<u32> = None;
        let mut stopped_at: Option<Instant> = None;
        println!("Enter: start / stop, Ctrl-C: cancel, q + Enter: quit");
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Some(line) = line? else { break };
                    if line.trim() == "q" { break; }
                    match current {
                        None => {
                            let id = p.start(HashMap::new()).await?;
                            current = Some(id);
                            println!("[preparing]");
                        }
                        Some(id) => {
                            stopped_at = Some(Instant::now());
                            p.stop(id).await?;
                        }
                    }
                }
                Some(s) = states.next() => {
                    let a = s.args()?;
                    if Some(a.session) == current && a.state == "recording" {
                        println!("[recording]");
                    }
                    if Some(a.session) == current && matches!(a.state.as_str(), "done" | "cancelled" | "error") {
                        current = None;
                    }
                }
                Some(s) = partials.next() => {
                    let a = s.args()?;
                    if Some(a.session) == current {
                        print!("\r\x1b[K{}", a.text);
                        std::io::stdout().flush()?;
                    }
                }
                Some(s) = finals.next() => {
                    let a = s.args()?;
                    let after = stopped_at.take().map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
                    println!("\r\x1b[K{}", a.text);
                    println!("(final {after:.2} s after stop)");
                }
                Some(s) = errors.next() => {
                    let a = s.args()?;
                    println!("\nerror [{}]: {}", a.code, a.message);
                }
                _ = tokio::signal::ctrl_c() => {
                    if let Some(id) = current {
                        let _ = p.cancel(id).await;
                    }
                    break;
                }
            }
        }
        Ok(())
    })
}
```

- [ ] **Step 2: Build release binaries**

Run: `scripts/dev cargo build --release --workspace`
Expected: success.

- [ ] **Step 3: End-to-end check in the outer dev container**

The release binaries run directly in the outer container, which has `libpulse.so.0`, `libpulse-simple.so.0`, the host session bus and the host PulseAudio socket. The real model was installed under `work/home` in Task 10.

```bash
HOME=$PWD/work/home KOEINPUT_DATA_DIR=$PWD/data target/release/koeinput-daemon &
sleep 3
target/release/koeinput status
HOME=$PWD/work/home KOEINPUT_DATA_DIR=$PWD/data target/release/koeinput transcribe --partials work/eval-data/audio/s01.wav work/eval-data/audio/s21.wav work/eval-data/audio/f12.wav
kill %1
```

Expected: `status` shows `ready: yes` and `model: parakeet-ja`; `transcribe` prints partial lines and final texts, and every `final ... after end of audio` is under 2 s.
Do not paste the user's transcripts into commit messages or docs.

The microphone test (`koeinput listen`) needs the user to speak. Leave it for the user and describe the command in `docs/development.md`.

- [ ] **Step 4: Write `docs/development.md`**

Plain English, short. Sections: "Build" (`scripts/dev cargo build --release --workspace`, why podman), "Test" (unit tests, `dbus-run-session` tests, opt-in model tests with `KOEINPUT_MODEL_TESTS=1` and `KOEINPUT_TEST_MODELS_DIR`, opt-in microphone test with `KOEINPUT_AUDIO_TESTS=1`), "Run the daemon in the dev container" (the commands from Step 3 with `HOME=$PWD/work/home`), "Try dictation" (`koeinput listen`: Enter, speak after `[recording]`, Enter), "Install a model" (`koeinput model install parakeet-ja`, `--from-file`), "Privacy" (`KOEINPUT_LOG_TEXT=1` is the only way text reaches logs).

- [ ] **Step 5: Run the full test suite**

Run: `scripts/dev cargo test --workspace && scripts/dev dbus-run-session -- cargo test -p koeinput-daemon --features test-hooks && scripts/dev cargo clippy --workspace --all-targets -- -D warnings`
Expected: all PASS, no clippy warnings.

- [ ] **Step 6: Commit**

```bash
git add crates docs/development.md Cargo.lock
git commit -m "Add CLI listen and status, and developer notes" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
