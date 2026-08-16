# Getting Started

## 1. Build the Rust workspace

```bash
cargo check
```

## 2. Run the Bevy example

```bash
cargo run --example simple_capture --package radredeye-bevy
```

The example creates a window, one camera, and a stdout sink.  
GPU framebuffer extraction is a stub; the pipeline wiring is real.

## 3. Try the Godot addon

- Copy `engines/godot/addons/radredeye_capture` into your Godot 4 project.
- Enable **radredeye Capture** in Project Settings › Plugins.
- Screenshots appear under `user://screenshots/` on a timer.

## 4. Read the guardrails

Before making changes, agents must read:

- `docs/AGENT_GUARDRAILS.md`
- `.guardrails/pre-work-check.md`
- `CLAUDE.md`
