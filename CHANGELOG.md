# Changelog

## Unreleased

- Include official Moonshine Tiny and Base checkpoints in the bundled and refreshed model catalog even though their Hugging Face metadata has no MLX tag.
- Verify both checkpoints with offline MLX Audio transcription on Apple Silicon.

## 1.0.0 — 2026-09-09

- Native Rust dictation app for Apple Silicon, with a menu-bar control and global ⌘⇧Space shortcut.
- Local microphone recording, WAV import, editable text, raw transcript recovery, and optional paste into the original active app.
- MLX Audio transcription, legacy MLX Whisper cache compatibility, and MLX-LM rewriting with S1-mini by Superwhisper.
- Downloaded-model dropdowns, paginated online discovery, all three published S1-mini MLX variants, custom model folders, and memory-aware recommendations.
- Isolated first-run runtime installation, offline inference workers, cancellable operations, and locked dependencies.
- MIT-licensed source and ad-hoc-signed macOS application bundles.
