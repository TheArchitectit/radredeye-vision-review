# Phase 10 — Maturing the Perception Plane (Design)

**Status:** 🔶 IN PROGRESS — designed, not yet implemented
**Companion:** `SPEC_AND_SPRINTS.md` (§ Phase 10), `ROADMAP.md`, `ARCHITECTURE.md`,
`ROADMAP_INTEGRATION.md` — design only; a separate agent implements after review.

Last Updated: 2026-08-13

---

## Overview

Phase 9 made radredeye a DevGate **observation source**: every `submit`
emit_s `PipelineMetrics` (counters + `radredeye_frame_latency_ms`), a
CPU-only golden harness guards visual regressions, and the daemon keeps a
`FrameStore` reachable via `GET /frame`. All committed and gated (36 tests,
clippy/doc/scan/regression clean).

Phase 10 **matures the perception plane** on those foundations. The
single-pipeline, construction-time-sink model that reached v0.1.0 cannot serve:

- **Multiple consuming agents** subscribing/unsubscribing at runtime.
- **Multiple logical capture streams** (per-camera, per-engine) routed
  independently instead of one global fan-out.
- **Replay/step-back** of recent frames for any agent, not just the daemon hook.
- **Semantic frame diffing** — "pixels" → "meaning," reusing the 9.2 golden
  diff as a reusable primitive.

This design unblocks Phase 10 with an **additive** restructure: new core types
(`SinkRegistry`, `CaptureStream`, `StreamRegistry`, `ReplayBuffer`, a `diff`
module + `SemanticDiffSink`) **wrap and reuse** the Phase 9 foundations rather
than replacing them. Existing callers (Bevy plugin, daemon, four sinks,
`BevyCapturePipeline`) keep working — construction-time `add_sink`/`configure`
become `#[deprecated]` wrappers over the new `&self` API.

---

## 1. Problem

Today `CapturePipeline` (core `lib.rs`) holds:

```rust
pub struct CapturePipeline {
    sinks: Vec<Arc<dyn CaptureSink>>,        // owned, construction-time only
    config: CaptureConfig,                   // owned, &mut to change
    last_submit: Mutex<Instant>,             // backpressure
    metrics: Mutex<Option<Arc<PipelineMetrics>>>, // &self register (9.1)
}
```

Limits this imposes on the perception plane:

1. **Sinks are append-only and require `&mut self`.** `add_sink`/`configure`
   force an exclusive borrow. The daemon works around this by wrapping the
   pipeline in `Arc<Mutex<CapturePipeline>>`; the Bevy plugin uses `DerefMut`.
   No agent can attach a sink to a *running* pipeline without the whole
   pipeline behind a mutex the capture path also locks — exactly the
   serialization 9.3 had to sidestep.
2. **One global fan-out, no addressing.** Every frame goes to every sink; no
   notion of "front camera" vs "rear camera," so multi-camera/multi-agent
   consumers cannot be isolated or routed.
3. **Replay lives only in the daemon.** The 9.3 `FrameStore`
   (`VecDeque<CapturedFrame>`, cap 30) is daemon-local; core has no replay
   primitive, so in-process Bevy or remote gRPC agents cannot step back
   through recent frames without the HTTP bridge.
4. **Diffing is pixel-only and test-private.** The 9.2 golden `mean_pixel_diff`
   is a free function in `tests/golden.rs`, not reusable by sinks/agents, with
   no path from "pixel diff" to "semantic diff."
5. **gRPC submit is not multi-thread sound.** `GrpcSink` owns a per-instance
   `current_thread` tokio `Runtime` and `block_on`s on `&self.rt` from
   `submit`. Two threads racing on it (current-thread `block_on` is not
   reentrant) is a latent panic once 10.0 makes the pipeline `Clone`-shared.

The spec (F2/F3/F4/F5) sketches the fixes; this design makes them concrete and
orders them so each sprint lands without a big-bang rewrite.

---

## 2. Proposed Architecture

All new types live in `radredeye-core`. New modules: `registry.rs`,
`stream.rs`, `replay.rs`, `diff.rs`, and `sinks/semantic_diff.rs`
(feature-gated). Relationships to existing types:

```
CapturePipeline  ──holds──►  Arc<StreamRegistry>
                                 │
                                 └─► CaptureStream ("default", + named)
                                        ├─► SinkRegistry  (runtime add/remove)
                                        ├─► Option<Arc<PipelineMetrics>>  (per-stream labels)
                                        └─► Option<Arc<ReplayBuffer>>    (promoted FrameStore)

Golden harness (9.2) ──uses──► core::diff::pixel_diff  (generalized mean_pixel_diff)
SemanticDiffSink ──uses──► core::diff + CaptureSink trait + downstream sink
Daemon FrameStore ──re-export──► core::ReplayBuffer  (backward-compat alias)
```

### 2.1 `SinkRegistry` (new, `registry.rs`)

Thread-safe, ordered, reference-counted; removable by identity (handle).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SinkHandle(pub(crate) u64);

#[derive(Clone, Default)]
pub struct SinkRegistry {
    sinks: Arc<Mutex<Vec<(SinkHandle, Arc<dyn CaptureSink>)>>>,
    next_id: Arc<AtomicU64>,
}

impl SinkRegistry {
    pub fn new() -> Self;
    pub fn register(&self, sink: Arc<dyn CaptureSink>) -> SinkHandle;
    pub fn unregister(&self, h: SinkHandle) -> Option<Arc<dyn CaptureSink>>;
    pub fn snapshot(&self) -> Vec<Arc<dyn CaptureSink>>; // ordered, lock-free iter
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

`snapshot()` is taken once per `submit` and iterated without holding the lock
across sink I/O — the same pattern 9.1 uses for `metrics_handle()`.

### 2.2 `ReplayBuffer` (new, `replay.rs`) — promotes daemon `FrameStore`

Promotes the 9.3 daemon `FrameStore` (`VecDeque<CapturedFrame>`, cap 30,
`get(index)` newest-first) into core almost verbatim, adding `replay(range)`.

```rust
/// Bounded ring buffer of recent frames (promoted from 9.3 `FrameStore`).
pub struct ReplayBuffer {
    frames: Mutex<VecDeque<CapturedFrame>>,
    cap: usize,
}

impl ReplayBuffer {
    pub fn new(cap: usize) -> Self;          // assert cap >= 1
    pub fn push(&self, frame: CapturedFrame);
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    /// `index = 0` = newest; higher = older. `None` if out of range.
    pub fn get(&self, index: usize) -> Option<CapturedFrame>;
    /// Ordered oldest→newest slice of the last `range` frames (clamped).
    pub fn replay(&self, range: Range<usize>) -> Vec<CapturedFrame>;
    pub fn cap(&self) -> usize;
}
```

`FrameStore` semantics (`index 0` = newest, oldest-eviction) are preserved so
the daemon's `frame_response` / `dispatch_request` keep working unchanged.

### 2.3 `CaptureStream` + `StreamRegistry` (new, `stream.rs`)

A named logical capture stream: its own sinks, optional per-stream
metrics, optional replay buffer. The pipeline holds a `StreamRegistry`; a
`"default"` stream always exists and mirrors the legacy single-pipeline path.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(pub Arc<str>);
impl StreamId {
    pub fn new(name: &str) -> Self;
    pub const DEFAULT: &'static str = "default";
}

/// One logical capture stream: sinks + telemetry + replay.
pub struct CaptureStream {
    id: StreamId,
    config: Arc<RwLock<CaptureConfig>>,
    sinks: SinkRegistry,
    last_submit: Mutex<Instant>,
    metrics: Mutex<Option<Arc<PipelineMetrics>>>,
    replay: Mutex<Option<Arc<ReplayBuffer>>>,
}

impl CaptureStream {
    pub fn id(&self) -> &StreamId;
    pub fn register_sink(&self, sink: Arc<dyn CaptureSink>) -> SinkHandle;
    pub fn unregister_sink(&self, h: SinkHandle) -> Option<Arc<dyn CaptureSink>>;
    pub fn configure(&self, config: CaptureConfig);   // &self (RwLock write)
    pub fn register_metrics(&self, m: Arc<PipelineMetrics>);
    pub fn attach_replay(&self, r: Arc<ReplayBuffer>);
    pub fn replay(&self) -> Option<Arc<ReplayBuffer>>;
    /// Apply config, backpressure, validate, fan out to sinks, record metrics,
    /// append to replay. Reuses the 9.1 emission logic verbatim.
    pub fn submit(&self, frame: &CapturedFrame);
    pub fn shutdown(&self);
}

/// Maps `StreamId -> Arc<CaptureStream>`. Held by `CapturePipeline`.
#[derive(Clone, Default)]
pub struct StreamRegistry {
    streams: Arc<RwLock<HashMap<StreamId, Arc<CaptureStream>>>>,
}

impl StreamRegistry {
    pub fn new() -> Self;                                   // creates "default"
    pub fn create_stream(&self, id: &str) -> Arc<CaptureStream>;
    pub fn stream(&self, id: &str) -> Option<Arc<CaptureStream>>;
    pub fn default_stream(&self) -> Arc<CaptureStream>;     // never None
    pub fn remove_stream(&self, id: &str) -> Option<Arc<CaptureStream>>;
    pub fn list(&self) -> Vec<StreamId>;
}
```

### 2.4 `CapturePipeline` restructure (10.0)

`CapturePipeline` becomes a thin facade over `StreamRegistry`. It is
`Clone + Send + Sync` (all internals are `Arc`-shared); cloning shares the
same streams/sinks (the "shared pipeline" semantics the spec F2 asks for).

```rust
#[derive(Clone)]
pub struct CapturePipeline {
    registry: StreamRegistry,
}

impl CapturePipeline {
    pub fn new() -> Self;                      // StreamRegistry with "default"
    // ---- new &self API (preferred) ----
    pub fn register_sink(&self, sink: Arc<dyn CaptureSink>) -> SinkHandle; // default stream
    pub fn unregister_sink(&self, h: SinkHandle) -> Option<Arc<dyn CaptureSink>>;
    pub fn configure_shared(&self, config: CaptureConfig);                 // &self
    pub fn create_stream(&self, id: &str) -> Arc<CaptureStream>;
    pub fn stream(&self, id: &str) -> Option<Arc<CaptureStream>>;
    pub fn submit_to(&self, stream_id: &str, frame: &CapturedFrame);
    pub fn submit(&self, frame: &CapturedFrame);   // = submit_to("default", frame)
    pub fn register_metrics(&self, m: Arc<PipelineMetrics>); // default stream
    pub fn attach_replay(&self, r: Arc<ReplayBuffer>);        // default stream
    pub fn sink_count(&self) -> usize;                        // default stream
    pub fn shutdown(&self);                                   // all streams

    // ---- legacy, kept for back-compat ----
    #[deprecated(note = "use `register_sink` (works on shared/&self pipelines)")]
    pub fn add_sink(&mut self, sink: Arc<dyn CaptureSink>);   // -> register_sink
    #[deprecated(note = "use `configure_shared` (works on &self)")]
    pub fn configure(&mut self, config: CaptureConfig);      // -> configure_shared
}
```

`submit` preserves the exact 9.1 emission order: `inc_submitted` →
backpressure check (`inc_dropped("backpressure")`) → `validate`
(`inc_dropped("invalid")`) → `apply_config` → fan-out
(`inc_sink_error` per failing sink) → `record_latency_ms`. The logic moves
into `CaptureStream::submit`; `CapturePipeline::submit` delegates to the
default stream.

### 2.5 `diff` module + `SemanticDiffSink` (new, `diff.rs`, `sinks/semantic_diff.rs`)

The 9.2 golden `mean_pixel_diff` is generalised into a reusable core helper:

```rust
/// Per-channel diff statistics between two equal-shape RGBA8 frames.
#[derive(Debug, Clone, Copy)]
pub struct DiffStats { pub mean_abs: f64, pub max_abs: u8, pub changed_pixels: usize }

/// Mean/max per-channel absolute pixel difference. Dimensions must match.
pub fn pixel_diff(a: &CapturedFrame, b: &CapturedFrame) -> DiffStats;

/// A semantic delta between two frames (pixel- or model-derived).
#[derive(Debug, Clone)]
pub struct FrameDelta {
    pub stream_id: Option<StreamId>,
    pub kind: DeltaKind,            // Pixel | Semantic
    pub summary: String,            // human-readable, guardrails-reviewable
    pub stats: Option<DiffStats>,   // present for Pixel; None for raw semantic
}
```

`SemanticDiffSink` implements `CaptureSink`, buffers the previous frame, and
on each `submit` computes a `FrameDelta`:

- If `RADREDEYE_DIFF_URL` is unset → `kind = Pixel`, `pixel_diff` of the pair,
  `summary` formatted from `DiffStats` (no network, no model output — safe to
  emit to any downstream sink).
- If `RADREDEYE_DIFF_URL` is set → POSTs the two PNG-encoded frames to the model
  endpoint and emits `kind = Semantic` with the model's reviewed summary.
  **Gated by guardrails (Law 4):** raw model output is never forwarded to an
  agent-consumer sink without an explicit review/allow-list (see §5 Risks).

```rust
// feature = "semantic-diff"
pub struct SemanticDiffSink {
    last: Mutex<Option<CapturedFrame>>,
    diff_url: Option<String>,        // from RADREDEYE_DIFF_URL at construction
    downstream: Arc<dyn CaptureSink>,// where the FrameDelta is delivered
}
impl CaptureSink for SemanticDiffSink { /* submit computes delta, forwards */ }
```

The 9.2 golden test is migrated to call `core::diff::pixel_diff` instead of
its private `mean_pixel_diff`, proving the generalisation is behaviour-identical.

---

## 3. Backward Compatibility

Additive-first; no breaking changes to public signatures.

| Existing API | Phase 10 status |
|---|---|
| `CapturePipeline::new()` | unchanged (constructs `"default"` stream) |
| `CapturePipeline::submit(&self, frame)` | unchanged behaviour; delegates to default stream |
| `CapturePipeline::add_sink(&mut self, …)` | kept, `#[deprecated]`, wraps `register_sink` |
| `CapturePipeline::configure(&mut self, …)` | kept, `#[deprecated]`, wraps `configure_shared` |
| `CapturePipeline::register_metrics(&self, …)` | unchanged; registers on default stream |
| `CapturePipeline::sink_count(&self)` | unchanged (default stream) |
| `CapturePipeline::shutdown(&self)` | unchanged; shuts all streams |
| `BevyCapturePipeline` (`Deref`/`DerefMut` to `CapturePipeline`) | unchanged; `add_sink` via `DerefMut` still compiles (deprecated warning). Migration to `register_sink` is a follow-up. |
| `BevyCapturePipeline::default()` | unchanged |
| Daemon `FrameStore`, `FRAME_STORE_CAP`, `handle_capture`, `dispatch_request`, `frame_response`, `parse_frame_index` | unchanged. `FrameStore` becomes a `pub use` re-export/alias of `core::ReplayBuffer` (deprecated alias kept for one cycle); `handle_capture` keeps pushing to the daemon-owned buffer. |
| 4 sinks (`StdoutSink`/`FileSink`/`HttpSink`/`WebSocketSink`/`GrpcSink`) | unchanged; they are `Arc<dyn CaptureSink>` and register identically |
| `PipelineMetrics` API | unchanged; gains an optional `stream` label only when constructed per-stream (see §4) |
| Golden test (`tests/golden.rs`) | migrated to `core::diff::pixel_diff`; record/compare behaviour identical |

The `#[deprecated]` attributes emit a warning (not an error) on the legacy
`&mut` methods, so the Bevy plugin and any external callers keep compiling
under `cargo build`. `cargo clippy -- -D warnings` is kept green by migrating
in-tree callers off the deprecated methods in the same sprint that introduces
them (10.0 migrates `BevyCapturePipeline` test usage; the daemon already uses
`Arc<Mutex<…>>` and can adopt `register_sink`). The deprecation is a soft
signal, not a removal — removal is deferred to a post-Phase-10 cleanup.

---

## 4. Telemetry (reuses `PipelineMetrics` + the `metrics` facade)

`PipelineMetrics` (9.1) is reused unchanged in shape. Per-stream labelling is
**additive and opt-in**: a `PipelineMetrics` registered on a `CaptureStream`
other than `"default"` stamps a `"stream"` label on every facade emission.

```rust
impl PipelineMetrics {
    pub fn new() -> Self;                     // no stream label (default / legacy)
    pub fn for_stream(stream_id: &str) -> Self;// stamps "stream"=<id> on all emissions
}
```

Facade emissions become (when a stream label is set):

| Metric | Labels |
|---|---|
| `radredeye_frame_submitted_total` | `stream` (optional) |
| `radredeye_frame_dropped_total` | `reason`, `stream` (optional) |
| `radredeye_sink_errors_total` | `sink`, `stream` (optional) |
| `radredeye_frame_latency_ms` | `stream` (optional) |

The default-stream (legacy) path emits **without** a `stream` label, so
existing exporters/dashboards see identical series to today — no cardinality
regression for the common case. `LATENCY_BUDGET_P95_MS` (3 ms, shared with the
5.6 benches) is unchanged; the 7.6 regression gate continues to gate the
default stream's p95. New per-stream latency budgets are **not** introduced
in Phase 10 (open question §5).

The internal testable counters (`frame_submitted`, `dropped_for`, `errors_for`,
`p95_latency_ms`) remain unlabelled aggregates on the `PipelineMetrics`
instance — one instance per stream, so per-stream assertions work by holding
the per-stream `Arc<PipelineMetrics>`, exactly as 9.1 tests do today.

---

## 5. Migration Plan (ordered, no big-bang)

Each step is independently mergeable and keeps the workspace green.

1. **Add new core modules without wiring** (folded into 10.0): create
   `registry.rs`, `replay.rs`, `stream.rs`, `diff.rs` with the new types and
   their own unit tests. `CapturePipeline` is untouched here — the new types
   are unused by it yet. Purely additive; no behaviour change.
2. **Restructure `CapturePipeline`** (10.0): replace `sinks`/`config`/`metrics`
   fields with `Arc<StreamRegistry>`; implement legacy methods as deprecated
   wrappers; impl `Clone + Send + Sync`; move `submit` body into
   `CaptureStream::submit`. Migrate in-tree `&mut` callers (Bevy test,
   daemon). All 36 tests still pass; deprecated methods still callable.
3. **Dynamic sink registration** (10.1): surface `register_sink`/`unregister_sink`
   (`&self`); add an integration test that attaches a recording sink mid-run
   and asserts it receives subsequent frames, then removes it and asserts
   delivery stops. Deprecation warnings migrate to `register_sink`.
4. **Named streams** (10.2): surface `create_stream`/`stream`/`submit_to`;
   add a test with two streams routing independently. `"default"` is the
   legacy path — `submit` is `submit_to("default", …)`.
5. **Promote `FrameStore` → core `ReplayBuffer`** (10.3): move the daemon
   `FrameStore` body into `core::replay`; daemon re-exports it as a deprecated
   alias; `handle_capture` pushes to the same buffer (now `Arc<ReplayBuffer>`);
   add `replay(range)` tests. Daemon tests unchanged.
6. **Generalise the golden diff + `SemanticDiffSink`** (10.4): add
   `core::diff::pixel_diff`; migrate `tests/golden.rs` to call it (assert
   identical behaviour); add feature-gated `SemanticDiffSink` with a
   pixel-fallback test (no `RADREDEYE_DIFF_URL` set → `kind = Pixel`).
7. **gRPC runtime soundness** (10.5): replace the per-instance
   `current_thread` runtime with a process-global
   `OnceLock<tokio::runtime::Runtime>` (multi-thread) shared by all
   `GrpcSink` instances; `submit` uses `runtime.block_on` on the shared
   handle (sync `submit` is never inside an async context, so `block_on` is
   legal). Add a ≥4-thread stress test asserting no panic/deadlock; latency
   is recorded via the 9.1 metrics already on the pipeline.

Ordering rationale: 10.0 is the prerequisite (everything depends on `&self`
+ `Clone`). 10.1–10.2 are the multi-agent surface. 10.3 unblocks in-process
replay. 10.4 generalises perception. 10.5 is an independent soundness fix
that 10.0 makes reachable (shared pipeline → concurrent `submit`).

---

## 6. Risks / Open Questions

1. **Headline sprint count.** `STATUS.md` advertises "40/42 (95%)", implying
   **2** remaining Phase-10 sprints. The spec (`SPEC_AND_SPRINTS.md`) sketches
   **6** distinct work items (10.0–10.5). This design adopts the spec's
   granularity because each item has a distinct deliverable, dependency, and
   DoD — collapsing them into 2 marquee sprints would violate the
   incremental-merge ethos and the file-size/DoD granularity the rest of the
   roadmap uses. **Open question (Law 4):** does the owner accept the headline
   total moving from 42 → **46** (40 done + 6 Phase-10 sprints), or should
   Phase 10 be reported as 2 sprints (10.0 + 10.1) with 10.2–10.5 folded as
   sub-tasks? This doc + `ROADMAP.md` use the 6-sprint form; the count is
   trivially re-collapsible in `STATUS.md` if the owner prefers 42.
2. **`#[deprecated]` vs clippy `-D warnings`.** The Bevy plugin's test uses
   `res.add_sink(...)` via `DerefMut`. A `#[deprecated]` attribute would make
   `cargo clippy -- -D warnings` fail unless the call is migrated or
   `#[allow(deprecated)]` is added. **Decision:** 10.0 migrates the in-tree
   caller to `register_sink` in the same sprint, so no `deprecated` warning
   fires from in-tree code. External callers (none known outside the repo)
   see a warning, not an error. Confirm acceptable.
3. **`CapturePipeline: Clone` semantics.** Cloning shares `StreamRegistry`
   (all streams/sinks). This is the desired "shared pipeline" behaviour, but
   it means `configure_shared` on one clone affects all clones. Document
   explicitly; the legacy `configure(&mut self)` on a clone still routes to
   `configure_shared`. Confirm shared-config-on-clone is the intended F2
   semantics.
4. **Per-stream metric cardinality.** Adding a `stream` label per named
   stream can multiply Prometheus series. Mitigation: only non-default
   streams stamp the label; document a recommended stream-count ceiling.
   Confirm whether to enforce a hard cap (e.g. refuse `create_stream` beyond
   N) or leave it advisory.
5. **`SemanticDiffSink` model-output review (Law 4).** Raw model output must
   not flow to agent-consumer sinks unreviewed. Phase 10 ships the
   **pixel-fallback** path (no `RADREDEYE_DIFF_URL`) as the default; the
   semantic path is feature-gated and documented as requiring a review
   sink / allow-list downstream. Confirm the guardrails boundary: is an
   in-repo review sink stub sufficient for 10.4, or should semantic
   emission be deferred to a later phase until the review pipeline exists?
6. **gRPC shared runtime.** A process-global `OnceLock<Runtime>` is simplest
   but means all gRPC sinks share one multi-thread runtime (fine for a
   capture library). Alternative: per-sink `Runtime` but with
   `block_in_place` + `spawn`. Recommend the shared `OnceLock` for
   simplicity and lower thread count. Confirm.
7. **`ReplayBuffer` memory.** Cap-30 RGBA frames at 1080p ≈ 30 × 8 MB ≈ 240 MB.
   The 9.3 cap (30) is preserved; per-stream buffers are opt-in
   (`attach_replay`). Confirm the default-stream replay is opt-in (off by
   default in core) vs on-by-default in the daemon (today's behaviour).

---

## 7. Sprint Breakdown

Sprint statuses below are mirrored in `ROADMAP.md` (Phase 10 section). All
are `⬜` (not started); Phase 10 is `🔶 IN PROGRESS` (designed).

| Sprint | Deliverable | C | Deps | DoD (exit criteria) | Files touched |
|---|---|---|---|---|---|
| **10.0** | CapturePipeline restructure (shared/cloneable, `&self` sinks/config, `StreamRegistry`+`CaptureStream`+`SinkRegistry`+`ReplayBuffer` modules added) | L | — | `CapturePipeline: Clone + Send + Sync`; `add_sink`/`configure` are `#[deprecated]` wrappers; in-tree callers migrated; all 36 tests pass; clippy/doc/scan/regression clean | `core/src/lib.rs`, `core/src/registry.rs`, `core/src/stream.rs`, `core/src/replay.rs`, `bevy/src/lib.rs`, `daemon/src/lib.rs` |
| **10.1** | Dynamic sink registration / unsubscribe API | M | 10.0 | Integration test adds a recording sink mid-run and receives subsequent frames; `unregister_sink` stops delivery; `register_sink` is `&self` | `core/src/stream.rs`, `core/src/lib.rs`, `core/tests/dynamic_sinks.rs` |
| **10.2** | Named capture streams / registry routing | M | 10.1 | Two named streams route independently (`submit_to`); `"default"` preserves legacy `submit`; isolation test passes | `core/src/stream.rs`, `core/src/lib.rs`, `core/tests/streams.rs` |
| **10.3** | Replay ring-buffer promoted to core (`ReplayBuffer`) | M | 9.3, 10.0 | `core::ReplayBuffer` retains last N frames; `replay(range)` returns ordered frames; daemon `FrameStore` is a deprecated re-export alias; daemon tests unchanged | `core/src/replay.rs`, `daemon/src/lib.rs`, `core/tests/replay.rs` |
| **10.4** | Semantic delta sink + reusable `diff` module | L | 10.0, 9.1 | `core::diff::pixel_diff` generalises the golden `mean_pixel_diff` (golden test migrated, behaviour identical); feature-gated `SemanticDiffSink` produces a `FrameDelta` for consecutive frames (pixel-fallback when `RADREDEYE_DIFF_URL` unset) | `core/src/diff.rs`, `core/src/sinks/semantic_diff.rs`, `core/Cargo.toml`, `core/tests/golden.rs` |
| **10.5** | GrpcSink thread-soundness fix (shared runtime) | M | 10.0 | `GrpcSink` uses a process-global `OnceLock<Runtime>`; ≥4-thread stress test calls `submit` with no panic/deadlock; latency recorded via 9.1 metrics | `core/src/sinks/grpc.rs`, `core/tests/grpc_stress.rs` |

**Phase 10 DoD (overall):** all six sprints merged; workspace green (build +
36+ new tests + clippy/doc/scan/regression); `BevyCapturePipeline`, the daemon,
and the four sinks unchanged from a caller's perspective; Phase 10 marked
✅ COMPLETE in `ROADMAP.md` and `STATUS.md`. Final test count is reported by
the implementation agent.

---

## 8. Relationship to Phase 9 (builds on, not replaces)

- **`PipelineMetrics` (9.1):** reused verbatim. Per-stream labelling is an
  additive constructor (`for_stream`); the default path emits unlabelled as
  today. The 5.6/7.6 latency gate keeps gating the default stream's p95.
- **Golden harness (9.2):** `tests/golden.rs` is migrated to call
  `core::diff::pixel_diff`; record/compare modes and the
  `golden_diff_detects_regression` assertion are unchanged. The diff becomes
  a *core primitive* reusable by sinks and agents.
- **Daemon `FrameStore` (9.3):** the type body moves to core as
  `ReplayBuffer`; the daemon keeps `FrameStore` as a deprecated re-export so
  `handle_capture`/`dispatch_request`/`frame_response` compile and behave
  identically. `GET /frame` keeps serving from the daemon's buffer (which is
  now a `ReplayBuffer`). In-process agents (Bevy) and remote agents (gRPC)
  gain the same `replay(range)` capability without the HTTP bridge.

Nothing in Phase 9 is deleted or behaviourally changed; Phase 10 widens the
surface around the same foundations.

---

*End of design. Implementation is executed by a separate agent after owner
review of this document and the open questions in §6.*
