# Architecture

## Goal

Let any game engine hand viewport frames to AI agents through a common pipeline.

## Layers

```
┌─────────────────────────────────────────────────────────────┐
│ Engine adapters (Bevy, Godot, Unity, Unreal, WebXR)         │
├─────────────────────────────────────────────────────────────┤
│ radredeye-core (frame types, sinks, pipeline)          │
├─────────────────────────────────────────────────────────────┤
│ Sinks (stdout, filesystem, HTTP, WebSocket, gRPC)            │
├─────────────────────────────────────────────────────────────┤
│ AI agent / LLM vision endpoint                              │
└─────────────────────────────────────────────────────────────┘
```

### Core concepts

- `CapturedFrame`: raw bytes + metadata.  No engine-specific handles leak past core.
- `CaptureSink`: a consumer of frames.  Sinks are isolated from each other — one sink failing never stops the others.
- `CapturePipeline`: owns all sinks and fans out each `CapturedFrame` to them, applying `CaptureConfig` (resize/format/backpressure) first.

### Engine adapters

Each adapter is responsible for:
1. Opt-in capture (a marker component or explicit setting).
2. Copying the backbuffer/render target into CPU memory.
3. Building a `CapturedFrame` and calling `pipeline.submit()` (directly for Bevy/WebXR, or via the daemon HTTP bridge for Godot/Unity/Unreal).

---

## Crates

The workspace contains 5 crates (`radredeye-webxr` is excluded from the
default workspace build because it targets `wasm32` and needs the
`wasm-bindgen`/`web-sys` toolchain, but its source is part of the repo).

| Crate | Role | Engine | Default build |
|-------|------|--------|--------------|
| `radredeye-core` | Engine-agnostic frame types, `CaptureSink` trait, `CapturePipeline` bus, built-in sinks + encoding helpers. | *(none)* | yes |
| `radredeye-bevy` | Bevy 0.15 plugin: observer-based screenshot capture, `CaptureCamera` marker, `BevyCapturePipeline` resource. | Bevy | yes |
| `radredeye-mcp` | `tiny_http` HTTP bridge that decodes Godot/Unity/Unreal PNG POSTs into `CapturedFrame`s and feeds the core pipeline. | Godot (+Unity/Unreal via HTTP) | yes |
| `radredeye-unity` | Re-exports core frame types for Unity ↔ Rust interop; Unity C# side POSTs to the daemon bridge. | Unity | yes |
| `radredeye-unreal` | Re-exports core frame types for Unreal ↔ Rust interop; Unreal C++ side POSTs to the daemon bridge. | Unreal | yes |
| `radredeye-webxr` | `wasm-bindgen` adapter: `WebGLRenderingContext.readPixels` → `CapturedFrame` (built for `wasm32`). | WebXR | no (wasm32) |

---

## Sinks × engines matrix

### Sinks (4 implemented)

| Sink | Crate | Feature flag | Default | Status | Ships frames as |
|------|-------|-------------|---------|--------|----------------|
| `StdoutSink` | core | *(always available)* | yes | ✅ EPIPE-safe | Metadata line to stdout |
| `FileSink` | core | `file-sink` | yes | ✅ functional | `frame_NNNN.png` files in a directory |
| `HttpSink` | core | `http-sink` | yes | ✅ functional | PNG `POST` to a URL |
| `WebSocketSink` | core | `websocket-sink` | no | ✅ functional (feature-gated) | Binary WebSocket messages (PNG), auto-reconnect |
| `GrpcSink` | core | `grpc-sink` | no | ✅ functional (feature-gated) | Client-streaming gRPC RPC via tonic |

> 3 sinks (`StdoutSink`, `FileSink`, `HttpSink`) are on by default.
> `WebSocketSink` and `GrpcSink` are feature-gated (opt-in) because they pull
> heavier deps (`tungstenite`, `tonic`/`prost`/`tokio`).

### Engine adapters (3 stubbed: Bevy + Godot implemented; Unreal/Unity/WebXR stubbed)

| Engine | Adapter crate / location | Capture path | Status |
|--------|--------------------------|-------------|--------|
| **Bevy** | `crates/radredeye-bevy` | Direct: observer screenshot → `CapturePipeline::submit` (in-process). | ✅ Implemented |
| **Godot** | `engines/godot/addons/radredeye_capture/` + `crates/radredeye-mcp` | HTTP bridge: addon POSTs PNG → daemon decodes → core pipeline. | ✅ Implemented |
| **Unity** | `crates/radredeye-unity` (Rust interop) + C# `RadredeyeCaptureBridge.cs` | HTTP bridge: C# captures backbuffer, POSTs PNG to daemon. | 🔶 Stubbed (crate re-exports core types; C# side not in repo) |
| **Unreal** | `crates/radredeye-unreal` (Rust interop) + C++ `RadredeyeCaptureComponent.h` | HTTP bridge: C++ captures viewport, POSTs PNG to daemon. | 🔶 Stubbed (crate re-exports core types; C++ side not in repo) |
| **WebXR** | `crates/radredeye-webxr` (`wasm-bindgen`) | Direct: `WebGLRenderingContext.readPixels` → `CapturedFrame` (WASM). | 🔶 Stubbed (WASM adapter; excluded from default workspace build) |

> "Stubbed" means the Rust crate exists and compiles but only exposes shared
> types (or a thin WASM helper). The native engine-side glue (C#/C++/JS) is the
> integration boundary the engine team owns; frames reach the pipeline via the
> daemon HTTP bridge (Unity/Unreal) or in-process (WebXR).

### Bridges

| Component | Status | Notes |
|-----------|--------|-------|
| `radredeye-mcp` | ✅ implemented | `tiny_http` server on `0.0.0.0:8765`; receives PNG POSTs at `/capture`, decodes into `CapturedFrame`, fans out through core `CapturePipeline`. |
| `GET /health` | ✅ implemented | Liveness probe returning `{"status":"ok"}`. |
| Proto schema | ✅ defined | `proto/radredeye.proto` — `FrameStreaming` service for the gRPC sink. |
