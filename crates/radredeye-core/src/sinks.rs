//! Built-in capture sinks and encoding helpers.

use crate::{CaptureSink, CapturedFrame, PixelFormat, SinkError};
use std::io::Write;

/// Convert any supported frame format into RGBA8 bytes.
pub fn rgba_bytes(frame: &CapturedFrame) -> Vec<u8> {
    match frame.format {
        PixelFormat::Rgba8 => frame.data.clone(),
        PixelFormat::Bgra8 => frame
            .data
            .chunks_exact(4)
            .flat_map(|c| [c[2], c[1], c[0], c[3]])
            .collect(),
    }
}

/// Encode a frame as a PNG byte vector.
#[cfg(any(feature = "file-sink", feature = "http-sink"))]
pub fn encode_png(frame: &CapturedFrame) -> Result<Vec<u8>, SinkError> {
    use image::codecs::png::PngEncoder;
    use image::ExtendedColorType;
    use image::ImageEncoder;

    let rgba = rgba_bytes(frame);
    let mut buf = Vec::new();
    let encoder = PngEncoder::new(&mut buf);
    encoder
        .write_image(&rgba, frame.width, frame.height, ExtendedColorType::Rgba8)
        .map_err(|e| SinkError::Encoding(e.to_string()))?;
    Ok(buf)
}

/// Print frame metadata to stdout. Silently ignores broken-pipe (EPIPE) errors.
pub struct StdoutSink;

impl CaptureSink for StdoutSink {
    fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError> {
        let mut stdout = std::io::stdout().lock();
        let msg = format!(
            "[StdoutSink] {}x{} {:?} frame @ {:?}\n",
            frame.width, frame.height, frame.format, frame.timestamp
        );
        match stdout.write_all(msg.as_bytes()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            Err(e) => Err(SinkError::Transport(e.to_string())),
        }
    }

    fn kind(&self) -> &'static str {
        "stdout"
    }
}

#[cfg(feature = "file-sink")]
pub mod file {
    use super::{encode_png, CaptureSink, CapturedFrame, SinkError};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Write captured frames to PNG files in a directory.
    pub struct FileSink {
        dir: PathBuf,
        counter: AtomicUsize,
    }

    impl FileSink {
        /// Create a sink that writes PNG files into `dir`, creating the
        /// directory tree if needed. Files are named `frame_0000.png`,
        /// `frame_0001.png`, … using an atomic counter.
        ///
        /// # Errors
        /// Returns [`SinkError::Transport`] if the directory cannot be created.
        pub fn new(dir: impl Into<PathBuf>) -> Result<Self, SinkError> {
            let dir = dir.into();
            std::fs::create_dir_all(&dir).map_err(|e| SinkError::Transport(e.to_string()))?;
            Ok(Self {
                dir,
                counter: AtomicUsize::new(0),
            })
        }
    }

    impl CaptureSink for FileSink {
        fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError> {
            let n = self.counter.fetch_add(1, Ordering::SeqCst);
            let path = self.dir.join(format!("frame_{:04}.png", n));
            let png = encode_png(frame)?;
            std::fs::write(&path, png).map_err(|e| SinkError::Transport(e.to_string()))?;
            Ok(())
        }

        fn kind(&self) -> &'static str {
            "file"
        }
    }
}

#[cfg(feature = "http-sink")]
pub mod http {
    use super::{encode_png, CaptureSink, CapturedFrame, SinkError};

    /// POST captured frames as PNG to an HTTP endpoint.
    pub struct HttpSink {
        url: String,
        content_type: String,
    }

    impl HttpSink {
        /// Create a sink that POSTs each frame as a PNG (`image/png`) to `url`.
        ///
        /// Override the content type with [`with_content_type`] if the server
        /// expects a different body encoding.
        ///
        /// [`with_content_type`]: HttpSink::with_content_type
        pub fn new(url: impl Into<String>) -> Self {
            Self {
                url: url.into(),
                content_type: "image/png".into(),
            }
        }

        /// Builder-style override of the `Content-Type` header sent with each
        /// POST. Useful when the receiver wraps the PNG in another format.
        pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
            self.content_type = content_type.into();
            self
        }
    }

    impl CaptureSink for HttpSink {
        fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError> {
            let png = encode_png(frame)?;
            let resp = ureq::post(&self.url)
                .set("Content-Type", &self.content_type)
                .send_bytes(&png)
                .map_err(|e| SinkError::Transport(e.to_string()))?;
            if resp.status() >= 400 {
                return Err(SinkError::Transport(format!("HTTP {}", resp.status())));
            }
            Ok(())
        }

        fn kind(&self) -> &'static str {
            "http"
        }
    }
}

#[cfg(feature = "websocket-sink")]
pub mod websocket {
    use super::{encode_png, CaptureSink, CapturedFrame, SinkError};
    use std::sync::Mutex;
    use tungstenite::{connect, stream::MaybeTlsStream, Message, WebSocket as TungWs};

    /// Sends each captured frame as a binary WebSocket message (PNG-encoded).
    #[derive(Debug)]
    pub struct WebSocketSink {
        url: String,
        inner: Mutex<TungWs<MaybeTlsStream<std::net::TcpStream>>>,
    }

    impl WebSocketSink {
        /// Connect to the given WebSocket URL.
        ///
        /// # Errors
        /// Returns [`SinkError::Transport`] if the initial handshake fails.
        pub fn connect(url: &str) -> Result<Self, SinkError> {
            let (ws, _resp) = connect(url).map_err(|e| SinkError::Transport(e.to_string()))?;
            Ok(Self {
                url: url.to_string(),
                inner: Mutex::new(ws),
            })
        }

        /// Reconnect to the configured URL.
        fn reconnect(&self) -> Result<(), SinkError> {
            let (ws, _resp) =
                connect(self.url.as_str()).map_err(|e| SinkError::Transport(e.to_string()))?;
            let mut inner = self.inner.lock().map_err(|e| SinkError::Transport(e.to_string()))?;
            *inner = ws;
            Ok(())
        }
    }

    impl CaptureSink for WebSocketSink {
        fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError> {
            let png = encode_png(frame)?;
            let mut inner = self.inner.lock().map_err(|e| SinkError::Transport(e.to_string()))?;

            // Try sending; if it fails, reconnect once and retry.
            if let Err(send_err) = inner.send(Message::Binary(png.clone())) {
                drop(inner);
                tracing::warn!(error = %send_err, "WebSocket send failed, reconnecting");
                self.reconnect()?;
                let mut inner = self.inner.lock().map_err(|e| SinkError::Transport(e.to_string()))?;
                inner
                    .send(Message::Binary(png))
                    .map_err(|e| SinkError::Transport(e.to_string()))?;
            }

            Ok(())
        }

        fn kind(&self) -> &'static str {
            "websocket"
        }
    }
}

#[cfg(feature = "grpc-sink")]
pub mod grpc;

/// Feature-gated semantic-diff sink (Phase 10.4).
#[cfg(feature = "semantic-diff")]
pub mod semantic_diff;
