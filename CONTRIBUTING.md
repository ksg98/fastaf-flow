# Contributing

Use an Apple Silicon Mac and follow the source setup in the README. Run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`, and `python3 -m unittest discover -s tests -v` before a pull request.

Keep the Rust app small. Inference belongs in the local MLX worker; audio and text must not enter online workers. Preserve the raw transcript when an inference operation fails. Avoid storing recordings or transcripts in logs, fixtures, issues, or crash reports.

Changes to the worker protocol, cancellation, model scanning, and transcript chunking should include focused tests. GPU smoke tests are opt-in through `scripts/smoke-test.sh`; automated tests must not record the microphone or silently download model weights.

When updating dependencies, regenerate the Rust and Python lockfiles and third-party notices (`python3 scripts/fetch-license-notices.py`, then `python3 scripts/licenses.py`). Include the exact model ID and quantization when reporting an inference issue, but use synthetic dictation rather than private content.

For release packaging, run `scripts/bundle.sh`, inspect the bundle, test first-run setup, and verify `codesign --verify --deep --strict 'dist/FastAF Flow.app'`. Release builds are currently ad-hoc signed. Do not claim notarization without completing Apple's signing/notarization flow.
