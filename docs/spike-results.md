# Spike results: Japanese recognition models on CPU (M0)

Date: 2026-10-17. Run `real1`. Raw outputs and full tables are in `work/results/real1/` (not in git).

The user asked for a moderate evaluation, not a strict one. This is a summary of one run, not a final verdict.

## Setup

- Machine: Intel Core i9-12900T, CPU only, `nice -n 10`, no other load.
- Threads per model: the fastest in a short sweep (8 for most ONNX models, 16 for SenseVoice, 24 for whisper.cpp).
- Runtimes: sherpa-onnx 1.13.8 (Python), whisper.cpp v1.9.5 (`whisper-cli`, one process per call).
- Data: the user's own voice, DJI wireless microphone, 16 kHz mono.
  - 25 read sentences full of technical terms and English words. CER uses only these.
  - 12 free-speech clips. They have no reference text, so they count only for latency.
  - 5 "no speech" clips. Two are not clean: n01 contains a spoken remark, and n05 has music with lyrics. The noise counts below are therefore not reliable.
- Dictionary variants (`+dict`) get the test sentences' own terms as hotwords or as the Whisper prompt. This is optimistic.

## Results

CER is normalized (punctuation, spaces and case removed; numerals not converted).
"Final p90" is the time to decode the whole utterance after it ends. "VAD p90" is the time to decode only the last VAD segment, the rest being decoded while the user speaks.

| Model | CER (95% interval) | Terms | English in Latin | Numerals | Punctuation | Final p90 | VAD p90 | RTF | Memory |
|---|---|---|---|---|---|---|---|---|---|
| parakeet-ja | 27.5% (21–33) | 33% | 22% | 64% | 39% | 0.53 s | 0.21 s | 0.03 | 1.2 GB |
| Zipformer | 34.6% (27–41) | 26% | 16% | 29% | 0% | 0.30 s | 0.12 s | 0.02 | 1.2 GB |
| SenseVoice 2024 | 32.2% (25–39) | 15% | 0% | 64% | 91% | 0.24 s | 0.09 s | 0.02 | 0.5 GB |
| SenseVoice 2025 | 88.0% (broken) | 2% | 3% | 29% | 0% | 0.24 s | 0.09 s | 0.01 | 0.5 GB |
| Qwen3-ASR 0.6B | 32.3% (26–38) | 30% | 22% | 43% | 100% | 2.85 s | 1.47 s | 0.17 | 3.1 GB |
| Qwen3-ASR 1.7B | 28.4% (22–34) | 33% | 22% | 79% | 97% | 5.32 s | 2.94 s | 0.33 | 4.7 GB |
| Cohere Transcribe | 20.9% (16–26) | 37% | 24% | 93% | 82% | 2.71 s | 1.12 s | 0.16 | 3.8 GB |
| kotoba-whisper v2.0 | 27.1% (21–32) | 28% | 16% | 79% | 3% | 5.57 s | 5.45 s | 0.58 | 0.8 GB |
| large-v3-turbo | 19.9% (14–25) | 43% | 32% | 86% | 15% | 5.82 s | 5.54 s | 0.60 | 0.8 GB |
| large-v3-turbo + prompt | 11.1% (6–16) | 61% | 51% | 100% | 15% | 6.06 s | 5.67 s | 0.62 | 0.9 GB |
| Qwen3-ASR 1.7B + hotwords | 75.3% (17–167) | 61% | 57% | 79% | 94% | 7.17 s | 3.66 s | 0.46 | 4.9 GB |

"Terms" and "English in Latin" are case-insensitive. "Punctuation" is the share of longer outputs with 、 or 。.
Other variants (`@ac`, kotoba + prompt, Qwen3-ASR 0.6B + hotwords) were worse; see the full table.

Partial results (parakeet-ja, decoding the growing buffer every 0.5 s): first partial p50 0.54 s, p90 1.05 s; update interval 0.5 s.
The partial text is rewritten a lot (about 200 characters per utterance), so the display will flicker.

Against the pass criteria, no model meets the CER or term targets.
As agreed, no model is dropped; the gaps are recorded here and in `work/results/real1/summary.md`.

## Findings

1. **Technical terms and English words are the main problem for every model.** Most English words come out in katakana.
   Since the references write them in Latin letters, this also inflates CER.
   A user dictionary that replaces known misrecognitions is the most direct fix, and it works with any model.
2. **Recognizer-side biasing helps terms but can break the output.** Whisper's prompt worked well (CER 11%, terms 61%).
   Qwen3-ASR hotwords sometimes made the model loop or overflow its prompt limit, which explains the huge interval.
   kotoba-whisper with a prompt produced almost no usable text.
3. **Whisper models are too slow on this CPU**: about 5.5 to 6 s per utterance, even for short ones.
   The reduced encoder window (`@ac`) cut the time but hurt accuracy and caused repetitions.
4. **parakeet-ja is the best fit for partial results and the default model.** It is fast (final p90 0.5 s, last-segment p90 0.2 s), uses 1.2 GB and keeps sentences complete. Its weak points are punctuation and English.
5. **Cohere Transcribe is the best candidate for an optional final-stage model.** It has the best CER among the models fast enough to be close to the target, good numerals and good punctuation.
   Decoding only the last segment takes 1.1 s at p90, but segment-wise decoding raised its CER from 21% to 28%.
6. **SenseVoice 2025-09-09 is broken for Japanese** in sherpa-onnx 1.13.8. SenseVoice 2024 inserts spaces and rewrites partial text a lot.

## Decisions for M1

- **Recommended**: Default model parakeet-ja (int8) through sherpa-onnx.
- **Recommended**: Keep the user dictionary (replacement after recognition) in the first release. It targets the biggest gap.
- **Recommended**: Revisit Cohere Transcribe as a final-stage model, and recognizer-side biasing, in M4.
- whisper.cpp stays out of the first release.
