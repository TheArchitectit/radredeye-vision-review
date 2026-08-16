# radredeye — Sprint Roadmap

**Last updated:** 2026-08-13  
**Current sprint:** Phase 10 — Maturing the Perception Plane (designed; 10.0 not yet started; companion `docs/radredeye/PHASE10_DESIGN.md`)

---

## How This Doc Works

Each phase lists **sprints** (discrete work units). Each sprint has:
- A clear deliverable
- Exit criteria (what "done" looks like)
- Estimated complexity (S / M / L)
- Dependencies on prior sprints

When starting a sprint, check off tasks as they complete. Update `STATUS.md` with the current phase and any blockers.

---

## Strategic Direction (post-rename)

radredeye has been renamed and reframed as a **framework-agnostic** visual
capture pipeline. The primary agent interface is now the **MCP stateless
protocol**, served by `radredeye-mcp` (which absorbed the old HTTP daemon).
Any application — game engines, desktop GUIs, browsers, terminals — can
submit frames via the `submit_frame` MCP tool or `POST /capture` with a
base64 PNG; no engine SDK is required. Agents retrieve frames via `get_frame`.
Full specification: [docs/radredeye/SPEC.md](docs/radredeye/SPEC.md).

---

## Phase 0 — Foundation  ✅ COMPLETE

| Sprint | Deliverable | Status |
|--------|-------------|--------|
| 0.1 | Rust workspace scaffold (`radredeye-core`, `radredeye-bevy`) | ✅ |
| 0.2 | `CapturedFrame`, `PixelFormat`, `CaptureSink` trait, `CapturePipeline` bus | ✅ |
| 0.3 | `StdoutSink` + 2 core tests passing | ✅ |
| 0.4 | Guardrails template imported, docs/CI scaffolded | ✅ |
| 0.5 | Godot addon ported from template | ✅ |

---

## Phase 1 — Core Sinks & Daemon Bridge  ✅ COMPLETE

Exit criteria: all three crates compile, daemon accepts Godot POSTs, sinks write files and POST HTTP. ✅ Phase complete (committed `f28fb4e`/`d8e0f14`).

| Sprint | Deliverable | Complexity | Status |
|--------|-------------|------------|--------|
| 1.1 | `FileSink` with PNG encoding (`feature = "file-sink"`) | S | ✅ committed `f28fb4e` |
| 1.2 | `HttpSink` with `ureq` POST (`feature = "http-sink"`) | S | ✅ committed `f28fb4e` |
| 1.3 | `radredeye-mcp` — HTTP bridge for Godot (`/capture` endpoint) | M | ✅ committed `f28fb4e` (fixed in 1.7) |
| 1.4 | Bevy plugin rewrite — observer-based screenshot API | M | ✅ committed `f28fb4e` |
| 1.5 | Godot addon: `emit_to_bridge` toggle + HTTP POST to daemon | S | ✅ committed `f28fb4e` |
| 1.6 | **Commit all uncommitted work** | S | ✅ committed `f28fb4e` |
| 1.7 | **Fix daemon build** (`for mut request in` in `main.rs:32`) | S | ✅ fixed + warning cleanup |

### Sprint 1.7 — Quick Fix
```diff
// crates/radredeye-mcp/src/main.rs:32
- for request in server.incoming_requests() {
+ for mut request in server.incoming_requests() {
```

---

## Phase 2 — Testing & CI

Exit criteria: workspace-wide `cargo test` passes, CI runs on push.

| Sprint | Deliverable | Complexity | Dependencies | Status |
|--------|-------------|------------|--------------|--------|
| 2.1 | Unit tests for `FileSink` (writes PNG to temp dir) | S | Phase 1 | ✅ 2 tests |
| 2.2 | Unit tests for `HttpSink` (mock server or feature-gated) | S | Phase 1 | ✅ transport error test |
| 2.3 | Integration test for `CapturePipeline` fan-out to multiple sinks | S | 2.1 | ✅ pipeline_drops_invalid_frame |
| 2.4 | Integration test for daemon `/capture` endpoint (POST PNG, verify sink receives frame) | M | Phase 1.7 | ✅ 4 tests (lib.rs) |
| 2.5 | Bevy plugin test (spawn camera with `CaptureCamera`, verify frame submitted) | M | Phase 1 | ✅ 4 tests (headless) |
| 2.6 | GitHub Actions CI — `cargo build`, `cargo test`, `cargo clippy` | S | 2.1–2.5 | ✅ ci.yml |
| 2.7 | Clean up template residue CI configs (Jenkinsfile, gitlab-ci.yml) or remove | S | — | ✅ removed |

---

## Phase 3 — Additional Sinks ✅ COMPLETE

Exit criteria: WebSocket and gRPC sinks available behind feature flags, tested.

| Sprint | Deliverable | Complexity | Dependencies | Status |
|--------|-------------|------------|--------------|--------|
| 3.1 | `WebSocketSink` (`feature = "websocket-sink"`) — connect-and-push via `tungstenite` | M | Phase 2 | ✅ |
| 3.2 | `StdoutSink` EPIPE fix — ignore broken-pipe on `write_all` | M | — | ✅ |
| 3.3 | `GrpcSink` (`feature = "grpc-sink"`) — stream frames via tonic | L | Phase 2 | ✅ |
| 3.4 | Tests for `GrpcSink` | M | 3.3 | ✅ |
| 3.5 | Define `.proto` schema for frame streaming | M | — | ✅ |
| 3.6 | Update SUPPORT_MATRIX.md with new sinks | S | 3.1–3.4 | ✅ |

---

## Phase 4 — Engine Adapters ✅ COMPLETE

Exit criteria: at least one additional engine adapter functional (Unity or WebXR).

| Sprint | Deliverable | Complexity | Dependencies | Status |
|--------|-------------|------------|--------------|--------|
| 4.1 | `radredeye-unity` crate scaffold | S | Phase 2 | ✅ |
| 4.2 | Unity C# native plugin — captures backbuffer, POSTs to daemon bridge | L | 4.1 | ✅ |
| 4.3 | `radredeye-unreal` crate scaffold | S | Phase 2 | ✅ |
| 4.4 | Unreal plugin — viewport capture + POST to daemon bridge | L | 4.3 | ✅ |
| 4.5 | WebXR adapter — canvas capture via `wasm-bindgen`, sends to core pipeline | L | Phase 2 | ✅ |
| 4.6 | Update SUPPORT_MATRIX.md with engine status | S | 4.1–4.5 | ✅ |

---

## Phase 5 — Production Hardening  ✅ COMPLETE

Exit criteria: documented config, error recovery, observability.

| Sprint | Deliverable | Complexity | Dependencies | Status |
|--------|-------------|------------|--------------|--------|
| 5.1 | Configurable capture resolution & format (CaptureConfig, resize_nearest) | M | Phase 3 | ✅ |
| 5.2 | Frame dropping / backpressure — min_interval drops fast frames | M | Phase 3 | ✅ |
| 5.3 | Structured logging (`tracing` crate) across all crates | S | — | ✅ |
| 5.4 | Health check endpoint on daemon (`GET /health`) | S | Phase 1.7 | ✅ |
| 5.5 | Graceful shutdown — CaptureSink::on_shutdown() + pipeline.shutdown() | M | 5.2 | ✅ |
| 5.6 | Benchmark: frame capture + sink latency budget | M | 5.2 | ✅ |

---

## Phase 6 — Docs & Release  ✅ COMPLETE

Exit criteria: docs complete, crate published or tagged.

| Sprint | Deliverable | Complexity | Dependencies | Status |
|--------|-------------|------------|--------------|--------|
| 6.1 | Rustdoc for all public APIs | S | Phase 5 | ✅ |
| 6.2 | Usage guide: Bevy integration walkthrough | S | — | ✅ |
| 6.3 | Usage guide: Godot integration walkthrough | S | — | ✅ |
| 6.4 | Update ARCHITECTURE.md with final sink/engine matrix | S | Phases 3–4 | ✅ |
| 6.5 | Version bump, CHANGELOG, tag release | S | All prior | ✅ |
| 6.6 | Reconcile doc drift (AGENTS.md, SUPPORT_MATRIX.md, ROADMAP.md) | S | Phase 5 | ✅ |

---

## Phase 9 — radredeye as a DevGate Observation Source  ✅ COMPLETE

Exit criteria: capture telemetry emits to DevGate observability; golden-frame
regression harness; perception-assisted debugging hook (daemon frame-store +
`GET /frame`).

| Sprint | Deliverable | Complexity | Dependencies | Status |
|--------|-------------|------------|--------------|--------|
| 9.1 | Capture telemetry seam — `PipelineMetrics` + `metrics` facade; `submit` emits `frame_submitted_total`, `frame_dropped_total{reason}`, `sink_errors_total{sink}`, `radredeye_frame_latency_ms`; `LATENCY_BUDGET_P95_MS = 3 ms` shared with 5.6 benches | L | 5.6, Phase 7 | ✅ |
| 9.2 | Visual/screenshot regression — golden-frame comparison (`tests/golden/`); CPU-only deterministic harness + per-adapter goldens in CI separately | M | 6.2, 6.3 | ✅ |
| 9.3 | Perception-assisted debugging hook — daemon `FrameStore` ring buffer (cap 30) + `GET /frame[?index=N]` returning PNG | L | 9.1 | ✅ |

---

## Phase 10 — Maturing the Perception Plane  ✅ COMPLETE

Exit criteria: the perception plane supports runtime sink attach/detach, multiple
named capture streams, core replay, and a reusable diff primitive — without
breaking existing callers. Companion design:
`docs/radredeye/PHASE10_DESIGN.md`.

| Sprint | Deliverable | Complexity | Dependencies | Status |
|--------|-------------|------------|--------------|--------|
| 10.0 | `CapturePipeline` restructure — `Clone` + `Send` + `Sync` facade over `StreamRegistry`; new `SinkRegistry`/`CaptureStream`/`ReplayBuffer` modules; legacy `add_sink`/`configure` become `#[deprecated]` `&self` wrappers; in-tree callers migrated | L | — | ✅ |
| 10.1 | Dynamic sink registration / unsubscribe API (`register_sink`/`unregister_sink` on `&self`) + integration test | M | 10.0 | ✅ |
| 10.2 | Named capture streams / registry routing (`create_stream`/`stream`/`submit_to`); `"default"` preserves legacy `submit` | M | 10.1 | ✅ |
| 10.3 | Replay ring-buffer promoted to core (`ReplayBuffer`); daemon `FrameStore` becomes a deprecated re-export alias | M | 9.3, 10.0 | ✅ |
| 10.4 | Reusable `diff` module (`pixel_diff` generalises the 9.2 golden check) + feature-gated `SemanticDiffSink` (pixel-fallback default) | L | 10.0, 9.1 | ✅ |
| 10.5 | `GrpcSink` thread-soundness fix — process-global `OnceLock<Runtime>` shared across instances; ≥4-thread stress test | M | 10.0 | ✅ |

---

## Quick Reference: Sprint Priorities

**Current:** Phase 10 — Maturing the Perception Plane **complete**; all 10 phases
done (46/46 sprints).

**Next up**: none — all sprints complete (46/46).

---

## Phase 11 — MCP-First, Framework-Agnostic  ⬜ PLANNED

Exit criteria: MCP stateless is the sole, well-tooled agent interface;
framework-agnostic capture works for desktop, browser, and terminal apps;
replay streaming and frame annotations are available to agents.

| Sprint | Deliverable | Complexity | Dependencies | Status |
|--------|-------------|------------|--------------|--------|
| 11.1 | MCP stateless as the primary interface + richer tools — streaming/SSE frame delivery, per-named-stream `list_sinks`, and a `semantic_diff` MCP tool (exposing `SemanticDiffSink` over the protocol) | L | Phase 10 | ⬜ |
| 11.2 | stdio MCP transport for local desktop agents (JSON-RPC 2.0 over stdio, alongside the existing HTTP transport) | M | 11.1 | ⬜ |
| 11.3 | More first-party adapters — desktop window capture (OS window/framebuffer grab), browser/DOM capture (Canvas/requestAnimationFrame), terminal TUI capture (vt/ANSI screen state → PNG) | L | 11.1 | ⬜ |
| 11.4 | Replay streaming + frame annotations — agents can subscribe to a named stream and receive replayed frames; frames carry optional metadata annotations (source, timestamp, adapter tag) | L | 11.1, 11.3 | ⬜ |

---

*To start a sprint: pick the lowest unblocked ⬜ sprint, check it off when done, update STATUS.md.*
