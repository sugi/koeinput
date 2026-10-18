# M4 measurement results

Date: 2026-10-18. Raw outputs are in `work/results/m4/` (not in git). This file has aggregate numbers only.

The measurement checks the M4 changes to the final text and sets the defaults from the results.
The decisions are in [decisions.md](decisions.md#quality-m4).

## Setup

- Machine: Intel Core i9-12900T, 24 hardware threads, CPU only.
- Program: release build of `koeinput transcribe` from branch `m4-quality` (on `cc4522e`), run directly in the dev container at normal priority. It runs the same session code as the daemon.
- Models: parakeet-ja (int8) for the partial and final text, silero-vad, and cohere-transcribe (int8) as the optional final-stage model.
- Data: the user's own recordings from the M0 spike.
  - 25 read sentences with references: CER and the `。` count.
  - 12 free-speech clips without references: latency, and the `。` inside the text.
  - 5 noise clips.
  - The read sentences are 3.6 to 18 s long. The free-speech clips are 4.2 to 36.7 s long; one is longer than 30 s.
- Load: 24 busy loops (one per hardware thread) at nice 10, started and stopped by the script. This stands in for the user's builds, which run at nice 10. The recognizer runs at nice 0.
- Other processes used 0.6 to 1.5 CPUs on average during each run.
- Tool: `tools/measure-m4` (see [How to run it again](#how-to-run-it-again)).

## Final text

Without load, `threads = 8`. The second number in the p90 column is from a second run.

| Final text from | CER (95% interval) | Character errors | Mid-sentence `。` in read sentences | `。` inside free speech | Final p50 | Final p90 | Final max |
|---|---|---|---|---|---|---|---|
| Segment join (`final_full_max_ms = 0`, as before M4) | 27.7% (21–34) | 255 | 1 (1 of 25 items) | 4 | 0.00 s | 0.02 s | 0.16 s |
| Whole utterance (default) | 27.8% (21–34) | 256 | 0 | 1 | 0.19 s | 0.41 s, 0.44 s | 0.64 s |
| Whole utterance, cohere-transcribe as final-stage model | 23.2% (16–30) | 213 | 0 | 6 | 1.10 s | 2.18 s, 2.23 s | 2.85 s |

- Character errors are counted over 920 reference characters.
- Whole utterance against segment join, item by item: 4 read sentences got better, 5 got worse, and 16 stayed the same. The difference in total is one character.
- In free speech, the whole-utterance decode left one `。` inside the text, in the only clip longer than 30 s. That clip still uses the segment join.
- The read sentences have 3 sentence ends inside the text. parakeet-ja puts a `。` at none of them, with either method. cohere-transcribe puts a `。` at all 3 and nowhere else.
- Peak memory of the process: 0.9 GiB with parakeet-ja, 4.4 GiB with cohere-transcribe loaded as well.
- 2 of the 5 noise clips gave text in every mode, one of them the clip with a spoken remark. The spike's VAD-based decode also gave 2; decoding the whole file there gave 4 with parakeet-ja and 5 with cohere-transcribe. The whole-utterance decode starts at the first speech the VAD finds, so it keeps the VAD's behavior on noise.

## Threads and CPU load

Whole-utterance final, parakeet-ja. Under load, two runs.

| threads | Final p90, no load | Final p90 under load | Final p50 under load | Final max under load | Partial interval p90 under load |
|---|---|---|---|---|---|
| 4 | 0.58 s | 1.17 s, 1.23 s | 0.52 s, 0.45 s | 1.45 s, 1.49 s | 0.52 s, 0.50 s |
| 8 (default) | 0.41 s, 0.44 s | 0.78 s, 0.76 s | 0.32 s, 0.32 s | 1.05 s, 1.25 s | 0.50 s, 0.50 s |
| 16 | 0.45 s | 0.95 s, 0.81 s | 0.42 s, 0.43 s | 1.47 s, 2.70 s | 0.50 s, 0.50 s |

With cohere-transcribe as the final-stage model and `threads = 8`, under load: final p50 2.45 s, p90 3.94 s, max 4.68 s.

The partial interval p50 was 0.50 s in every run. Changing `threads` did not change any output text.

## Fillers

The filler rule matched only twice in the parakeet-ja outputs, both えっと in free speech, at the start of a phrase (right after punctuation or a space). It matched nothing in the read sentences, and no reference contains a filler string. With cohere-transcribe it matched once, also えっと at the start of a phrase. No removal inside a word was found, so the rule stays as it is.

## What this means

- The whole-utterance final keeps the accuracy, removes the `。` at pause points for recordings up to 30 s, and costs about 0.4 s at p90 without load and 0.8 s with every CPU busy. With the 250 ms tail that is about 1 s, within the 2 s target. `final_full_max_ms` stays at 30000.
- `threads = 8` is the fastest setting both without and with load. It stays the default.
- cohere-transcribe makes about one sixth fewer character errors, but its final text takes 2.2 s at p90 without load and 3.9 s under load, which misses the 2 s target, and it needs about 3.5 GiB more memory, which misses the 4 GB target. It stays optional and off by default.
- The partial text keeps its 0.5 s interval under load with every setting, within the 1 s target.

## Limits

- `koeinput transcribe` feeds the audio faster than real time, so the partial decodes run back to back. That is a harder case for the load than the daemon, which decodes once every 0.5 s. The partial interval here is max(0.5 s, decode time per tick), as in the spike.
- The final latency here is the time after the end of the audio. The daemon records 250 ms more (the tail) before the final decode.
- The read sentences are short and read in one breath, so they have few pauses: the mid-sentence `。` count is small with either method. The free speech, where pauses are common, has no references, so its `。` count shows only how many there are, not whether they are wrong.
- The load runs were done twice, the default mode and cohere-transcribe without load twice, the rest once. 4 threads are clearly slower under load. 8 threads were the fastest in both load runs, but the gap to 16 threads (0.05 s and 0.17 s at p90) is about as large as the difference between the two runs with 16 threads (0.14 s).
- The CPU frequency and temperature were not controlled.

## How to run it again

```sh
scripts/dev cargo build --release --workspace
HOME=$PWD/work/home KOEINPUT_DATA_DIR=$PWD/data target/release/koeinput model install cohere-transcribe --from-file work/models/_dl/sherpa-onnx-cohere-transcribe-14-lang-int8-2026-04-01.tar.bz2
tools/measure-m4
```

Run it in the outer dev container, not through `scripts/dev`, so that the recognizer runs at normal priority.
`tools/measure-m4 --modes whole,load-t8` runs only some modes, and `--score-only` scores the saved outputs again. The script writes `summary.md` with aggregates only and keeps the raw outputs, which contain recognized text, in `work/results/m4/` with mode 0700. It skips the cohere modes when that model is not installed.
