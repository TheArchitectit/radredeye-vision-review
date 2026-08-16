# Godot Integration Walkthrough

This guide shows how to capture viewport frames from a [Godot 4](https://godotengine.org)
project and route them through the radredeye pipeline — either to local PNG
files, to the Rust daemon bridge over HTTP, or both.

**Audience:** developers wiring Godot 4 games into the radredeye perception
layer. No prior radredeye knowledge is required.

---

## 1. How Godot capture works

Unlike Bevy (which is itself a Rust crate and links the pipeline directly), Godot
runs in its own GDScript runtime. radredeye bridges the two with a small
`@tool` addon plus an optional Rust HTTP daemon:

```
Godot viewport ──get_image()──▶ PNG bytes ──┬──▶ user://screenshots/*.png  (disk)
                                            │
                                            └──▶ HTTP POST ──▶ radredeye-mcp ──▶ CapturePipeline ──▶ sinks
```

- The **addon** (`engines/godot/addons/radredeye_capture/`) is a single GDScript
  `@tool` node that auto-captures the viewport on a timer.
- The **daemon** (`radredeye-mcp`) is a tiny `tiny_http` server that
  receives PNG `POST`s at `/capture`, decodes them into `CapturedFrame`s, and fans
  them out through the same `CapturePipeline` the Bevy plugin uses.

You can use **disk-only mode** (no Rust side) or **bridge mode** (POST to the
daemon). Bridge mode is what lets an AI agent observe a running Godot game.

---

## 2. Prerequisites

- Godot 4.x (the addon uses `@tool`, `HTTPRequest`, and `Viewport.get_texture()`).
- The radredeye workspace checked out if you want bridge mode.
- A Rust toolchain to build/run the daemon (bridge mode only).

| Component | Disk-only mode | Bridge mode |
|----------|----------------|-------------|
| Godot 4 addon | required | required |
| `radredeye-mcp` | not needed | required (running) |
| Rust toolchain | not needed | required |

---

## 3. Install the addon

1. Copy `engines/godot/addons/radredeye_capture/` into your Godot project's
   `res://addons/radredeye_capture/` folder.
2. Open the project in the Godot editor.
3. Enable the addon in **Project Settings › Plugins** (tick "radredeye
   Capture").

The addon is a `@tool` script, so it also captures frames while editing — handy
for previewing your capture interval before running the game.

---

## 4. Configure the addon

The addon exposes these `@export` properties on its node. Add the
`RadredeyeCapture` node to your scene (or instantiate it from code) and set them in
the inspector:

| Property | Default | Description |
|----------|---------|-------------|
| `enabled` | `true` | Master toggle. `false` disables capture entirely. |
| `save_to_disk` | `true` | Keep local PNG copies under `output_directory`. |
| `capture_interval_seconds` | `1.0` | Seconds between captures (frame cadence). |
| `output_directory` | `user://screenshots` | Where disk PNGs are written. |
| `emit_to_bridge` | `false` | If `true`, POST each PNG to `bridge_url`. |
| `bridge_url` | `http://127.0.0.1:8765/capture` | Daemon `/capture` endpoint. |

### Modes

- **Disk only** (default, `emit_to_bridge = false`): screenshots go to
  `user://screenshots/` on the timer. No Rust side needed.
- **Bridge mode** (`emit_to_bridge = true`): the addon POSTs each PNG to the
  daemon, which fans the frame out through the core `CapturePipeline`. Combine
  with `save_to_disk = true` to keep local copies too.

### Setting the cadence

`capture_interval_seconds` controls how often the viewport is grabbed. Lower
values capture more frames (more agent context) at the cost of CPU/IO. The
daemon's pipeline also has its own `CaptureConfig::min_interval` backpressure,
so even if Godot POSTs faster than the consumer can handle, frames are dropped
gracefully rather than queued unboundedly.

---

## 5. Run the daemon bridge (bridge mode)

In a second terminal, from the radredeye repo root:

```bash
cargo run -p radredeye-mcp
```

The daemon listens on `0.0.0.0:8765` and prints:

```
[radredeye-mcp] listening on 0.0.0.0:8765
```

By default it registers three sinks:

1. `StdoutSink` — prints frame metadata to the daemon's stdout.
2. `FileSink` — writes PNGs to `captures/godot/`.
3. `HttpSink` — if `RADREDEYE_HTTP_SINK_URL` is set, forwards each PNG onward.

To forward frames to a vision endpoint, set the env var before starting:

```bash
RADREDEYE_HTTP_SINK_URL=https://my-vision-endpoint.example.com/ingest \
  cargo run -p radredeye-mcp
```

### Daemon endpoints

| Method | Path | Body | Purpose |
|--------|------|------|---------|
| `POST` | `/capture` | raw PNG bytes (`Content-Type: image/png`) | Submit a captured frame; returns `ok` (200) or `error: …` (400). |
| `GET` | `/health` | *(none)* | Liveness probe; returns `{"status":"ok"}` (200). |

The addon posts to `/capture` with `Content-Type: image/png` and the raw PNG
buffer as the body. The daemon decodes the PNG into a `CapturedFrame`
(`PixelFormat::Rgba8`) and feeds it to the pipeline — exactly the same code path
the Bevy plugin uses.

---

## 6. Start capturing

With the addon enabled and (for bridge mode) the daemon running:

1. Run your Godot project (`F5` in the editor, or `godot --path .`).
2. Every `capture_interval_seconds`, the addon grabs the viewport:
   - In disk mode: a `screenshot_<timestamp>_NNNN.png` appears under
     `user://screenshots/`.
   - In bridge mode: the daemon logs `[StdoutSink] …` and writes
     `captures/godot/frame_NNNN.png`.
3. Stop the game to stop capturing. The daemon stays running until you stop it
   (`Ctrl-C`); each sink's `on_shutdown()` hook flushes buffered data.

---

## 7. Minimal GDScript setup

If you prefer to wire the addon from code instead of the editor, add this to an
autoload or scene script:

```gdscript
extends Node

func _ready() -> void:
    var capture = preload("res://addons/radredeye_capture/screenshot_autosave.gd").new()
    add_child(capture)

    # Configure from code (mirrors the @export inspector properties).
    capture.enabled = true
    capture.save_to_disk = true
    capture.capture_interval_seconds = 0.5        # 2 FPS
    capture.output_directory = "user://screenshots"
    capture.emit_to_bridge = true
    capture.bridge_url = "http://127.0.0.1:8765/capture"

    # The addon starts its own timer in _ready(); settings above must be set
    # before the node enters the tree, or call capture_now() manually.
```

To capture a single frame on demand (e.g. on a button press or event):

```gdscript
func _on_capture_button_pressed() -> void:
    var saved_path = $RadredeyeCapture.capture_now()
    print("captured: ", saved_path)
```

---

## 8. Verifying the bridge

Quick end-to-end smoke test:

1. Start the daemon: `cargo run -p radredeye-mcp`.
2. Check liveness:

   ```bash
   curl http://127.0.0.1:8765/health
   # {"status":"ok"}
   ```

3. POST a test PNG manually (any small PNG file works):

   ```bash
   curl -X POST --data-binary @my_screenshot.png \
     -H "Content-Type: image/png" \
     http://127.0.0.1:8765/capture
   # ok
   ```

   The daemon should log a `[StdoutSink] …` line and write
   `captures/godot/frame_0000.png`.

4. Run the Godot project with `emit_to_bridge = true` and confirm the daemon
   logs a frame per `capture_interval_seconds`.

---

## 9. Troubleshooting

| Symptom | Likely cause / fix |
|---------|-------------------|
| No PNGs in `user://screenshots/` | `enabled = false`, or `save_to_disk = false` and bridge off. |
| `[RadredeyeCapture] Failed to get viewport image` | Viewport not yet rendered (common on the very first frame); the addon retries next tick. |
| `[RadredeyeCapture] Failed to POST frame` | Daemon not running, or `bridge_url` wrong. Check `emit_to_bridge` and the daemon is on `0.0.0.0:8765`. |
| Daemon returns `error: …` (400) | Body was not a valid PNG; ensure the addon sends `img.save_png_to_buffer()`. |
| Frames arrive faster than the consumer handles | Raise `capture_interval_seconds`, or set the pipeline `CaptureConfig::min_interval` (backpressure drops, never blocks). |
| `capture_now()` returns `""` | `get_image()` failed (viewport empty); capture again next frame. |

---

## 10. Next steps

- **Bevy engines** link the pipeline directly — see
  [`BEVY_INTEGRATION.md`](BEVY_INTEGRATION.md).
- **Architecture & sink×engine matrix**: [`ARCHITECTURE.md`](ARCHITECTURE.md).
- **Engine support matrix**: [`SUPPORT_MATRIX.md`](SUPPORT_MATRIX.md).
- **Full API reference**: `cargo doc -p radredeye-mcp --open`.
