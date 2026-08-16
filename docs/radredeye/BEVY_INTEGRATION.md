# Bevy Integration Walkthrough

This guide shows how to capture rendered frames from a [Bevy](https://bevyengine.org)
app and route them through the radredeye pipeline to sinks (stdout, files,
HTTP, WebSocket, gRPC).

**Audience:** developers wiring Bevy 0.15 apps into the radredeye perception
layer. No prior radredeye knowledge is required.

---

## 1. Prerequisites

- Rust toolchain (edition 2021+) and a working `cargo`.
- The radredeye workspace checked out (this repo).
- A display/GPU available for the Bevy example (headless servers need a virtual
  framebuffer such as `xvfb`).
- Bevy 0.15 (declared as a workspace dependency).

The two relevant crates:

| Crate | Role |
|-------|------|
| `radredeye-core` | Engine-agnostic frame types, `CaptureSink` trait, `CapturePipeline` bus, built-in sinks. |
| `radredeye-bevy` | Bevy plugin that copies the backbuffer into `CapturedFrame`s and feeds the pipeline. |

`radredeye-bevy` depends on `radredeye-core`, so adding the plugin
crate pulls in the core automatically.

---

## 2. Add the plugin

Import the plugin and add it to your `App` exactly like any other Bevy plugin:

```rust
use bevy::prelude::*;
use radredeye_bevy::RadredeyeCapturePlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RadredeyeCapturePlugin) // <-- registers the capture pipeline
        .add_systems(Startup, setup)
        .run();
}
```

`RadredeyeCapturePlugin` does two things on build:

1. Initialises the [`BevyCapturePipeline`] resource (a thin `Deref` wrapper around
   the core [`CapturePipeline`]).
2. Registers a `PostUpdate` system that requests screenshots from every camera
   marked with [`CaptureCamera`], respecting each camera's throttle.

[`BevyCapturePipeline`]: https://docs.rs/radredeye-bevy/latest/radredeye_bevy/struct.BevyCapturePipeline.html
[`CapturePipeline`]: https://docs.rs/radredeye-core/latest/radredeye_core/struct.CapturePipeline.html

---

## 3. Mark a camera with `CaptureCamera`

Capture is **opt-in**. A camera without the [`CaptureCamera`] marker component is
ignored entirely, so you can run multiple cameras and only stream the ones you
want.

```rust
use bevy::prelude::*;
use radredeye_bevy::CaptureCamera;

fn setup(mut commands: Commands) {
    // CaptureCamera::enabled() turns capture on with a 1-second throttle.
    commands.spawn((Camera2d, CaptureCamera::enabled()));
}
```

[`CaptureCamera`]: https://docs.rs/radredeye-bevy/latest/radredeye_bevy/struct.CaptureCamera.html

### Throttling

`CaptureCamera` exposes two fields:

| Field | Type | Meaning |
|-------|------|---------|
| `enabled` | `bool` | Master toggle. `false` skips the camera. |
| `throttle_seconds` | `Option<f32>` | Minimum seconds between captures. `None` means "every frame". |

The convenience constructor `CaptureCamera::enabled()` sets `enabled = true` and
`throttle_seconds = Some(1.0)` (one frame per second by default). To capture
faster or slower, build the component manually:

```rust
// Capture at 10 FPS (throttle 0.1s).
commands.spawn((
    Camera2d,
    CaptureCamera { enabled: true, throttle_seconds: Some(0.1) },
));

// Capture every rendered frame (no throttle).
commands.spawn((
    Camera3d::default(),
    CaptureCamera { enabled: true, throttle_seconds: None },
));
```

---

## 4. Configure resolution / format / backpressure

The pipeline applies a [`CaptureConfig`] to every frame *before* it reaches the
sinks. This lets you downscale or reformat captured frames without touching the
engine adapter code.

```rust
use std::time::Duration;
use radredeye_core::{CaptureConfig, PixelFormat};

fn configure_pipeline(mut pipeline: ResMut<radredeye_bevy::BevyCapturePipeline>) {
    pipeline.configure(CaptureConfig {
        target_width: Some(320),
        target_height: Some(180),
        target_format: Some(PixelFormat::Rgba8),
        min_interval: Some(Duration::from_millis(100)), // backpressure: drop faster frames
    });
}
```

| Field | Effect |
|-------|--------|
| `target_width` / `target_height` | Nearest-neighbour resize (RGBA8) before sinking. |
| `target_format` | Convert frames to this `PixelFormat` (BGRA8 → RGBA8) before sinking. |
| `min_interval` | Drop frames that arrive sooner than this `Duration` (pipeline-level backpressure). |

`BevyCapturePipeline` derefs to `CapturePipeline`, so you call `configure`
directly on the resource.

[`CaptureConfig`]: https://docs.rs/radredeye-core/latest/radredeye_core/struct.CaptureConfig.html

---

## 5. Register sinks

Sinks implement the [`CaptureSink`] trait and are registered as
`Arc<dyn CaptureSink>`. Register them once during startup:

```rust
use std::sync::Arc;
use radredeye_core::{
    sinks::{file::FileSink, http::HttpSink, StdoutSink},
    CaptureSink,
};

fn register_sinks(mut pipeline: ResMut<radredeye_bevy::BevyCapturePipeline>) {
    // Print frame metadata to stdout (always available, no feature flag).
    pipeline.add_sink(Arc::new(StdoutSink));

    // Write PNGs to ./captures/bevy (feature = "file-sink", on by default).
    pipeline.add_sink(Arc::new(FileSink::new("captures/bevy").expect("create dir")));

    // POST each PNG to a vision endpoint (feature = "http-sink", on by default).
    if let Ok(url) = std::env::var("RADREDEYE_HTTP_SINK_URL") {
        pipeline.add_sink(Arc::new(HttpSink::new(url)));
    }
}
```

Available sinks (see [`ARCHITECTURE.md`](ARCHITECTURE.md) for the full matrix):

| Sink | Feature flag | Default | Ships frames as |
|------|-------------|---------|-----------------|
| `StdoutSink` | *(always)* | yes | Metadata line to stdout (EPIPE-safe) |
| `FileSink` | `file-sink` | yes | `frame_NNNN.png` files in a directory |
| `HttpSink` | `http-sink` | yes | PNG `POST` to a URL |
| `WebSocketSink` | `websocket-sink` | no | Binary WebSocket messages (PNG) |
| `GrpcSink` | `grpc-sink` | no | Client-streaming gRPC RPC |

[`CaptureSink`]: https://docs.rs/radredeye-core/latest/radredeye_core/trait.CaptureSink.html

---

## 6. Run the bundled example

The workspace ships a minimal example at
`crates/radredeye-bevy/examples/simple_capture.rs`. It wires a stdout sink,
a file sink, and an optional HTTP sink, then spawns a 2D camera with capture
enabled.

```bash
# From the repo root — requires a display/GPU.
cargo run -p radredeye-bevy --example simple_capture

# Optionally forward frames to an HTTP endpoint:
RADREDEYE_HTTP_SINK_URL=http://127.0.0.1:8765/capture \
  cargo run -p radredeye-bevy --example simple_capture
```

While the example runs you will see `[StdoutSink] …` lines on stdout and PNG files
appear under `captures/bevy/`. Stop the process with `Ctrl-C`; the pipeline's
[`shutdown`](https://docs.rs/radredeye-core/latest/radredeye_core/struct.CapturePipeline.html#method.shutdown)
hook lets each sink flush buffered data.

---

## 7. Minimal complete example

Putting §2–§5 together into one runnable file:

```rust
//! Minimal radredeye + Bevy app: one capture camera feeding stdout + file sinks.
use bevy::prelude::*;
use std::sync::Arc;
use radredeye_bevy::{BevyCapturePipeline, CaptureCamera, RadredeyeCapturePlugin};
use radredeye_core::sinks::{file::FileSink, StdoutSink};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RadredeyeCapturePlugin)
        .add_systems(Startup, (setup, register_sinks).chain())
        .run();
}

fn setup(mut commands: Commands) {
    // 1-second throttle (default), capture enabled.
    commands.spawn((Camera2d, CaptureCamera::enabled()));
}

fn register_sinks(mut pipeline: ResMut<BevyCapturePipeline>) {
    pipeline.add_sink(Arc::new(StdoutSink));
    pipeline.add_sink(Arc::new(FileSink::new("captures/bevy").expect("create dir")));
}
```

Save as `examples/my_capture.rs` inside a crate that depends on
`radredeye-bevy`, then run `cargo run --example my_capture`.

---

## 8. How capture works under the hood

1. The `PostUpdate` system iterates cameras with `CaptureCamera`.
2. For each enabled camera whose throttle has elapsed, it spawns a Bevy
   `Screenshot` entity (window or render-image target) with an observer.
3. When Bevy captures the screenshot, the observer converts it to RGBA8, wraps it
   in a `CapturedFrame`, and calls `pipeline.submit(&frame)`.
4. The pipeline applies `CaptureConfig` (resize/format/backpressure) and fans the
   frame out to every registered sink.

Because capture uses Bevy 0.15's observer-based screenshot API, it works with
both window and render-image camera targets and never blocks the render loop.

---

## 9. Troubleshooting

| Symptom | Likely cause / fix |
|---------|-------------------|
| No `[StdoutSink]` lines, no PNGs | Camera missing `CaptureCamera`, or `enabled = false`. |
| Frames arrive too fast / disk fills up | Set `throttle_seconds` or `CaptureConfig::min_interval`. |
| `FileSink::new` panics on `expect` | The output directory's parent is not writable; check permissions. |
| Example fails with a graphics error | No display/GPU; run under `xvfb-run` or a virtual display. |
| `HttpSink` returns transport errors | Endpoint down or unreachable; check `RADREDEYE_HTTP_SINK_URL`. |

---

## 10. Next steps

- **Godot engines** use the daemon bridge instead — see
  [`GODOT_INTEGRATION.md`](GODOT_INTEGRATION.md).
- **Architecture & sink×engine matrix**: [`ARCHITECTURE.md`](ARCHITECTURE.md).
- **Full API reference**: `cargo doc -p radredeye-core -p radredeye-bevy --open`.
