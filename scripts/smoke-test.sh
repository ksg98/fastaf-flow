#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .test-artifacts
say -v Samantha -o .test-artifacts/dictation.aiff 'So, I need to send the report by Friday. No, wait, make that Thursday. And please include the project budget.'
# afconvert is included with macOS; no ffmpeg installation is needed.
afconvert -f WAVE -d LEI16@16000 -c 1 .test-artifacts/dictation.aiff .test-artifacts/dictation.wav
cargo test --test runtime_smoke -- --ignored --nocapture
