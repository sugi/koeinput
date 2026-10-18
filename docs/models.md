# Models

KoeInput downloads speech models from the k2-fsa/sherpa-onnx `asr-models` release page; it does not ship them.
`data/models.toml` is the catalog (id, URL, SHA-256, size, license). `koeinput model list` prints the licenses.
The licenses below are the ones the upstream model pages state. They apply to the models, not to KoeInput.

The archives are published by the sherpa-onnx project (k2-fsa) at
<https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models>.
`zipformer-ja` (the ONNX files come from reazon-research/reazonspeech-k2-v2) and `silero-vad` (snakers4/silero-vad publishes `silero_vad.onnx`) are republished unchanged.
The other models were converted by sherpa-onnx (see its export scripts); the `int8` ones are also quantized by it.

| Id | Model | Author | License | Upstream |
|---|---|---|---|---|
| `parakeet-ja` (default) | Parakeet TDT-CTC 0.6B (ja) | NVIDIA (NeMo team) | CC-BY-4.0 | <https://huggingface.co/nvidia/parakeet-tdt_ctc-0.6b-ja> |
| `zipformer-ja` | ReazonSpeech k2-v2 (Zipformer) | Reazon Human Interaction Lab | Apache-2.0 | <https://huggingface.co/reazon-research/reazonspeech-k2-v2> |
| `sensevoice-2024` | SenseVoiceSmall | FunAudioLLM (Alibaba, FunASR) | FunASR Model License ("model-license") | <https://huggingface.co/FunAudioLLM/SenseVoiceSmall> |
| `qwen3-asr-0.6b` | Qwen3-ASR-0.6B | Qwen team (Alibaba) | Apache-2.0 | <https://huggingface.co/Qwen/Qwen3-ASR-0.6B> |
| `cohere-transcribe` | Cohere Transcribe 03-2026 | Cohere Labs | Apache-2.0 | <https://huggingface.co/CohereLabs/cohere-transcribe-03-2026> |
| `silero-vad` | Silero VAD | Silero Team | MIT | <https://github.com/snakers4/silero-vad> |

## Attribution: Parakeet TDT-CTC 0.6B Japanese

The default model is licensed under [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/), which requires this attribution.

- Title: Parakeet TDT-CTC 0.6B (ja), `parakeet-tdt_ctc-0.6b-ja`
- Author: NVIDIA (NeMo team)
- Source: <https://huggingface.co/nvidia/parakeet-tdt_ctc-0.6b-ja>
- License: CC-BY-4.0, <https://creativecommons.org/licenses/by/4.0/>
- Changes: the sherpa-onnx project (<https://github.com/k2-fsa/sherpa-onnx>) exported only the CTC decoder branch of the model to ONNX (the TDT decoder is not included), quantized it to int8 (dynamic QUInt8) and replaced the ONNX metadata
  (script: `scripts/nemo/parakeet-tdt_ctc-0.6b-ja/export-onnx-ctc.py`).
  KoeInput makes no further change to the model.

Keep this notice when you redistribute the model. NVIDIA does not endorse KoeInput.

## SenseVoice

The Hugging Face page names the license "model-license", the FunASR Model Open Source License Agreement
(<https://github.com/modelscope/FunASR/blob/main/MODEL_LICENSE>).
In short, you may use, copy, modify and share the model if you credit the source and author and keep the model name; this also applies to derived models.
The license also ends if you breach it, can be withdrawn for denigrating the software, and can be revised by Alibaba.
Read the agreement itself before you redistribute the model.
The credit is: SenseVoiceSmall by FunAudioLLM (Alibaba), <https://huggingface.co/FunAudioLLM/SenseVoiceSmall>.

## Apache-2.0 and MIT models

ReazonSpeech k2-v2, Qwen3-ASR and Cohere Transcribe are under the
[Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0); Silero VAD is under the MIT license.
Keep the license text and copyright notice of the upstream project when you redistribute them.
The pages above hold the license files.
