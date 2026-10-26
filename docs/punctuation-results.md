# Sentence punctuation measurement results

Date: 2026-10-27. Raw outputs are in `work/results/punct/` and `work/results/punct-rule/` (not in git). This file has aggregate numbers only.

The measurement checks the change that keeps sentence breaks in the final text: the chunked final decode at longer pauses, the end marks and the punctuation rules.
It sets the default pause length. The decisions are in [decisions.md](decisions.md#sentence-punctuation).
The earlier measurement of the whole-utterance decode is in [m4-results.md](m4-results.md).

## Setup

- Machine and data: the same as in [m4-results.md](m4-results.md#setup): Intel Core i9-12900T, 24 hardware threads, CPU only. 25 read sentences with references, 12 free-speech clips, 5 noise clips.
- Program: release build of `koeinput transcribe` from branch `sentence-punctuation`, run by `tools/measure-m4` in the dev container.
- Models: parakeet-ja (int8) and silero-vad. cohere-transcribe was not measured this time.
- Priority: the whole script ran at nice 10, so the recognizer ran at nice 10 and the load loops at nice 19 (the script adds 10, capped at 19). M4 ran the recognizer at nice 0 and the load at nice 10. The load runs here are therefore not the same as in M4: the recognizer had a lower priority than before, but the load had a still lower one.
- Three runs:
  - Run A: without load, final build (with the `、` rule), commit `517bbe2`. Modes: whole, split600, split800, split1000. Other processes used 1.6 to 2.6 CPUs on average.
  - Run B: without load, before the `、` rule (commit `938b63c`). The same modes. Other processes used 1.1 to 1.8 CPUs.
  - Run C: with load (24 busy loops), before the `、` rule (commit `938b63c`). Modes: whole under load and split800 under load. Other processes used 0.9 to 1.2 CPUs. The `、` rule is a string replacement and does not change latency, so Run C was not repeated with it.
- Modes: `whole` sets `final_split_silence_ms = 0`, one decode of the whole utterance as M4 measured it, but with the new end mark and question rules. `splitN` sets it to N ms. `threads = 8` everywhere.
- Tool: `tools/measure-m4` (see [How to run it again](#how-to-run-it-again)).

## Run A: final build, without load

| Mode | CER (95% interval) | `。` inside read sentences | Mid-sentence `。` in read sentences | Sentence ends found in read sentences | Final p50 | Final p90 | Final max | `。` inside free speech | `？` inside free speech |
|---|---|---|---|---|---|---|---|---|---|
| whole (no split) | 27.8% (21–34) | 0 | 0 (0 of 25 items) | 0 of 3 | 0.21 s | 0.48 s | 0.75 s | 5 | 1 |
| split600 | 27.8% (21–34) | 3 | 1 (1 of 25 items) | 2 of 3 | 0.21 s | 0.53 s | 0.68 s | 16 | 0 |
| split800 (default) | 27.7% (21–34) | 1 | 1 (1 of 25 items) | 0 of 3 | 0.19 s | 0.55 s | 0.70 s | 13 | 0 |
| split1000 | 27.8% (21–34) | 0 | 0 (0 of 25 items) | 0 of 3 | 0.19 s | 0.43 s | 0.67 s | 11 | 0 |

- In every mode, 2 of the 5 noise clips gave text, the peak memory was 0.9 GiB, and the partial interval was 0.50 s at p50 and p90.
- The end mark and the question rules are also on in `whole`, which is why it has 5 `。` inside free speech where M4 had 1.

## Run B: before the `、` rule, without load

| Mode | `。` inside free speech | Sentence ends found in read sentences | Final p90 |
|---|---|---|---|
| whole | 1 | 0 of 3 | 0.45 s |
| split600 | 14 | 1 of 3 | 0.48 s |
| split800 | 10 | 0 of 3 | 0.52 s |
| split1000 | 7 | 0 of 3 | 0.46 s |

Compared with Run A, the `、` rule added 4 `。` to whole, 2 to split600, 3 to split800 and 4 to split1000 in free speech.
The final p90 differs by up to 0.05 s between the two runs for the same mode. This is run-to-run noise: other processes used more CPU during Run A.

## Run C: with load

24 busy loops, threads 8.

| Mode | Final p50 | Final p90 | Final max | Partial interval p90 |
|---|---|---|---|---|
| whole | 0.39 s | 0.83 s | 1.28 s | 0.50 s |
| split800 | 0.40 s | 1.04 s | 1.49 s | 0.50 s |

With the 250 ms tail the daemon adds before the final decode, split800 under load is about 1.3 s at p90, inside the 2 s target from M4.

## Manual check of the free speech

The final texts of the modes were read side by side (not quoted here; counts only).

- Before the `、` rule, split600 put about 5 `。` at real sentence ends and about 4 in the middle of a sentence. split800 had about 4 at real ends and about 2 in the middle, plus some after a short filler word. split1000 had fewer of both, and it missed real sentence ends that split800 found.
- The `、` rule added 3 `。` to split800 in free speech. All 3 are at real sentence ends.
- split800 with the rule has 13 `。` inside free speech: about 7 at real sentence ends, 2 in the middle of a sentence (the speaker paused to find a word), 2 after a short filler word, and 2 unclear.
- The `。` in the middle of a sentence come from pauses where the speaker stops to think.
- In the M4 measurement (one decode of the whole utterance), the free speech had 1 `。` inside the text in total.

## What this means

- The split puts sentence breaks in the free speech: 13 `。` inside the text with 800 ms instead of 1 or 5 without the split, about 7 of them at real sentence ends.
- 800 ms is the default. 600 ms puts more `。` in the middle of sentences. 1000 ms misses real sentence ends that 800 ms found. The accuracy is the same in all modes (CER 27.7% to 27.8%).
- The latency stays within the target: final p90 about 0.55 s without load, and about 1.3 s with the tail under load.
- The `、` rule turns the `、` that the model writes at a real sentence end after a polite form into `。`. All 3 cases it changed in split800 were sentence ends.

## Limits

- The read sentences were read in one breath, so they have almost no pauses. With 800 ms the split found 0 of their 3 sentence ends, and only 600 ms found 2. The gain shows in the free speech.
- The free speech has no references. The counts of `。` at real sentence ends come from one reader and are approximate; 2 of the 13 were unclear.
- A pause where the speaker stops to think can still put a `。` in the middle of a sentence ([followups.md](followups.md)).
- The three runs differ in the build and the load on the machine (other processes used 0.9 to 2.6 CPUs). Run C ran before the `、` rule. Each mode ran once, so differences of a few hundredths of a second at p90 are noise.
- All runs used nice 10 for the recognizer, unlike M4, so the load runs are not directly comparable to the load runs in [m4-results.md](m4-results.md). The whole-versus-split800 comparison in Run C is valid, because both ran the same way.
- The CPU frequency and temperature were not controlled.
- cohere-transcribe was not measured with the split.

## How to run it again

```sh
scripts/dev cargo build --release --workspace
tools/measure-m4 --modes whole,split600,split800,split1000,load-t8,load-split800
```

Modes `split600`, `split800` and `split1000` run without load, and `load-split800` runs under load; all use 8 threads. The older modes pin `final_split_silence_ms = 0`.
Run it in the outer dev container, not through `scripts/dev`. To run it at nice 10 as here, start it with `nice -n 10 tools/measure-m4 ...`.
The script also writes `compare.md` with the final text of every mode. It holds recognized text and stays private.
