//! Sprint 5.6 benchmark — `CapturePipeline::submit` throughput.
//!
//! Measures how many frames per second the pipeline can accept and dispatch
//! to sinks when sink cost is effectively zero (a no-op `DevNullSink`). This
//! isolates the pipeline bus + `CaptureConfig` overhead from any real sink
//! I/O, giving a pure upper bound on submit throughput.
//!
//! Run with: `cargo bench -p radredeye-core -- --quick`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::time::Instant;
use radredeye_core::{
    CaptureConfig, CapturePipeline, CaptureSink, CapturedFrame, PixelFormat, SinkError,
};

/// No-op sink: discards every frame without any I/O. We deliberately avoid
/// `StdoutSink` here because it writes to stdout and would pollute criterion's
/// measurement output and skew timings with terminal I/O latency.
struct DevNullSink;

impl CaptureSink for DevNullSink {
    fn submit(&self, _frame: &CapturedFrame) -> Result<(), SinkError> {
        Ok(())
    }
}

/// Build a small RGBA8 frame of the given dimensions. Kept tiny so the bench
/// measures pipeline dispatch overhead, not pixel memcpy.
fn make_frame(width: u32, height: u32) -> CapturedFrame {
    let bytes = width as usize * height as usize * 4;
    CapturedFrame {
        width,
        height,
        format: PixelFormat::Rgba8,
        data: vec![0u8; bytes],
        timestamp: Instant::now(),
    }
}

/// Build a pipeline wired to `n` discard sinks and no `CaptureConfig` overrides
/// (passthrough path — the common case).
fn pipeline_with_sinks(n: usize) -> CapturePipeline {
    let pipeline = CapturePipeline::new();
    for _ in 0..n {
        pipeline.register_sink(Arc::new(DevNullSink));
    }
    pipeline
}

fn bench_submit(c: &mut Criterion) {
    let frame = make_frame(64, 64);

    let mut group = c.benchmark_group("submit_throughput");
    // Throughput mode: criterion reports ops/sec (submits/sec) for each batch.
    group.throughput(Throughput::Elements(1));

    for sink_count in [1usize, 4, 16] {
        let pipeline = pipeline_with_sinks(sink_count);
        group.bench_with_input(
            BenchmarkId::new("sinks", sink_count),
            &pipeline,
            |b, pipeline| {
                b.iter(|| {
                    // A single measured iteration submits one frame; criterion
                    // samples many iterations and reports the per-submit rate.
                    pipeline.submit(&frame);
                });
            },
        );
    }

    // Backpressure path: a min_interval of 0 lets every frame through while
    // still exercising the Mutex + elapsed check on each submit.
    let throttled = CapturePipeline::new();
    throttled.configure_shared(CaptureConfig {
        target_width: None,
        target_height: None,
        target_format: None,
        min_interval: Some(std::time::Duration::ZERO),
    });
    throttled.register_sink(Arc::new(DevNullSink));
    group.bench_function("backpressure_zero_interval", |b| {
        b.iter(|| {
            throttled.submit(&frame);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_submit);
criterion_main!(benches);
