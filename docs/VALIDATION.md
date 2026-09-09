# v1.0.0 validation

Validated on an Apple M3 Max with 64 GB unified memory, macOS 26.6.2, Rust 1.96.0, and Python 3.12.13 on September 9, 2026.

## Automated checks

- `cargo fmt --check`
- `cargo clippy --locked --all-targets -- -D warnings` and the optional screenshot feature
- Four Rust model-catalog tests: metadata-only caches, legacy Whisper identification, missing shards, invalid shard paths, low-memory recommendations, and quantization metadata
- Five Python protocol tests: the 16 trained S1-mini control combinations, Unicode-safe bounded chunking, malformed input recovery, offline download rejection, and online-worker inference rejection without leaking the request
- Opt-in Rust-to-Python GPU integration test: ping, model-load error, cancellation during inference, worker restart, Qwen3-ASR transcription, S1-mini correction of a spoken false start, valid empty filler output, and unloading
- First-run runtime installation from the packaged app using its bundled uv, with a new app-data directory and a working directory outside the source checkout
- Packaged CLI runtime check and WAV-to-clean-text processing through that fresh environment
- Hugging Face catalog refresh and Qwen3-ASR model download through the explicit online worker
- Dictate, Models, and Setup windows rendered and visually inspected from actual native app screenshots
- Application Info.plist validation, ARM64 binary inspection, and ad-hoc code-signature verification

## Real inference checks

The synthetic recording was generated locally with macOS `say`. It contains a correction from Friday to Thursday and a request to include a project budget.

| Path | Result |
| --- | --- |
| Existing `mlx-community/whisper-large-v3-turbo-q4` NPZ cache | Raw speech transcribed successfully |
| `mlx-community/Qwen3-ASR-0.6B-4bit` through MLX Audio 0.5.3 | Raw speech transcribed successfully |
| `mlx-community/S1-mini-MLX-4bit` through MLX-LM 0.31.3 | Clean text retained Thursday and the budget request, removing the superseded Friday |
| S1-mini input `um` | Valid empty output |
| Cancel then retry through the Rust service | Successful inference after worker restart |

One Qwen3-ASR pass took about 2.7 seconds and one S1-mini normalization took about 1 second, measured inside the worker on this machine. These are smoke-test observations, not a benchmark or a latency guarantee. Cold imports/model loading, recording length, memory pressure, and hardware change timings.

## Limits of this validation

Live microphone permission and capture, physical global-keyboard interactions, and Accessibility-gated insertion into third-party apps require interactive testing on the user's Mac. The build machine did not grant FastAF Flow Accessibility access. These flows are implemented but are not claimed as end-to-end verified here. All audio used in inference tests was synthetic; no ambient microphone audio was recorded.

S1-mini 8-bit and BF16 were discovered from the publisher's actual model repositories but were not downloaded for the GPU tests. They use the same MLX-LM loading path. The full 236-entry bundled model library is not individually validated; third-party checkpoints may require additional files or dependencies.

The app targets macOS 14+ on Apple Silicon; this release was not tested on every supported OS/device. It is ad-hoc signed and not notarized. OS crashes or force-kills can leave temporary recordings behind until system cleanup. Model memory recommendations are estimates, not allocation guarantees.
