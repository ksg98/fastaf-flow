# Model guide

Start with **Qwen3-ASR 0.6B 4-bit** and **S1-mini by Superwhisper 4-bit**. This pair uses approximately 2.8 GB including the app's estimated inference overhead. Leave additional memory for macOS and other apps. The app selects an already downloaded, suitable pair automatically when possible.

## Downloaded models

The Dictate dropdowns show local models. Use Models to browse downloads, or Add folder to scan a model directory or a parent directory. Rescan local picks up changes made by other applications. There is no model deletion command in v1, so shared caches are never removed by the app.

Supported identification includes Whisper, Parakeet, Qwen3-ASR, Moonshine, Voxtral, SenseVoice, VibeVoice ASR, and other MLX Audio ASR configurations. Actual runtime support depends on MLX Audio 0.5.3 and the checkpoint's configuration, tokenizer, and auxiliary files. Unsupported checkpoints produce an error without replacing the transcript. A model's appearance in the library is not a compatibility certification.

Moonshine Tiny and Base are listed under **Models** as `moonshine-ai/moonshine-tiny` and `moonshine-ai/moonshine-base`. MLX Audio loads these original FP32 safetensors checkpoints directly; they do not need an MLX conversion or MLX tag. Their weights are approximately 108 MB and 246 MB, respectively. These two checkpoints support English. The separate Moonshine streaming architecture is not supported by the pinned MLX Audio runtime. Sources: [MLX Audio's Moonshine implementation](https://github.com/Blaizzy/mlx-audio/tree/main/mlx_audio/stt/models/moonshine), [Tiny](https://huggingface.co/moonshine-ai/moonshine-tiny), [Base](https://huggingface.co/moonshine-ai/moonshine-base).

Model cache scans inspect configs and check weight-file and shard availability. They do not hash multi-GB weights on each launch. Corrupted files can still fail when loaded; repair their download using the original model tooling.

## S1-mini by Superwhisper

| MLX variant | Approximate weights | App's estimated runtime memory |
| --- | --- | --- |
| `mlx-community/S1-mini-MLX-4bit` | 335 MB | 0.9 GB |
| `mlx-community/S1-mini-MLX-8bit` | about 0.6 GB | 1.3 GB |
| `mlx-community/S1-mini-MLX-bf16` | about 1.2 GB | 2.1 GB |

These are the three variants published by MLX Community at the v1 release. Refresh online uses the Hub's paginated model listing and can add newly published variants. The app does not invent nonexistent download URLs for 2-, 3-, or 6-bit weights.

Locally converted S1-mini directories can use other MLX-supported quantizations. Keep `s1-mini` in the directory name so the app can identify the model, and preserve the upstream model's attribution and license. The scanner reads quantization bits from `config.json`; include a tokenizer and all weight shards.

## Memory estimates

The estimate is `1.35 × weight size + overhead`, where overhead is 1 GB for ASR and 0.45 GB for rewriting. For undownloaded models, weight size is estimated from the family and quantization. Unknown families receive a conservative fallback estimate.

The recommendation budget is the smaller of 60% of physical memory and currently available memory minus 1 GB. Already downloaded models get preference, then compact dictation-oriented families. This is a heuristic, not a memory guarantee or a quality benchmark.

If both selected models do not fit that budget together, the worker releases cached models between stages. Unload models in Setup releases them manually. Switching between legacy NPZ Whisper and MLX Audio also releases the previous speech backend.

## Languages

Whisper uses ISO language codes; Qwen3-ASR uses full language names, which the worker maps. Models without a language argument auto-detect using their own supported languages. S1-mini v1 is English-only. See the [publisher's model card](https://huggingface.co/superwhisper/s1-mini) for its exact trained input format and limitations.
