//! Reusable frame-diff primitives (Phase 10.4).
//!
//! [`pixel_diff`] generalises the Phase 9.2 golden `mean_pixel_diff` into a
//! reusable core helper that any sink or agent can call. It returns
//! [`DiffStats`] (mean/max absolute per-channel difference + changed-pixel
//! count) for two equal-shape frames. The golden harness
//! (`tests/golden.rs`) is migrated to call this so the comparison mechanism
//! becomes a shared primitive instead of test-private code.
//!
//! [`FrameDelta`] is the higher-level "semantic delta between two frames"
//! value produced by the (feature-gated) `SemanticDiffSink`: a `Pixel` delta
//! is derived purely from [`pixel_diff`] (no network, no model output — safe
//! to emit anywhere); a `Semantic` delta is intended to come from a reviewed
//! model endpoint and is never forwarded raw to agent-consumer sinks
//! (guardrails Law 4).

use crate::sinks::rgba_bytes;
use crate::stream::StreamId;
use crate::CapturedFrame;
#[cfg(test)]
use crate::PixelFormat;

/// Per-channel diff statistics between two equal-shape RGBA8 frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffStats {
    /// Mean absolute per-channel difference over all bytes (`[0, 255]`).
    pub mean_abs: f64,
    /// Maximum absolute per-channel difference across all bytes (`[0, 255]`).
    pub max_abs: u8,
    /// Number of pixels (4-byte groups) that differ in any channel.
    pub changed_pixels: usize,
}

/// The kind of [`FrameDelta`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaKind {
    /// Derived purely from [`pixel_diff`] — no network/model, safe anywhere.
    Pixel,
    /// Intended to be derived from a reviewed model endpoint. Raw model
    /// output is never forwarded to agent-consumer sinks (guardrails Law 4);
    /// the pixel-fallback stats are attached for review.
    Semantic,
}

/// A semantic delta between two frames (pixel- or model-derived).
#[derive(Debug, Clone)]
pub struct FrameDelta {
    /// The stream the delta belongs to, if known.
    pub stream_id: Option<StreamId>,
    /// How the delta was derived.
    pub kind: DeltaKind,
    /// Human-readable, guardrails-reviewable summary.
    pub summary: String,
    /// Per-channel statistics. Present for `Pixel` and the pixel-fallback of
    /// `Semantic`; `None` only when no previous frame existed.
    pub stats: Option<DiffStats>,
}

/// Compute per-channel absolute pixel difference between two frames.
///
/// Both frames are normalised to RGBA8 (via [`crate::sinks::rgba_bytes`]) before
/// comparing, so a BGRA frame and an RGBA frame of the same dimensions
/// compare on their RGBA representation. When the dimensions differ the
/// result reports a maximal diff (mean `255.0`, all pixels changed) rather
/// than panicking — callers that need an exact comparison should check
/// dimensions first.
pub fn pixel_diff(a: &CapturedFrame, b: &CapturedFrame) -> DiffStats {
    let ra = rgba_bytes(a);
    let rb = rgba_bytes(b);
    if ra.len() != rb.len() {
        let pixels = ra.len().max(rb.len()) / 4;
        return DiffStats {
            mean_abs: 255.0,
            max_abs: 255,
            changed_pixels: pixels,
        };
    }
    if ra.is_empty() {
        return DiffStats {
            mean_abs: 0.0,
            max_abs: 0,
            changed_pixels: 0,
        };
    }

    let mut sum: u64 = 0;
    let mut max_abs: u8 = 0;
    for (x, y) in ra.iter().zip(rb.iter()) {
        let d = (*x as i64 - *y as i64).unsigned_abs();
        sum += d;
        if d > u64::from(max_abs) {
            max_abs = d as u8;
        }
    }

    let mut changed_pixels = 0usize;
    for (pa, pb) in ra.chunks_exact(4).zip(rb.chunks_exact(4)) {
        if pa != pb {
            changed_pixels += 1;
        }
    }

    DiffStats {
        mean_abs: sum as f64 / ra.len() as f64,
        max_abs,
        changed_pixels,
    }
}

/// Build a human-readable summary of [`DiffStats`] for a `Pixel` delta.
pub fn pixel_summary(stats: &DiffStats) -> String {
    format!(
        "pixel diff: mean_abs={:.4} max_abs={} changed_pixels={}",
        stats.mean_abs, stats.max_abs, stats.changed_pixels
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn rgba(w: u32, h: u32, data: Vec<u8>) -> CapturedFrame {
        CapturedFrame {
            width: w,
            height: h,
            format: PixelFormat::Rgba8,
            data,
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn identical_frames_zero_diff() {
        let a = rgba(2, 2, vec![10, 20, 30, 255, 10, 20, 30, 255, 10, 20, 30, 255, 10, 20, 30, 255]);
        let b = a.clone();
        let s = pixel_diff(&a, &b);
        assert_eq!(s.mean_abs, 0.0);
        assert_eq!(s.max_abs, 0);
        assert_eq!(s.changed_pixels, 0);
    }

    #[test]
    fn mean_abs_matches_simple_expectation() {
        // one pixel differs by 4 in one channel over 4 bytes → mean = 4/4 = 1.0
        let a = rgba(1, 1, vec![0, 0, 0, 255]);
        let b = rgba(1, 1, vec![4, 0, 0, 255]);
        let s = pixel_diff(&a, &b);
        assert_eq!(s.mean_abs, 1.0);
        assert_eq!(s.max_abs, 4);
        assert_eq!(s.changed_pixels, 1);
    }

    #[test]
    fn bgra_vs_rgba_compare_on_rgba() {
        // BGRA (B=0,G=0,R=255,A=255) → RGBA (R=255,G=0,B=0,A=255)
        let a = CapturedFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Bgra8,
            data: vec![0, 0, 255, 255],
            timestamp: Instant::now(),
        };
        let b = rgba(1, 1, vec![255, 0, 0, 255]);
        let s = pixel_diff(&a, &b);
        assert_eq!(s.mean_abs, 0.0);
        assert_eq!(s.changed_pixels, 0);
    }

    #[test]
    fn mismatched_dimensions_report_max_diff() {
        let a = rgba(1, 1, vec![0, 0, 0, 255]);
        let b = rgba(2, 1, vec![0, 0, 0, 255, 0, 0, 0, 255]);
        let s = pixel_diff(&a, &b);
        assert_eq!(s.mean_abs, 255.0);
        assert_eq!(s.max_abs, 255);
        assert_eq!(s.changed_pixels, 2);
    }

    #[test]
    fn pixel_summary_format() {
        let s = DiffStats {
            mean_abs: 1.5,
            max_abs: 9,
            changed_pixels: 3,
        };
        let text = pixel_summary(&s);
        assert!(text.contains("mean_abs=1.5000"));
        assert!(text.contains("max_abs=9"));
        assert!(text.contains("changed_pixels=3"));
    }
}
