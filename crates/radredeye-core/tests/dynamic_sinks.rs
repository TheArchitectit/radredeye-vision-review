//! Phase 10.1 — Dynamic sink registration / unsubscription.
//!
//! Verifies the `&self` [`CapturePipeline::register_sink`] /
//! [`unregister_sink`] API: a recording sink attached *mid-run* receives
//! subsequent frames, and removing it by handle stops delivery — all without
//! an exclusive `&mut` borrow on a running pipeline.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use radredeye_core::{CapturePipeline, CaptureSink, CapturedFrame, PixelFormat, SinkError};

/// A sink that counts every frame it receives.
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
fn sink_attached_mid_run_receives_subsequent_frames() {
    let pipeline = CapturePipeline::new();

    // No sinks yet — a frame is dropped silently (no panic).
    pipeline.submit(&frame());

    // Attach sink A mid-run.
    let a = Arc::new(RecordingSink::new("a"));
    let _handle_a = pipeline.register_sink(a.clone());
    pipeline.submit(&frame());
    assert_eq!(a.count(), 1, "sink A should receive the frame after registration");

    // Attach sink B mid-run (after frames have already been flowing).
    let b = Arc::new(RecordingSink::new("b"));
    let handle_b = pipeline.register_sink(b.clone());
    pipeline.submit(&frame());
    assert_eq!(a.count(), 2, "sink A keeps receiving");
    assert_eq!(b.count(), 1, "sink B receives subsequent frames only");

    // Remove B by handle; delivery to B must stop, A must continue.
    let removed = pipeline.unregister_sink(handle_b);
    assert!(removed.is_some(), "unregister returns the removed sink");
    pipeline.submit(&frame());
    assert_eq!(a.count(), 3, "sink A still receives");
    assert_eq!(b.count(), 1, "sink B no longer receives after unregister");

    // Unregistering an already-removed handle is a no-op (None).
    assert!(pipeline.unregister_sink(handle_b).is_none());
}

#[test]
fn register_sink_is_shared_self_no_mut_borrow() {
    // register_sink / unregister_sink take &self, so a pipeline shared behind
    // an Arc can be subscribed-to without a &mut borrow.
    let pipeline = Arc::new(CapturePipeline::new());
    let shared = pipeline.clone();
    let a = Arc::new(RecordingSink::new("a"));
    let handle = shared.register_sink(a.clone());
    pipeline.submit(&frame());
    assert_eq!(a.count(), 1);
    assert!(shared.unregister_sink(handle).is_some());
    pipeline.submit(&frame());
    assert_eq!(a.count(), 1, "delivery stopped after unregister on the clone");
}

#[test]
fn sink_count_reflects_register_unregister() {
    let pipeline = CapturePipeline::new();
    assert_eq!(pipeline.sink_count(), 0);
    let h1 = pipeline.register_sink(Arc::new(RecordingSink::new("a")));
    let h2 = pipeline.register_sink(Arc::new(RecordingSink::new("b")));
    assert_eq!(pipeline.sink_count(), 2);
    pipeline.unregister_sink(h1);
    assert_eq!(pipeline.sink_count(), 1);
    pipeline.unregister_sink(h2);
    assert_eq!(pipeline.sink_count(), 0);
}
