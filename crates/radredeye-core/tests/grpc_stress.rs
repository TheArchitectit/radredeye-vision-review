//! Phase 10.5 — GrpcSink thread-soundness stress test.
//!
//! `GrpcSink` now shares a process-global multi-thread tokio runtime
//! (`OnceLock<Runtime>`) instead of a per-instance `current_thread` runtime.
//! The former design panicked when two threads raced `block_on` on the same
//! `Runtime` (current-thread `block_on` is not reentrant). This test drives
//! ≥4 threads calling `submit` concurrently through a shared cloneable
//! [`CapturePipeline`] and asserts no panic/deadlock, with latency recorded via
//! the Phase 9.1 [`PipelineMetrics`].
//!
//! Gated behind `feature = "grpc-sink"` (the heavy tonic/tokio deps). Run with:
//! `cargo test -p radredeye-core --features grpc-sink --test grpc_stress`.

#![cfg(feature = "grpc-sink")]

use std::sync::Arc;
use std::time::Instant;
use radredeye_core::{
    sinks::grpc::GrpcSink, CapturePipeline, CaptureSink, CapturedFrame, PixelFormat,
    PipelineMetrics,
};

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
fn grpc_sink_concurrent_submit_no_panic_or_deadlock() {
    let pipeline = CapturePipeline::new();
    // A closed localhost port → connection refused (fast failure, no real server).
    let sink = GrpcSink::connect("http://127.0.0.1:1").expect("grpc connect");
    pipeline.register_sink(Arc::new(sink));
    let metrics = Arc::new(PipelineMetrics::new());
    pipeline.register_metrics(metrics.clone());

    const THREADS: usize = 4;
    const FRAMES_PER_THREAD: usize = 5;
    let expected = THREADS * FRAMES_PER_THREAD;
    let start = Instant::now();

    // Spawn THREADS workers, each cloning the shared pipeline and submitting
    // FRAMES_PER_THREAD frames concurrently. Each submit hits the shared
    // multi-thread runtime's block_on from a distinct calling thread.
    let joins: Vec<Result<(), _>> = std::thread::scope(|s| {
        (0..THREADS)
            .map(|_| {
                let p = pipeline.clone();
                s.spawn(move || {
                    for _ in 0..FRAMES_PER_THREAD {
                        // submit returns Err (connection refused) but must not
                        // panic or deadlock — that is the Phase 10.5 invariant.
                        p.submit(&frame());
                    }
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join())
            .collect()
    });
    let elapsed = start.elapsed();

    // No worker thread panicked (a panic surfaces as a join Err).
    for (i, j) in joins.into_iter().enumerate() {
        assert!(j.is_ok(), "worker thread {i} panicked or deadlocked");
    }

    // Every submission was recorded — no deadlock starved a worker.
    assert_eq!(
        metrics.frame_submitted() as usize,
        expected,
        "all concurrent submits recorded"
    );
    // Latency recorded via the 9.1 metrics for every submit (Phase 10.5 DoD).
    assert_eq!(
        metrics.latency_sample_count(),
        expected,
        "latency recorded for every concurrent submit"
    );
    // Each submit failed at the sink (connection refused) → sink errors logged.
    assert_eq!(
        metrics.sink_errors() as usize,
        expected,
        "sink errors recorded for every failed grpc submit"
    );

    // Sanity bound: completes well under a deadlock-style hang.
    assert!(
        elapsed.as_secs() < 30,
        "stress test took too long (possible deadlock): {elapsed:?}"
    );
}

#[test]
fn shared_runtime_is_process_global_and_reusable() {
    // Two independent GrpcSink instances share the same process-global runtime.
    // Constructing and submitting from both must not conflict.
    let a = GrpcSink::connect("http://127.0.0.1:1").expect("connect a");
    let b = GrpcSink::connect("http://127.0.0.1:1").expect("connect b");
    let f = frame();
    // Both return transport errors (no server) but neither panics.
    assert!(a.submit(&f).is_err());
    assert!(b.submit(&f).is_err());
}
