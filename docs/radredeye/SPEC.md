# radredeye — System Specification & Phased Plan

> **radredeye** is a framework-agnostic visual capture pipeline for AI agents.
> Its agent-facing interface is the **MCP stateless protocol**: any AI agent
> that speaks Model Context Protocol can list capture streams, pull frames as
> PNG, and submit frames from any application framework.

This document is the authoritative specification for radredeye (formerly
"Vision Enabler") and a forward-looking roadmap. Every API, route, feature
flag, and constant named here is verified against the in-tree source at the
time of writing.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Design Principles](#2-design-principles)
3. [Architecture](#3-architecture)
4. [The Adapter Model](#4-the-adapter-model)
5. [MCP Interface Specification](#5-mcp-interface-specification)
6. [Feature Flags](#6-feature-flags)
7. [Security & Guardrails](#7-security--guardrails)
8. [Migration Notes](#8-migration-notes)
9. [Phased Roadmap](#9-phased-roadmap)
10. [Quickstart](#10-quickstart)

---

## 1. Overview

**radredeye** = framework-agnostic visual capture for AI agents, exposed over
MCP stateless.

A radredeye deployment is a single long-running **`radredeye-mcp`** server that
owns one shared `CapturePipeline` and one global `ReplayBuffer`. AI agents talk
to it over the **MCP Streamable HTTP transport** (stateless, no session id) at
`POST /mcp`. Application frameworks — game engines, desktop GUIs, browsers,
terminals — push frames in through the universal `submit_frame` MCP tool (or the
legacy `POST /capture` back-compat route). Sinks fan accepted frames out to
stdout / files / HTTP / WebSocket / gRPC / semantic-diff. Agents read the latest
frames back with `get_frame`.

One-liner:

> radredeye turns *any* application's rendered pixels into a tool an MCP-speaking
> AI agent can call.

---

## 2. Design Principles

| # | Principle | How radredeye honours it |
|---|-----------|--------------------------|
| P1 | **Framework-agnostic** | The only thing an app framework must produce is a PNG (base64) handed to `submit_frame`. No game-engine SDK is required. |
| P2 | **MCP stateless is the agent interface** | The agent-facing surface is JSON-RPC 2.0 at `POST /mcp`, protocolVersion `2025-06-18`, no `Mcp-Session-Id`. Each request is independent. |
| P3 | **Sinks are output fan-out, not the interface** | Sinks (`StdoutSink`, `FileSink`, `HttpSink`, `WebSocketSink`, `GrpcSink`, `SemanticDiffSink`) push frames *out* of radredeye; agents never talk to sinks directly. |
| P4 | **Guardrails enforced** | The repo is built on `agent-guardrails-template`: pattern rules forbid `.unwrap()`, `unsafe`, `panic!`, `todo!`, `unimplemented!`, and unchecked derefs; DevGate gates enforce line limits; `cargo clippy -- -D warnings` and `cargo audit` must pass. |
| P5 | **No hidden capture** | Capture is always opt-in via an explicit adapter or an explicit `submit_frame` call. radredeye never injects itself into a process. |
| P6 | **Performance budget** | Pipeline-core dispatch p95 latency ≤ 3 ms at 1080p (`LATENCY_BUDGET_P95_MS = 3.0`), enforced by criterion benches. |
| P7 | **Structured telemetry** | `PipelineMetrics` emits `radredeye_frame_submitted_total`, `radredeye_frame_dropped_total{reason}`, `radredeye_sink_errors_total{sink}`, and the `radredeye_frame_latency_ms` histogram. Facade macros no-op when no global recorder is installed. |

---

## 3. Architecture

### 3.1 Crates

| Crate | In default build? | Purpose |
|-------|-------------------|---------|
| `radredeye-core` | yes | Engine-agnostic frame types (`CapturedFrame`, `PixelFormat`), `CaptureSink` trait, `CapturePipeline` facade over `StreamRegistry`, `ReplayBuffer` (cap 30), built-in sinks, `CaptureConfig`, `PipelineMetrics`. |
| `radredeye-bevy` | yes | Bevy 0.15 plugin + `CaptureCamera` marker + observer-based screenshot adapter. |
| `radredeye-mcp` | yes | **The agent interface.** Stateless MCP server + legacy back-compat HTTP routes; wraps the shared pipeline + replay buffer. |
| `radredeye-unity` | yes (member) | Unity adapter (platform-specific; excluded from default build path in practice). |
| `radredeye-unreal` | yes (member) | Unreal adapter (platform-specific; excluded from default build path in practice). |
| `radredeye-webxr` | **no** | WebXR adapter, excluded via workspace `exclude`. |

> Note: `radredeye-webxr` is listed in the workspace `exclude` list, so a plain
> `cargo build --workspace` does not compile it. `radredeye-unity` and
> `radredeye-unreal` are workspace members but are platform-specific adapters
> kept out of the default agent-facing build.

### 3.2 Data flow

```
                       app framework / engine adapter
                                  │
                                  ▼
                         CapturedFrame { w, h, format, data, timestamp }
                                  │
                       CapturePipeline::submit / submit_to
                                  │
                  ┌───────────────┴────────────────┐
                  ▼                                ▼
        CaptureConfig applied                backpressure check
        (resize / format / min_interval)     (drop if too soon)
                  │                                │
                  └───────────────┬────────────────┘
                                  ▼
                      fan-out to every CaptureSink
                    (stdout / file / http / ws / grpc / semdiff)
                                  │
                                  ▼
                       ReplayBuffer (ring, cap 30)
                                  │
                                  ▼
                  MCP server reads Nth-newest via get_frame
```

The MCP server and the legacy HTTP routes share the *same* `Arc<Mutex<CapturePipeline>>`
and `Arc<ReplayBuffer>`, so an agent reading `get_frame` and an app pushing
`submit_frame` observe identical frames.

### 3.3 Stream × sink matrix

A `CapturePipeline` always has a `"default"` stream. Named streams are created
with `CapturePipeline::create_stream(id)`. Each stream carries its own sinks,
its own `CaptureConfig`, and an optional `ReplayBuffer`. The MCP `list_streams`
tool enumerates streams with `sink_count` and `has_replay`; `list_sinks`
enumerates the `kind()` labels of the sinks on a stream (defaulting to
`"default"`).

---

## 4. The Adapter Model

radredeye distinguishes two roles:

- **App-framework adapters** — concrete integrations that live *inside* a host
  application process and copy its rendered pixels into a `CapturedFrame` (or a
  base64 PNG). In-tree adapters: **Bevy** (`radredeye-bevy`, Bevy 0.15 plugin +
  `CaptureCamera` marker + observer screenshot), **Unity** (`radredeye-unity`),
  **Unreal** (`radredeye-unreal`), **WebXR** (`radredeye-webxr`, excluded), and
  the **Godot** addon at `engines/godot/addons/radredeye_capture/`
  (`plugin.cfg`, `screenshot_autosave.gd`).
- **Universal capture path** — *any* framework (not just a game engine) can
  submit a base64 PNG via the MCP `submit_frame` tool or the legacy
  `POST /capture` route. No adapter SDK is required. Desktop GUIs, browsers, and
  terminal/TUI apps all use this path.

An adapter's only job is to produce pixels + dimensions + a timestamp; it never
talks to sinks or to the MCP transport directly.

---

## 5. MCP Interface Specification

This is the centerpiece of radredeye: the agent-facing surface.

### 5.1 Transport

| Property | Value |
|----------|-------|
| Endpoint | `POST /mcp` |
| Transport | MCP **Streamable HTTP**, **stateless** |
| Session | **None.** No `Mcp-Session-Id` header is issued or required. `initialize` is answered but not enforced. |
| protocolVersion | `2025-06-18` |
| Wire format | JSON-RPC 2.0 |
| Server capabilities | `tools` |
| Server info | `{ "name": "radredeye", "version": "<crate version>" }` |
| Listen address | `0.0.0.0:8765` |

### 5.2 Request / response shape

**Request** (JSON-RPC 2.0):

```json
{ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
  "params": { "name": "get_frame", "arguments": { "index": 0 } } }
```

**Response** (success):

```json
{ "jsonrpc": "2.0", "id": 1,
  "result": { "content": [ ... ], "isError": false } }
```

**Response** (error):

```json
{ "jsonrpc": "2.0", "id": 1,
  "error": { "code": -32601, "message": "method not found: foo" } }
```

**Notifications** — requests with no `id` (e.g.
`notifications/initialized`) get HTTP `202` with an empty body and **no**
JSON-RPC response.

### 5.3 Supported methods

| Method | Behaviour |
|--------|-----------|
| `initialize` | Returns `protocolVersion`, `capabilities.tools`, `serverInfo`. Stateless: no session is created. |
| `ping` | Returns `{}`. |
| `tools/list` | Returns the five-tool catalog below. |
| `tools/call` | Dispatches to a tool by `params.name` with `params.arguments`. |
| `resources/list` | Returns `{ "resources": [] }` (none defined). |
| `prompts/list` | Returns `{ "prompts": [] }` (none defined). |
| *(other)* | JSON-RPC error `-32601 method not found`. |

### 5.4 Tool catalog

#### `list_streams`
List capture streams and their sink counts.

```json
{ "name": "list_streams",
  "description": "List capture streams and their sink counts.",
  "inputSchema": { "type": "object", "properties": {} } }
```

Returns a text content block with a pretty-printed JSON array of
`{ "id", "sink_count", "has_replay" }` objects.

#### `list_sinks`
List the sink kinds attached to a stream (default: `"default"`).

```json
{ "name": "list_sinks",
  "description": "List the sink kinds attached to a stream (default: 'default').",
  "inputSchema": { "type": "object",
    "properties": { "stream": { "type": "string", "description": "stream id" } } } }
```

Returns a text content block with a pretty-printed JSON array of `kind()`
labels (e.g. `["stdout","file","http"]`).

#### `get_frame`
Return the Nth-newest captured frame as a PNG image. `index = 0` = latest.

```json
{ "name": "get_frame",
  "description": "Return the Nth-newest captured frame as a PNG image. index 0 = latest.",
  "inputSchema": { "type": "object",
    "properties": {
      "stream": { "type": "string", "description": "stream id" },
      "index":  { "type": "integer", "description": "0 = newest" } } } }
```

Returns an `image` content block:

```json
[{ "type": "image", "data": "<base64 png>", "mimeType": "image/png" }]
```

Resolution order: the named stream's attached replay buffer first, then the
global frame store. If no frame exists at `index` on `stream`, returns a
JSON-RPC error `-32000` with message `no frame at index N on stream 'S'`.

#### `submit_frame`
Submit a base64-encoded PNG frame into a stream. **Universal capture path** for
any app framework.

```json
{ "name": "submit_frame",
  "description": "Submit a base64-encoded PNG frame into a stream. Universal capture path for any app framework.",
  "inputSchema": { "type": "object",
    "properties": {
      "png_base64": { "type": "string", "description": "base64 PNG" },
      "stream":     { "type": "string", "description": "stream id" } },
    "required": ["png_base64"] } }
```

The server base64-decodes the PNG, decodes it to RGBA8 (`PixelFormat::Rgba8`),
stamps `Instant::now()`, submits to the named stream (default `"default"`), and
pushes a copy to the global frame store. Returns a text content block:
`submitted frame to stream '<stream>'`. Invalid base64 → `-32602`; invalid PNG
→ `-32602`.

#### `health`
Liveness + pipeline stats.

```json
{ "name": "health",
  "description": "Liveness + pipeline stats.",
  "inputSchema": { "type": "object", "properties": {} } }
```

Returns a text content block with JSON:

```json
{ "status": "ok",
  "protocol": "mcp-stateless",
  "protocol_version": "2025-06-18",
  "streams": ["default"],
  "default_sinks": 2,
  "frame_store_len": 0 }
```

### 5.5 Worked example: `submit_frame` then `get_frame` roundtrip

Submit a 1×1 red PNG (base64 elided):

```jsonc
// → POST /mcp
{ "jsonrpc": "2.0", "id": 5, "method": "tools/call",
  "params": { "name": "submit_frame",
              "arguments": { "png_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNk+P+/HgAFhAJ/wlseKgAAAABJRU5ErkJggg==" } } }

// ← 200 application/json
{ "jsonrpc": "2.0", "id": 5,
  "result": { "content": [{ "type": "text", "text": "submitted frame to stream 'default'" }],
              "isError": false } }
```

Read it back (index 0 = newest):

```jsonc
// → POST /mcp
{ "jsonrpc": "2.0", "id": 6, "method": "tools/call",
  "params": { "name": "get_frame", "arguments": { "index": 0 } } }

// ← 200 application/json
{ "jsonrpc": "2.0", "id": 6,
  "result": { "content": [{ "type": "image", "data": "iVBORw...", "mimeType": "image/png" }],
              "isError": false } }
```

### 5.6 Legacy back-compat routes

The same server also serves pre-MCP routes that share the pipeline + frame
store:

| Route | Purpose |
|-------|---------|
| `POST /capture` | Legacy PNG submission from any app framework (back-compat with the Godot addon). |
| `GET /frame[?index=N]` | Latest (or Nth-newest) stored frame as a raw PNG. |
| `GET /health` | Liveness probe (`{"status":"ok"}`). |

These are kept for existing adapters but are **not** the agent interface. New
agents should use `/mcp`.

### 5.7 Environment variables

| Variable | Effect |
|----------|--------|
| `RADREDEYE_HTTP_SINK_URL` | If set at server startup, registers an `HttpSink` pointed at this URL on the default stream. |

---

## 6. Feature Flags

All flags live on `radredeye-core`.

| Flag | Default | Effect |
|------|---------|--------|
| `file-sink` | **on** | Enables `FileSink` (writes PNGs to a directory). |
| `http-sink` | **on** | Enables `HttpSink` (POSTs PNGs to a URL). |
| `semantic-diff` | **on** | Enables `SemanticDiffSink` + `core::diff::pixel_diff`. Default path is pixel-diff (no model); the model path requires a review sink per guardrails Law 4. |
| `websocket-sink` | off | Enables `WebSocketSink`. |
| `grpc-sink` | off | Enables `GrpcSink` (pulls in `tonic`, `prost`, `tokio`). |

`radredeye-mcp` has no feature flags of its own; it depends on `radredeye-core`
with default features and exposes the server unconditionally.

---

## 7. Security & Guardrails

### 7.1 No hidden capture
radredeye never injects into a process. Capture is always explicit: an
app-framework adapter runs inside the host app and copies pixels, **or** an
external caller pushes a PNG via `submit_frame` / `POST /capture`. There is no
global screen-grab.

### 7.2 Opt-in adapters
Each adapter crate/addon is an explicit dependency the host app opts into.
Nothing is auto-loaded.

### 7.3 Guardrails (from `agent-guardrails-template`)
- **Pattern rules** (`.guardrails/`): `PREVENT-013` forbids `.unwrap()`; the
  `PREVENT-RUST-001..004` family forbids `unsafe`, `panic!`, `todo!`,
  `unimplemented!`, and unchecked dereferences.
- **DevGate line limits**: `scripts/devgate/regression_check.py` enforces a
  **600-line soft / 900-line hard** limit on `crates/**/*.rs`.
- **Clippy**: `cargo clippy --workspace -- -D warnings` must pass.
- **Audit**: `cargo audit` must pass.

### 7.4 Performance budget
`LATENCY_BUDGET_P95_MS = 3.0` — pipeline-core dispatch p95 latency must stay
≤ 3 ms at 1920×1080. Enforced by criterion benches (`submit_throughput`,
`resize_nearest`); a Phase 7.6 regression gate can fail PRs that regress p95.

### 7.5 Structured telemetry
`PipelineMetrics` (registered via `CapturePipeline::register_metrics`) emits:

| Facade metric | Type | Labels |
|---------------|------|--------|
| `radredeye_frame_submitted_total` | counter | — / `stream` |
| `radredeye_frame_dropped_total` | counter | `reason` / `+stream` |
| `radredeye_sink_errors_total` | counter | `sink` / `+stream` |
| `radredeye_frame_latency_ms` | histogram | — / `stream` |

Facade calls are no-ops when no global `metrics` recorder is installed, so
emission never panics. Internal counters are testable without a recorder.

### 7.6 Sink isolation
A sink returning `Err` from `CaptureSink::submit` is logged and **does not**
abort the pipeline or other sinks.

---

## 8. Migration Notes

### 8.1 Daemon → `radredeye-mcp`
The previous design used a bespoke daemon HTTP bridge (`/capture`, `/frame`).
That bridge is retained as **legacy back-compat** but is no longer the
agent-facing interface. The agent-facing interface is now the **stateless MCP
server** at `POST /mcp`. Both share the same pipeline and frame store, so a
migration is non-breaking: existing adapters keep POSTing to `/capture`, while
agents switch to MCP tools.

### 8.2 Connecting an AI agent
An MCP-capable agent connects to `http://localhost:8765/mcp` as a Streamable
HTTP MCP server. Because the server is stateless, the agent may skip
`initialize` (it is answered but not enforced) and call `tools/list` /
`tools/call` directly.

Minimal curl smoke test:

```bash
# list tools
curl -s localhost:8765/mcp -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'

# submit a frame (base64 PNG omitted)
curl -s localhost:8765/mcp -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"submit_frame","arguments":{"png_base64":"<...>"}}}'

# read the latest frame back
curl -s localhost:8765/mcp -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_frame","arguments":{"index":0}}}'
```

---

## 9. Phased Roadmap

The MCP stateless server + universal `submit_frame` path is the **Phase A**
baseline that already exists. Forward phases extend it without breaking the
stateless contract.

### Phase A — Ship MCP stateless + adapters *(current)*
- ✅ Stateless MCP server at `POST /mcp`, protocolVersion `2025-06-18`.
- ✅ Five tools: `list_streams`, `list_sinks`, `get_frame`, `submit_frame`, `health`.
- ✅ Universal capture via `submit_frame` (base64 PNG from any framework).
- ✅ Bevy adapter; Godot addon; Unity/Unreal/WebXR adapter crates.
- ✅ Sinks: stdout, file, http (default), websocket, grpc, semantic-diff.
- ✅ `ReplayBuffer` (cap 30) shared between MCP and legacy routes.
- ✅ Guardrails, clippy `-D warnings`, DevGate line limits, telemetry.

### Phase B — Richer MCP tools
- `list_sinks` per *named* stream (currently only `"default"` labels via
  `sink_kinds()`; named-stream path exists in core but expose it cleanly).
- A `semantic_diff` MCP tool wrapping `core::diff::pixel_diff` /
  `SemanticDiffSink` so an agent can request a diff frame between two indices.
- `get_frame` metadata variant: return dimensions/format/timestamp without the
  full PNG (cheaper polling).

### Phase C — stdio MCP transport for local desktop agents
- Add an MCP **stdio** transport variant (same `McpServer` core, different
  transport) so a local desktop agent can spawn `radredeye-mcp` as a subprocess
  without HTTP. Keeps the stateless tool catalog identical.

### Phase D — More first-party adapters
- Desktop window capture adapter (OS-native: DXGI/CGWindowCapture/X11).
- Browser / DOM adapter (CDP-based) submitting frames via `submit_frame`.
- Terminal / TUI adapter (vt100 render → PNG).
- Promote Unity/Unreal/WebXR adapters to first-class, buildable status.

### Phase E — Replay streaming & frame annotations
- SSE-based frame *streaming* from the MCP server (a stateful *read* channel
  layered on top of the stateless tool surface) so an agent can subscribe to
  new frames instead of polling `get_frame`.
- Frame annotations: attach bounding boxes / labels to a `CapturedFrame` and
  return them alongside the PNG in `get_frame`.
- Larger / configurable `ReplayBuffer` cap and per-stream replay buffers
  surfaced through `list_streams`.

---

## 10. Quickstart

### Build
```bash
cargo build --workspace          # builds core, bevy, mcp (+ unity/unreal)
cargo clippy --workspace -- -D warnings
cargo test    --workspace
```

### Run the server
```bash
cargo run -p radredeye-mcp
# [radredeye-mcp] listening on 0.0.0.0:8765 (MCP stateless @ /mcp; legacy @ /capture /frame /health)
```

Optional: forward every accepted frame to an HTTP endpoint:
```bash
RADREDEYE_HTTP_SINK_URL=https://example.com/ingest cargo run -p radredeye-mcp
```

### Connect an agent
Point any MCP-capable agent at `http://localhost:8765/mcp` (Streamable HTTP,
stateless). Call `tools/list` to discover the five tools, then `tools/call` to
list streams, submit a frame from any app framework, and read frames back as
PNG image content blocks.

---

*This specification is verified against `crates/radredeye-core/src/lib.rs`,
`crates/radredeye-core/src/metrics.rs`, `crates/radredeye-mcp/src/mcp.rs`,
`crates/radredeye-mcp/src/main.rs`, and the workspace `Cargo.toml`.*
