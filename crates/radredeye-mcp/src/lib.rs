//! radredeye — stateless capture server core.
//!
//! Decodes PNG bytes into [`CapturedFrame`] and submits to a [`CapturePipeline`].
//! Also maintains a small ring buffer of recent frames ([`ReplayBuffer`]) so a
//! debugging agent can retrieve the latest frame via `GET /frame`.
//!
//! The primary agent interface is the **stateless MCP server** exposed by the
//! [`mcp`] module: AI agents connect over the MCP Streamable HTTP transport
//! (no session) and call tools to list/get/submit capture frames. Any app
//! framework (not just a game engine) can push frames in via `POST /capture`
//! or the `submit_frame` MCP tool.
//!
//! The ring buffer was promoted to `radredeye_core::ReplayBuffer` in
//! Phase 10.3; [`FrameStore`] is kept as a deprecated alias for one cycle.

use std::sync::{Arc, Mutex};
use std::time::Instant;
use tiny_http::{Header, Method, Response};
use radredeye_core::{sinks, CapturePipeline, CapturedFrame, PixelFormat};

/// Default ring-buffer capacity for the daemon frame store (spec F4: cap 30).
/// Re-exported from core so the daemon and core agree on the canonical value.
pub use radredeye_core::FRAME_STORE_CAP;
/// Re-export of the core ring buffer (Phase 10.3 promotion of the 9.3
/// `FrameStore`). In-tree daemon code uses this type directly.
pub use radredeye_core::ReplayBuffer;

/// Stateless MCP (Model Context Protocol) server: exposes the capture pipeline
/// to AI agents over the `mcp-stateless` transport at `POST /mcp`.
pub mod mcp;
pub use mcp::{McpOutcome, McpServer};

/// Deprecated alias for [`ReplayBuffer`] (kept for one cycle for external
/// callers that reference `FrameStore`). In-tree code uses [`ReplayBuffer`]
/// directly so no deprecation warning fires under `clippy -D warnings`.
#[deprecated(note = "use radredeye_core::ReplayBuffer directly")]
pub type FrameStore = ReplayBuffer;

/// Decode PNG bytes into (width, height, RGBA pixel data).
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// Encode a [`CapturedFrame`] to PNG bytes (reuses the core `encode_png` helper).
pub fn encode_frame_png(frame: &CapturedFrame) -> Result<Vec<u8>, String> {
    sinks::encode_png(frame).map_err(|e| format!("{e}"))
}

/// Decode a PNG payload, submit the resulting frame to the pipeline, **and**
/// push it into the [`ReplayBuffer`] so `GET /frame` can serve it later (Phase
/// 9.3). The ring buffer is `radredeye_core::ReplayBuffer` (Phase 10.3
/// promotion of the 9.3 `FrameStore`); `FrameStore` is kept as a deprecated
/// alias. Returns `Ok(())` on success.
pub fn handle_capture(
    pipeline: &Arc<Mutex<CapturePipeline>>,
    store: &ReplayBuffer,
    png_bytes: &[u8],
) -> Result<(), String> {
    let (width, height, data) = decode_png(png_bytes)?;
    let frame = CapturedFrame {
        width,
        height,
        format: PixelFormat::Rgba8,
        data,
        timestamp: Instant::now(),
    };
    pipeline
        .lock()
        .map_err(|e| format!("lock poisoned: {e}"))?
        .submit(&frame);
    store.push(frame);
    Ok(())
}

/// Serve a stored frame as a PNG for `GET /frame[?index=N]`.
///
/// Returns a `tiny_http` [`Response`]:
/// - `200` `image/png` with the encoded frame on success,
/// - `404` when no frame is stored at the requested index,
/// - `500` when the stored frame cannot be PNG-encoded.
pub fn frame_response(store: &ReplayBuffer, index: usize) -> Response<std::io::Cursor<Vec<u8>>> {
    match store.get(index) {
        None => Response::from_string("no frame stored at index")
            .with_status_code(404),
        Some(frame) => match encode_frame_png(&frame) {
            Ok(png) => {
                let resp = Response::from_data(png).with_status_code(200);
                match Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]) {
                    Ok(h) => resp.with_header(h),
                    Err(_) => resp, // header build failed — still return the bytes
                }
            }
            Err(e) => {
                eprintln!("[daemon] frame encode error: {e}");
                Response::from_string(format!("encode error: {e}")).with_status_code(500)
            }
        },
    }
}

/// Parse the `index` query parameter from a request URL.
///
/// `GET /frame` → `Some(0)` (latest). `GET /frame?index=2` → `Some(2)`.
/// A non-numeric `index` value falls back to `0` (latest).
pub fn parse_frame_index(url: &str) -> usize {
    let Some(query) = url.split('?').nth(1) else {
        return 0;
    };
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some("index") {
            return it.next().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        }
    }
    0
}

/// Pure dispatch for a single HTTP request (Phase 9.3).
///
/// Takes the request method, URL, and body and routes to `/health`, `/frame`,
/// or `/capture`, returning a `tiny_http` [`Response`]. This is split out of the
/// binary's request loop so the routing logic is unit-testable without binding a
/// real socket (the daemon's HTTP integration test exercises it directly).
///
/// `health_body` is the JSON returned for `GET /health`.
pub fn dispatch_request(
    method: &Method,
    url: &str,
    body: &[u8],
    pipeline: &Arc<Mutex<CapturePipeline>>,
    store: &ReplayBuffer,
    health_body: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    // GET /health — liveness probe.
    if method == &Method::Get && url == "/health" {
        return match Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]) {
            Ok(h) => Response::from_string(health_body)
                .with_status_code(200)
                .with_header(h),
            Err(_) => Response::from_string(health_body).with_status_code(200),
        };
    }

    // GET /frame[?index=N] — perception-assisted debugging (Phase 9.3).
    if method == &Method::Get && url.split('?').next() == Some("/frame") {
        let index = parse_frame_index(url);
        return frame_response(store, index);
    }

    // POST /capture — decode PNG and submit to pipeline + frame store.
    if method == &Method::Post && url.split('?').next() == Some("/capture") {
        return match handle_capture(pipeline, store, body) {
            Ok(()) => Response::from_string("ok").with_status_code(200),
            Err(e) => {
                eprintln!("[daemon] error: {e}");
                Response::from_string(format!("error: {e}")).with_status_code(400)
            }
        };
    }

    // Unknown route / method.
    Response::from_string("not found").with_status_code(404)
}

#[cfg(test)]
mod tests {
    use super::*;
    use radredeye_core::{sinks::StdoutSink, CaptureSink, SinkError};

    /// A sink that records every frame it receives.
    struct RecordingSink {
        frames: Mutex<Vec<(u32, u32)>>,
    }
    impl RecordingSink {
        fn new() -> Self {
            Self {
                frames: Mutex::new(Vec::new()),
            }
        }
    }
    impl CaptureSink for RecordingSink {
        fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError> {
            self.frames
                .lock()
                .unwrap() // guardrails-allow PREVENT-013: test mock sink; Mutex::lock is infallible here
                .push((frame.width, frame.height));
            Ok(())
        }
    }

    /// Build a minimal valid 1×1 red PNG in memory.
    fn tiny_png() -> Vec<u8> {
        // Use the `image` crate to encode a 1×1 red RGBA image.
        use image::codecs::png::PngEncoder;
        use image::ImageEncoder;

        let rgba = vec![255u8, 0, 0, 255]; // 1 pixel, red
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(&rgba, 1, 1, image::ExtendedColorType::Rgba8)
            .expect("png encode");
        buf
    }

    #[test]
    fn decode_png_valid() {
        let png = tiny_png();
        let (w, h, data) = decode_png(&png).expect("decode failed");
        assert_eq!((w, h), (1, 1));
        assert_eq!(data.len(), 4);
        assert_eq!(&data, &[255, 0, 0, 255]);
    }

    #[test]
    fn decode_png_invalid() {
        let result = decode_png(&[0, 1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn handle_capture_submits_frame() {
        let recorder = Arc::new(RecordingSink::new());
        let pipeline = CapturePipeline::new();
        pipeline.register_sink(recorder.clone());
        let pipeline = Arc::new(Mutex::new(pipeline));
        let store = ReplayBuffer::new(FRAME_STORE_CAP);

        let png = tiny_png();
        handle_capture(&pipeline, &store, &png).expect("handle failed");

        let frames = recorder.frames.lock().unwrap(); // guardrails-allow PREVENT-013: test assertion on mocked recorder
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], (1, 1));
    }

    #[test]
    fn handle_capture_rejects_bad_png() {
        let pipeline = CapturePipeline::new();
        pipeline.register_sink(Arc::new(StdoutSink));
        let pipeline = Arc::new(Mutex::new(pipeline));
        let store = ReplayBuffer::new(FRAME_STORE_CAP);

        let result = handle_capture(&pipeline, &store, &[0, 1, 2]);
        assert!(result.is_err());
    }

    // ---- Phase 9.3: frame store + GET /frame ----

    #[test]
    fn frame_store_retains_latest_and_evicts_oldest() {
        let store = ReplayBuffer::new(2);
        let f1 = CapturedFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![1, 2, 3, 4],
            timestamp: Instant::now(),
        };
        let f2 = f1.clone();
        let f3 = f1.clone();
        store.push(f1);
        store.push(f2);
        assert_eq!(store.len(), 2);
        assert_eq!(store.get(0).unwrap().data, vec![1, 2, 3, 4]);
        // Over capacity → evicts oldest.
        store.push(f3);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn frame_store_index_zero_is_latest() {
        let store = ReplayBuffer::new(FRAME_STORE_CAP);
        let mut frame = CapturedFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![10, 0, 0, 255],
            timestamp: Instant::now(),
        };
        store.push(frame.clone());
        frame.data = vec![20, 0, 0, 255];
        store.push(frame);

        let latest = store.get(0).expect("latest frame");
        assert_eq!(latest.data, vec![20, 0, 0, 255]);
        let older = store.get(1).expect("older frame");
        assert_eq!(older.data, vec![10, 0, 0, 255]);
        assert!(store.get(2).is_none());
    }

    #[test]
    fn frame_store_empty_returns_none() {
        let store = ReplayBuffer::new(FRAME_STORE_CAP);
        assert!(store.is_empty());
        assert!(store.get(0).is_none());
    }

    #[test]
    fn parse_frame_index_defaults_and_parsing() {
        assert_eq!(parse_frame_index("/frame"), 0);
        assert_eq!(parse_frame_index("/frame?index=3"), 3);
        assert_eq!(parse_frame_index("/frame?index=0"), 0);
        assert_eq!(parse_frame_index("/frame?index=abc"), 0);
        assert_eq!(parse_frame_index("/frame?foo=bar&index=5"), 5);
        assert_eq!(parse_frame_index("/capture"), 0);
    }

    #[test]
    fn get_frame_then_capture_returns_that_frame_png() {
        // POST /capture a PNG, then GET /frame returns that PNG.
        let recorder = Arc::new(RecordingSink::new());
        let pipeline = CapturePipeline::new();
        pipeline.register_sink(recorder.clone());
        let pipeline = Arc::new(Mutex::new(pipeline));
        let store = ReplayBuffer::new(FRAME_STORE_CAP);

        let png = tiny_png();

        // Simulate POST /capture via the pure dispatcher.
        let capture_resp = dispatch_request(
            &Method::Post,
            "/capture",
            &png,
            &pipeline,
            &store,
            r#"{"status":"ok"}"#,
        );
        assert_eq!(capture_resp.status_code(), 200);

        // The recording sink saw the frame.
        let frames = recorder.frames.lock().unwrap(); // guardrails-allow PREVENT-013: test assertion on mocked recorder
        assert_eq!(frames.len(), 1);

        // Simulate GET /frame — should return the captured frame as PNG.
        let frame_resp = dispatch_request(
            &Method::Get,
            "/frame",
            &[],
            &pipeline,
            &store,
            r#"{"status":"ok"}"#,
        );
        assert_eq!(frame_resp.status_code(), 200);

        // Extract the PNG body from the response and verify it decodes to the
        // same dimensions/pixels we posted.
        let body = frame_resp.into_reader().into_inner();
        let (w, h, data) = decode_png(&body).expect("decode /frame body");
        assert_eq!((w, h), (1, 1));
        assert_eq!(&data, &[255, 0, 0, 255]);
    }

    #[test]
    fn get_frame_returns_404_when_empty() {
        let pipeline = Arc::new(Mutex::new(CapturePipeline::new()));
        let store = ReplayBuffer::new(FRAME_STORE_CAP);

        let resp = dispatch_request(
            &Method::Get,
            "/frame?index=0",
            &[],
            &pipeline,
            &store,
            r#"{"status":"ok"}"#,
        );
        assert_eq!(resp.status_code(), 404);
    }

    #[test]
    fn get_frame_index_out_of_range_returns_404() {
        let pipeline = Arc::new(Mutex::new(CapturePipeline::new()));
        let store = ReplayBuffer::new(FRAME_STORE_CAP);
        store.push(CapturedFrame {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            data: vec![0, 0, 0, 255],
            timestamp: Instant::now(),
        });

        let resp = dispatch_request(
            &Method::Get,
            "/frame?index=5",
            &[],
            &pipeline,
            &store,
            r#"{"status":"ok"}"#,
        );
        assert_eq!(resp.status_code(), 404);
    }

    #[test]
    fn health_route_returns_ok() {
        let pipeline = Arc::new(Mutex::new(CapturePipeline::new()));
        let store = ReplayBuffer::new(FRAME_STORE_CAP);
        let resp = dispatch_request(
            &Method::Get,
            "/health",
            &[],
            &pipeline,
            &store,
            r#"{"status":"ok"}"#,
        );
        assert_eq!(resp.status_code(), 200);
    }

    #[test]
    fn unknown_route_returns_404() {
        let pipeline = Arc::new(Mutex::new(CapturePipeline::new()));
        let store = ReplayBuffer::new(FRAME_STORE_CAP);
        let resp = dispatch_request(
            &Method::Get,
            "/nope",
            &[],
            &pipeline,
            &store,
            r#"{"status":"ok"}"#,
        );
        assert_eq!(resp.status_code(), 404);
    }
}
