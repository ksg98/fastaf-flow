FastAF Flow v1.0.0 brings local dictation to Apple Silicon with a native Rust app, MLX Audio transcription, and S1-mini by Superwhisper rewriting through MLX-LM.

- Record from the app or use ⌘⇧Space, with toggle and hold-to-talk modes.
- Edit, copy, restore raw text, or optionally insert dictation into the active app.
- Discover existing model caches, browse 236 bundled model entries, and download models explicitly.
- Choose all published S1-mini MLX variants: 4-bit, 8-bit, and BF16; discover custom local quants too.
- Get recommendations based on total and available memory, with sequential model loading when memory is tight.
- Install the local runtime from Setup; inference stays offline after setup and model downloads.

Download the DMG or ZIP, move FastAF Flow.app to Applications, open it, and choose **Setup → Install runtime**. Select your models, then grant Microphone access when starting your first recording. Accessibility access is needed only for automatic insertion.

**Requirements:** Apple Silicon, macOS 14+, 8 GB or more memory. First setup needs internet and several GB of disk space. Model weights are not bundled.

**Signing:** This initial release is ad-hoc signed, not Apple notarized. If blocked, use System Settings → Privacy & Security → Open Anyway after attempting to open the app.

**Validation:** Rust checks, Python protocol tests, a real Qwen3-ASR → S1-mini GPU integration test, cancellation/recovery, and fresh packaged-runtime setup passed on M3 Max / macOS 26.6.2. Physical microphone/hotkey and Accessibility-gated insertion flows still need interactive verification; the complete model catalog is not individually tested. See [validation details](https://github.com/ksg98/fastaf-flow/blob/main/docs/VALIDATION.md).

MIT-licensed application code. Model licenses remain with their publishers, including S1-mini by Superwhisper's naming clause.
