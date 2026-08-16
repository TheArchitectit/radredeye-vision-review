//! Unreal Engine adapter for radredeye.
//!
//! Unreal captures its viewport via `FViewport::ReadPixels` and POSTs PNG frames
//! to the daemon bridge (`radredeye-mcp`) over HTTP.
//!
//! See `unreal/Public/RadredeyeCaptureComponent.h` for the C++ side.

pub use radredeye_core::{CapturedFrame, PixelFormat};
