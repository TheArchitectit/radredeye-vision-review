# radredeye

Framework-agnostic visual capture for AI agents, exposed over the MCP stateless protocol.  
Any application — game engines, desktop GUIs, browsers, terminals — can feed pixels in; any MCP-speaking agent can pull frames back out.

## What it does

- Captures rendered frames from ANY application framework (game engine, desktop app, browser, terminal).
- Encodes them into an agent-friendly format ([`CapturedFrame`](crates/radredeye-core/src/lib.rs)).
- Routes frames to configurable sinks: stdout, filesystem, HTTP, WebSocket, gRPC.
- Exposes the pipeline over the MCP stateless protocol via `radredeye-mcp` — agents call `get_frame` to retrieve frames; any framework calls `submit_frame` to feed frames in.
- Stays behind the guardrails in [`.guardrails/`](.guardrails/): no hidden capture, performance budgets, explicit opt-in.

## Repo layout

```
├── .guardrails/                 # Safety rules imported from agent-guardrails-template
├── crates/
│   ├── radredeye-core      # Framework-agnostic frame types + sink pipeline
│   ├── radredeye-bevy      # Bevy 0.15 plugin + resource wrapper
│   ├── radredeye-mcp       # MCP stateless server — the agent interface
│   ├── radredeye-unity     # Unity adapter (re-exports core types)
│   ├── radredeye-unreal    # Unreal adapter (re-exports core types)
│   └── radredeye-webxr     # WebXR adapter (excluded from default build)
├── engines/
│   └── godot/                   # Godot 4 addon
├── docs/                        # Guardrails + spec docs
└── scripts/                     # Setup helpers
```

> `radredeye-webxr` targets `wasm32` and is excluded from the default workspace
> build via the workspace `exclude` list.

## Quick start

```bash
# Build all default-workspace crates
cargo build --workspace

# Start the MCP stateless server (the agent interface)
cargo run -p radredeye-mcp
# → point any MCP client at http://localhost:8765/mcp
```

Example adapters that feed frames into the pipeline:

- **Bevy** — `cargo run -p radredeye-bevy --example simple_capture` (plugin wires the pipeline; GPU framebuffer extraction via Bevy Screenshot system (verified compiling + running)).
- **Godot** — enable the addon at `engines/godot/addons/radredeye_capture/` and set `emit_to_bridge = true` to POST PNGs to `http://127.0.0.1:8765/capture`.

Any other app can capture by sending a base64 PNG via the `submit_frame` MCP tool or `POST /capture` — no engine SDK required.

## Adapters

| Adapter | Status | Notes |
---------|--------|-------|
| Any framework | ✅ universal | `submit_frame` over MCP or `POST /capture` — just produce a base64 PNG; no SDK required |
| Bevy | 🚧 scaffold | Example adapter — plugin + resource ready; GPU extraction stub |
| Godot | ✅ addon | Example adapter — `engines/godot/addons/radredeye_capture` auto-saves screenshots |
| Unity | 📝 planned | Example adapter — C# / Unity Render Texture |
| Unreal | 📝 planned | Example adapter — SceneCaptureComponent2D |
| WebXR | 📝 planned | Example adapter — WebGL framebuffer (excluded from default build) |

## Guardrails

This project uses the [Agent Guardrails Template](https://github.com/TheArchitectit/agent-guardrails-template) as its operating system for AI-assisted development. Start here:

- [`docs/AGENT_GUARDRAILS.md`](docs/AGENT_GUARDRAILS.md)
- [`.guardrails/pre-work-check.md`](.guardrails/pre-work-check.md)
- [`CLAUDE.md`](CLAUDE.md)
- [`PROMPTING_GUIDE.md`](PROMPTING_GUIDE.md)

## License

MIT OR Apache-2.0
