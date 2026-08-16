//! gRPC sink for streaming captured frames to a remote collector.
//!
//! Compile the proto schema with `tonic-build` and expose the generated types
//! behind `feature = "grpc-sink"`.

use crate::{CaptureSink, CapturedFrame, SinkError};
use std::sync::OnceLock;

// Include the generated protobuf code.
pub mod proto {
    tonic::include_proto!("radredeye");
}

use proto::CapturedFrame as ProtoFrame;
use proto::PixelFormat as ProtoPixelFormat;
use proto::frame_streaming_client::FrameStreamingClient;
use tonic::transport::Channel;

/// Process-global multi-thread tokio runtime shared by every [`GrpcSink`].
///
/// Phase 10.5 replaces the former per-instance `current_thread` runtime (which
/// panicked when two threads raced `block_on` on the same `Runtime`) with a
/// single shared multi-thread runtime. `GrpcSink::submit` is always called from
/// synchronous code (never inside an async context), so `block_on` on the shared
/// handle is legal; multiple sinks/threads may call it concurrently because
/// each blocks its own calling thread on the shared multi-thread runtime.
static GRPC_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn shared_runtime() -> &'static tokio::runtime::Runtime {
    GRPC_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("construct shared gRPC tokio runtime") // guardrails: expect() not flagged; runtime build failure is fatal
    })
}

/// Streams captured frames to a gRPC `FrameStreaming` service via
/// client-streaming RPC.
pub struct GrpcSink {
    endpoint: String,
}

impl std::fmt::Debug for GrpcSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcSink")
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

impl GrpcSink {
    /// Connect to a gRPC endpoint.
    ///
    /// The actual channel is established lazily on the first RPC call (inside
    /// [`submit`](CaptureSink::submit)); this constructor only records the
    /// endpoint. The shared multi-thread runtime is initialised on first use.
    ///
    /// # Errors
    /// Currently always succeeds — kept as `Result` for API stability. The
    /// channel/transport error surfaces from `submit`.
    pub fn connect(endpoint: &str) -> Result<Self, SinkError> {
        Ok(Self {
            endpoint: endpoint.to_string(),
        })
    }

    fn convert_frame(frame: &CapturedFrame) -> ProtoFrame {
        ProtoFrame {
            width: frame.width,
            height: frame.height,
            format: match frame.format {
                crate::PixelFormat::Rgba8 => ProtoPixelFormat::Rgba8 as i32,
                crate::PixelFormat::Bgra8 => ProtoPixelFormat::Bgra8 as i32,
            },
            data: frame.data.clone(),
            timestamp_ms: frame.timestamp.elapsed().as_millis() as u64,
        }
    }
}

impl CaptureSink for GrpcSink {
    fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError> {
        let endpoint = self.endpoint.clone();
        let proto_frame = Self::convert_frame(frame);

        // block_on on the shared multi-thread runtime is safe from synchronous
        // code; concurrent submits from multiple threads each block their own
        // calling thread (Phase 10.5 thread-soundness fix).
        shared_runtime().block_on(async move {
            let channel = Channel::from_shared(endpoint)
                .map_err(|e| SinkError::Transport(e.to_string()))?
                .connect()
                .await
                .map_err(|e| SinkError::Transport(e.to_string()))?;

            let mut client = FrameStreamingClient::new(channel);

            let stream = tokio_stream::once(proto_frame);
            let request = tonic::Request::new(stream);

            client
                .stream_frames(request)
                .await
                .map_err(|e| SinkError::Transport(e.to_string()))?;

            Ok(())
        })
    }

    fn kind(&self) -> &'static str {
        "grpc"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_rgba_frame() {
        let frame = CapturedFrame {
            width: 2,
            height: 2,
            format: crate::PixelFormat::Rgba8,
            data: vec![0u8; 16],
            timestamp: std::time::Instant::now(),
        };
        let proto = GrpcSink::convert_frame(&frame);
        assert_eq!(proto.width, 2);
        assert_eq!(proto.height, 2);
        assert_eq!(proto.format, ProtoPixelFormat::Rgba8 as i32);
        assert_eq!(proto.data.len(), 16);
    }

    #[test]
    fn convert_bgra_frame() {
        let frame = CapturedFrame {
            width: 1,
            height: 1,
            format: crate::PixelFormat::Bgra8,
            data: vec![255u8; 4],
            timestamp: std::time::Instant::now(),
        };
        let proto = GrpcSink::convert_frame(&frame);
        assert_eq!(proto.format, ProtoPixelFormat::Bgra8 as i32);
    }
}
