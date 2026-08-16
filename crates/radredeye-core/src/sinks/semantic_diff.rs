//! Semantic frame-diff sink (Phase 10.4, feature = "semantic-diff").
//!
//! [`crate::sinks::semantic_diff::SemanticDiffSink`] implements [`CaptureSink`],
//! buffers the previous frame, and on each [`submit`](crate::CaptureSink::submit)
//! computes a [`FrameDelta`](crate::diff::FrameDelta) between the previous and
//! current frame:
//!
//! - **Pixel fallback** (the default; `RADREDEYE_DIFF_URL` unset): `kind = Pixel`,
//!   derived purely from [`pixel_diff`](crate::diff::pixel_diff). No network,
//!   no model output — safe to emit anywhere.
//! - **Semantic path** (`RADREDEYE_DIFF_URL` set): `kind = Semantic`. For Phase 10
//!   only the pixel-fallback statistics are attached; raw model output is
//!   **never** forwarded to agent-consumer sinks (guardrails Law 4). The
//!   semantic/model integration is deferred behind a review sink and documented
//!   as the review boundary.
//!
//! The computed [`FrameDelta`](crate::diff::FrameDelta) is stored on the sink
//! and retrieved via [`SemanticDiffSink::last_delta`](crate::sinks::semantic_diff::SemanticDiffSink::last_delta)
//! — an agent **pulls** the reviewed delta rather than having model output
//! pushed to it. The current frame is also forwarded to the configured
//! `downstream` sink (frames are safe; no model output flows downstream).

use std::sync::{Arc, Mutex};

use crate::diff::{pixel_diff, pixel_summary, DeltaKind, DiffStats, FrameDelta};
use crate::{CaptureSink, CapturedFrame, SinkError};

/// A capture sink that computes a per-frame [`FrameDelta`] vs the previous
/// frame and forwards the current frame to a downstream sink.
///
/// See the [module docs](self) for the pixel-fallback vs semantic path
/// distinction and the guardrails Law 4 review boundary.
pub struct SemanticDiffSink {
    prev: Mutex<Option<CapturedFrame>>,
    latest: Mutex<Option<FrameDelta>>,
    /// When `Some`, the semantic path is selected (kind = Semantic). The actual
    /// model integration is deferred (Phase 10 ships pixel-fallback only); raw
    /// model output is never forwarded to agent-consumer sinks.
    diff_url: Option<String>,
    downstream: Arc<dyn CaptureSink>,
}

impl std::fmt::Debug for SemanticDiffSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticDiffSink")
            .field("diff_url", &self.diff_url)
            .field("has_prev", &self.prev.lock().map(|p| p.is_some()).unwrap_or(false))
            .finish()
    }
}

impl SemanticDiffSink {
    /// Construct a sink that forwards frames to `downstream`. If the
    /// `RADREDEYE_DIFF_URL` environment variable is set (and non-empty), the
    /// semantic path is selected (kind = Semantic); otherwise the pixel-fallback
    /// path (kind = Pixel) is used.
    pub fn new(downstream: Arc<dyn CaptureSink>) -> Self {
        // RADREDEYE_DIFF_URL enables the semantic path (gated; model output is
        // never forwarded to agent-consumer sinks — guardrails Law 4).
        let diff_url = std::env::var("RADREDEYE_DIFF_URL")
            .ok()
            .filter(|s| !s.is_empty());
        Self::with_diff_url(downstream, diff_url)
    }

    /// Construct with an explicit `diff_url` (`Some` selects the semantic
    /// path). Useful for tests that must force one path regardless of the
    /// process environment.
    pub fn with_diff_url(downstream: Arc<dyn CaptureSink>, diff_url: Option<String>) -> Self {
        Self {
            prev: Mutex::new(None),
            latest: Mutex::new(None),
            diff_url,
            downstream,
        }
    }

    /// The most recently computed [`FrameDelta`], if any. An agent pulls this
    /// reviewed value rather than receiving model output directly (Law 4).
    pub fn last_delta(&self) -> Option<FrameDelta> {
        self.latest.lock().ok().and_then(|slot| slot.clone())
    }

    /// Whether the semantic path is selected (a `RADREDEYE_DIFF_URL` is set).
    pub fn is_semantic(&self) -> bool {
        self.diff_url.is_some()
    }

    fn kind(&self) -> DeltaKind {
        if self.diff_url.is_some() {
            DeltaKind::Semantic
        } else {
            DeltaKind::Pixel
        }
    }

    fn build_summary(&self, stats: &DiffStats) -> String {
        if self.diff_url.is_none() {
            pixel_summary(stats)
        } else {
            // Semantic path: model output is NOT forwarded to agent-consumer
            // sinks (guardrails Law 4). Phase 10 ships the pixel-fallback
            // statistics only; the reviewed model integration is deferred.
            format!(
                "semantic delta (gated): pixel-fallback stats mean_abs={:.4} \
                 max_abs={} changed_pixels={}; model output not forwarded (Law 4)",
                stats.mean_abs, stats.max_abs, stats.changed_pixels
            )
        }
    }

    fn store_delta(&self, delta: FrameDelta) {
        if let Ok(mut slot) = self.latest.lock() {
            *slot = Some(delta);
        }
    }
}

impl CaptureSink for SemanticDiffSink {
    fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError> {
        let delta = {
            let mut prev = self
                .prev
                .lock()
                .map_err(|e| SinkError::Transport(format!("prev lock: {e}")))?;
            let kind = self.kind();
            let delta = match prev.as_ref() {
                Some(previous) => {
                    let stats = pixel_diff(previous, frame);
                    let summary = self.build_summary(&stats);
                    FrameDelta {
                        stream_id: None,
                        kind,
                        summary,
                        stats: Some(stats),
                    }
                }
                None => FrameDelta {
                    stream_id: None,
                    kind,
                    summary: "first frame (no previous)".to_string(),
                    stats: None,
                },
            };
            *prev = Some(frame.clone());
            delta
        };
        self.store_delta(delta);
        // Forward the current frame to the downstream sink. Only frames flow
        // downstream — never raw model output (guardrails Law 4).
        self.downstream.submit(frame)
    }

    fn on_shutdown(&self) {
        self.downstream.on_shutdown();
    }

    fn kind(&self) -> &'static str {
        "semantic-diff"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CaptureSink, PixelFormat};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    /// A downstream sink that counts received frames.
    struct CountSink {
        count: AtomicUsize,
    }
    impl CaptureSink for CountSink {
        fn submit(&self, _frame: &CapturedFrame) -> Result<(), SinkError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn kind(&self) -> &'static str {
            "count"
        }
    }

    fn frame(tag: u8) -> CapturedFrame {
        CapturedFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![tag, 0, 0, 255],
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn pixel_fallback_kind_and_stats_when_no_diff_url() {
        let downstream = Arc::new(CountSink {
            count: AtomicUsize::new(0),
        });
        let sink = SemanticDiffSink::with_diff_url(downstream.clone(), None);
        assert!(!sink.is_semantic());

        // First frame: no previous → delta with no stats.
        sink.submit(&frame(10)).expect("submit 0");
        let first = sink.last_delta().expect("delta after first");
        assert_eq!(first.kind, DeltaKind::Pixel);
        assert!(first.stats.is_none());
        assert!(first.summary.contains("first frame"));
        assert_eq!(downstream.count.load(Ordering::Relaxed), 1);

        // Second frame: pixel diff computed.
        sink.submit(&frame(20)).expect("submit 1");
        let second = sink.last_delta().expect("delta after second");
        assert_eq!(second.kind, DeltaKind::Pixel);
        let stats = second.stats.expect("pixel stats");
        // one channel differs by 10 over 4 bytes → mean = 10/4 = 2.5
        assert_eq!(stats.mean_abs, 2.5);
        assert_eq!(stats.max_abs, 10);
        assert_eq!(stats.changed_pixels, 1);
        assert!(second.summary.contains("mean_abs=2.5000"));
        assert_eq!(downstream.count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn semantic_kind_when_diff_url_set_but_no_model_output_forwarded() {
        let downstream = Arc::new(CountSink {
            count: AtomicUsize::new(0),
        });
        let sink = SemanticDiffSink::with_diff_url(
            downstream.clone(),
            Some("http://example.invalid/diff".to_string()),
        );
        assert!(sink.is_semantic());

        sink.submit(&frame(1)).expect("submit 0");
        sink.submit(&frame(2)).expect("submit 1");
        let delta = sink.last_delta().expect("delta");
        // kind is Semantic (gated), but the summary documents Law 4: no raw
        // model output is forwarded; only pixel-fallback stats are attached.
        assert_eq!(delta.kind, DeltaKind::Semantic);
        assert!(delta.summary.contains("not forwarded"));
        assert!(delta.stats.is_some(), "pixel-fallback stats still attached");
        // downstream only ever receives frames (2 frames), never model output.
        assert_eq!(downstream.count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn delta_recorded_in_stream_order() {
        let downstream = Arc::new(CountSink {
            count: AtomicUsize::new(0),
        });
        let sink = SemanticDiffSink::with_diff_url(downstream, None);
        // stream_id is unset when computed directly on the sink (no stream
        // context).
        sink.submit(&frame(5)).expect("ok");
        assert!(sink.last_delta().expect("d").stream_id.is_none());
    }
}
