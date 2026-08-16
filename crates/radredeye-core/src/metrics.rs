//! Capture-pipeline telemetry seam (Phase 9.1).
//!
//! [`PipelineMetrics`] holds its own counters and a latency sample buffer so
//! tests can assert on them **without installing a global `metrics` recorder**.
//! At the same time every mutator also calls the `metrics` facade macros
//! (`counter!` / `histogram!`) so an external exporter (Prometheus, OTel, …)
//! installed by the host application picks the same observations up. The facade
//! macros no-op when no global recorder is registered, so emission is always
//! safe — including the `None`-metrics path inside [`crate::CapturePipeline`].
//!
//! The metrics emitted are:
//!
//! | Metric (facade name)                       | Type      | Labels          |
//! |--------------------------------------------|-----------|-----------------|
//! | `radredeye_frame_submitted_total`     | counter   | —               |
//! | `radredeye_frame_dropped_total`       | counter   | `reason`        |
//! | `radredeye_sink_errors_total`         | counter   | `sink`          |
//! | `radredeye_frame_latency_ms`          | histogram | —               |
//!
//! [`LATENCY_BUDGET_P95_MS`] is the spec B1 latency budget (p95 ≤ 3 ms for
//! pipeline-core dispatch at 1080p). It is shared with the 5.6 criterion benches
//! and referenced from `BENCHMARKS.md` so a Phase 7.6 regression gate can fail
//! PRs that regress the p95 beyond budget.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Spec B1 latency budget: pipeline-core dispatch p95 must stay at or below
/// this value (milliseconds) at 1920×1080. Shared with the 5.6 criterion benches
/// so a Phase 7.6 gate can detect a p95 regression.
pub const LATENCY_BUDGET_P95_MS: f64 = 3.0;

/// Telemetry counters + latency accumulator for [`crate::CapturePipeline`].
///
/// Designed to be **testable without a global recorder**: each mutator updates
/// interior-mutable counters that tests read back directly, *and* emits to the
/// `metrics` facade so a host-installed exporter receives the same data. When no
/// recorder is installed the facade calls are no-ops — never panics.
///
/// Cloned cheaply behind `Arc<PipelineMetrics>` and registered on a pipeline via
/// [`crate::CapturePipeline::register_metrics`].
pub struct PipelineMetrics {
    frame_submitted_total: AtomicU64,
    frame_dropped_total: AtomicU64,
    sink_errors_total: AtomicU64,
    dropped_by_reason: Mutex<HashMap<String, u64>>,
    errors_by_sink: Mutex<HashMap<String, u64>>,
    latencies_ms: Mutex<Vec<f64>>,
    /// Optional `stream` label stamped on every facade emission (Phase 10 §4).
    /// `None` (the default [`PipelineMetrics::new`] path) emits unlabelled, so
    /// the legacy/default stream produces the identical series to Phase 9.
    stream_label: Option<String>,
}

impl Default for PipelineMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineMetrics {
    /// Create a fresh metrics handle with all counters at zero. No `stream`
    /// label is emitted (the legacy/default path), so facade series are
    /// identical to Phase 9.
    pub fn new() -> Self {
        Self {
            frame_submitted_total: AtomicU64::new(0),
            frame_dropped_total: AtomicU64::new(0),
            sink_errors_total: AtomicU64::new(0),
            dropped_by_reason: Mutex::new(HashMap::new()),
            errors_by_sink: Mutex::new(HashMap::new()),
            latencies_ms: Mutex::new(Vec::new()),
            stream_label: None,
        }
    }

    /// Create a metrics handle that stamps a `stream` label on every facade
    /// emission. Use this for non-default [`crate::stream::CaptureStream`]s so
    /// per-stream series are distinguishable in an exporter. The default stream
    /// should use [`PipelineMetrics::new`] (no label) to avoid a cardinality
    /// regression for the common case. The internal testable counters remain
    /// unlabelled aggregates on this instance (one instance per stream).
    pub fn for_stream(stream_id: &str) -> Self {
        let mut m = Self::new();
        m.stream_label = Some(stream_id.to_string());
        m
    }

    /// The configured `stream` label, if any.
    pub fn stream_label(&self) -> Option<&str> {
        self.stream_label.as_deref()
    }

    /// Record one frame submission (`frame_submitted_total` +1).
    pub fn inc_submitted(&self) {
        self.frame_submitted_total.fetch_add(1, Ordering::Relaxed);
        // Facade: no-op when no global recorder is installed.
        match &self.stream_label {
            Some(s) => {
                metrics::counter!("radredeye_frame_submitted_total", "stream" => s.clone())
                    .increment(1);
            }
            None => {
                metrics::counter!("radredeye_frame_submitted_total").increment(1);
            }
        }
    }

    /// Record a dropped frame labelled by `reason` (e.g. `"backpressure"`,
    /// `"invalid"`). `frame_dropped_total` +1.
    pub fn inc_dropped(&self, reason: &str) {
        self.frame_dropped_total.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut by_reason) = self.dropped_by_reason.lock() {
            *by_reason.entry(reason.to_string()).or_insert(0) += 1;
        }
        match &self.stream_label {
            Some(s) => {
                metrics::counter!(
                    "radredeye_frame_dropped_total",
                    "reason" => reason.to_string(),
                    "stream" => s.clone()
                )
                .increment(1);
            }
            None => {
                metrics::counter!("radredeye_frame_dropped_total", "reason" => reason.to_string())
                    .increment(1);
            }
        }
    }

    /// Record a sink submission error labelled by `sink_kind`
    /// (e.g. `"stdout"`, `"file"`, `"http"`). `sink_errors_total` +1.
    pub fn inc_sink_error(&self, sink_kind: &str) {
        self.sink_errors_total.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut by_sink) = self.errors_by_sink.lock() {
            *by_sink.entry(sink_kind.to_string()).or_insert(0) += 1;
        }
        match &self.stream_label {
            Some(s) => {
                metrics::counter!(
                    "radredeye_sink_errors_total",
                    "sink" => sink_kind.to_string(),
                    "stream" => s.clone()
                )
                .increment(1);
            }
            None => {
                metrics::counter!("radredeye_sink_errors_total", "sink" => sink_kind.to_string())
                    .increment(1);
            }
        }
    }

    /// Record a `submit` wall-clock latency sample (milliseconds). Stored in an
    /// internal buffer (for testable p95 computation) and emitted to the
    /// `radredeye_frame_latency_ms` histogram facade.
    pub fn record_latency_ms(&self, ms: f64) {
        if let Ok(mut latencies) = self.latencies_ms.lock() {
            latencies.push(ms);
        }
        match &self.stream_label {
            Some(s) => {
                metrics::histogram!("radredeye_frame_latency_ms", "stream" => s.clone()).record(ms);
            }
            None => {
                metrics::histogram!("radredeye_frame_latency_ms").record(ms);
            }
        }
    }

    /// Total frames submitted (every `submit` call).
    pub fn frame_submitted(&self) -> u64 {
        self.frame_submitted_total.load(Ordering::Relaxed)
    }

    /// Total frames dropped (all reasons).
    pub fn frame_dropped(&self) -> u64 {
        self.frame_dropped_total.load(Ordering::Relaxed)
    }

    /// Frames dropped for a specific `reason`.
    pub fn dropped_for(&self, reason: &str) -> u64 {
        self.dropped_by_reason
            .lock()
            .map(|m| *m.get(reason).unwrap_or(&0))
            .unwrap_or(0)
    }

    /// Total sink submission errors (all sinks).
    pub fn sink_errors(&self) -> u64 {
        self.sink_errors_total.load(Ordering::Relaxed)
    }

    /// Sink errors for a specific `sink_kind`.
    pub fn errors_for(&self, sink_kind: &str) -> u64 {
        self.errors_by_sink
            .lock()
            .map(|m| *m.get(sink_kind).unwrap_or(&0))
            .unwrap_or(0)
    }

    /// Number of latency samples recorded.
    pub fn latency_sample_count(&self) -> usize {
        self.latencies_ms
            .lock()
            .map(|v| v.len())
            .unwrap_or(0)
    }

    /// All recorded latency samples (milliseconds), in insertion order.
    pub fn latencies_ms(&self) -> Vec<f64> {
        self.latencies_ms.lock().map(|v| v.clone()).unwrap_or_default()
    }

    /// Approximate p95 of the recorded latency samples (milliseconds).
    ///
    /// Returns `None` when no samples have been recorded. Uses nearest-rank on a
    /// sorted copy so it is deterministic and allocation-bounded; suitable for
    /// test assertions, not a high-throughput hot path.
    pub fn p95_latency_ms(&self) -> Option<f64> {
        let mut samples = self.latencies_ms();
        if samples.is_empty() {
            return None;
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // Nearest-rank p95: ceil(0.95 * n) - 1 (0-indexed), clamped to last.
        let n = samples.len();
        let rank = ((0.95_f64 * n as f64).ceil() as usize)
            .saturating_sub(1)
            .min(n - 1);
        Some(samples[rank])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapturePipeline, CaptureSink, CapturedFrame, PixelFormat, SinkError};
    use std::sync::Arc;
    use std::time::Instant;

    fn frame_2x2() -> CapturedFrame {
        CapturedFrame {
            width: 2,
            height: 2,
            format: PixelFormat::Rgba8,
            data: vec![0u8; 16],
            timestamp: Instant::now(),
        }
    }

    /// A sink that always fails, used to exercise `sink_errors_total`.
    struct FailingSink;
    impl CaptureSink for FailingSink {
        fn submit(&self, _frame: &CapturedFrame) -> Result<(), SinkError> {
            Err(SinkError::Transport("simulated failure".to_string()))
        }
        fn kind(&self) -> &'static str {
            "failing"
        }
    }

    #[test]
    fn counters_increment_on_submit() {
        let metrics = Arc::new(PipelineMetrics::new());
        let pipeline = CapturePipeline::new();
        pipeline.register_metrics(metrics.clone());
        pipeline.register_sink(Arc::new(FailingSink));

        assert_eq!(metrics.frame_submitted(), 0);
        assert_eq!(metrics.sink_errors(), 0);
        assert_eq!(metrics.latency_sample_count(), 0);

        pipeline.submit(&frame_2x2());

        assert_eq!(metrics.frame_submitted(), 1);
        assert_eq!(metrics.sink_errors(), 1);
        assert_eq!(metrics.errors_for("failing"), 1);
        assert_eq!(metrics.latency_sample_count(), 1);
    }

    #[test]
    fn dropped_frame_increments_dropped_counter() {
        let metrics = Arc::new(PipelineMetrics::new());
        let pipeline = CapturePipeline::new();
        pipeline.register_metrics(metrics.clone());
        pipeline.register_sink(Arc::new(crate::StdoutSink));
        // Tight backpressure: every frame after the first is dropped.
        pipeline.configure_shared(crate::CaptureConfig {
            min_interval: Some(std::time::Duration::from_secs(60)),
            ..Default::default()
        });

        pipeline.submit(&frame_2x2()); // accepted
        pipeline.submit(&frame_2x2()); // dropped (backpressure)

        assert_eq!(metrics.frame_submitted(), 2);
        assert_eq!(metrics.frame_dropped(), 1);
        assert_eq!(metrics.dropped_for("backpressure"), 1);
    }

    #[test]
    fn no_metrics_registered_is_noop() {
        // No register_metrics call — submit must not panic and must still run.
        let pipeline = CapturePipeline::new();
        pipeline.register_sink(Arc::new(crate::StdoutSink));
        pipeline.submit(&frame_2x2());
    }

    #[test]
    fn latency_budget_constant_matches_spec() {
        // Spec B1: pipeline-core dispatch p95 ≤ 3 ms @ 1080p.
        assert_eq!(LATENCY_BUDGET_P95_MS, 3.0);
    }

    #[test]
    fn for_stream_stamps_label_and_keeps_internal_counters_unlabelled() {
        let m = PipelineMetrics::for_stream("front");
        assert_eq!(m.stream_label(), Some("front"));
        // internal counters still work unlabelled (per-instance aggregates)
        m.inc_submitted();
        m.inc_dropped("invalid");
        m.inc_sink_error("stdout");
        assert_eq!(m.frame_submitted(), 1);
        assert_eq!(m.dropped_for("invalid"), 1);
        assert_eq!(m.errors_for("stdout"), 1);
        // default constructor has no stream label
        assert!(PipelineMetrics::new().stream_label().is_none());
    }

    #[test]
    fn p95_latency_computation() {
        let metrics = PipelineMetrics::new();
        // 20 samples: 0.0..19.0 → p95 nearest-rank = index ceil(0.95*20)-1 = 18 → 18.0
        for i in 0..20u64 {
            metrics.record_latency_ms(i as f64);
        }
        assert_eq!(metrics.latency_sample_count(), 20);
        let p95 = metrics.p95_latency_ms().expect("some samples");
        assert_eq!(p95, 18.0);
    }
}
