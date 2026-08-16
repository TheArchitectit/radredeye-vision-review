# AGENTS.md

## What This Is

radredeye is a framework-agnostic visual capture pipeline for AI agents. It captures rendered frames from ANY application framework (game engines, desktop GUIs, browsers, terminals), encodes them, and routes them to configurable sinks (stdout, filesystem, HTTP, WebSocket, gRPC). The agent-facing interface is the MCP stateless protocol, served by `radredeye-mcp`.

## Architecture

```
App-framework adapters (Bevy plugin, Godot addon, Unity/Unreal/WebXR stubs,
  and ANY app via submit_frame over MCP / POST /capture)
        ↓  submits CapturedFrame (base64 PNG decoded by radredeye-mcp or built in-process)
radredeye-core (CapturePipeline facade over StreamRegistry fans out to all registered sinks)
        ↓
Sinks: StdoutSink, FileSink, HttpSink (on by default); WebSocketSink, GrpcSink (feature-gated); SemanticDiffSink (feature-gated)
        ↓
radredeye-mcp (MCP stateless server — the agent interface; listens on 0.0.0.0:8765)
  MCP tools: list_streams, list_sinks, get_frame, submit_frame, health
  Legacy routes: POST /capture, GET /frame[?index=N], GET /health
```

**Six crates in the workspace:**

| Crate | Purpose |
|-------|---------|
| `radredeye-core` | Framework-agnostic frame types (`CapturedFrame`, `PixelFormat`), `CaptureSink` trait, `CapturePipeline` facade over `StreamRegistry`, `ReplayBuffer` ring buffer (cap 30), built-in sinks (Stdout/File/HTTP/WebSocket/gRPC/SemanticDiff), `CaptureConfig`, `PipelineMetrics` |
| `radredeye-bevy` | Bevy 0.15 plugin (`RadredeyeCapturePlugin`), `CaptureCamera` marker component, `BevyCapturePipeline` resource wrapper |
| `radredeye-mcp` | **MCP stateless server — the agent interface** (absorbed the old daemon). Listens on `0.0.0.0:8765`; `POST /mcp` (JSON-RPC 2.0, stateless, protocolVersion `2025-06-18`, capabilities `tools`) plus legacy `POST /capture`, `GET /frame[?index=N]`, `GET /health`. Wraps a shared `CapturePipeline` + `ReplayBuffer`. MCP tools: `list_streams`, `list_sinks`, `get_frame`, `submit_frame`, `health`. |
| `radredeye-unity` | Re-exports core frame types for Unity ↔ Rust interop; Unity C# side submits frames via MCP/`POST /capture` |
| `radredeye-unreal` | Re-exports core frame types for Unreal ↔ Rust interop; Unreal C++ side submits frames via MCP/`POST /capture` |

> `radredeye-webxr` (`wasm-bindgen` adapter) also exists in the repo but is
> excluded from the default workspace build (targets `wasm32`).

**Data flow:**
1. App-framework adapter copies backbuffer → `CapturedFrame { width, height, format, data: Vec<u8>, timestamp }`, or an arbitrary app submits a base64 PNG via `submit_frame` (MCP) / `POST /capture`.
2. Adapter/app calls `pipeline.submit(&frame)` (in-process) or the MCP server decodes the submitted PNG and feeds the shared `CapturePipeline`.
3. Pipeline iterates all registered `Arc<dyn CaptureSink>` sinks and calls `submit()` on each.
4. An agent retrieves frames by calling the `get_frame` MCP tool (returns the Nth-newest frame as `image/png` base64; index 0 = latest) or `GET /frame[?index=N]`.

## Essential Commands

```bash
# Build (workspace)
cargo build --workspace

# Build specific crate
cargo build -p radredeye-core

# Run all workspace tests
cargo test --workspace

# Run core tests only
cargo test -p radredeye-core

# Lint the whole workspace
cargo clippy --workspace -- -D warnings

# Build docs (warning-free)
cargo doc --no-deps --workspace

# Check specific crate
cargo check -p radredeye-core
cargo check -p radredeye-bevy

# Run Bevy example (requires display/GPU)
cargo run -p radredeye-bevy --example simple_capture

# Run the MCP stateless server (the agent interface)
cargo run -p radredeye-mcp

# Regression / guardrails gates (DevGate executors)
python3 scripts/devgate/regression_check.py --all --no-audit --no-settings
node scripts/devgate/guardrails-scan.mjs
```

## Feature Flags

Defined in `crates/radredeye-core/Cargo.toml`:

| Feature | Default | Enables |
|---------|---------|---------|
| `file-sink` | yes | `FileSink` + `image` dep (PNG encoding) |
| `http-sink` | yes | `HttpSink` + `ureq` dep |
| `websocket-sink` | no | `WebSocketSink` + `tungstenite` dep |
| `grpc-sink` | no | `GrpcSink` + `tonic`/`prost`/`tokio` deps |

`StdoutSink` is always available (no feature gate).

## Known Build Issues

None. All six workspace crates compile clean; 20 tests pass; `cargo clippy --workspace -- -D warnings` and `cargo doc --no-deps --workspace` are warning-free.

## Environment Variables

| Variable | Used by | Purpose |
|----------|---------|---------|
| `RADREDEYE_HTTP_SINK_URL` | Bevy example, `radredeye-mcp` | If set, adds an `HttpSink` that POSTs PNG frames to this URL |

## Code Patterns & Conventions

- **Trait-object sinks**: Sinks implement `CaptureSink` (requires `Send + Sync`), registered as `Arc<dyn CaptureSink>`. New sinks go in `crates/radredeye-core/src/sinks/` as submodules.
- **Feature gating**: Optional sinks use `#[cfg(feature = "...")]` and optional deps in `Cargo.toml`.
- **Bevy resource wrapper**: `BevyCapturePipeline` uses `Deref`/`DerefMut` to `CapturePipeline` so you call `.add_sink()` directly on the resource.
- **Capture opt-in**: In Bevy, spawn a camera with `CaptureCamera::enabled()` — cameras without this marker are ignored. Default throttle is 1 second.
- **Bevy screenshot pattern**: Uses Bevy 0.15's observer API — `Screenshot::window()` + `.observe(on_screenshot)`.
- **MCP server is synchronous**: `radredeye-mcp` uses `tiny_http` (blocking). No async runtime. The `POST /mcp` route is stateless JSON-RPC 2.0 (no `Mcp-Session-Id`).
- **PNG encoding**: Shared via `sinks::encode_png()` and `sinks::rgba_bytes()` helpers; BGRA→RGBA conversion is automatic.

## Godot Addon

Located at `engines/godot/addons/radredeye_capture/`. GDScript `@tool` that auto-captures viewport frames:
- Saves PNGs to `user://screenshots` by default
- Can POST to the MCP server at `http://127.0.0.1:8765/capture` when `emit_to_bridge = true`
- Configurable interval via `capture_interval_seconds`

## Project Status

See [STATUS.md](STATUS.md) for current sprint and blockers.  
See [ROADMAP.md](ROADMAP.md) for the full phased sprint plan.

## Repo Quirks

- **Guardrails template residue**: Many files (`.guardrails/`, `scripts/`, `ci/`, `ide/`, `.cursor/`, `.opencode/`, `.claude/`) come from the [agent-guardrails-template](https://github.com/TheArchitectit/agent-guardrails-template). CI configs reference a Go MCP server and Python test suite that don't exist in this repo. Don't try to run `go test` or `make build` from CI configs.
- **CLAUDE.md** contains Agent-GDUI-2026 context and game design philosophy — read it for guardrails expectations but don't apply the Go/CQRS patterns to Rust code.
- **`.claudeignore`** excludes lockfiles, `node_modules/`, `dist/`, `build/`, `*.log`, `*.csv`, `*.svg`.
- **License**: Dual MIT/Apache-2.0.
- **Bevy version**: 0.15 (workspace dep).
- **Tests are concentrated in `radredeye-core`, `radredeye-bevy`, and `radredeye-mcp`** (not every crate has them). Run `cargo test --workspace`.

## Adding a New Sink

1. Create `crates/radredeye-core/src/sinks/my_sink.rs`
2. Implement `CaptureSink` trait (`submit(&self, frame: &CapturedFrame) -> Result<(), SinkError>`)
3. Add `pub mod my_sink;` to `crates/radredeye-core/src/sinks.rs`
4. If it has an optional dep, gate it behind a feature flag in `Cargo.toml`
5. Add integration test in `crates/radredeye-core/src/lib.rs` tests module

## Adding a New Adapter (any app framework)

The universal capture path requires no engine SDK and no Rust crate — just produce a base64 PNG:

1. Start the MCP server: `cargo run -p radredeye-mcp`
2. From your app, `POST /capture` (or call the `submit_frame` MCP tool) with a base64-encoded PNG body.
3. The server decodes the PNG into a `CapturedFrame` and feeds the shared `CapturePipeline` + `ReplayBuffer`.
4. Agents retrieve frames via `get_frame` (MCP) or `GET /frame[?index=N]`.

For a deeper, in-process integration (when you control the app's build):

1. Create a new crate under `crates/` (e.g., `radredeye-unity`)
2. Depend on `radredeye-core = { path = "../radredeye-core" }`
3. Copy the framework's backbuffer into a `CapturedFrame` and call `pipeline.submit()`
4. Register sinks via `CapturePipeline::add_sink(Arc::new(...))`
