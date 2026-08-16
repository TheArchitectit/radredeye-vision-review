# radredeye — Benchmarks (Sprint 5.6)

Criterion-based micro-benchmarks for `radredeye-core`, the engine-agnostic
capture pipeline. These establish the **baseline latency / throughput budget**
that Phase 9.1 turns into a regression gate (block PRs that regress beyond the
budget recorded here).

**Last updated:** 2026-08-13 (Sprint 5.6 + Phase 9.1 telemetry seam)
**Crate:** `radredeye-core`
**Harness:** [`criterion`](https://bheisler.github.io/criterion.rs) 0.5.1

---

## How to run

Full run (default criterion settings — 100 samples, ~5 s measurement per bench,
can take several minutes):

```sh
cargo bench -p radredeye-core
```

Fast run (reduced sample size — for a quick smoke / CI gate). Criterion flags
must be scoped to a single bench with `--bench <name>`, because `cargo bench`
also runs the crate's `lib` unittests and forwards everything after `--` to
*every* target — the standard test harness rejects unknown options like
`--sample-size`, so passing them globally errors out before the benches run:

```sh
cargo bench -p radredeye-core --bench submit_throughput -- \
    --sample-size 10 --warm-up-time 1 --measurement-time 2
cargo bench -p radredeye-core --bench resize_nearest -- \
    --sample-size 10 --warm-up-time 1 --measurement-time 2
```

Bench targets (declared in `crates/radredeye-core/Cargo.toml` with
`harness = false`):

| Bench | What it measures |
|-------|------------------|
| `submit_throughput` | `CapturePipeline::submit` throughput (submits/sec) against a no-op `DevNullSink`, across 1/4/16 sinks and the backpressure path. |
| `resize_nearest`     | `resize_nearest` (RGBA8 nearest-neighbour) latency at three representative resolutions. |

> `DevNullSink` is a tiny in-bench `struct` implementing `CaptureSink` that
> returns `Ok(())`. `StdoutSink` is deliberately **not** used — it writes to
> stdout, which would pollute criterion's measurement output and skew timings
> with terminal I/O.

---

## Baseline numbers

Captured with `--sample-size 10 --warm-up-time 1 --measurement-time 2`.
Each cell is the criterion **median**; the bracketed pair is the lower/upper
bound of the confidence interval. Throughput is `Throughput::Elements(1)`, so
`thrpt` = submits per second.

### `submit_throughput` — `CapturePipeline::submit` (Rgba8 64×64 frame)

| Benchmark                          | Time / submit (median) | Throughput (submits/sec) |
|------------------------------------|------------------------|---------------------------|
| `submit_throughput/sinks/1`         | 73.35 ns [71.89, 75.44] | 13.63 M/s [13.26 M, 13.91 M] |
| `submit_throughput/sinks/4`         | 125.85 ns [123.56, 128.04] | 7.95 M/s [7.81 M, 8.09 M] |
| `submit_throughput/sinks/16`        | 138.89 ns [136.97, 141.50] | 7.20 M/s [7.07 M, 7.30 M] |
| `submit_throughput/backpressure_zero_interval` | 163.29 ns [161.18, 166.11] | 6.12 M/s [6.02 M, 6.20 M] |

**Reading:** with a single no-op sink the pipeline dispatches ~13.6 M
frames/sec on a 64×64 frame (the per-submit cost is dominated by the
`frame.validate()` size check + `Vec` clone in `apply_config`, since
`CaptureConfig` is empty and `apply_config` returns a clone). Adding sinks
adds ~2–3 ns per sink of `Arc<dyn CaptureSink>` dispatch. The backpressure
path (`min_interval = Some(0)`) adds the `Mutex::lock` + `Instant::elapsed`
check, costing ~25 ns on top of the single-sink path.

### `resize_nearest` — RGBA8 nearest-neighbour resize (latency)

| Benchmark | Transform | Latency (median) |
|-----------|-----------|------------------|
| `resize_nearest/1080p_to_720p` | 1920×1080 → 1280×720 | 971.77 µs [961.74 µs, 981.30 µs] |
| `resize_nearest/480p_to_1080p` | 640×480 → 1920×1080 | 2.255 ms [2.233 ms, 2.272 ms] |
| `resize_nearest/720p_to_360p`  | 1280×720 → 640×360  | 242.24 µs [237.85 µs, 247.68 µs] |

**Reading:** latency scales ~linearly with the *output* pixel count (the inner
loop writes one 4-byte RGBA pixel per iteration). 1080p→720p writes 921 600 px
(≈ 971 µs → ~1.05 ns/px); 480p→1080p writes 2 073 600 px (≈ 2.255 ms →
~1.09 ns/px); 720p→360p writes 230 400 px (≈ 242 µs → ~1.05 ns/px). The upscale
is the worst case because it allocates and fills the largest output buffer.

---

## Machine / toolchain

| Item | Value |
|------|-------|
| `rustc` | `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| `cargo` | `cargo 1.97.1 (c980f4866 2026-06-30)` |
| `criterion` | 0.5.1 (dev-dependency on `radredeye-core`) |
| CPU cores | 24 (logical) |
| Profile | `bench` (release, optimized) |
| OS | Linux (container) |

> Numbers are specific to this machine and build. Re-capture on the target CI
> runner before treating these as the gate budget; the *ratios* between benches
> are more portable than the absolute values.

---

## Regression-gate note (Phase 9.1)

These baselines are the **regression-gate inputs for Phase 9.1** ("Capture
telemetry → DevGate observability"). When 9.1 lands, the gate should fail a PR
that regresses `submit_throughput/sinks/1` below ~10 M submits/sec (a ~25 %
budget) or grows any `resize_nearest` case beyond ~1.25× its median here. The
exact budget thresholds are to be finalized in 9.1; this document records the
measured starting point.

### Latency budget — `LATENCY_BUDGET_P95_MS = 3 ms` (spec B1)

Phase 9.1 introduces a named constant —
`radredeye_core::metrics::LATENCY_BUDGET_P95_MS` (= `3.0`, milliseconds) —
that codifies the **spec B1 latency budget**: pipeline-core dispatch p95 must
stay at or below **3 ms** at 1920×1080. This constant is **shared** between:

- the 5.6 criterion benches (the `submit_throughput` baseline above is the
  reference p95), and
- the Phase 9.1 `radredeye_frame_latency_ms` histogram emitted inside
  `CapturePipeline::submit` (see `crates/radredeye-core/src/metrics.rs`).

So a Phase 7.6 regression gate can later read the 9.1 histogram's p95 and fail a
PR that pushes it beyond `LATENCY_BUDGET_P95_MS`. The histogram is emitted to
the `metrics` facade (no-op when no global recorder is installed) and also
accumulated in `PipelineMetrics`'s internal sample buffer for testable p95
computation (`PipelineMetrics::p95_latency_ms`).
