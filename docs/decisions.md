# KoeInput: decisions so far

Last updated: 2026-10-18.

Each item has one of these labels:

- **Decided**: agreed with the user.
- **Recommended**: proposed, not final.
- **To verify**: not yet confirmed. Do not treat it as fact.
- **Open**: not discussed enough to decide.

## Name

**Decided**: KoeInput (声インプット), short form KI.

- Packages: `koeinput` and `fcitx5-koeinput`.
- Command: `koeinput`. Daemon: `koeinput-daemon`.
- D-Bus name: `org.nemui.KoeInput`.
- A two-letter package name such as `ki` was rejected because it collides easily (an existing crate, and the Kotlin interactive shell command).
- The repository directory was renamed from `voice-flood` to `koeinput` on 2026-10-18.

## Goal

Dictate Japanese text straight into the focused text field on Linux, while Mozc stays active.

The main use is technical text, such as instructions to AI agents.
This text has many technical terms and English words.

The user tried Speech Note first.
Copy and paste from it did not work reliably, and even when it works, the extra step is tedious.
So the core value of this project is to commit text through the input method, with no copy and paste.

## Scope and constraints

- **Decided**: Local only. Nothing is sent to cloud speech recognition or cloud LLMs.
- **Decided**: Japanese first. The first release supports Japanese only.
- **Decided**: Ship as a Debian package first, then RPM and other formats that fit.
- **Decided**: Integrate as an fcitx5 addon (module), not as an engine.
  An engine would be exclusive with Mozc; an addon can commit and show preedit while Mozc stays active.
- **Decided**: Write both the addon (C++) and the daemon ourselves. See [Rejected: fcitx5-vinput](#rejected-fcitx5-vinput).
- **Decided**: Run recognition in a separate daemon process with user privileges.
  fcitx5 runs a single-threaded event loop, so inference inside it would freeze all typing.
- **Decided**: The daemon runs on the host in the end (systemd user unit).
  During development it can run in the dev container, which shares the host session bus and PulseAudio socket.
- **Decided**: Push-to-talk with an fcitx5 hotkey. The hotkey only works when a text field has focus.
- **Recommended**: Use the session D-Bus between the addon, the daemon and the CLI.
  fcitx-utils has D-Bus support built into the fcitx5 event loop.
  Also, the dev container shares only the bus, PulseAudio and Wayland sockets with the host, so a Unix socket made in the container is not visible to the host fcitx5.

## Rejected: fcitx5-vinput

<https://github.com/xifan2333/fcitx5-vinput> (GPL-3.0), read at commit `ac545c6` (v2.4.2, 2026-10-14).

**Decided**: Do not use it, not even as the IM layer.
The user tried it and found it too focused on Chinese, with privacy problems and too much reliance on cloud APIs.

**Decided**: Never copy vinput source code. It is GPL-3.0.
Its design ideas may guide our design, but all code is written from scratch.
Pass this rule on to every agent that works on this project.

Ideas worth reusing:

- Open the microphone when recording starts, not at daemon start.
- Ask the sound server for 16 kHz mono audio and let it resample.
- Use a modifier-only trigger key.
  Tell tap from hold by duration, ignore key auto-repeat, and cancel on a chord or on focus out.
- Ignore recordings shorter than about 0.5 s.
- Each partial result replaces the whole preedit text.

Mistakes to avoid:

- Synchronous D-Bus calls or polling on the fcitx5 event loop. vinput polls every 200 ms with a 5 s timeout, so a hung daemon freezes all typing.
- Blocking file I/O in the addon.
- Forcing the SurroundingText capability on every input context.
- Broadcasting transcripts as signals that every process on the session bus receives.
- Showing "recording" before audio actually arrives.
- Overwriting Mozc's preedit while Mozc is composing.

## Hardware and environment

Measured on 2026-10-17:

- CPU: Intel Core i9-12900T (Alder Lake, 8 P-cores + 8 E-cores, 24 threads, AVX2, AVX-VNNI, no AVX-512). No NPU.
- GPU: Intel UHD 770 (32 EU). It is small, and the dev container has no `/dev/dri`. The first round of evaluation uses the CPU only.
- RAM: 125 GiB. Desktop machine, no battery. Memory is not the real limit; CPU speed is.
- Default microphone: DJI Wireless Mic Rx (USB).
- Host desktop: KDE Plasma on Wayland, fcitx5 with Mozc.
  Apps connected to fcitx5: WezTerm, Chrome, Firefox, Emacs, Discord.
- Dev container: Debian 13. The host runs a newer Debian (kernel `7.2.9+deb14`).
  The host's fcitx5 version does not need to be known in advance. See [fcitx5 versions](#fcitx5-versions).
  A daemon built in the container should run on the host, because glibc is backward compatible.

## Recording behavior

- **Decided**: Do not keep the microphone open all the time. Open it when the hotkey is pressed.
- **Decided**: Share the microphone with other apps through PipeWire, so video meetings keep working.
  Avoid Bluetooth headset microphones, which can switch the headset to a low-quality call profile.
- **Decided**: Show visual feedback while recording.
  Switch to "recording" when the first audio data arrives, not when the key is pressed.
  The device takes time to wake up, and the user should start speaking only after the indicator changes.
- **Decided** (revised 2026-10-17): Committing Mozc's composition is not required.
  If Mozc is composing when the hotkey is pressed, either do not start recording, or leave the composition as it is.
  The M2 spec picks one.
- **Recommended**: Keep recording for 0.2 to 0.3 s after the key is released, so the last syllable is not cut off.
  This adds to the final latency. Check the right length with the evaluation data.
- **Decided**: Ask for a small capture latency (for example 20 ms).
  Measured on 2026-10-17 through the PulseAudio API on this machine: with default settings, audio arrives in 2-second chunks.
  That adds up to 2 s of delay, and `parecord` drops the last chunk when it stops.
- **To verify**: Keep the stream connected but inactive for a few seconds after release, to restart faster.
  Check whether KDE's microphone indicator stays on in that state.

## Partial and final results

- **Decided**: Show partial results all the time while the user speaks. They may change.
- **Decided**: The final result may take 2 s or more if partial results are visible. Under 1 s is preferred.
- **Recommended**: Start with one model.
  While the user speaks, decode the growing audio again every 0.5 to 1 s and show it as the partial result.
  On release, decode the whole utterance for the final result.
  If accuracy or format is not good enough, use a more accurate model only for the final result.
- Verified on 2026-10-17: sherpa-onnx has no true streaming Japanese model.
  Qwen3-ASR's official streaming mode needs vLLM, which means a GPU.

## Output format

**Decided** requirements:

- Add punctuation.
- Write numbers as Arabic numerals by default, but in kanji when the context calls for it (for example 一般, 十分, 一人).
- Remove fillers such as えー and えっと.
- Write English words in Latin letters (Rust, not ラスト).
- Get technical terms right. Support a user dictionary.

**Recommended** approach:

- Pick a model that produces most of this format by itself (written-style output).
  Use rules only for narrow cases.
  Rule-based numeral conversion breaks words like 一般, and rule-based filler removal breaks normal words like あの and まあ.
- Build the dictionary in two layers:
  1. Give terms to the recognizer: hotwords, or the Whisper initial prompt.
  2. After recognition, replace known misrecognitions with the right term.
     Build this table from corrections made in the evaluation CLI.
- **To verify**: Reuse the Mozc user dictionary as a source of terms.
- No LLM post-processing at first. On this CPU it takes seconds, gets slower with longer text, and may rewrite what the user said.

## fcitx5 addon

- **Recommended**: Keep the addon minimal: hotkey, preedit, commit and asynchronous D-Bus calls. Everything else goes in the daemon.
  An addon crash stops all typing.
- **Decided**: Detect key release the same way vinput does: release events in an event watcher at the `PreInputMethod` phase.
  No separate test beforehand; M2 confirms it. The code is our own.
- **Decided** (2026-10-18): The default hotkey is Right Alt, for now.
  It must be changeable in the fcitx5 settings window.
- **Decided** (2026-10-18): Two ways to press the hotkey:
  - hold it while speaking, and release it to finish (push-to-talk);
  - tap it to start, then press it again to finish (hands-free, for long dictation).
- **Decided** (2026-10-18): If Mozc is composing when the hotkey is pressed, do not start recording, and pass the key to Mozc.
- **Decided** (2026-10-18): M2 already shows partial results, as preedit in the app.
  Apps without preedit support get them in the fcitx5 panel.
- **Decided** (M4, 2026-10-18): Partial results can move to the fcitx5 panel, for every app or for listed apps, with the "Partial text" setting (see [Quality (M4)](#quality-m4)). The default stays in the app.
  Terminals and Emacs handle inline preedit differently; the host check tells which apps need the list.
- **Decided** (2026-10-22): Listed apps show `…` plus the end of the partial text, 30 characters by default, with the "Apps that show only the end of partial text" list.
  Terminals draw preedit on one row and cut it off at the window edge (checked in WezTerm's renderer). The input method cannot make them wrap, and the classic UI panel line is a single line too.
  The list is empty by default because only WezTerm's name (`org.wezfurlong.wezterm`) was checked.
- **Open**: Cancel with Esc, a maximum recording length, and what to show when the daemon does not respond.
- **Open**: Undo of the last commit. It depends on app support for surrounding text.

## fcitx5 versions

- **Decided**: Support every fcitx5 5.x release.
- Verified on 2026-10-17 at packages.debian.org: the core library has kept the same soname (`libfcitx5core7`) from 5.0.5 (Debian 11) to 5.1.23 (Debian testing).
  fcitx5 hides class internals behind private pointers, which keeps the binary interface stable within a soname.
- **Recommended**: Write the addon against the 5.0 API.
  Use newer APIs only behind compile-time version checks.
  Build against the oldest supported release and the newest one in CI.
- An addon built against an older 5.x should load in a newer 5.x.
  So a package built in the Debian 13 container (5.1.12) can be installed on the host.
  If the host ever moves to a new soname, the package dependency check refuses the install instead of failing at run time.

## Daemon

- **Recommended**: Write it in Rust. Decide after the spike.
  sherpa-onnx has an official Rust crate (`sherpa-onnx`, v1.13.8 on 2026-09-11), and whisper.cpp is available through `whisper-rs`.
  The community crate `sherpa-rs` was archived in 2026-06. Do not use it.
- **Recommended**: Return results with unicast signals or method replies, not broadcast signals.
- **Recommended**: Accept WAV files as input as well as the microphone, so tests and replay use the same code path.
- **Recommended**: Run one model at a time, chosen in the config. No parallel multi-model runs in the daemon.
- **Recommended**: Keep the model loaded in memory.
- **Recommended**: Measure latency with the CPU under load as well. This machine often runs builds. Done in M4: [m4-results.md](m4-results.md).
- **Open**: The D-Bus interface (methods and signals).

## Evaluation

- **Decided**: Evaluation is done only through a CLI. It is not connected to the IM.
  As a result, there is no candidate choice in the IM, no hiding of which model produced which output, no global evaluation mode and no "reset on restart".
  Partial results are not stored, because replay can produce them again.
- **Decided**: Data is the user's own voice, recorded with the real microphone in the real room.
  The set has 30 to 50 utterances:
  - a read script with technical terms, English words and numbers
  - free speech, which includes fillers
  - clips with only silence or background noise
- **Decided**: Reference text comes from the script, or is typed or corrected by the user.
- **Decided**: Store the data under `work/` in this repository, excluded from git by `.gitignore`, with mode 0700.
  Keep it until it is deleted by hand.
- **Decided**: Measure latency fairly by replaying saved audio through each model one at a time.
- **Decided** (2026-10-17): Strict evaluation is not needed now, because models can be changed later.
  Reference text for free-speech items is optional; check them only for clearly wrong Japanese.
  The evaluation scripts stay as developer tools and are not part of the packages.

### Metrics

- CER (character error rate) after normalization, with a 95% bootstrap interval.
  Normalization hides format errors, so format is measured separately on the raw output.
- Format: term accuracy, the share of English words written in Latin letters, fillers left in the output, and presence of punctuation.
- Latency: time to the first partial result, partial update interval, and final latency (p50 and p90).
- RTF (real-time factor) and resident memory.
- Any output on silence-only or noise-only clips.

### Pass criteria

**Decided**, set before measuring:

| Metric | Target |
|---|---|
| First partial result | Within 1 s of the start of speech |
| Partial update interval | 1 s or less |
| Final latency | p90 within 2 s after key release (under 1 s is ideal) |
| CER (normalized) | 7% or less |
| Term accuracy (with dictionary) | 90% or more |
| English words in Latin letters | 90% or more |
| Output on silence or noise only | 0 cases |
| Resident memory | 4 GB or less |

The 7% CER target is provisional.
It comes from the top models scoring 6 to 7% on JSUT in an external comparison.

**Decided**: A model that misses a target is not dropped.
For each model and metric, record:

- the measured value, the target, the gap and pass or fail
- notes on whether and how the gap could be closed
- test conditions: model version, quantization, thread count, runtime version, CPU load
- the raw outputs, so results can be scored again against new targets without running the models again

## Spike candidates

**Recommended** for the first spike, all on CPU:

| Role | Model | Runtime | Notes |
|---|---|---|---|
| Partial and final (fast) | parakeet-tdt_ctc-0.6b-ja | sherpa-onnx | CC-BY-4.0. Model card says it outputs punctuation |
| Partial and final (fast) | ReazonSpeech Zipformer (k2 v2) | sherpa-onnx | Apache-2.0 |
| Partial and final (fast) | SenseVoice Small | sherpa-onnx | FunASR model license. Punctuation and ITN option |
| Final only | kotoba-whisper v2.0 | whisper.cpp | Apache-2.0 |
| Final only | whisper large-v3-turbo | whisper.cpp | MIT (to verify) |
| Final only | Qwen3-ASR 0.6B and 1.7B | sherpa-onnx | Apache-2.0. Hotword support in sherpa-onnx |
| Final only | Cohere Transcribe (2B) | sherpa-onnx | Apache-2.0. About 2.1 GB as int8 |

Notes:

- An external comparison (HEROZ Tech Blog, 2026-08-18) measured on a discrete GPU.
  Its RTF numbers do not transfer to this CPU.
  Its VRAM column is not the size of an int8 model on CPU, so it is not a reason to drop parakeet-ja or Cohere Transcribe.
- Output format (numerals, fillers, English words) is unknown for almost every model. The spike checks it first.
- Only Qwen3-ASR has confirmed hotword support. For other models, the dictionary may have to rely on replacement after recognition.

## Plan

1. Done: write this document.
2. Done: record the evaluation data (42 of 43 items; s07 had no speech).
3. Done: spike summary in [spike-results.md](spike-results.md). Default model: parakeet-ja.
4. From here on, follow the milestones in [the overall design](superpowers/specs/2026-10-17-overall-design.md).
5. Done (2026-10-17): M1, the daemon and the CLI, merged into `main`. The microphone check with `koeinput listen` is still to be done by the user.
6. Done (2026-10-18): M2, the minimal fcitx5 addon, merged into `main`.
7. Done (2026-10-18): M3, the Debian packages. The host install and the host check are pending for the user. Current state: [status.md](status.md).
8. Implemented (2026-10-18): M4, quality, on branch `m4-quality`; the final review and the merge are pending. Measured defaults in [m4-results.md](m4-results.md).

## Debian packages

Decided for M3 on 2026-10-18 (details in [the M3 spec](superpowers/specs/2026-10-18-m3-debian-packaging-design.md)):

- **Decided**: Every build, for development and for packages, uses the upstream sherpa-onnx static archive without text to speech (`no-tts`, v1.13.8), pinned by size and SHA-256 in `packaging/sherpa-onnx.env`.
  The default archive links espeak-ng and ucd-tools (GPL-3.0-or-later) into both binaries, although KoeInput never uses text to speech. Binaries built from it could only be shipped under GPL terms.
  `scripts/check-no-gpl` fails the build if their symbols appear.
- **Decided**: The packages are amd64 only. The upstream archive exists only for x86_64 Linux; arm64 would need sherpa-onnx built from source, which is not planned.
- **Decided**: Accept the binary size for the first release. Both binaries embed ONNX Runtime (66 MiB installed, a 19 MB `.deb`), because `koeinput transcribe` runs the recognizer in its own process. Running `transcribe` through the daemon would halve the size; revisit if size matters.
- **Decided**: Packages build with the distribution's Rust (rustc 1.89 or newer). Debian 13 has 1.85, so the Debian 13 build takes `rustc` and `cargo` from trixie-backports. The development image keeps its own Rust.
- **Decided**: debhelper compat 13, which Debian 13 and testing both support. No dh-cargo, because our crates are not in Debian; the crates come from a vendor tarball, and the build runs offline.
- **Decided**: The copyright lines of each crate in `debian/copyright` come from the crate's license files. Cargo `authors` are used only when those files name no holder. `scripts/gen-copyright` generates the file.
- **Decided**: The daemon is not enabled at login. D-Bus starts it on demand through the systemd user unit, and the addon asks for it when fcitx5 loads the addon, so the model is loaded by the first dictation.
- **Decided** (2026-10-22): The Debian targets are trixie and forky. They are named by codename, because `testing` moves to the next release once forky is released. The forky packages are also the packages for sid, and the release workflow tests them on sid; sid's libraries are as new as testing's or newer.
  The trixie packages get the version `<version>~trixie1`, as the Ubuntu builds get `~<dist>1`. Before, the trixie and testing packages had the same version and file names, so the testing packages were easy to take for the Debian 13 ones. They cannot work on Debian 13: a build on testing links float functions of libm (`asinf`, `acosf` and others) at the symbol version `GLIBC_2.43`, and Debian 13 has glibc 2.41. So the `libc6 (>= 2.43)` dependency is correct, and the fix is the trixie build, not a looser dependency.

## Quality (M4)

Decided for M4 on 2026-10-18 (details in [the M4 spec](superpowers/specs/2026-10-18-m4-quality-design.md), numbers in [m4-results.md](m4-results.md)):

- **Decided** (replaced by [Sentence punctuation](#sentence-punctuation)): The final text comes from one decode of the whole utterance, from the first speech to the end of the recording, when that span is at most `final_full_max_ms`. Longer recordings join the segment texts as before. The default stays at 30000 (30 s), as measured on the user's recordings:
  - accuracy is the same: 256 character errors against 255 with the segment join, out of 920 reference characters (CER 27.8% and 27.7%);
  - for recordings up to 30 s, the `。` in the middle of a sentence is gone: 1 in the 25 read sentences before, 0 after. In free speech, the `。` inside the text went from 4 to 1, and that one is in the only recording longer than 30 s, which still uses the segment join;
  - it costs time: final p90 0.41 s instead of 0.02 s without load, 0.78 s with every CPU busy. With the 250 ms tail that is about 1 s, within the 2 s target. The decode time grows with the length (1.05 s for a 22 s recording under load), so a 60 s limit would likely miss the target under load; the limit stays at 30 s.
- **Decided**: `threads` stays at 8. It was the fastest without load and with all 24 hardware threads busy at nice 10: final p90 0.78 s against 1.17 s with 4 threads and 0.95 s with 16 (0.76, 1.23 and 0.81 s in a second run). 16 threads come close, 4 are clearly slower. The partial interval stayed at 0.5 s (p90 at most 0.52 s) with every setting.
- **Recommended**: Cohere Transcribe as the optional final-stage model (`koeinput model use cohere-transcribe --final`), off by default. It made 213 character errors against 256 (CER 23.2% against 27.8%), but its final p90 is 2.2 s without load and 3.9 s under load, above the 2 s target, and the process needs 4.4 GiB with both models (0.9 GiB with parakeet-ja alone), above the 4 GB target. The user decides whether the accuracy is worth the wait.
- **Decided**: The filler rule stays as it is. In the outputs it matched only twice, both えっと in free speech, at the start of a phrase. It removed nothing from the read sentences, and no reference contains a filler string.
- **Decided**: The dictionary gets `koeinput dict add/remove/list`, and the daemon reloads the file at the next session after it changed, so no restart is needed.
- **Decided**: The addon setting "Partial text" chooses between the app (the default), the fcitx5 panel, and off. A second setting lists apps that always use the panel. This answers the open item about the panel; which apps need the list comes from the host check.

## Sentence punctuation

Decided on 2026-10-27, after the M4 measurement showed that a whole-utterance decode leaves no `。` inside the text (numbers in [punctuation-results.md](punctuation-results.md)).
This replaces the M4 decision of one decode of the whole utterance.

- **Decided**: At the end of a recording, the VAD segments are grouped into chunks. A gap of at least `final_split_silence_ms` between two segments starts a new chunk. Each chunk is decoded again as one piece, by the final-stage model when one is set and otherwise by the default model. The model puts its own `。` or `？` at the end of a chunk. `final_split_silence_ms = 0` never splits, which gives the M4 behavior of one decode.
- **Decided**: `final_split_silence_ms` defaults to 800. On the user's recordings, 600 put more `。` in the middle of sentences, and 1000 missed real sentence ends that 800 found. Final p90 stays about 0.55 s without load. With the 250 ms tail, 800 under load is about 1.3 s at p90, inside the 2 s target. CER is the same in every mode (27.7% to 27.8%).
- **Decided**: `final_full_max_ms` is now a total budget (default 30 s) for the whole-chunk decodes in one utterance. The latest chunks use it first. A chunk that does not fit in what is left joins its VAD segment texts instead. In a long dictation the last 30 s or so get the better decode and the older part uses the segment texts. This keeps the final latency bounded.
- **Decided** (the user's choice): Every chunk end gets an end mark, including the end of the whole text. It is `？` after a polite question ending (`です` `ます` `ません` `ました` `でした` `でしょう` `ましょう`, each followed by `か`), and `。` otherwise. A chunk that already ends in `。` `．` `.` `？` `?` `！` `!` `、` or `,` gets nothing added. Short Latin-only text also gets `。`, so `git worktree` becomes `git worktree。`.
- **Decided**: A half-width `?` or `!` right after Japanese becomes `？` or `！`, and a `。` right after a polite question ending becomes `？`.
- **Decided**: A `、` directly after `です` `ます` `ました` `でした` `ません` or `ください` becomes `。`. Directly after a polite question ending it becomes `？`. This also applies to the partial text. `ますが、` `ですね、` and a half-width `,` stay as they are. In the free speech the model wrote `、` at real sentence ends after polite forms. All 3 cases this rule changed in free speech (with 800) are real sentence ends, and none appeared in the read sentences.
- **Decided**: `！` only comes from the model. No rule adds it.
- **Decided**: Post-processing (dictionary, fillers, punctuation rules) runs per chunk. A dictionary key that spans a long pause no longer matches.
- **Accepted**: A pause where the speaker stops to think can still put a `。` in the middle of a sentence. With 800 on the free speech, about 2 of the 13 `。` were of this kind. See [followups.md](followups.md).

## License and hosting

- **Decided** (2026-10-18): The project license is `MIT OR Apache-2.0`.
- **Decided** (2026-10-18): The repository goes to a private GitHub repository, `sugi/koeinput`, with GitHub Actions for CI.
  Making it public is the user's decision.

## Open items

None that block work. Items marked Open above are decided in the milestone that needs them.
