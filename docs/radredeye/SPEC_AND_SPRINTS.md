# radredeye — Full System Specification & Granular Sprint Plan

**Status:** Specification only. No source/Cargo/crate changes are made by this document (Law 1 scope; read-only toolset).
**Builds on:** `docs/radredeye/ROADMAP_INTEGRATION.md` (review + integration strategy narrative — not repeated here), `STATUS.md`, `ROADMAP.md`, `docs/radredeye/SPRINT_WORKFLOW.md`.  
**Companion roadmap (review + integration strategy):** **[`ROADMAP_INTEGRATION.md`](ROADMAP_INTEGRATION.md)**.
**Supersedes false premises in the integration roadmap** with code-grounded facts from a senior review (findings F1–F12 below).
**Persist path:** `docs/radredeye/SPEC_AND_SPRINTS.md`

### Corrective premises applied (senior code review — trust, do not re-derive)

| # | Premise corrected | Evidence |
|---|---|---|
| F1 | **6 crate dirs**, not 3. `Cargo.toml` `members=["crates/*"]`, `exclude=["crates/radredeye-webxr"]`. **5 members**; `webxr` excluded (never built/tested by CI). Test count **20** (12 core + 4 daemon + 4 bevy). `AGENTS.md` is stale. | `Cargo.toml`; `crates/` listing; `crates/radredeye-webxr/Cargo.toml`; `ROADMAP_INTEGRATION.md §2` |
| F2 | **Dynamic sink registration is blocked.** `add_sink(&mut self)` / `configure(&mut self)` need exclusive `&mut self`; `sinks: Vec<Arc<dyn CaptureSink>>` has no interior mutability. Once shared (daemon's `Arc<Mutex<CapturePipeline>>`) you cannot add sinks / retarget config. Phases 9.3 & 10 require `Arc<Mutex<Vec<…>>>` (or `RwLock`) + `submit(&self)`. | `crates/radredeye-core/src/lib.rs` `CapturePipeline` (`sinks`, `add_sink`, `configure`) |
| F3 | **Zero telemetry.** `submit(&self, frame)` returns `()`; no metrics/histogram; drops/errors only logged. Origin latency **destroyed** at daemon: `handle_capture` sets `timestamp = Instant::now()` on ingest (`lib.rs`), discarding engine render time; `Instant` is non-serializable. Need serializable origin `Duration`/`u64` on `CapturedFrame`. | `lib.rs` `submit`; `crates/radredeye-mcp/src/lib.rs:57` `handle_capture` |
| F4 | **Daemon cannot serve a frame (9.3 blocked).** Only `POST /capture` + `GET /health`; stores no frame; no `GET /frame`. 9.3 needs a last-frame ring buffer + `GET /frame`. Daemon is single-threaded blocking `tiny_http` — cannot capture AND serve concurrently without restructuring. | `crates/radredeye-mcp/src/main.rs`; `lib.rs` |
| F5 | **`GrpcSink` unsound.** `sinks/grpc.rs` `submit` → `self.rt.block_on` on a `current_thread` runtime **per frame**; `Runtime` stored by value; unsafe if `submit` called from multiple threads (`Send+Sync` permits it); blocks capture thread. Fix for Phase 10. | `crates/radredeye-core/src/sinks/grpc.rs` `GrpcSink` |
| F6 | **Scanner severity gate vacuous for Rust (7.3 premise FALSE).** `guardrails-scan.mjs:32` filters to `critical`/`error` only. `PREVENT-013` (`unwrap`) & `PREVENT-024` (hallucinated import) are `"warning"` → dropped. Scanner ignores `forbidden_context` (only `new RegExp(pattern).test(line)`) → `PREVENT-013`'s non-test exclusion never applies → would false-positive on `#[cfg(test)]` unwraps (`daemon/lib.rs:57,102`). `PREVENT-024` pattern is npm-shaped (triple-underscore), inert for Rust. 7.3 must **bump 013/024 to `error` + patch severity gate + add test-module exclusion**; 8.2 needs an `expect()` rule. | `scripts/.../guardrails-scan.mjs:32`; `.guardrails/prevention-rules/pattern-rules.json`; `daemon/lib.rs` |
| F7 | **No pre-commit mechanism exists** (`.git/hooks` only `*.sample`; no husky/lefthook/cargo-husky/`.pre-commit-config.yaml`). BUT `agent-guardrails-template` (in-repo residue at `.claude/hooks/pre-commit.sh` + 5 workflow YAMLs) **ships** the hook & workflows. So 8.1 **COPY from template**, then add `cargo clippy/audit/scan`. DevGate scaffold has neither. | `.git/hooks/` listing; `find` over bevy repo; `agent-guardrails-template/.github/workflows/*` |
| F8 | **Failure registry empty** (header-only); the 1.7 daemon fix (`main.rs:32`) is NOT logged. | `.guardrails/failure-registry.jsonl`; `ROADMAP.md` sprint 1.7 |
| F9 | **`apply_config` always clones the full framebuffer before checking config** → masks real sink cost. Fix before 5.6 baseline. | `lib.rs` `apply_config` (`let mut out = frame.clone();`) |
| F10 | **`ARCHITECTURE.md` "Sinks are async-friendly" is FALSE** — fully synchronous, blocking (ureq/tungstenite/tonic block). Correct the 9/10 throughput spec. | `docs/radredeye/ARCHITECTURE.md`; `sinks.rs`, `sinks/grpc.rs` |
| F11 | **`workspace.package.repository` = agent-guardrails-template URL (WRONG).** Fix before 6.5 publish. | `Cargo.toml` `repository = "https://github.com/TheArchitectit/agent-guardrails-template"` |
| F12 | **Readiness verdicts:** 5.6 Ready-with-work; 6 Ready-with-work; 7 **BLOCKED** (7.3 premise); 8 Ready-with-work; 9 **BLOCKED** (no telemetry / daemon frame-serve); 10 **BLOCKED** (architectural restructure). | this review |

---

# PART A — FULL SYSTEM SPECIFICATION

## A.1 Scope & Context

radredeye is the **perception** plane of a three-layer agentic-dev stack: it gives AI agents eyes into running game engines by capturing rendered viewport frames (Bevy/Godot/Unity/Unreal/WebXR), normalizing them into a `CapturedFrame`, and fanning them out through `CapturePipeline` to trait-object sinks (stdout, filesystem, HTTP, WebSocket, gRPC). **DevGate** (`DevGate-Agentic-Framework`) is the **enforcement** plane — a polyglot (Node/Python) quality gate that validates, gates, and deploys agent-produced code via regression/pattern/semantic scanners, a failure registry, and prevention rules. **Guardrails** (`agent-guardrails-template`, the Four Laws) is the **safety/behavior** plane — read-before-edit, stay-in-scope, verify-before-commit, halt-when-uncertain — plus halt-conditions, three-strikes, and scope-validation. The thesis (from `ROADMAP_INTEGRATION.md`): *agents perceive through radredeye, behave under Guardrails, and ship only after DevGate enforces the gate.* This document specifies radredeye as it is, corrects the roadmap's false premises, and binds the three planes together.

## A.2 Architecture Spec

### Components
- **`radredeye-core`** — engine-agnostic: `CapturedFrame`, `PixelFormat`, `CaptureSink`, `CapturePipeline`, `CaptureConfig`, `SinkError`, built-in sinks (`StdoutSink`, `FileSink`, `HttpSink`, `WebSocketSink`, `GrpcSink`). Sink registry lives here.
- **`radredeye-bevy`** — Bevy 0.15 plugin (`RadredeyeCapturePlugin`), `CaptureCamera` marker, `BevyCapturePipeline` resource (Deref wrapper). Observer-based screenshot capture.
- **`radredeye-mcp`** — HTTP bridge for Godot. Listens `0.0.0.0:8765` (`tiny_http`, blocking). `POST /capture` (PNG decode → `CapturedFrame` → pipeline), `GET /health`. Holds an `Arc<Mutex<CapturePipeline>>`.
- **`radredeye-unity` / `radredeye-unreal`** — adapter crates (C# native plugin / C++ plugin) that POST backbuffers to the daemon. **`radredeye-webxr`** — `cdylib`/`wasm-bindgen` WebGL canvas adapter; **excluded** from the workspace (`Cargo.toml` `exclude`).
- **Godot addon** — `engines/godot/addons/radredeye_capture/` GDScript `@tool`; optional `emit_to_bridge` POST to daemon.
- **Sinks (CORRECTED per F10):** sinks are **synchronous and blocking**, not "async-friendly." `StdoutSink` locks stdout + writes metadata. `FileSink` encodes PNG + writes file. `HttpSink` uses blocking `ureq`. `WebSocketSink` uses blocking `tungstenite` (with auto-reconnect). `GrpcSink` blocks the calling thread on a per-frame `current_thread` tokio runtime (F5). A `submit()` call does not return until the sink's I/O completes; there is no async boundary inside core. Throughput is therefore bounded by the slowest blocking sink on the fan-out path.

### Data-flow (frame lifecycle)
1. Engine adapter copies backbuffer → builds `CapturedFrame { width, height, format, data, timestamp, <new origin field> }` and records a **serializable** origin timestamp (F3).
2. Adapter calls `pipeline.submit(&frame)` (`&self`).
3. `submit` applies backpressure (`min_interval` via `last_submit: Mutex<Instant>`), validates, applies `CaptureConfig` (resize/format), then fans out to every registered sink under a lock.
4. Each sink `submit`s; errors logged (F3: no metric today).
5. On shutdown, `pipeline.shutdown()` calls `on_shutdown()` on each sink.

### CapturePipeline dynamic-registration gap (F2) + proposed restructure
`CapturePipeline.sinks: Vec<Arc<dyn CaptureSink>>` with `add_sink(&mut self)` / `configure(&mut self)` is **immutable once shared**. The daemon already shares it as `Arc<Mutex<CapturePipeline>>`, but can only add sinks at startup. Phases 9.3 (on-demand sink) and 10 (multi-agent) *require* runtime registration.

**Proposed restructure (delivered in Phase 10.0, prerequisite for 9.3/10.x):**
```rust
pub struct CapturePipeline {
    sinks: Arc<Mutex<Vec<Arc<dyn CaptureSink>>>>,   // was: Vec<Arc<dyn CaptureSink>>
    config: Arc<RwLock<CaptureConfig>>,              // was: CaptureConfig
    last_submit: Arc<Mutex<Instant>>,
    metrics: Option<Arc<PipelineMetrics>>,           // F3
}
impl CapturePipeline {
    pub fn add_sink(&self, sink: Arc<dyn CaptureSink>) { self.sinks.lock().unwrap().push(sink); }
    pub fn configure(&self, cfg: CaptureConfig) { *self.config.write().unwrap() = cfg; }
    pub fn submit(&self, frame: &CapturedFrame) { /* lock sinks, fan out */ }
}
```
`CapturePipeline` becomes `Clone` + `Send+Sync` cheaply (all interior state is `Arc`), enabling `Arc<CapturePipeline>` to be cloned into engines, daemon, and agents. `BevyCapturePipeline` Deref wrapper continues to work (methods become `&self`).

## A.3 Interface / Contract Specs (signatures)

### `CapturedFrame` (corrected per F3)
```rust
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
    pub timestamp: Instant,          // in-process monotonic (kept)
    // NEW (F3): serializable engine-origin clock. Monotonic milliseconds since an
    // engine-defined epoch (e.g., process start). Set by the adapter; preserved
    // across daemon decode (do NOT overwrite with Instant::now() at ingest).
    pub origin_ms: u64,
}
```
**Units:** `width`/`height` in pixels; `data` RGBA8/BGRA8 (4 bytes/px); `timestamp` process-local `Instant`; `origin_ms` `u64` ms, monotonic, engine-scoped. Rationale: `Instant` is non-serializable and is clobbered at daemon ingest (`handle_capture` sets `Instant::now()`); `origin_ms` lets 9.1 measure engine→daemon→sink latency truthfully.

### `PixelFormat` (unchanged)
```rust
pub enum PixelFormat { Rgba8, Bgra8 }   // 4 bytes/px each
```

### `CaptureSink` (trait)
```rust
pub trait CaptureSink: Send + Sync {
    fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError>;
    fn on_shutdown(&self) {}   // default no-op
}
pub enum SinkError { Transport(String), Encoding(String) }
```

### `CapturePipeline` (current + proposed restructure signature)
Current (`lib.rs`): `new() -> Self`, `configure(&mut self, CaptureConfig)`, `add_sink(&mut self, Arc<dyn CaptureSink>)`, `submit(&self, &CapturedFrame)`, `sink_count(&self) -> usize`, `shutdown(&self)`, private `apply_config(&self, &CapturedFrame) -> CapturedFrame`. Proposed (Phase 10.0, A.2): `add_sink(&self, …)`, `configure(&self, …)`, `submit(&self, …)`, plus `register_metrics(&self, Arc<PipelineMetrics>)`, `set_frame_store(Arc<Mutex<VecDeque<CapturedFrame>>>)` for 9.3.

### `BevyCapturePipeline` Deref wrapper (unchanged shape)
```rust
#[derive(Resource)]
pub struct BevyCapturePipeline(pub CapturePipeline);
impl Deref   for BevyCapturePipeline { type Target = CapturePipeline; ... }
impl DerefMut for BevyCapturePipeline { ... }   // DerefMut retained pre-restructure; becomes &self after 10.0
```
`CaptureCamera { enabled: bool, throttle_seconds: Option<f32> }`; `CaptureCamera::enabled()` defaults `throttle_seconds = Some(1.0)`.

### Daemon REST API
- `GET /health` → `200 {"status":"ok"}` (current, `main.rs`).
- `POST /capture` → body = PNG bytes; `handle_capture(&Arc<Mutex<CapturePipeline>>, &[u8]) -> Result<(), String>`; on success `200 "ok"`, on decode error `400 "error: …"`. **CORRECTION (F3):** decode must preserve the frame's `origin_ms` from a request header/JSON envelope (currently the POST body is raw PNG with no origin channel — add an optional `X-Vision-Origin-Ms` header or multipart/JSON envelope carrying `origin_ms`).
- **PROPOSED `GET /frame` (9.3, F4):** returns the most-recent (or `?index=N`) captured frame as PNG from a ring buffer.
  - `GET /frame` → `200 image/png` (latest) or `404 {"error":"no frame"}`.
  - `GET /frame?index=N` → `200 image/png` (Nth-most-recent) or `404`.
  - Ring buffer: `Arc<Mutex<VecDeque<CapturedFrame>>>` capacity `FRAME_STORE_CAPACITY = 30`; fed by `submit` (or a wrapping sink). Single-threaded `tiny_http` serves it as another branch in the request loop (no new thread needed for the minimal version); concurrent capture+serve requires the async restructure in Phase 10.

### gRPC `.proto` (cite path `proto/radredeye.proto`, reproduce)
```proto
syntax = "proto3";
package radredeye;
enum PixelFormat { PIXEL_FORMAT_UNSPECIFIED=0; PIXEL_FORMAT_RGBA8=1; PIXEL_FORMAT_BGRA8=2; }
message CapturedFrame {
  uint32 width = 1; uint32 height = 2; PixelFormat format = 3;
  bytes data = 4; uint64 timestamp_ms = 5;   // NOTE: add origin_ms mapping in 9.1
}
message FrameAck { bool accepted = 1; }
service FrameStreaming { rpc StreamFrames (stream CapturedFrame) returns (FrameAck); }
```
Generated behind `feature="grpc-sink"`; `GrpcSink::connect(endpoint) -> Result<Self, SinkError>`. **F5 fix (10.5):** replace per-frame `current_thread` runtime + `block_on` with a shared `Runtime` (multi-thread or a process-global) so concurrent/multi-threaded `submit` is sound.

## A.4 Configuration Spec
- **`CaptureConfig`** (`lib.rs`): `target_width: Option<u32>`, `target_height: Option<u32>`, `target_format: Option<PixelFormat>`, `min_interval: Option<Duration>`. Applied in `apply_config` (resize via `resize_nearest`, format via `rgba_bytes`). **F9 fix:** short-circuit passthrough (no `frame.clone()`) when no transform is required, so benchmark/telemetry measures real sink cost.
- **Env vars:** `RADREDEYE_HTTP_SINK_URL` — if set, daemon/bevy add an `HttpSink`. (Only config-style var; not a secret — confirmed for 8.3.)
- **Cargo features** (`crates/radredeye-core/Cargo.toml`): `file-sink` (default, `image`), `http-sink` (default, `ureq`), `websocket-sink` (`tungstenite`), `grpc-sink` (`tonic`, `prost`, `tonic-build`). `StdoutSink` always available. Add `metrics` (optional, Phase 9.1) and `observability` feature if needed.

## A.5 Observability Spec (NEW per F3)
- **Existing:** `tracing` spans/events in `submit` (debug "frame dropped (backpressure)", warn "dropped invalid frame", error "sink submission failed"). No structured metrics.
- **NEW (9.1, net-new):**
  - `PipelineMetrics` struct behind `Arc`, registered via `register_metrics` (Phase 10.0 field). Emitted **inside `submit`** so every fan-out pays the same clock as the 5.6 criterion bench.
  - Metrics (use the `metrics` facade + a recorder; recommend `metrics` 0.21 + `metrics-exporter-prometheus` or a `PrintingRecorder` for CI):
    - `radredeye_frame_latency_ms` — **histogram**, observed = `submit` entry→fan-out complete (uses `origin_ms` when present to also derive engine→sink). **p50/p95/p99** reported.
    - `radredeye_frame_submitted_total` — counter.
    - `radredeye_frame_dropped_total{reason="backpressure"|"invalid"}` — counter.
    - `radredeye_sink_errors_total{sink="http"|"grpc"|…}` — counter.
  - **Shared latency budget (5.6 ↔ 9.1):** one constant `LATENCY_BUDGET_P95_MS` governs both the `criterion` `submit_throughput` assertion and the 9.1 regression gate. The criterion bench exercises the *same* `submit` path; 9.1 emits histograms for the *same* path in production. A PR that pushes p95 past the budget fails the guardrails gate (7.6/9.1).
  - **Where emitted:** default `PrintingRecorder` to stdout in dev; Prometheus exporter behind `observability` feature for deployment. Document that network sinks are excluded from the latency SLO (F10) — they are reported via `sink_errors_total`/throughput only.

## A.6 Non-Functional Requirements
- **Latency budget (proposed, concrete — justifies F9/F10):**
  - **B1 Pipeline core dispatch** (`submit` → all sinks returned, synthetic zero-cost sink): **p95 ≤ 3 ms @ 1920×1080 (8.3 MB)**. Justification: dominant cost is the in-memory `frame.clone()` (+`validate`); modern DRAM copy ≈ 1–3 ms for 8.3 MB; fan-out trivial. After F9 short-circuit, passthrough with empty config costs ~0 clone.
  - **B2 `StdoutSink` end-to-end:** **p95 ≤ 5 ms @ 1080p** (adds stdout lock + short metadata write).
  - **B3 `FileSink`/`HttpSink`:** **NOT latency-SLO'd** (PNG encode ~tens of ms + disk/network); throughput-only.
  - **B4 `GrpcSink`/`WebSocketSink`:** **best-effort, blocking, no latency SLO**; capture thread blocked (F5/F10).
- **Throughput target:** **≥ 60 frames/sec** for the B1 path @ 1080p on the benchmark host (`criterion submit_throughput`); **≥ 20 fps** `FileSink` @ 1080p. Realistic multi-sink throughput limited by the slowest blocking sink on the fan-out path.
- **Max file-size limits (DevGate regression, Rust-specific — F6):** `*.rs` **600 soft / 900 hard** lines. Justification: Rust is denser than the TS baseline DevGate uses (`SRC_SOFT=300`/`SRC_HARD=500` in `regression_check.py`); a trait-object pipeline module runs ~400–700 LOC; 900 hard leaves headroom for generated/proto-adjacent code. `*.gd` keep 500 hard; `*.cs`/`*.cpp` 600/900. Apply via `scripts/devgate/regression_check.py` `FILE_SIZE_DIRS` extension (7.2).
- **Backpressure semantics:** `min_interval` drops frames arriving sooner than the interval (logged, counted in `frame_dropped_total{reason="backpressure"}`); Bevy default camera throttle 1.0 s. No unbounded queueing.

## A.7 DevGate + Guardrails Integration Spec
- **CI workflows (author/adapt, trigger on PR + push to `main`):**
  - `guardrails.yml` (Phase 7.6): DevGate regression (`python scripts/devgate/regression_check.py --all --no-audit --no-settings --soft-as-hard`) + `node scripts/devgate/guardrails-scan.mjs` + `cargo audit` (RustSec) + `cargo clippy --workspace -- -D warnings`. Mirror template `regression-guard.yml` + `guardrails-lint.yml`. Add a **non-gating** `wasm32` build job for `radredeye-webxr` (`cargo build -p radredeye-webxr --target wasm32-unknown-unknown`) so the excluded crate is still smoke-checked.
  - `secret-validation.yml` (8.3): Gitleaks action (`gitleaks/gitleaks-action@v2`) + `.env`/credential file scan. `RADREDEYE_HTTP_SINK_URL` confirmed config-not-secret (documented exclusion).
  - `documentation-check.yml` (8.4): 500-line doc limit (`find *.md -exec wc -l`), required `## Overview`/`Last Updated:` sections, broken-internal-link check. Maps template `documentation-check.yml`.
  - Keep existing `ci.yml` (build/test/clippy fast lane).
- **Pre-commit hook (8.1, F7):** **COPY** the in-repo `.claude/hooks/pre-commit.sh` (guardrails-template residue, already present) and **install** via `git config core.hooksPath .claude/hooks` (currently `.git/hooks/` only has `*.sample`, so nothing runs). Then **extend** it with: `python3 scripts/devgate/regression_check.py --pre-commit --no-audit --no-settings`, `node scripts/devgate/guardrails-scan.mjs`, `cargo clippy --workspace -- -D warnings`, `cargo audit`. Do **not** author from scratch (the integration roadmap's "must author" premise was wrong — the template ships it).
- **Prevention-rules additions (8.2):**
  - `PREVENT-RUST-001` `panic!` outside `#[cfg(test)]`/`#[test]`.
  - `PREVENT-RUST-002` `unsafe` without a `// SAFETY:` (or `// Safety:`) comment on the preceding line.
  - `PREVENT-RUST-003` `.expect(`/`.expect("...")` **without** a descriptive (non-empty) message → covers the 3 daemon `.expect()`s (`main.rs:18,27,40`) that `PREVENT-013` misses (F6).
  - `PREVENT-RUST-004` `TODO|FIXME|HACK|XXX` without a ticket ref `#\d+`/`issue`/`ticket`.
  - **CRITICAL correction (7.3, F6):** (a) **bump `PREVENT-013` (`unwrap`) and `PREVENT-024` (hallucinated import) from `warning` → `error`** in `pattern-rules.json` so they survive the `guardrails-scan.mjs:32` `critical`/`error` filter; (b) **patch the scanner severity gate** to also surface `warning`-severity rules whose `file_glob` matches `*.rs` (or simply rely on the bump); (c) **add a test-module exclusion** — patch `guardrails-scan.mjs` per-line check to honor each rule's `forbidden_context` (currently ignored), so `#[cfg(test)]`/`#[test]` unwraps in `daemon/lib.rs:57,102` are not false-positives; (d) **improve `PREVENT-024`** pattern from npm-shaped triple-underscore `_\w+_\w+_\w+` to also catch Rust hallucinated `use` paths (e.g., `use\s+[\w]+_[\w]+_[\w]+`). Without (a–c) the 7.3 gate is **vacuous** — confirmed by code review.
- **Failure-registry entry (7.5, F8):** append to `.guardrails/failure-registry.jsonl` the Sprint 1.7 daemon fix: `failure_id` `FAIL-0001`, `category:"build"`, `severity:"high"`, `error_message:"cannot borrow ... as mutable, as it is not declared as mutable"` (the `for mut request in server.incoming_requests()` borrow error), `root_cause:"missing 'mut' on the request binding in the tiny_http loop"`, `affected_files:["crates/radredeye-mcp/src/main.rs:32"]`, `fix_commit:"f28fb4e"` (landed 1.7), `regression_pattern:"for request in server.incoming_requests\\(\\)"`, `prevention_rule:"PREVENT-RUST-002 (SAFETY) + code review"`, `status:"resolved"`. Proves the registry→rule→scanner loop end-to-end.
- **`cargo audit` replaces `npm audit` (7.4, F8/G8):** `regression_check.py` runs with `--no-audit` (no `package.json`); add a separate `cargo audit` step (RustSec advisory DB) as the Rust security gate; blocks on runtime HIGH/CRITICAL.
- **A/B vendoring decision (7.1):** **COPY** DevGate executors into `scripts/devgate/` (Option A) — keeps this repo's `.guardrails/` as single source of truth; the submodule option (B) would introduce a *second* `.guardrails/` (pi-mega-compact rules). Recorded as a **Law-4 halt** (see §Summary).

---

# PART B — GRANULAR SPRINT PLAN

> Style mirrors `ROADMAP.md` (Goal / Detailed design-spec / Acceptance criteria / Files to touch / Dependencies / Complexity / Effort / Definition of Done). Verdicts from F12 shown per phase. Readiness: **5.6 Ready-with-work; 6 Ready-with-work; 7 BLOCKED→unblocks after 7.3; 8 Ready-with-work; 9 BLOCKED; 10 BLOCKED.**

## Phase 5.6 — Benchmark Harness (Ready-with-work)
**5.6 Benchmark: Criterion harness + latency budget** — *M — 3 PD*
- **Goal:** Establish a reproducible capture→sink latency/throughput baseline that the 9.1 regression gate enforces.
- **Detailed design-spec:** Add `criterion` dev-dep to `radredeye-core`. Bench groups: `submit_throughput` (synthetic zero-cost sink + real `StdoutSink`, measure frames/sec @ 1080p/4K), `resize_nearest` (latency for 1920×1080→target). **Prerequisite (F9):** short-circuit `apply_config` passthrough — when `CaptureConfig` is empty, `submit` must NOT clone the framebuffer (currently `apply_config` always `frame.clone()`), so the bench measures real sink cost, not a forced copy. Record `LATENCY_BUDGET_P95_MS = 3.0` (B1) and throughput ≥ 60 fps in `BENCHMARKS.md`.
- **Acceptance criteria:** `cargo bench -p radredeye-core` runs; `submit_throughput` reports ≥ 60 fps @ 1080p; `resize_nearest` reports latency; `BENCHMARKS.md` documents p50/p95/p99 + the budget constant; `apply_config` has a passthrough fast-path unit test.
- **Files to touch:** `crates/radredeye-core/Cargo.toml`, `benches/*.rs` (new), `crates/radredeye-core/src/lib.rs` (`apply_config`), `BENCHMARKS.md` (new).
- **Dependencies:** 5.2 (backpressure) ✅.
- **Complexity:** M. **Effort:** 3 person-days. **DoD:** benches green; baseline numbers committed; F9 short-circuit merged.

## Phase 6 — Docs & Release (+ new 6.6 reconciliation) (Ready-with-work)
**6.1 Rustdoc for all public APIs** — *S — 1 PD*
- **Goal:** Warning-free rustdoc for every public item.
- **Spec:** `cargo doc --workspace --no-deps`; document `CapturedFrame`, `CaptureSink`, `CapturePipeline`, sinks, `BevyCapturePipeline`, daemon API. Add doc for the new `origin_ms` once 9.1/10.0 land.
- **Acceptance:** `cargo doc` exits 0 with no warnings; public items have doc comments.
- **Files:** all crate `lib.rs`/`sinks*.rs`/`bevy lib.rs`/`daemon lib.rs`. **Deps:** 5.6. **C:** S. **Effort:** 1 PD. **DoD:** `cargo doc` clean.

**6.2 Bevy integration walkthrough** — *S — 2 PD*
- **Goal:** Guide that builds & runs `simple_capture`. **Spec:** markdown under `docs/`; `cargo run -p radredeye-bevy --example simple_capture`. **Acceptance:** guide's commands succeed on a GPU host. **Files:** `docs/radredeye/bevy-guide.md`. **Deps:** —. **C:** S. **Effort:** 2 PD. **DoD:** guide verified.

**6.3 Godot integration walkthrough** — *S — 2 PD*
- **Goal:** Guide for the `radredeye_capture` addon + daemon. **Spec:** `engines/godot/addons/radredeye_capture/` usage; `cargo run -p radredeye-mcp`. **Acceptance:** addon + daemon flow documented & runnable. **Files:** `docs/radredeye/godot-guide.md`. **Deps:** —. **C:** S. **Effort:** 2 PD. **DoD:** guide verified.

**6.4 Final `ARCHITECTURE.md` matrix** — *S — 1 PD*
- **Goal:** Accurate sinks×engines table. **Spec:** update `docs/radredeye/ARCHITECTURE.md` — **correct "Sinks are async-friendly" → "Sinks are synchronous/blocking" (F10)**; mark `webxr` excluded; list 6 crates/5 members. **Acceptance:** matrix matches `SUPPORT_MATRIX.md` + `Cargo.toml`. **Files:** `docs/radredeye/ARCHITECTURE.md`, `SUPPORT_MATRIX.md`. **Deps:** 3,4. **C:** S. **Effort:** 1 PD. **DoD:** doc corrected & reviewed.

**6.5 Version bump, CHANGELOG, tag v0.1.0** — *S — 1 PD*
- **Goal:** Publish-ready release. **Spec:** bump `version` in `Cargo.toml` workspace; write `CHANGELOG.md`; add `scripts/release.sh` wrapping `cargo publish` (clean tree → gate → tag → publish, per DevGate `deploy.sh` spirit). **F11 fix:** change `workspace.package.repository` from `agent-guardrails-template` URL to the radredeye repo URL. **Acceptance:** `cargo publish --dry-run` clean; `repository` correct; `release.sh --check` passes. **Files:** `Cargo.toml`, `CHANGELOG.md` (new), `scripts/release.sh` (new). **Deps:** 5.6, 6.1–6.4. **C:** S. **Effort:** 1 PD. **DoD:** release artifacts staged (tag optional).

**6.6 Drift reconciliation (new)** — *S — 1 PD*
- **Goal:** Kill stale docs. **Spec (F1):** rewrite `AGENTS.md` "Three crates…2 core tests…no bevy/daemon tests" → **6 crate dirs, 5 workspace members (webxr excluded), 20 tests (12 core + 4 daemon + 4 bevy)**; update `SUPPORT_MATRIX.md` (Unity/Unreal/WebXR = Phase 4 done, not "planned"); remove ROADMAP residual "implemented, uncommitted" notes (commit `f28fb4e` landed them). **Acceptance:** `AGENTS.md` crate/test counts match `Cargo.toml`+actual test run; `SUPPORT_MATRIX.md` matches ROADMAP. **Files:** `AGENTS.md`, `docs/radredeye/SUPPORT_MATRIX.md`, `ROADMAP.md`. **Deps:** —. **C:** S. **Effort:** 1 PD. **DoD:** docs consistent.

## Phase 7 — DevGate Adoption (Enforcement plane) — **BLOCKED until 7.3**
**7.1 Vendor DevGate executors** — *M — 3 PD*
- **Goal:** In-repo DevGate gate. **Spec:** copy `scripts/regression_check.py`, `guardrails-scan.mjs`, `log_failure.py` into `scripts/devgate/` (Option A). **A/B vendoring = Law-4 halt** (submodule vs copy). **Acceptance:** scripts present & runnable from repo root; decision recorded in `docs/radredeye/ROADMAP_INTEGRATION.md §6`. **Files:** `scripts/devgate/*`. **Deps:** —. **C:** M. **Effort:** 3 PD. **DoD:** scripts vendored; decision logged.

**7.2 Rust-aware `regression_check.py`** — *M — 2 PD*
- **Goal:** Size-gate Rust. **Spec:** extend `FILE_SIZE_DIRS` to `("src","extensions","crates","engines")`; add `*.rs`/`*.gd`/`*.cs`/`*.cpp` globs; Rust limits **600 soft / 900 hard** (F6/A.6); run with `--no-audit --no-settings` (no `package.json`/`MEGACOMPACT_*`). **Acceptance:** a seeded 950-line `*.rs` fails; `MEGACOMPACT_*`/npm paths skipped. **Files:** `scripts/devgate/regression_check.py`. **Deps:** 7.1. **C:** M. **Effort:** 2 PD. **DoD:** Rust files size-checked.

**7.3 Rust-aware `guardrails-scan.mjs` + severity/rules fix (CRITICAL, unblocks Phase 7)** — *L — 5 PD*
- **Goal:** Make the Rust pattern gate **non-vacuous**. **Spec (F6):** (a) `pattern-rules.json`: bump `PREVENT-013` (`\.unwrap\(\)`) and `PREVENT-024` (hallucinated import) `severity` `"warning"`→`"error"`; (b) patch `guardrails-scan.mjs:32` severity filter to include `warning` rules whose `file_glob` matches `*.rs` (or rely on bump); (c) **add test-module exclusion** — patch the per-line scan to honor each rule's `forbidden_context` (currently ignored via `new RegExp(pattern).test(line)` only), so `#[cfg(test)]`/`#[test]` unwraps (`daemon/lib.rs:57,102`) are exempt; (d) improve `PREVENT-024` to catch Rust hallucinated `use` paths; (e) extend `walk()` to emit `*.rs`/`*.gd`. **Acceptance:** seeding a `unwrap()` in a non-test `*.rs` **and** a test-module `unwrap()` (excluded) both behave correctly (former fails, latter passes); `PREVENT-024` fires on a bogus `use` import. **Files:** `scripts/devgate/guardrails-scan.mjs`, `.guardrails/prevention-rules/pattern-rules.json`. **Deps:** 7.1. **C:** L. **Effort:** 5 PD. **DoD:** gate non-vacuous on Rust; Phase 7 unblocked.

**7.4 `cargo audit` step** — *S — 1 PD*
- **Goal:** Rust security gate. **Spec:** add `cargo audit` (RustSec) step; blocks on runtime HIGH/CRITICAL. **Acceptance:** clean audit passes; a seeded advisory blocks. **Files:** CI (7.6), `scripts/devgate/`. **Deps:** 7.1. **C:** S. **Effort:** 1 PD. **DoD:** audit wired.

**7.5 Populate failure-registry (1.7 daemon bug)** — *S — 1 PD*
- **Goal:** Prove the loop. **Spec (F8):** append `FAIL-0001` (Sprint 1.7 `for mut request` borrow fix, `main.rs:32`, `fix_commit:"f28fb4e"`) to `.guardrails/failure-registry.jsonl`; cross-ref in `regression-guard.yml` comment step. **Acceptance:** `python scripts/devgate/log_failure.py --list` shows the entry; scanner references it. **Files:** `.guardrails/failure-registry.jsonl`. **Deps:** 7.1. **C:** S. **Effort:** 1 PD. **DoD:** entry present.

**7.6 `guardrails.yml` CI workflow** — *M — 2 PD*
- **Goal:** PR gate. **Spec:** `.github/workflows/guardrails.yml` = regression (`--no-audit --no-settings --soft-as-hard`) + `guardrails-scan.mjs` + `cargo audit` + `cargo clippy --workspace -- -D warnings`; **non-gating** `wasm32` build job for `radredeye-webxr`. **Acceptance:** PR with a Rust `unwrap()` violation fails; clean PR passes; webxr wasm job reports only. **Files:** `.github/workflows/guardrails.yml`. **Deps:** 7.2–7.4. **C:** M. **Effort:** 2 PD. **DoD:** gate enforces in CI.

## Phase 8 — Guardrails Enforcement + Rust Prevention Rules (Ready-with-work)
**8.1 Pre-commit hook (COPY + extend)** — *S — 1 PD*
- **Goal:** Local gate. **Spec (F7):** **copy** in-repo `.claude/hooks/pre-commit.sh` (do NOT author from scratch) and install `git config core.hooksPath .claude/hooks`; extend with `regression_check.py --pre-commit --no-audit --no-settings`, `guardrails-scan.mjs`, `cargo clippy --workspace -- -D warnings`, `cargo audit`. **Acceptance:** a commit adding a Rust `unwrap()` is blocked; keep the AI-attribution + trufflehog checks. **Files:** `.claude/hooks/pre-commit.sh`, `.git/config` (hookspath). **Deps:** 7.1–7.3. **C:** S. **Effort:** 1 PD. **DoD:** commit blocked on gate failure.

**8.2 Expand Rust prevention rules** — *M — 3 PD*
- **Goal:** Safety↔enforcement wiring. **Spec (F6):** add `PREVENT-RUST-001` (`panic!` outside tests), `PREVENT-RUST-002` (`unsafe` w/o `// SAFETY`), `PREVENT-RUST-003` (`.expect(` w/o message — covers `daemon/main.rs:18,27,40`), `PREVENT-RUST-004` (`TODO` w/o ticket). All `severity:"error"`, `file_glob:["*.rs"]`; honor `forbidden_context`. **Acceptance:** seeded violations in each category fail the scan; existing `#[cfg(test)]`/message-bearing `.expect()` pass. **Files:** `pattern-rules.json`, `scripts/devgate/guardrails-scan.mjs` (exclusion support already in 7.3). **Deps:** 7.3. **C:** M. **Effort:** 3 PD. **DoD:** rules live + scanned.

**8.3 `secret-validation.yml`** — *S — 1 PD*
- **Goal:** Secret gate. **Spec:** Gitleaks action + `.env`/credential scan; document `RADREDEYE_HTTP_SINK_URL` as config-not-secret. **Acceptance:** workflow runs; no false block on `RADREDEYE_HTTP_SINK_URL`. **Files:** `.github/workflows/secret-validation.yml`. **Deps:** —. **C:** S. **Effort:** 1 PD. **DoD:** secrets scanned.

**8.4 `documentation-check.yml`** — *S — 1 PD*
- **Goal:** Doc hygiene. **Spec:** 500-line limit + required sections + broken-link check on `docs/` (maps template `documentation-check.yml`). **Acceptance:** over-long doc fails; this SPEC_AND_SPRINTS.md must stay ≤ 500 lines or be split. **Files:** `.github/workflows/documentation-check.yml`. **Deps:** —. **C:** S. **Effort:** 1 PD. **DoD:** docs linted.

**8.5 Four-Laws operationalization** — *S — 1 PD*
- **Goal:** Make laws operational. **Spec:** add `CONTRIBUTING.md` referencing Four Laws + `skills/shared-prompts/four-laws.md`; CI note for read-before-edit. **Acceptance:** `CONTRIBUTING.md` present; laws cited. **Files:** `CONTRIBUTING.md` (new), CI note. **Deps:** —. **C:** S. **Effort:** 1 PD. **DoD:** laws documented.

## Phase 9 — radredeye as a DevGate Observation Source (BLOCKED: no telemetry / daemon frame-serve)
**9.1 Capture telemetry seam (net-new)** — *L — 5 PD*
- **Goal:** Emit frame-latency / drop-rate metrics that gate the 5.6 baseline. **Spec (F3):** add `PipelineMetrics` (the `metrics` facade + recorder) registered via a `register_metrics(&self, Arc<PipelineMetrics>)` method (Phase 10.0 field); emit `radredeye_frame_latency_ms` histogram, `frame_submitted_total`, `frame_dropped_total{reason}`, `sink_errors_total{sink}` **inside `submit`**. Share `LATENCY_BUDGET_P95_MS` (B1=3 ms) with the 5.6 criterion bench so the 7.6 gate fails PRs regressing p95. Also record `origin_ms` propagation (F3). **Acceptance:** running the pipeline emits histograms; a bench PR exceeding p95 fails `guardrails.yml`; metrics cover dropped frames. **Files:** `lib.rs` (`submit`, new `metrics` module), `Cargo.toml` (`metrics` dep), `BENCHMARKS.md`. **Deps:** 5.6, 10.0 (metrics field) — note 10.0 may be pulled forward. **C:** L. **Effort:** 5 PD. **DoD:** telemetry emitted + gated.

**9.2 Visual/screenshot regression** — *M — 3 PD*
- **Goal:** Golden-frame comparison across adapters. **Spec:** capture golden PNGs per adapter (Bevy/Godot/WebXR) into `tests/golden/`; compare on CI (pixel-diff threshold). Feeds guardrails perception. **Acceptance:** adapter regressions caught by golden diff. **Files:** `tests/golden/`, `crates/*/tests`. **Deps:** 6.2,6.3. **C:** M. **Effort:** 3 PD. **DoD:** golden tests in CI.

**9.3 Perception-assisted debugging hook (daemon frame-store + GET /frame)** — *L — 5 PD*
- **Goal:** On `cargo test` failure, DevGate can fetch a fresh frame. **Spec (F4):** daemon holds a `Arc<Mutex<VecDeque<CapturedFrame>>>` ring buffer (cap 30); `submit` (or a wrapping sink) pushes; add `GET /frame` (+`?index=N`) returning PNG. Preserve `origin_ms` via request envelope (F3). **Minimal version works in single-threaded `tiny_http`** (new request-loop branch). **Acceptance:** `POST /capture` then `GET /frame` returns the PNG; `GET /frame?index=0` = latest. **Files:** `daemon/src/main.rs`, `daemon/src/lib.rs` (`handle_capture` pushes to store), `proto` (optional). **Deps:** 9.1 (origin_ms), 10.0 (optional dynamic sink for on-demand). **C:** L. **Effort:** 5 PD. **DoD:** frame retrievable via HTTP.

## Phase 10 — Maturing the Perception Plane (BLOCKED: architectural restructure)
**Prerequisite sprint — 10.0 CapturePipeline restructure** — *L — 5 PD*
- **Goal:** Enable runtime sink registration + shared pipeline. **Spec (F2):** change `sinks: Vec<…>` → `Arc<Mutex<Vec<Arc<dyn CaptureSink>>>>`, `config: Arc<RwLock<CaptureConfig>>`; convert `add_sink`/`configure` to `&self`; add `metrics: Option<Arc<PipelineMetrics>>` (F3) and `frame_store` hook (F4). Make `CapturePipeline: Clone + Send + Sync`. Keep `BevyCapturePipeline` Deref working. **Acceptance:** daemon can `add_sink` post-construction; pipeline cloneable across threads; all 20 tests pass. **Files:** `lib.rs`, `bevy lib.rs`, `daemon lib.rs`. **Deps:** — (do first). **C:** L. **Effort:** 5 PD. **DoD:** restructure merged; unblocks 9.3 on-demand + 10.x.

### Sub-initiative A — Multi-agent capture orchestration
- **Spec sketch:** Extend the fan-out bus to multiple cameras/engines and multiple consuming agents. A registry maps **named capture streams** (`stream_id`) → set of sinks/agents; agents subscribe/unsubscribe at runtime (requires 10.0). The bus already supports N sinks; this adds addressing + per-stream fan-out + authz for agent consumers.
- **First sprint — 10.1 Dynamic sink registration / subscribe API** — *M — 3 PD*
  - **Goal:** Runtime sink add/remove. **Spec:** `SubscribeSink`/`UnsubscribeSink` on `CapturePipeline` (post-10.0 `&self`); reference-counted sink lifecycle; thread-safe. **Acceptance:** integration test adds a sink mid-run and sees frames; removal stops delivery. **Files:** `lib.rs`. **Deps:** 10.0. **C:** M. **Effort:** 3 PD. **DoD:** dynamic registration tested.
- **10.2 Named capture streams / registry** — *M — 3 PD*
  - **Goal:** `stream_id`-keyed routing. **Spec:** `StreamRegistry` mapping id→`Vec<Arc<dyn CaptureSink>>`; `submit_to(stream_id, frame)`. **Acceptance:** two streams route independently. **Files:** `lib.rs` (+ new module). **Deps:** 10.1. **C:** M. **Effort:** 3 PD. **DoD:** streams isolated.

### Sub-initiative B — Replay buffer
- **Spec sketch:** Ring-buffer of recent `CapturedFrame`s for agent replay/step-back, reusing the daemon ring buffer from 9.3. Exposes `get_frames(range)` + serialization to gRPC/WebSocket for remote agents.
- **First sprint — 10.3 Replay ring-buffer (shared)** — *M — 3 PD*
  - **Goal:** In-core replay store. **Spec:** promote the 9.3 `VecDeque<CapturedFrame>` (cap configurable, default 30) into core as `FrameStore`; `submit` appends; `replay(range) -> Vec<CapturedFrame>`. **Acceptance:** store retains last N frames; replay returns them in order. **Files:** `lib.rs` (`FrameStore`), `daemon` (reuse). **Deps:** 9.3, 10.0. **C:** M. **Effort:** 3 PD. **DoD:** replay retrievable.

### Sub-initiative C — Semantic frame diffing
- **Spec sketch:** A vision model computes *semantic* deltas between frames (not pixels) and feeds agents — perception graduating from "pixels" to "meaning," validated by guardrails/enforcement. New `SemanticDiffSink` calls a model (HTTP/gRPC) with two frames + prompt, returns a delta struct.
- **First sprint — 10.4 Semantic delta sink (sketch + first cut)** — *L — 5 PD*
  - **Goal:** Compare two frames semantically. **Spec:** `SemanticDiffSink` implementing `CaptureSink`; buffers last frame; on next `submit`, POSTs pair to a model endpoint (`RADREDEYE_DIFF_URL` env), emits a `FrameDelta` (text/struct) to a downstream sink or log. Gated by guardrails (no raw model output to agents without review). **Acceptance:** diff sink produces a delta for consecutive frames in a test. **Files:** `sinks/semantic_diff.rs` (new, feature-gated), `Cargo.toml`. **Deps:** 10.0, 9.1 (metrics). **C:** L. **Effort:** 5 PD. **DoD:** semantic delta demonstrable.

**10.5 GrpcSink unsoundness fix** — *M — 2 PD*
- **Goal:** Make gRPC submit thread-safe/non-blocking. **Spec (F5):** replace per-frame `current_thread` runtime + `block_on` in `sinks/grpc.rs` with a shared `Runtime` (multi-thread or process-global `OnceLock`); ensure `submit` from multiple threads is sound. **Acceptance:** stress test calling `submit` from ≥4 threads with no panic/deadlock; latency recorded in 9.1. **Files:** `sinks/grpc.rs`. **Deps:** 10.0. **C:** M. **Effort:** 2 PD. **DoD:** gRPC submit sound.

---

# SUMMARY (TIGHT)

- **Key numbers:** Latency budget **B1 p95 ≤ 3 ms @ 1920×1080** (pipeline core dispatch; shared constant with 5.6/9.1); **B2 `StdoutSink` p95 ≤ 5 ms**; network sinks (HTTP/gRPC/WS/File) **not latency-SLO'd** (blocking, F10). Throughput **≥ 60 fps @ 1080p** (B1), **≥ 20 fps FileSink**. Rust file-size limits **600 soft / 900 hard** (F6/A.6). Backpressure via `min_interval`; Bevy default throttle 1.0 s.
- **Total sprint count:** **27** (5.6×1; Phase 6×6 incl. 6.6; 7×6; 8×5; 9×3; 10×6 incl. 10.0 + 3 sub-initiative first sprints + grpc fix).
- **Corrected crate count:** **6 crate directories** (`core`, `bevy`, `daemon`, `unity`, `unreal`, `webxr`); **5 workspace members**; `webxr` **excluded** (`Cargo.toml` `exclude`). **Tests: 20** (12 core + 4 daemon + 4 bevy). `AGENTS.md` is stale (F1) — fixed in 6.6.
- **Phase readiness (F12):** 5.6 Ready-with-work; 6 Ready-with-work; 7 **BLOCKED** (unblocks at 7.3); 8 Ready-with-work; 9 **BLOCKED** (telemetry + daemon frame-serve); 10 **BLOCKED** (architectural restructure, starts at 10.0).
- **Law-4 halts (Halt-when-uncertain):**
  1. **A/B vendoring decision** (submodule vs copied `scripts/devgate/`) — unresolved; 7.1 records the call.
  2. **`webxr` include-vs-document** — spec *decides*: keep excluded + add non-gating `wasm32` CI job + document; flag for owner sign-off.
  3. **Metrics crate choice** (`metrics` facade + Prometheus vs OpenTelemetry vs custom) — spec recommends `metrics` facade + `PrintingRecorder`/Prometheus; confirm if owner prefers OTel.
  4. **Go MCP server** — recommend NO (overkill); confirm before 8.x (carried from `ROADMAP_INTEGRATION.md`).
  5. **Uncommitted `ci/` deletions** loose end (from prior review) — out of scope; flagged to owner.
- **Persist path:** `docs/radredeye/SPEC_AND_SPRINTS.md`
