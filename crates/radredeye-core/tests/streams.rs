//! Phase 10.2 — Named capture streams / registry routing.
//!
//! Verifies that named streams route independently: a frame submitted to
//! `submit_to("front", …)` reaches only the sinks registered on `"front"`,
//! not those on `"rear"` or the `"default"` stream. `submit` (the legacy path)
//! continues to deliver to the `"default"` stream only.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use radredeye_core::{CapturePipeline, CaptureSink, CapturedFrame, PixelFormat, SinkError};

struct RecordingSink {
    count: AtomicUsize,
    label: &'static str,
}

impl RecordingSink {
    fn new(label: &'static str) -> Self {
        Self {
            count: AtomicUsize::new(0),
            label,
        }
    }
    fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

impl CaptureSink for RecordingSink {
    fn submit(&self, _frame: &CapturedFrame) -> Result<(), SinkError> {
        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn kind(&self) -> &'static str {
        self.label
    }
}

fn frame() -> CapturedFrame {
    CapturedFrame {
        width: 1,
        height: 1,
        format: PixelFormat::Rgba8,
        data: vec![0, 0, 0, 255],
        timestamp: Instant::now(),
    }
}

#[test]
fn named_streams_route_independently() {
    let pipeline = CapturePipeline::new();

    // Sinks on each stream.
    let front_sink = Arc::new(RecordingSink::new("front"));
    let rear_sink = Arc::new(RecordingSink::new("rear"));
    let default_sink = Arc::new(RecordingSink::new("default"));

    // The "default" stream always exists.
    pipeline.register_sink(default_sink.clone());

    // Create named streams and register sinks on them.
    let front = pipeline.create_stream("front");
    front.register_sink(front_sink.clone());
    let rear = pipeline.create_stream("rear");
    rear.register_sink(rear_sink.clone());

    // submit_to routes only to the targeted stream.
    pipeline.submit_to("front", &frame());
    assert_eq!(front_sink.count(), 1);
    assert_eq!(rear_sink.count(), 0, "rear must not receive front frames");
    assert_eq!(default_sink.count(), 0, "default must not receive front frames");

    pipeline.submit_to("rear", &frame());
    assert_eq!(rear_sink.count(), 1);
    assert_eq!(front_sink.count(), 1, "front must not receive rear frames");
    assert_eq!(default_sink.count(), 0);

    // submit (legacy) routes to the default stream only.
    pipeline.submit(&frame());
    assert_eq!(default_sink.count(), 1);
    assert_eq!(front_sink.count(), 1, "named streams must not receive default frames");
    assert_eq!(rear_sink.count(), 1);
}

#[test]
fn submit_to_unknown_stream_drops_frame_without_panicking() {
    let pipeline = CapturePipeline::new();
    let sink = Arc::new(RecordingSink::new("orphan"));
    pipeline.create_stream("known").register_sink(sink.clone());

    // An unknown stream is logged and dropped — never panics.
    pipeline.submit_to("does-not-exist", &frame());
    assert_eq!(sink.count(), 0, "unknown stream must not deliver anywhere");

    // The known stream is unaffected.
    pipeline.submit_to("known", &frame());
    assert_eq!(sink.count(), 1);
}

#[test]
fn create_stream_replaces_existing() {
    let pipeline = CapturePipeline::new();
    let first = pipeline.create_stream("cam");
    let a = Arc::new(RecordingSink::new("a"));
    first.register_sink(a.clone());

    // Recreating replaces the stream; the old sinks are gone from the registry.
    let second = pipeline.create_stream("cam");
    let b = Arc::new(RecordingSink::new("b"));
    second.register_sink(b.clone());

    pipeline.submit_to("cam", &frame());
    assert_eq!(a.count(), 0, "old stream's sinks are not retained after replace");
    assert_eq!(b.count(), 1, "new stream's sinks receive frames");
}

#[test]
fn per_stream_metrics_are_isolated() {
    use radredeye_core::PipelineMetrics;

    let pipeline = CapturePipeline::new();
    let front = pipeline.create_stream("front");
    let front_metrics = Arc::new(PipelineMetrics::for_stream("front"));
    front.register_metrics(front_metrics.clone());
    front.register_sink(Arc::new(RecordingSink::new("front")));

    let default_metrics = Arc::new(PipelineMetrics::new());
    pipeline.register_metrics(default_metrics.clone());
    pipeline.register_sink(Arc::new(RecordingSink::new("default")));

    pipeline.submit_to("front", &frame());
    pipeline.submit(&frame()); // default

    assert_eq!(front_metrics.frame_submitted(), 1);
    assert_eq!(front_metrics.stream_label(), Some("front"));
    assert_eq!(default_metrics.frame_submitted(), 1);
    assert!(default_metrics.stream_label().is_none());
}
