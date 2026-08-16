# Godot addon

Place `addons/radredeye_capture` into your Godot 4 project's `res://addons/` folder and enable it in **Project Settings › Plugins**.

## Modes

- **Disk only** (default): screenshots go to `user://screenshots/` on a timer.
- **Bridge mode**: enable `emit_to_bridge` and run the Rust daemon:

  ```bash
  cargo run -p radredeye-mcp
  ```

  The addon will POST each PNG to `http://127.0.0.1:8765/capture`,
  which the daemon fans out through the same `CapturePipeline` used by the
  Bevy plugin.

## Configuration

| Property | Default | Notes |
|----------|---------|-------|
| `enabled` | `true` | Master toggle |
| `save_to_disk` | `true` | Keep local PNGs |
| `capture_interval_seconds` | `1.0` | Frame cadence |
| `emit_to_bridge` | `false` | POST to Rust daemon |
| `bridge_url` | `http://127.0.0.1:8765/capture` | Daemon endpoint |
