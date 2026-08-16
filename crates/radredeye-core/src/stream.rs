//! Named logical capture streams and the registry that holds them (Phase 10.2).
//!
//! A [`CaptureStream`] is one logical capture channel — e.g. "front camera"
//! vs "rear camera" — with its own [`SinkRegistry`], optional per-stream
//! [`PipelineMetrics`], optional [`ReplayBuffer`], and its own
//! backpressure/last-submit clock. The [`StreamRegistry`] maps [`StreamId`] →
//! `Arc<CaptureStream>` and is held by [`crate::CapturePipeline`].
//!
//! A `"default"` stream always exists and mirrors the legacy single-pipeline
//! path: `CapturePipeline::submit` delegates to it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::metrics::PipelineMetrics;
use crate::registry::{SinkHandle, SinkRegistry};
use crate::replay::ReplayBuffer;
use crate::sinks;
use crate::{resize_nearest, CaptureConfig, CapturedFrame, PixelFormat};

/// Name of the always-present legacy stream.
pub const DEFAULT_STREAM_NAME: &str = "default";

/// A lightweight, cloneable, hashable name for a [`CaptureStream`].
///
/// Backed by an `Arc<str>` so cloning a [`StreamId`] is cheap and two ids for
/// the same name compare equal (needed as a `HashMap` key).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(pub(crate) Arc<str>);

impl StreamId {
    /// Create a stream id from a name.
    pub fn new(name: &str) -> Self {
        Self(Arc::from(name))
    }

    /// The name of the default stream (`"default"`).
    pub const DEFAULT: &'static str = DEFAULT_STREAM_NAME;

    /// Borrow the stream name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// One logical capture stream: sinks + telemetry + replay.
///
/// All mutators take `&self` (interior mutability), so a stream shared behind an
/// `Arc` — as [`crate::CapturePipeline`] holds it — can be reconfigured, have
/// sinks attached/detached, and accept frames without an exclusive borrow.
pub struct CaptureStream {
    id: StreamId,
    config: Arc<RwLock<CaptureConfig>>,
    sinks: SinkRegistry,
    last_submit: Mutex<Instant>,
    metrics: Mutex<Option<Arc<PipelineMetrics>>>,
    replay: Mutex<Option<Arc<ReplayBuffer>>>,
}

impl CaptureStream {
    /// Construct a fresh, empty stream with the default config.
    pub(crate) fn new(id: StreamId) -> Self {
        Self {
            id,
            config: Arc::new(RwLock::new(CaptureConfig::default())),
            sinks: SinkRegistry::new(),
            // start "long ago" so the first frame is never backpressure-dropped.
            last_submit: Mutex::new(Instant::now() - Duration::from_secs(3600)),
            metrics: Mutex::new(None),
            replay: Mutex::new(None),
        }
    }

    /// This stream's id.
    pub fn id(&self) -> &StreamId {
        &self.id
    }

    /// Register a sink on this stream. Returns a handle for later removal.
    pub fn register_sink(&self, sink: Arc<dyn crate::CaptureSink>) -> SinkHandle {
        self.sinks.register(sink)
    }

    /// Remove a previously registered sink by handle.
    pub fn unregister_sink(&self, handle: SinkHandle) -> Option<Arc<dyn crate::CaptureSink>> {
        self.sinks.unregister(handle)
    }

    /// Replace this stream's capture configuration (resize/format/backpressure).
    pub fn configure(&self, config: CaptureConfig) {
        if let Ok(mut slot) = self.config.write() {
            *slot = config;
        }
    }

    /// Register a [`PipelineMetrics`] handle on this stream.
    pub fn register_metrics(&self, metrics: Arc<PipelineMetrics>) {
        if let Ok(mut slot) = self.metrics.lock() {
            *slot = Some(metrics);
        }
    }

    /// Attach a [`ReplayBuffer`] to this stream; subsequent accepted frames are
    /// appended to it. Opt-in — core streams have no replay buffer by default.
    pub fn attach_replay(&self, replay: Arc<ReplayBuffer>) {
        if let Ok(mut slot) = self.replay.lock() {
            *slot = Some(replay);
        }
    }

    /// The attached replay buffer, if any.
    pub fn replay(&self) -> Option<Arc<ReplayBuffer>> {
        self.replay.lock().ok().and_then(|slot| slot.clone())
    }

    /// Number of sinks currently registered on this stream.
    pub fn sink_count(&self) -> usize {
        self.sinks.len()
    }

    /// Stable `kind()` labels of the sinks registered on this stream, in
    /// registration order.
    pub fn sink_kinds(&self) -> Vec<&'static str> {
        self.sinks.kinds()
    }

    /// Snapshot the registered metrics handle (if any) without holding the lock
    /// across emission. `None` when no metrics are registered — a no-op.
    fn metrics_handle(&self) -> Option<Arc<PipelineMetrics>> {
        self.metrics.lock().ok().and_then(|slot| slot.clone())
    }

    fn record_latency(&self, metrics: &Option<Arc<PipelineMetrics>>, start: Instant) {
        if let Some(m) = metrics {
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            m.record_latency_ms(ms);
        }
    }

    /// Apply [`CaptureConfig`] (resize/reformat) to a frame, returning a new
    /// frame. With no config set this is a zero-cost passthrough (clone).
    fn apply_config(&self, frame: &CapturedFrame, config: &CaptureConfig) -> CapturedFrame {
        let mut out = frame.clone();
        if let Some(target) = config.target_format {
            if out.format != target {
                let rgba = sinks::rgba_bytes(&out);
                out = CapturedFrame {
                    width: out.width,
                    height: out.height,
                    format: PixelFormat::Rgba8,
                    data: rgba,
                    timestamp: out.timestamp,
                };
            }
        }
        let tw = config.target_width.unwrap_or(out.width);
        let th = config.target_height.unwrap_or(out.height);
        if tw != out.width || th != out.height {
            out = resize_nearest(&out, tw, th);
        }
        out
    }

    /// Submit a frame to every registered sink on this stream.
    ///
    /// Preserves the exact Phase 9.1 emission order: `inc_submitted` →
    /// backpressure check (`inc_dropped("backpressure")`) → `validate`
    /// (`inc_dropped("invalid")`) → `apply_config` → fan-out
    /// (`inc_sink_error` per failing sink) → replay append →
    /// `record_latency_ms`.
    pub fn submit(&self, frame: &CapturedFrame) {
        let start = Instant::now();
        let metrics = self.metrics_handle();
        if let Some(m) = &metrics {
            m.inc_submitted();
        }

        // Snapshot the config once (cheap clone) so we don't hold the RwLock
        // across sink I/O.
        let config = self
            .config
            .read()
            .map(|c| c.clone())
            .unwrap_or_default();

        // Backpressure: drop frames that arrive faster than min_interval.
        if let Some(interval) = config.min_interval {
            let too_soon = match self.last_submit.lock() {
                Ok(mut last) => {
                    let elapsed = last.elapsed();
                    if elapsed < interval {
                        true
                    } else {
                        *last = Instant::now();
                        false
                    }
                }
                Err(_) => true, // poisoned lock — conservatively drop
            };
            if too_soon {
                tracing::debug!("frame dropped (backpressure)");
                if let Some(m) = &metrics {
                    m.inc_dropped("backpressure");
                }
                self.record_latency(&metrics, start);
                return;
            }
        }

        if let Err(e) = frame.validate() {
            tracing::warn!(error = %e, "dropped invalid frame");
            if let Some(m) = &metrics {
                m.inc_dropped("invalid");
            }
            self.record_latency(&metrics, start);
            return;
        }

        let effective = self.apply_config(frame, &config);
        let sinks = self.sinks.snapshot();
        for sink in &sinks {
            if let Err(e) = sink.submit(&effective) {
                tracing::error!(error = %e, sink = sink.kind(), "sink submission failed");
                if let Some(m) = &metrics {
                    m.inc_sink_error(sink.kind());
                }
            }
        }

        // Append the processed frame to the optional replay buffer.
        if let Ok(slot) = self.replay.lock() {
            if let Some(buf) = slot.as_ref() {
                buf.push(effective.clone());
            }
        }

        self.record_latency(&metrics, start);
    }

    /// Shutdown hook — flushes every registered sink.
    pub fn shutdown(&self) {
        for sink in self.sinks.snapshot() {
            sink.on_shutdown();
        }
    }
}

/// Maps [`StreamId`] → `Arc<CaptureStream>`. Held (shared) by
/// [`crate::CapturePipeline`].
///
/// Cloning a `StreamRegistry` shares the same underlying map, so a cloned
/// [`crate::CapturePipeline`] sees the same streams. A `"default"` stream
/// always exists.
#[derive(Clone, Default)]
pub struct StreamRegistry {
    streams: Arc<RwLock<HashMap<StreamId, Arc<CaptureStream>>>>,
}

impl std::fmt::Debug for StreamRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self
            .streams
            .read()
            .map(|s| s.keys().map(|k| k.as_str().to_string()).collect())
            .unwrap_or_default();
        f.debug_struct("StreamRegistry").field("streams", &names).finish()
    }
}

impl StreamRegistry {
    /// Create a registry with a single `"default"` stream.
    pub fn new() -> Self {
        let mut map = HashMap::new();
        let default_id = StreamId::new(StreamId::DEFAULT);
        map.insert(default_id.clone(), Arc::new(CaptureStream::new(default_id)));
        Self {
            streams: Arc::new(RwLock::new(map)),
        }
    }

    /// Create (or replace) a named stream and return a handle to it.
    pub fn create_stream(&self, id: &str) -> Arc<CaptureStream> {
        let stream = Arc::new(CaptureStream::new(StreamId::new(id)));
        if let Ok(mut streams) = self.streams.write() {
            streams.insert(StreamId::new(id), Arc::clone(&stream));
        }
        stream
    }

    /// Look up a stream by name.
    pub fn stream(&self, id: &str) -> Option<Arc<CaptureStream>> {
        self.streams
            .read()
            .ok()
            .and_then(|s| s.get(&StreamId::new(id)).cloned())
    }

    /// The always-present `"default"` stream. Never `None`.
    pub fn default_stream(&self) -> Arc<CaptureStream> {
        if let Some(s) = self.stream(StreamId::DEFAULT) {
            return s;
        }
        // Defensive: the default stream should always exist, but if it was
        // removed, recreate it rather than panic.
        let s = Arc::new(CaptureStream::new(StreamId::new(StreamId::DEFAULT)));
        if let Ok(mut streams) = self.streams.write() {
            streams.insert(StreamId::new(StreamId::DEFAULT), Arc::clone(&s));
        }
        s
    }

    /// Remove a named stream. Removing `"default"` is allowed but it will be
    /// recreated on the next [`StreamRegistry::default_stream`] call.
    pub fn remove_stream(&self, id: &str) -> Option<Arc<CaptureStream>> {
        self.streams
            .write()
            .ok()
            .and_then(|mut s| s.remove(&StreamId::new(id)))
    }

    /// Snapshot of the names of all current streams.
    pub fn list(&self) -> Vec<StreamId> {
        self.streams
            .read()
            .ok()
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CaptureSink, SinkError};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn frame() -> CapturedFrame {
        CapturedFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![0, 0, 0, 255],
            timestamp: Instant::now(),
        }
    }

    struct CountingSink {
        count: AtomicUsize,
    }
    impl CaptureSink for CountingSink {
        fn submit(&self, _frame: &CapturedFrame) -> Result<(), SinkError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn kind(&self) -> &'static str {
            "counting"
        }
    }

    #[test]
    fn default_stream_always_present() {
        let reg = StreamRegistry::new();
        let d = reg.default_stream();
        assert_eq!(d.id().as_str(), "default");
        let names = reg.list();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].as_str(), "default");
    }

    #[test]
    fn create_and_lookup_named_stream() {
        let reg = StreamRegistry::new();
        let front = reg.create_stream("front");
        assert_eq!(front.id().as_str(), "front");
        assert!(reg.stream("front").is_some());
        assert!(reg.stream("rear").is_none());
        assert_eq!(reg.list().len(), 2);
    }

    #[test]
    fn submit_fans_out_to_registered_sinks() {
        let reg = StreamRegistry::new();
        let stream = reg.default_stream();
        let sink = Arc::new(CountingSink {
            count: AtomicUsize::new(0),
        });
        let handle = stream.register_sink(sink.clone());
        stream.submit(&frame());
        assert_eq!(sink.count.load(Ordering::Relaxed), 1);
        stream.unregister_sink(handle);
        stream.submit(&frame());
        assert_eq!(sink.count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn submit_appends_to_attached_replay() {
        let reg = StreamRegistry::new();
        let stream = reg.default_stream();
        let buf = Arc::new(ReplayBuffer::new(5));
        stream.attach_replay(Arc::clone(&buf));
        stream.submit(&frame());
        stream.submit(&frame());
        assert_eq!(buf.len(), 2);
        // replay oldest-first returns both frames
        assert_eq!(buf.replay(0..2).len(), 2);
    }

    #[test]
    fn per_stream_metrics_recorded() {
        let reg = StreamRegistry::new();
        let stream = reg.default_stream();
        let m = Arc::new(PipelineMetrics::new());
        stream.register_metrics(Arc::clone(&m));
        stream.submit(&frame());
        assert_eq!(m.frame_submitted(), 1);
        assert_eq!(m.latency_sample_count(), 1);
    }

    #[test]
    fn backpressure_drops_frame() {
        let reg = StreamRegistry::new();
        let stream = reg.default_stream();
        let m = Arc::new(PipelineMetrics::new());
        stream.register_metrics(Arc::clone(&m));
        stream.configure(CaptureConfig {
            min_interval: Some(Duration::from_secs(60)),
            ..Default::default()
        });
        stream.submit(&frame()); // accepted
        stream.submit(&frame()); // dropped (backpressure)
        assert_eq!(m.frame_submitted(), 2);
        assert_eq!(m.dropped_for("backpressure"), 1);
        // no replay attached on the default stream by default
        assert!(stream.replay().is_none());
    }
}
