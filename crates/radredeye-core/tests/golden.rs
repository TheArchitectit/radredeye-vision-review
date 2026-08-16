//! Phase 9.2 — Visual / screenshot regression harness (golden-frame comparison).
//!
//! Real per-adapter golden capture (Bevy / Godot / WebXR) needs a running display
//! and GPU — unavailable in headless CI. This test proves the **comparison
//! mechanism** end-to-end on a deterministic, CPU-only synthetic frame: a known
//! RGBA gradient is encoded to PNG and compared (pixel-diff ≤ threshold) against a
//! committed golden PNG under `tests/golden/`.
//!
//! ## Modes
//!
//! - **Record** (`GOLDEN_RECORD=1`): writes the golden PNG when missing (or
//!   overwrites it). Use this to (re)generate `tests/golden/gradient_8x4.png`
//!   after an intentional change to the synthetic fixture.
//! - **Compare** (default): decodes the committed golden and the freshly-encoded
//!   frame, asserts the mean per-channel absolute pixel difference is within
//!   threshold. This is the CI mode.
//!
//! A second test (`golden_diff_detects_regression`) encodes a *different* frame
//! and asserts the comparison flags it — proving the harness catches regressions,
//! not just that it always passes.
//!
//! See `tests/golden/README.md` for why true per-adapter golden capture is wired
//! into CI separately (Phase 6.2 / 6.3 reference the adapters).

use std::path::PathBuf;
use std::time::Instant;
use radredeye_core::{diff::pixel_diff, sinks, CapturedFrame, PixelFormat};

/// Maximum acceptable mean per-channel absolute pixel difference between the
/// encoded frame and the golden. `0` for the deterministic gradient fixture
/// (identical input → identical pixels); a small non-zero budget would tolerate
/// lossy re-encoding, which the `image` PNG encoder is not.
const PIXEL_DIFF_THRESHOLD: f64 = 0.0;

/// Repo-relative directory holding committed golden PNGs.
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("golden")
}

/// Deterministic 8×4 RGBA gradient fixture (CPU-only, no GPU/display needed).
///
/// Channel values are pure functions of (x, y) so the golden is reproducible
/// across machines and toolchain versions. R = x*32, G = y*64, B = 128, A = 255.
fn gradient_frame() -> CapturedFrame {
    let (width, height) = (8u32, 4u32);
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            data.push((x * 32) as u8); // R
            data.push((y * 64) as u8); // G
            data.push(128); // B
            data.push(255); // A
        }
    }
    CapturedFrame {
        width,
        height,
        format: PixelFormat::Rgba8,
        data,
        timestamp: Instant::now(),
    }
}

/// A deliberately *different* frame (solid colour) used to prove the comparison
/// detects regressions rather than always passing.
fn solid_frame() -> CapturedFrame {
    let (width, height) = (8u32, 4u32);
    CapturedFrame {
        width,
        height,
        format: PixelFormat::Rgba8,
        data: vec![200u8; (width * height * 4) as usize],
        timestamp: Instant::now(),
    }
}

/// Decode PNG bytes into (width, height, RGBA8 pixel data).
fn decode_png_rgba(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let img = image::load_from_memory(bytes).expect("decode golden png");
    let rgba = img.to_rgba8();
    (rgba.width(), rgba.height(), rgba.into_raw())
}

/// Build an RGBA8 [`CapturedFrame`] from decoded RGBA pixel data.
fn rgba_frame(w: u32, h: u32, data: Vec<u8>) -> CapturedFrame {
    CapturedFrame {
        width: w,
        height: h,
        format: PixelFormat::Rgba8,
        data,
        timestamp: Instant::now(),
    }
}

/// Mean per-channel absolute pixel difference between two equal-shape RGBA8
/// frames, computed via the reusable core primitive `radredeye_core::diff::pixel_diff`
/// (Phase 10.4 generalisation of this harness's former private `mean_pixel_diff`).
fn mean_pixel_diff(a: &CapturedFrame, b: &CapturedFrame) -> f64 {
    pixel_diff(a, b).mean_abs
}

#[test]
fn golden_gradient_matches() {
    let frame = gradient_frame();
    let encoded = sinks::encode_png(&frame).expect("encode gradient frame");
    let golden_path = golden_dir().join("gradient_8x4.png");

    let record = std::env::var("GOLDEN_RECORD")
        .map(|v| v == "1")
        .unwrap_or(false);
    if record {
        std::fs::create_dir_all(golden_dir()).expect("create golden dir");
        std::fs::write(&golden_path, &encoded).expect("write golden png");
        eprintln!(
            "[golden] recorded {} ({} bytes)",
            golden_path.display(),
            encoded.len()
        );
        return;
    }

    // Compare mode.
    let golden_bytes =
        std::fs::read(&golden_path).expect("golden missing; run: GOLDEN_RECORD=1 cargo test -p radredeye-core golden");
    let (gw, gh, gpx) = decode_png_rgba(&golden_bytes);
    let (ew, eh, epx) = decode_png_rgba(&encoded);
    assert_eq!((gw, gh), (ew, eh), "golden vs encoded dimensions differ");
    let diff = mean_pixel_diff(&rgba_frame(gw, gh, gpx), &rgba_frame(ew, eh, epx));
    assert!(
        diff <= PIXEL_DIFF_THRESHOLD,
        "golden pixel-diff {diff:.4} exceeds threshold {PIXEL_DIFF_THRESHOLD}"
    );
}

#[test]
fn golden_diff_detects_regression() {
    // A modified frame must NOT match the gradient golden — this proves the
    // comparison catches regressions rather than always passing.
    let frame = solid_frame();
    let encoded = sinks::encode_png(&frame).expect("encode solid frame");
    let golden_path = golden_dir().join("gradient_8x4.png");

    let golden_bytes = match std::fs::read(&golden_path) {
        Ok(b) => b,
        Err(_) => {
            // Golden not present (e.g. fresh checkout before record). We can't
            // compare, so skip rather than fail — the record-mode run creates it.
            eprintln!("[golden] gradient_8x4.png absent; skipping regression-detection test");
            return;
        }
    };
    let (gw, gh, gpx) = decode_png_rgba(&golden_bytes);
    let (ew, eh, epx) = decode_png_rgba(&encoded);
    assert_eq!((gw, gh), (ew, eh), "fixture dimensions must match golden");
    let diff = mean_pixel_diff(&rgba_frame(gw, gh, gpx), &rgba_frame(ew, eh, epx));
    assert!(
        diff > PIXEL_DIFF_THRESHOLD,
        "regression not detected: solid frame matched gradient golden (diff {diff:.4})"
    );
}
