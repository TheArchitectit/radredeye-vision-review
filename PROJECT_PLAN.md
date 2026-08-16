# radredeye — Project Plan

**Mission:** Build the missing viewport layer between game engines and AI agents, starting with Bevy and Godot, then expanding to Unity, Unreal, and WebXR.

## Phase 0 — Guardrails & scaffold ✅

- Import Agent Guardrails Template operational files.
- Create Rust workspace with `radredeye-core` and `radredeye-bevy`.
- Port the template's Godot `radredeye_capture` addon under `engines/godot/`.
- Document architecture and support matrix.

## Phase 1 — Core capture bus

- Define `CapturedFrame`, `CaptureSink`, `CapturePipeline`.
- Implement filesystem and HTTP sinks behind traits.
- Add configuration (`capture_interval`, `max_fps`, `quality`).

## Phase 2 — Bevy GPU extraction

- Implement real render-target copy in `radredeye-bevy` using Bevy 0.15 render graph APIs.
- Support `Camera` targeting both window and render texture.
- Add performance budget: skip frames, throttle, downscale.

## Phase 3 — Godot integration beyond screenshots

- Pipe `screenshot_autosave.gd` output into the core `CapturePipeline` via a Rust GDExtension or UDP socket.
- Add throttling and hotkey capture.

## Phase 4 — Unity & Unreal adapters

- C# Unity package (UPM).
- Unreal Engine plugin (C++).

## Phase 5 — WebXR / web games

- Browser adapter via WebGL backbuffer readback.
- Optional WebSocket sink for real-time agent streaming.

## Phase 6 — Guardrails enforcement in CI

- Add guardrail policy checks to PRs using `.guardrails/prevention-rules/`.
- Keep dependency and security guardrails aligned with `docs/AGENT_GUARDRAILS.md`.
