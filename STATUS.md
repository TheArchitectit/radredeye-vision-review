# radredeye — Status

**Last updated:** 2026-08-13  
**Current sprint:** Phase 10 — Maturing the Perception Plane (complete; 10.0–10.5 merged and gated)

## Strategic direction

The project has been renamed to **radredeye**. The agent-facing interface is now the **MCP stateless protocol**, served by `radredeye-mcp` (the old HTTP daemon has been absorbed into this server). radredeye is **framework-agnostic**: any application — game engines, desktop GUIs, browsers, terminals — can submit frames via the `submit_frame` MCP tool or `POST /capture`. Full specification at [docs/radredeye/SPEC.md](docs/radredeye/SPEC.md).

See **[ROADMAP.md](ROADMAP.md)** for the full phased sprint plan.  
See **[docs/radredeye/SPRINT_WORKFLOW.md](docs/radredeye/SPRINT_WORKFLOW.md)** for the dynamic workflow tracker.  
See **[docs/radredeye/ROADMAP_INTEGRATION.md](docs/radredeye/ROADMAP_INTEGRATION.md)** for the review + 3-layer (radredeye / DevGate / Guardrails) integration roadmap.  
See **[docs/radredeye/SPEC_AND_SPRINTS.md](docs/radredeye/SPEC_AND_SPRINTS.md)** for the full system specification + granular sprint plan (Phases 5.6–10).

---

## Current Phase Summary

| Phase | Name | Status | Progress |
|-------|------|--------|----------|
| 0 | Foundation | ✅ Complete | 5/5 |
| 1 | Core Sinks & Daemon Bridge | ✅ Complete | 7/7 |
| 2 | Testing & CI | ✅ Complete — 20 tests, GitHub Actions CI | 7/7 |
| 3 | Additional Sinks (WebSocket, gRPC) | ✅ Complete | 6/6 |
| 4 | Engine Adapters (Unity, Unreal, WebXR) | ✅ Complete | 6/6 |
| 5 | Production Hardening | ✅ Complete | 6/6 |
| 6 | Docs & Release | ✅ Complete | 6/6 |
| 9 | radredeye as a DevGate Observation Source | ✅ Complete | 3/3 |
| 10 | Maturing the Perception Plane | ✅ Complete | 6/6 |

**Overall: 46/46 sprints complete (100%)**

---

## Blockers

None. All crates compile, workspace tests pass (**74 tests**; 78 with `grpc-sink`).

---

## What's Done

### Phase 0–2 (Foundation → Testing)
- Rust workspace with 6 crates: `radredeye-core`, `radredeye-bevy`, `radredeye-mcp`, `radredeye-unreal`, `radredeye-unity`, `radredeye-webxr` (excluded from default build)
- Core: `CapturedFrame`, `CaptureSink` trait, `CapturePipeline`, `StdoutSink`, `FileSink`, `HttpSink`
- Bevy: observer-based screenshot capture, `CaptureCamera` marker, plugin wiring
- Godot: auto-capture addon, optional HTTP POST to daemon bridge
- Daemon → `radredeye-mcp`: MCP stateless server (the agent interface; absorbed the old HTTP daemon). `POST /mcp` (JSON-RPC 2.0, stateless) plus legacy `/capture` + `/health` endpoints
- 20 tests passing (12 core, 4 daemon, 4 bevy)
- GitHub Actions CI configured

### Phase 3 (Additional Sinks)
- WebSocket sink (`tungstenite`, feature-gated)
- gRPC sink (`tonic`, feature-gated)

### Phase 4 (Engine Adapters)
- Unity C# adapter stub
- Unreal C++ adapter stub
- WebXR JavaScript adapter stub

### Phase 5 (Production Hardening) — COMPLETE
- [x] CaptureConfig: resolution/format overrides + resize_nearest
- [x] Backpressure: min_interval frame dropping
- [x] Structured logging: tracing across all crates
- [x] Health check: GET /health on daemon
- [x] Graceful shutdown: CaptureSink::on_shutdown() + pipeline.shutdown()
- [x] Benchmark harness (Sprint 5.6): `criterion` dev-dep + `submit_throughput`/`resize_nearest` benches; baselines in `BENCHMARKS.md`

### Phase 6 (Docs & Release) — COMPLETE
- [x] 6.1: Rustdoc for all public APIs
- [x] 6.2: Bevy integration walkthrough
- [x] 6.3: Godot integration walkthrough
- [x] 6.4: Final ARCHITECTURE.md sink×engine matrix
- [x] 6.5: Version bump + CHANGELOG + `scripts/release.sh`
- [x] 6.6: Reconcile doc drift (AGENTS.md, SUPPORT_MATRIX.md, ROADMAP.md)

### Phase 9 (radredeye as a DevGate Observation Source) — ✅ COMPLETE
- [x] **9.1 — Capture telemetry seam:** `PipelineMetrics` (`metrics` facade +
      internal counters/latency buffer) registered via
      `CapturePipeline::register_metrics`; `submit` emits
      `frame_submitted_total`, `frame_dropped_total{reason}`,
      `sink_errors_total{sink}`, and the `radredeye_frame_latency_ms`
      histogram. `LATENCY_BUDGET_P95_MS = 3 ms` (spec B1) shared with the 5.6
      benches / `BENCHMARKS.md`. No-op-safe when no recorder/metrics handle is
      installed.
- [x] **9.2 — Visual/screenshot regression:** `tests/golden/` +
      `crates/radredeye-core/tests/golden.rs` — deterministic CPU-only
      golden-frame comparison (gradient fixture, `GOLDEN_RECORD=1` record mode,
      default compare mode, pixel-diff ≤ threshold). A second test proves the
      diff catches regressions. True per-adapter golden capture (display/GPU)
      is wired into CI separately (Phase 6.2/6.3).
- [x] **9.3 — Perception-assisted debugging hook:** daemon `FrameStore` ring
      buffer (cap 30); `handle_capture` pushes each decoded frame;
      `GET /frame[?index=N]` returns the stored frame as PNG (`index=0`=latest,
      404 when empty/out-of-range). Request routing extracted into a testable
      `dispatch_request`; integration test does POST `/capture` → GET `/frame`
      and compares dimensions/pixels.

### Phase 10 (Maturing the Perception Plane) — ✅ COMPLETE
- [x] **10.0 — `CapturePipeline` restructure:** now a `Clone + Send + Sync`
      facade over a `StreamRegistry`; `add_sink`/`configure` kept as
      `#[deprecated]` `&self` wrappers; in-tree callers migrated; new
      `SinkRegistry`/`ReplayBuffer`/`CaptureStream`/`StreamRegistry`/`diff`
      modules.
- [x] **10.1 — Dynamic sink registration:** `register_sink`/`unregister_sink`
      (`&self`) + integration test.
- [x] **10.2 — Named capture streams:** `create_stream`/`stream`/`submit_to`;
      `"default"` preserves legacy `submit`.
- [x] **10.3 — `ReplayBuffer` promoted to core:** daemon `FrameStore` is a
      deprecated re-export alias; daemon HTTP path unchanged.
- [x] **10.4 — Reusable `diff` + `SemanticDiffSink`:** `core::diff::pixel_diff`
      generalises the 9.2 golden check; feature-gated `SemanticDiffSink` ships
      pixel-fallback default (guardrails Law 4 review boundary documented).
- [x] **10.5 — gRPC thread-soundness:** shared process-global
      `OnceLock<Runtime>`; ≥4-thread stress test passes.

---

## What's Next

1. **Phase 10**: Maturing the Perception Plane — **complete** ✅. Six sprints
   (10.0–10.5) merged and gated: `CapturePipeline` restructure → dynamic sinks,
   named streams, replay ring-buffer, semantic diff, gRPC thread-soundness.
   All 46 sprints complete. Design: `docs/radredeye/PHASE10_DESIGN.md`.

---

## Guardrails & DevGate Integration (done)

The cross-engine capture pipeline is now wired to the [DevGate Agentic Framework](https://github.com/TheArchitectit/DevGate-Agentic-Framework) and the [agent-guardrails-template](https://github.com/TheArchitectit/agent-guardrails-template) for enforced agent safety + regression gates. Full spec: [docs/radredeye/SPEC_AND_SPRINTS.md](docs/radredeye/SPEC_AND_SPRINTS.md); integration roadmap: [docs/radredeye/ROADMAP_INTEGRATION.md](docs/radredeye/ROADMAP_INTEGRATION.md).

**Three enforcement layers**
- **Pattern scan** — `scripts/devgate/guardrails-scan.mjs` enforces `.guardrails/prevention-rules/pattern-rules.json` over the full `crates/` + `engines/` tree. Rust rules `PREVENT-013` (`.unwrap()`) and `PREVENT-RUST-001..004` (`unsafe`, `panic!`, `todo!`/`unimplemented!`, unchecked derefs) block at `error`.
- **Regression / size** — `scripts/devgate/regression_check.py` (Rust-hardened) enforces 600/900 soft/hard line limits on `crates/**/*.rs` + `engines/**/*.{gd,cs,cpp}` and scans the failure registry.
- **Rust safety** — `cargo clippy --workspace -- -D warnings` and `cargo audit` run in the pre-commit hook and CI.

**CI** (`.github/workflows/`): `guardrails.yml` (regression + pattern scan + clippy + cargo-audit), `secret-validation.yml`, `documentation-check.yml` (scoped to project docs, ≤500 lines), plus the existing `ci.yml`.

**Deliberate deviations (documented)**
- `PREVENT-024` (hallucinated imports) intentionally excludes `*.rs`: a regex cannot distinguish `use radredeye_core::X` from a fake crate. Rust import validity is enforced by `cargo build`/`cargo check` + `cargo audit` instead.
- `cargo-audit` runs in CI as `continue-on-error` (not yet blocking) until the dependency tree is triaged against rustsec advisories. `cargo-audit` is not installed locally, so the pre-commit hook skips it (guarded) until installed.
- The pre-commit hook requires `Co-Authored-By:` (guardrails-template convention) and is installed via `git config core.hooksPath .claude/hooks`.

---

## Key Metrics

| Metric | Value |
|--------|-------|
| Total tests | 74 (78 with grpc-sink) |
| Test pass rate | 100% |
| Crates in workspace | 6 |
| Engine adapters | 3 (Bevy, Godot, Unreal/Unity/WebXR stubs) |
| Sinks | 4 (Stdout, File, HTTP, WebSocket/gRPC) |
| Sprints complete | 46/46 (100%) |
