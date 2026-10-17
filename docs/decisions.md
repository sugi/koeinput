# Japanese voice input for fcitx5: decisions so far

Last updated: 2026-10-17.

Each item has one of these labels:

- **Decided**: agreed with the user.
- **Recommended**: proposed, not final.
- **To verify**: not yet confirmed. Do not treat it as fact.
- **Open**: not discussed enough to decide.

## Goal

Dictate Japanese text straight into the focused text field on Linux, while Mozc stays active.

The main use is technical text, such as instructions to AI agents.
This text has many technical terms and English words.

The user tried Speech Note first.
Copy and paste from it did not work reliably, and even when it works, the extra step is tedious.
So the core value of this project is to commit text through the input method, with no copy and paste.

## Scope and constraints

- **Decided**: Local only. Nothing is sent to cloud speech recognition or cloud LLMs.
- **Decided**: Japanese first.
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
- Ask PipeWire for 16 kHz mono 16-bit audio and let it resample.
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
- Overwriting Mozc's preedit without committing Mozc's composition first.

## Hardware and environment

Measured on 2026-10-17:

- CPU: Intel Core i9-12900T (Alder Lake, 8 P-cores + 8 E-cores, 24 threads, AVX2, AVX-VNNI, no AVX-512). No NPU.
- GPU: Intel UHD 770 (32 EU). It is small, and the dev container has no `/dev/dri`. The first round of evaluation uses the CPU only.
- RAM: 125 GiB. Desktop machine, no battery. Memory is not the real limit; CPU speed is.
- Default microphone: DJI Wireless Mic Rx (USB).
- Host desktop: KDE Plasma on Wayland, fcitx5 with Mozc.
  Apps connected to fcitx5: WezTerm, Chrome, Firefox, Emacs, Discord.
- Dev container: Debian 13. The host runs a newer Debian (kernel `7.2.9+deb14`).
  The addon must be built against the host's fcitx5 version.
  A daemon built in the container should run on the host, because glibc is backward compatible.

## Recording behavior

- **Decided**: Do not keep the microphone open all the time. Open it when the hotkey is pressed.
- **Decided**: Share the microphone with other apps through PipeWire, so video meetings keep working.
  Avoid Bluetooth headset microphones, which can switch the headset to a low-quality call profile.
- **Decided**: Show visual feedback while recording.
  Switch to "recording" when the first audio data arrives, not when the key is pressed.
  The device takes time to wake up, and the user should start speaking only after the indicator changes.
- **Decided**: If Mozc is composing text when the hotkey is pressed, commit that text first.
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
- **To verify**: How another addon can make Mozc commit its composition.
  If the fcitx5 API cannot do it, a workaround such as sending a key is needed.
- **To verify**: Key release events arrive reliably through KWin on Wayland.
- **Open**: Which hotkey. A modifier-only key such as Right Alt, or 無変換 / 変換, which Mozc also uses.
- **Open**: Where to show partial results: inline preedit, or the fcitx5 panel.
  Terminals and Emacs handle inline preedit differently.
- **Open**: Cancel with Esc, a maximum recording length, and what to show when the daemon does not respond.
- **Open**: Undo of the last commit. It depends on app support for surrounding text.

## Daemon

- **Recommended**: Write it in Rust. Decide after the spike.
  sherpa-onnx has an official Rust crate (`sherpa-onnx`, v1.13.8 on 2026-09-11), and whisper.cpp is available through `whisper-rs`.
  The community crate `sherpa-rs` was archived in 2026-06. Do not use it.
- **Recommended**: Return results with unicast signals or method replies, not broadcast signals.
- **Recommended**: Accept WAV files as input as well as the microphone, so tests and replay use the same code path.
- **Recommended**: Run one model at a time, chosen in the config. No parallel multi-model runs in the daemon.
- **Recommended**: Keep the model loaded in memory.
- **Recommended**: Measure latency with the CPU under load as well. This machine often runs builds.
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
2. Record the evaluation data.
3. Spike: measure the candidates on CPU.
   Output: a results table and a recommendation for the daemon's language, runtime and model setup (one model or two).
4. Write the spec for the daemon and the CLI, including the evaluation CLI (record, correct, replay).
5. Build the minimal fcitx5 addon: hotkey, then commit.
   Before that, verify how to make Mozc commit and that key release events arrive.
6. Add partial results, the dictionary and the remaining details.

## Open items

- Project name. Working name: `voice-flood` (the directory name). Other ideas: tincan, cyrano. `voinput` is too close to vinput.
- License of this project.
- Final choice of the daemon language (after the spike).
- The D-Bus interface.
- The hotkey.
