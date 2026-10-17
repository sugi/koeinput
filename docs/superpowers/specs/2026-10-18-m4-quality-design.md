# M4: Quality

Status: approved for implementation, 2026-10-18 (the user asked for no review stop).

Part of [the overall design](2026-10-17-overall-design.md). Background decisions are in [decisions.md](../../decisions.md); open review findings are in [followups.md](../../followups.md).

## Goal

Fix what is known to hurt daily use before the user reports more.
The overall design says "the user's daily pain points are fixed"; the user has not used the addon on the host yet, so M4 covers the known problems, and items from the user's host check are added when they arrive.

Done when:

- The final text no longer has a `。` in the middle of a sentence at a pause, and long dictations are not split mid-word, measured on the user's recordings.
- An optional, more accurate final-stage model can be turned on with one command.
- The user can add dictionary entries with one command, and the daemon uses them without a restart.
- The addon can show partial text in the panel instead of the app, globally or per app.
- Latency under CPU load is measured and the defaults are set from the measurement.
- The actionable items in `docs/followups.md` (M4 and M2 lists) are fixed or explicitly kept with a reason.

Out of scope: hotword biasing in the recognizer, LLM post-processing, undo, voice commands, GPU or NPU inference, languages other than Japanese.

## 1. Final text from the whole utterance

Today the final text joins the texts of the VAD segments, each decoded on its own.
Parakeet puts `。` at the end of every segment, so a pause in the middle of a sentence gives `…。…`, and the decoder never sees the context across a pause.

**Decided**: the final decode runs once over the whole utterance, from the start of the first speech segment to the end of the recording (including the tail), when that span is at most `final_full_max_ms` (default 30000).
Longer recordings fall back to the current segment join. Partial results stay as they are (fixed segments plus the current one).

- Config key `final_full_max_ms` (0 turns the whole-utterance decode off). Range 0 to 120000.
- With the default model, a 30 s span decodes in about 1 s on this CPU (RTF 0.03), which keeps the final latency within the 2 s target.
- If the measurement in section 7 shows a worse CER or a final p90 above 2 s for the default model, the default changes accordingly and the reason goes into decisions.md.
- Silero's 20 s maximum speech length then only affects partial results and the fallback; it stays.

## 2. Optional final-stage model

**Decided**: a second, more accurate model can decode the final text, while the default model keeps producing partial results.

- Config key `final_model` (a catalog id, empty by default). The daemon loads it next to the main model; `Ready` is true when both are loaded.
- `koeinput model use ID --final` sets it (and installs nothing); `koeinput model use --final none` clears it.
- Two read-only D-Bus properties are added (backward compatible): `FinalModel` (s, the configured id or empty) and `FinalModelReady` (b). `koeinput status` shows both models.
- The final-stage model decodes the whole utterance (section 1). Over the section 1 limit, the main model's segment join is used, because decoding every segment again with the slower model would delay the final text by several seconds.
- The spike's best candidate is Cohere Transcribe (`cohere-transcribe`, CER 21% vs 28%, final p90 about 2.7 s, 3.8 GB). It is documented as the recommended choice; the user decides.
- If the final-stage model fails to load, the daemon logs it and works with the main model only (`Ready` is true, `FinalModelReady` is false, and `status` points to the journal).

## 3. User dictionary from the command line

- `koeinput dict add WRONG RIGHT`, `koeinput dict remove WRONG`, `koeinput dict list` edit `~/.config/koeinput/dictionary.tsv` (written through a temporary file and rename, keeping comments and order; duplicates replace the old entry).
- The daemon reloads the dictionary when the file's modification time or size changed, checked once at the start of each session (one `stat`, no watcher thread).
- `koeinput transcribe` uses the same dictionary (it already loads it).

## 4. Partial text in the panel

The addon gets a "Partial text" setting: "In the app" (client preedit, the current behavior), "In the panel" (panel preedit), or "Off".
A second setting, "Apps that show partial text in the panel", is a list of program names (matched against `InputContext::program()`, case-insensitive); for those apps the panel is used whatever the first setting says.
Defaults: "In the app", empty list. The host check tells which apps need the list (text-input-v3 clients such as Chrome and Discord may commit the composition on blur).
The setting replaces M2's "Show partial results" boolean (old configs: true → "In the app", false → "Off").

## 5. Addon follow-ups from M2

- `ISO_Level3_Shift` and `Mode_switch` count as modifier keys for the "lone modifier does not cancel" rule and for the repeat rule.
- After a lost release of a modifier hotkey, a press of any other key clears the stored keycode and keysym (so that key's release passes).
- Two configured hotkeys held together: the addon remembers the keycode of the press that started the session. A press of another configured hotkey while that key is down is part of a chord: it is swallowed with its release and does nothing.
- The test helper `DaemonProcess` kills its child on a ready-timeout abort, writes to the log only with async-signal-safe calls between fork and exec, and removes its temporary directories.

Kept with a reason (recorded in followups.md): rolling a bare key into a combination hotkey (rare, needs key-state tracking of every key), keycode-0 frontends (none known on the host), the input method switch during a session (fcitx5 handles that key first; the session keeps running, which is acceptable), `NO_AUTO_START` for `Stop`/`Cancel` (fcitx5's D-Bus API has no portable way to set it; the window is milliseconds).

## 6. Daemon and CLI follow-ups

- A `Start` that arrives while the daemon shuts down after a bus loss is refused (the shared state is marked as shutting down under the same lock as the cancel).
- `koeinput model use` errors name the config file; `set_model` on a dangling symlink writes to the symlink's target path, not over the link.
- `koeinput transcribe` reports a failed file with its path exactly once (the error carries whether it already names the path, instead of a substring check).
- The model download stall (no idle timeout in ureq 3) stays open: Ctrl-C works and the install is atomic.

## 7. Measurement

On the user's own recordings in `work/eval-data/` (never quoted, never committed; only aggregate numbers go into docs):

- CER (normalized, as in the spike) and final latency p50/p90 for: segment join (today), whole-utterance final (section 1), and whole-utterance final with `cohere-transcribe` as final-stage model.
- The count of mid-sentence `。` at pause points (a `。` followed by more text from the same sentence, judged by the reference text).
- Final latency p90 and the partial update interval with the CPU loaded: all 24 hardware threads busy with a `nice -n 10` background load (the user's usual build load), for `threads` = 4, 8 and 16. The default `threads` is set to the best value under load if it differs from 8.
- Fillers: the filler list is checked against the outputs; any filler removal that breaks a word in the outputs is fixed in the rules (with a unit test from a made-up sentence, not from the user's text).

Results go to `docs/m4-results.md` (aggregates only).

## Testing

- Session tests for the whole-utterance final (span limit, fallback, the final-stage recognizer used only for the final, errors from either recognizer).
- Engine and daemon tests for a second recognizer (fake), its load failure, and `Ready`.
- CLI tests for `dict add/remove/list` (comments and order kept, duplicates replaced) and `model use --final`.
- A daemon test that changes the dictionary between two sessions and sees the new replacement in the second.
- Addon tests for the partial-text setting (each mode, the per-app list, the migration of the old boolean) and the M2 follow-ups.
- Existing suites stay green in both images.
