//! Shared core for the radredeye capture pipeline.
//! Engine-agnostic: defines frames, sinks, and the pipeline bus.

use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

pub mod diff;
pub mod metrics;
pub mod registry;
pub mod replay;
pub mod sinks;
pub mod stream;

pub use metrics::{PipelineMetrics, LATENCY_BUDGET_P95_MS};
pub use registry::{SinkHandle, SinkRegistry};
pub use replay::ReplayBuffer;
pub use sinks::StdoutSink;
pub use stream::{CaptureStream, StreamId, StreamRegistry};

/// Default replay ring-buffer capacity (spec F4: cap 30). Re-exported from the
/// replay module for callers that want the canonical constant.
pub use replay::FRAME_STORE_CAP;

/// Runtime configuration applied to every frame before it reaches sinks.
///
/// Use this to downscale or reformat captured frames without changing the
/// engine adapter code.
#[derive(Debug, Clone, Default)]
pub struct CaptureConfig {
    /// If set, resize frames to this width (nearest-neighbour) before sinking.
    pub target_width: Option<u32>,
    /// If set, resize frames to this height (nearest-neighbour) before sinking.
    pub target_height: Option<u32>,
    /// If set, convert frames to this pixel format before sinking.
    pub target_format: Option<PixelFormat>,
    /// Minimum time between accepted frames. Frames arriving sooner are dropped.
    /// This is the pipeline-level backpressure mechanism.
    pub min_interval: Option<Duration>,
}

/// Pixel layout of a captured framebuffer.
///
/// Every [`CapturedFrame`] declares how its byte buffer is laid out so that
/// sinks and helpers (e.g. [`sinks::rgba_bytes`]) can convert between formats
/// without guessing. Both variants are 8-bit, 4 bytes per pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 8-bit RGBA, 4 bytes per pixel.
    Rgba8,
    /// 8-bit BGRA, 4 bytes per pixel (common Windows D3D output).
    Bgra8,
}

/// A single captured viewport frame.
///
/// This is the engine-agnostic unit of data that flows through the
/// [`CapturePipeline`]. Engine adapters copy a render target/backbuffer into
/// `data`, tag it with the matching [`PixelFormat`] and dimensions, stamp a
/// [`Instant`] timestamp, then hand it to [`CapturePipeline::submit`]. No
/// engine-specific handles ever leak past this struct.
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// How `data` is laid out (channels + byte order).
    pub format: PixelFormat,
    /// Raw pixel buffer, `width * height * bytes_per_pixel()` bytes long.
    pub data: Vec<u8>,
    /// When the frame was captured (engine adapter stamps this on creation).
    pub timestamp: Instant,
}

impl CapturedFrame {
    /// Bytes per pixel for the stored format.
    pub fn bytes_per_pixel(&self) -> usize {
        match self.format {
            PixelFormat::Rgba8 | PixelFormat::Bgra8 => 4,
        }
    }

    /// Sanity check that `data` length matches the declared dimensions.
    pub fn validate(&self) -> Result<(), SinkError> {
        let expected = self.width as usize * self.height as usize * self.bytes_per_pixel();
        if self.data.len() == expected {
            Ok(())
        } else {
            Err(SinkError::Transport(format!(
                "frame size mismatch: expected {} bytes, got {}",
                expected,
                self.data.len()
            )))
        }
    }
}

/// Something that consumes captured frames.
///
/// Implementations are registered with [`CapturePipeline::add_sink`] as
/// `Arc<dyn CaptureSink>` and invoked once per accepted frame. Sinks must be
/// [`Send`] + [`Sync`] because the pipeline fans frames out from any thread.
pub trait CaptureSink: Send + Sync {
    /// Receive and process a single frame.
    ///
    /// Returning [`Err`] only logs the failure — it never aborts the pipeline
    /// or the other sinks. Sinks that buffer/network should report transport
    /// problems via [`SinkError::Transport`] and encoding problems via
    /// [`SinkError::Encoding`].
    fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError>;

    /// Called once when the pipeline is shutting down. Flush buffered data here.
    fn on_shutdown(&self) {}

    /// Short, stable label identifying the sink kind (e.g. `"stdout"`,
    /// `"file"`, `"http"`). Used to label `sink_errors_total{sink}` metrics
    /// (Phase 9.1). The default returns `"sink"`; override for distinct
    /// labels.
    fn kind(&self) -> &'static str {
        "sink"
    }
}

/// Errors emitted while submitting frames.
///
/// Sinks return this from [`CaptureSink::submit`]. The pipeline logs the error
/// and continues with the next sink, so an error never aborts a capture run.
#[derive(Error, Debug)]
pub enum SinkError {
    /// A network/filesystem/IO failure moving the frame to its destination.
    #[error("transport error: {0}")]
    Transport(String),
    /// A failure encoding the frame (e.g. PNG compression) before sending.
    #[error("encoding error: {0}")]
    Encoding(String),
}

/// The central capture bus. Engine adapters call [`submit`] and the pipeline
/// applies the active [`CaptureConfig`] (resize/format/backpressure) then fans
/// the resulting frame out to every registered [`CaptureSink`] on the targeted
/// stream. Sinks are isolated — one sink's error never stops the others.
///
/// # Phase 10: shared / cloneable facade
///
/// `CapturePipeline` is now a thin [`Clone`] + [`Send`] + [`Sync`] facade over
/// a [`StreamRegistry`]. All state lives inside `Arc`-shared streams, so
/// cloning a pipeline shares the same streams/sinks (the "shared pipeline"
/// semantics the spec F2 asks for). Sinks and config are mutated through `&self`
/// methods ([`register_sink`], [`configure_shared`]) so a running pipeline held
/// behind an `Arc` can be reconfigured and subscribed-to without an exclusive
/// borrow.
///
/// A `"default"` stream always exists and mirrors the legacy single-pipeline
/// path: [`submit`] delegates to it. Named streams are created with
/// [`create_stream`] and targeted with [`submit_to`].
///
/// The legacy `&mut self` [`add_sink`] / [`configure`] are kept as
/// `#[deprecated]` wrappers so existing callers keep compiling; in-tree callers
/// have been migrated to the `&self` API.
///
/// [`submit`]: CapturePipeline::submit
/// [`register_sink`]: CapturePipeline::register_sink
/// [`configure_shared`]: CapturePipeline::configure_shared
/// [`create_stream`]: CapturePipeline::create_stream
/// [`submit_to`]: CapturePipeline::submit_to
/// [`add_sink`]: CapturePipeline::add_sink
/// [`configure`]: CapturePipeline::configure
#[derive(Clone)]
pub struct CapturePipeline {
    registry: StreamRegistry,
}

impl Default for CapturePipeline {
    fn default() -> Self {
        Self {
            registry: StreamRegistry::new(),
        }
    }
}

impl CapturePipeline {
    /// Create an empty pipeline with a single `"default"` stream and the
    /// default [`CaptureConfig`] (no resize, no backpressure). Add sinks with
    /// [`register_sink`].
    ///
    /// [`register_sink`]: CapturePipeline::register_sink
    pub fn new() -> Self {
        Self::default()
    }

    // ---- new &self API (preferred) ----

    /// Register a sink on the `"default"` stream. Sinks are kept in
    /// registration order and each accepted frame is delivered to all of them.
    /// Returns a [`SinkHandle`] for later removal via [`unregister_sink`].
    ///
    /// [`unregister_sink`]: CapturePipeline::unregister_sink
    pub fn register_sink(&self, sink: Arc<dyn CaptureSink>) -> SinkHandle {
        self.registry.default_stream().register_sink(sink)
    }

    /// Remove a previously registered sink (by handle) from the `"default"`
    /// stream. Returns the sink if it was present.
    pub fn unregister_sink(&self, handle: SinkHandle) -> Option<Arc<dyn CaptureSink>> {
        self.registry.default_stream().unregister_sink(handle)
    }

    /// Set the capture configuration on the `"default"` stream. Takes `&self`
    /// (interior mutability) so a shared pipeline can be reconfigured.
    pub fn configure_shared(&self, config: CaptureConfig) {
        self.registry.default_stream().configure(config);
    }

    /// Create (or replace) a named stream and return a handle to it.
    pub fn create_stream(&self, id: &str) -> Arc<CaptureStream> {
        self.registry.create_stream(id)
    }

    /// Look up a named stream.
    pub fn stream(&self, id: &str) -> Option<Arc<CaptureStream>> {
        self.registry.stream(id)
    }

    /// Submit a frame to a named stream. If the stream does not exist the frame
    /// is logged and dropped (no panic).
    pub fn submit_to(&self, stream_id: &str, frame: &CapturedFrame) {
        match self.registry.stream(stream_id) {
            Some(s) => s.submit(frame),
            None => tracing::warn!(stream = stream_id, "submit_to unknown stream; frame dropped"),
        }
    }

    /// Submit a frame to every registered sink on the `"default"` stream.
    ///
    /// This applies backpressure (`CaptureConfig::min_interval`) and a
    /// [`CapturedFrame::validate`] size check, dropping frames that fail either
    /// (logged, not panicked). Surviving frames are resized/reformatted per
    /// [`CaptureConfig`] and fanned out to each sink. A sink returning `Err` is
    /// logged and does not abort the other sinks.
    ///
    /// When a [`PipelineMetrics`] handle is registered (see
    /// [`register_metrics`]), this emits the Phase 9.1 telemetry. Emission is
    /// a no-op (never panics) when no metrics handle is registered or no global
    /// `metrics` recorder is installed.
    ///
    /// [`register_metrics`]: CapturePipeline::register_metrics
    pub fn submit(&self, frame: &CapturedFrame) {
        self.registry.default_stream().submit(frame);
    }

    /// Register a [`PipelineMetrics`] handle on the `"default"` stream.
    /// Takes `&self` so a shared pipeline can register without a `&mut` borrow.
    pub fn register_metrics(&self, metrics: Arc<PipelineMetrics>) {
        self.registry.default_stream().register_metrics(metrics);
    }

    /// Attach a [`ReplayBuffer`] to the `"default"` stream; subsequent accepted
    /// frames are appended to it. Opt-in — by default no replay buffer is
    /// attached in core.
    pub fn attach_replay(&self, replay: Arc<ReplayBuffer>) {
        self.registry.default_stream().attach_replay(replay);
    }

    /// Number of sinks currently registered on the `"default"` stream.
    pub fn sink_count(&self) -> usize {
        self.registry.default_stream().sink_count()
    }

    /// Names of all current streams (always includes `"default"`).
    ///
    /// Used by the MCP `list_streams` tool to enumerate capture channels.
    pub fn stream_ids(&self) -> Vec<String> {
        self.registry
            .list()
            .iter()
            .map(|s| s.as_str().to_string())
            .collect()
    }

    /// Number of sinks currently registered on a named stream (0 if absent).
    pub fn stream_sink_count(&self, id: &str) -> usize {
        self.stream(id).map(|s| s.sink_count()).unwrap_or(0)
    }

    /// Whether a named stream has a replay buffer attached.
    pub fn stream_has_replay(&self, id: &str) -> bool {
        self.stream(id).map(|s| s.replay().is_some()).unwrap_or(false)
    }

    /// Stable `kind()` labels of the sinks on the `"default"` stream.
    pub fn sink_kinds(&self) -> Vec<&'static str> {
        self.registry.default_stream().sink_kinds()
    }

    /// Shutdown hook — flushes every sink on every stream. Call before exit.
    pub fn shutdown(&self) {
        for id in self.registry.list() {
            if let Some(s) = self.registry.stream(id.as_str()) {
                s.shutdown();
            }
        }
    }

    // ---- legacy, kept for back-compat ----

    /// Register a sink on the `"default"` stream (legacy `&mut self` form).
    ///
    /// Kept for backward compatibility; prefer [`register_sink`] which works on
    /// a shared `&self` pipeline.
    ///
    /// [`register_sink`]: CapturePipeline::register_sink
    #[deprecated(note = "use `register_sink` (works on shared/&self pipelines)")]
    pub fn add_sink(&mut self, sink: Arc<dyn CaptureSink>) {
        self.register_sink(sink);
    }

    /// Set the capture configuration (legacy `&mut self` form).
    ///
    /// Kept for backward compatibility; prefer [`configure_shared`] which
    /// works on `&self`.
    ///
    /// [`configure_shared`]: CapturePipeline::configure_shared
    #[deprecated(note = "use `configure_shared` (works on &self)")]
    pub fn configure(&mut self, config: CaptureConfig) {
        self.configure_shared(config);
    }
}

/// Nearest-neighbour resize for RGBA8 frames.
///
/// This is the sampling strategy used by [`CapturePipeline`] when
/// [`CaptureConfig::target_width`]/[`CaptureConfig::target_height`] is set. The
/// input frame *must* be [`PixelFormat::Rgba8`] (BGRA frames are converted by
/// the pipeline before resizing). The returned frame shares the source
/// `timestamp` so latency is preserved.
///
/// [`CapturePipeline`]: struct.CapturePipeline.html
pub fn resize_nearest(frame: &CapturedFrame, new_w: u32, new_h: u32) -> CapturedFrame {
    assert_eq!(frame.format, PixelFormat::Rgba8, "resize expects RGBA8 input");
    let bpp = 4usize;
    let mut out = vec![0u8; (new_w * new_h * 4) as usize];
    for y in 0..new_h {
        let src_y = ((y as f64 * frame.height as f64) / new_h as f64) as u32;
        for x in 0..new_w {
            let src_x = ((x as f64 * frame.width as f64) / new_w as f64) as u32;
            let si = ((src_y * frame.width + src_x) * bpp as u32) as usize;
            let di = ((y * new_w + x) * bpp as u32) as usize;
            out[di..di + bpp].copy_from_slice(&frame.data[si..si + bpp]);
        }
    }
    CapturedFrame {
        width: new_w,
        height: new_h,
        format: PixelFormat::Rgba8,
        data: out,
        timestamp: frame.timestamp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_frame(format: PixelFormat) -> CapturedFrame {
        // 2×2 image, 4 bytes per pixel = 16 bytes
        CapturedFrame {
            width: 2,
            height: 2,
            format,
            data: vec![0u8; 16],
            timestamp: Instant::now(),
        }
    }

    #[test]
    fn pipeline_runs_sinks() {
        let pipeline = CapturePipeline::new();
        pipeline.register_sink(Arc::new(StdoutSink));
        assert_eq!(pipeline.sink_count(), 1);
    }

    #[test]
    fn validates_frame_size() {
        let frame = CapturedFrame {
            width: 2,
            height: 2,
            format: PixelFormat::Rgba8,
            data: vec![0; 15], // wrong size
            timestamp: Instant::now(),
        };
        assert!(frame.validate().is_err());
    }

    #[test]
    fn rgba_passthrough() {
        let frame = small_frame(PixelFormat::Rgba8);
        let out = sinks::rgba_bytes(&frame);
        assert_eq!(out, frame.data);
    }

    #[test]
    fn bgra_to_rgba_conversion() {
        let mut frame = small_frame(PixelFormat::Bgra8);
        // BGRA pixel: B=1, G=2, R=3, A=4
        frame.data = vec![1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let out = sinks::rgba_bytes(&frame);
        // Should be RGBA: R=3, G=2, B=1, A=4
        assert_eq!(&out[..4], &[3, 2, 1, 4]);
    }

    #[test]
    fn encode_png_produces_valid_header() {
        let frame = small_frame(PixelFormat::Rgba8);
        let png = sinks::encode_png(&frame).expect("encode failed");
        // PNG magic bytes
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(png.len() > 8);
    }

    #[cfg(feature = "file-sink")]
    #[test]
    fn file_sink_writes_png() {
        let dir = std::env::temp_dir().join("radredeye_test_file_sink");
        let _ = std::fs::remove_dir_all(&dir); // clean slate
        let sink = sinks::file::FileSink::new(&dir).expect("create dir failed");
        let frame = small_frame(PixelFormat::Rgba8);
        sink.submit(&frame).expect("submit failed");

        let path = dir.join("frame_0000.png");
        assert!(path.exists(), "expected PNG at {}", path.display());

        let bytes = std::fs::read(&path).expect("read failed");
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);

        // cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "file-sink")]
    #[test]
    fn file_sink_increments_filenames() {
        let dir = std::env::temp_dir().join("radredeye_test_file_incr");
        let _ = std::fs::remove_dir_all(&dir);
        let sink = sinks::file::FileSink::new(&dir).expect("create dir failed");
        let frame = small_frame(PixelFormat::Rgba8);

        sink.submit(&frame).expect("submit 0 failed");
        sink.submit(&frame).expect("submit 1 failed");

        assert!(dir.join("frame_0000.png").exists());
        assert!(dir.join("frame_0001.png").exists());
        assert!(!dir.join("frame_0002.png").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "http-sink")]
    #[test]
    fn http_sink_transport_error_on_bad_url() {
        let sink = sinks::http::HttpSink::new("http://127.0.0.1:1/bad");
        let frame = small_frame(PixelFormat::Rgba8);
        let result = sink.submit(&frame);
        assert!(result.is_err(), "expected transport error for unreachable URL");
    }

    #[test]
    fn pipeline_drops_invalid_frame() {
        let pipeline = CapturePipeline::new();
        pipeline.register_sink(Arc::new(StdoutSink));
        let bad = CapturedFrame {
            width: 10,
            height: 10,
            format: PixelFormat::Rgba8,
            data: vec![0; 4], // way too small
            timestamp: Instant::now(),
        };
        // Should not panic — just log and continue
        pipeline.submit(&bad);
    }

    #[cfg(feature = "websocket-sink")]
    #[test]
    fn websocket_connect_refused() {
        // No server running on this port — should get a transport error
        let result = sinks::websocket::WebSocketSink::connect("ws://127.0.0.1:1");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{err}").contains("transport"));
    }

    #[test]
    fn resize_nearest_upscale() {
        let mut frame = small_frame(PixelFormat::Rgba8);
        frame.data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let out = resize_nearest(&frame, 4, 4);
        assert_eq!(out.width, 4);
        assert_eq!(out.height, 4);
        assert_eq!(out.data.len(), 64); // 4×4×4
    }

    #[test]
    fn resize_nearest_downscale() {
        // 4×1 RGBA frame
        let frame = CapturedFrame {
            width: 4,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            timestamp: Instant::now(),
        };
        let out = resize_nearest(&frame, 2, 1);
        assert_eq!(out.width, 2);
        assert_eq!(out.height, 1);
        assert_eq!(out.data.len(), 8);
    }

    #[test]
    fn capture_config_resize() {
        let pipeline = CapturePipeline::new();
        pipeline.configure_shared(CaptureConfig {
            target_width: Some(1),
            target_height: Some(1),
            target_format: None,
            min_interval: None,
        });
        pipeline.register_sink(Arc::new(StdoutSink));
        let frame = small_frame(PixelFormat::Rgba8);
        // Should not panic — config is applied inside submit
        pipeline.submit(&frame);
    }

    #[test]
    fn pipeline_is_clone_send_sync_shared() {
        // The facade is Clone + Send + Sync; cloning shares streams/sinks.
        let pipeline = CapturePipeline::new();
        pipeline.register_sink(Arc::new(StdoutSink));
        let cloned = pipeline.clone();
        // a sink registered on the clone is visible to the original (shared).
        let h = cloned.register_sink(Arc::new(StdoutSink));
        assert_eq!(pipeline.sink_count(), 2);
        // unregister by handle removes exactly that sink
        assert!(pipeline.unregister_sink(h).is_some());
        assert_eq!(pipeline.sink_count(), 1);

        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CapturePipeline>();
    }

    /// Verifies the legacy `&mut self` API still works (kept for back-compat).
    /// `#[allow(deprecated)]` suppresses the soft deprecation warning so clippy
    /// `-D warnings` stays green while still exercising the wrappers.
    #[test]
    #[allow(deprecated)]
    fn legacy_add_sink_and_configure_still_function() {
        let mut pipeline = CapturePipeline::new();
        pipeline.add_sink(Arc::new(StdoutSink));
        assert_eq!(pipeline.sink_count(), 1);
        pipeline.configure(CaptureConfig::default());
        let frame = small_frame(PixelFormat::Rgba8);
        pipeline.submit(&frame);
    }
}
