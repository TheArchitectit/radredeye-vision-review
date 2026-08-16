//! Sprint 5.6 benchmark — nearest-neighbour resize latency.
//!
//! Measures `radredeye_core::resize_nearest` (added in Sprint 5.1) at a
//! few representative resolutions: a 1080p→720p downscale (the common capture
//! downscale path) and a 480p→1080p upscale (worst-case pixel expansion). The
//! function is the free `resize_nearest` in `lib.rs` (RGBA8 in, RGBA8 out) and
//! is the same code `CapturePipeline::apply_config` invokes when
//! `CaptureConfig::target_width`/`target_height` are set.
//!
//! Run with: `cargo bench -p radredeye-core -- --quick`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::time::Instant;
use radredeye_core::{CapturedFrame, PixelFormat, resize_nearest};

/// Build a solid RGBA8 frame of the given dimensions filled with a non-zero
/// pattern so the resize actually copies real bytes (avoids any zero-page
/// fast path the allocator might apply).
fn make_frame(width: u32, height: u32) -> CapturedFrame {
    let bytes = width as usize * height as usize * 4;
    let mut data = Vec::with_capacity(bytes);
    let mut v = 0u8;
    for _ in 0..bytes {
        data.push(v);
        v = v.wrapping_add(7);
    }
    CapturedFrame {
        width,
        height,
        format: PixelFormat::Rgba8,
        data,
        timestamp: Instant::now(),
    }
}

fn bench_resize_nearest(c: &mut Criterion) {
    let mut group = c.benchmark_group("resize_nearest");

    // (src_w, src_h, dst_w, dst_h, label) — representative transforms.
    let cases: &[(u32, u32, u32, u32, &str)] = &[
        (1920, 1080, 1280, 720, "1080p_to_720p"),
        (640, 480, 1920, 1080, "480p_to_1080p"),
        (1280, 720, 640, 360, "720p_to_360p"),
    ];

    for &(sw, sh, dw, dh, label) in cases {
        let frame = make_frame(sw, sh);
        group.bench_with_input(
            BenchmarkId::new(label, format!("{dw}x{dh}")),
            &(dw, dh),
            |b, &(dw, dh)| {
                b.iter(|| {
                    let _out = resize_nearest(&frame, dw, dh);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_resize_nearest);
criterion_main!(benches);
