# FastAF Flow interface

A small recording desk for dictation: one prominent record control, a live input meter, an editable transcript, and a narrow model shelf. Native Rust / egui, no webview.

Palette: paper #F8FAFC, white #FFFFFF, ink #16213B, muted blue #59677C, cobalt #325CDF, recording red #BC3451. Cobalt belongs to the record button and active selection; red means a live microphone. Typography: the macOS system face (with bundled fallback) at 15px for controls, 28px for the app title, and 20px for the transcript. Sentence case throughout.

Layout: left-aligned model shelf (270px) and a flexible writing area. Status and shortcut remain visible below the writing area. Model downloads live in a separate library tab. Avoid repeating cards: use spacing and a single divider between setup and writing. One distinctive element: a wide horizontal meter on the recording control, showing actual microphone energy, never decorative activity.

Review: this is a utility rather than a marketing dashboard. No hero, fabricated stats, or animated empty-state waveform. The editor takes most of the window. App stays useful without downloaded models: its setup action and library are accessible first.
