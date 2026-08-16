//! Unity engine adapter for radredeye.
//!
//! Unity captures its backbuffer via C# (`ScreenCapture.CaptureScreenshotAsTexture`)
//! and POSTs PNG frames to the daemon bridge (`radredeye-mcp`) over HTTP.
//!
//! This crate provides:
//! - Shared type definitions for Unity ↔ Rust interop
//! - A bridge client for sending frames from Unity's native plugin layer
//!
//! See `unity/Runtime/RadredeyeCaptureBridge.cs` for the C# side.

pub use radredeye_core::{CapturedFrame, PixelFormat};
