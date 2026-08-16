//! WebXR/WebGL adapter for radredeye.
//!
//! Captures pixels from an HTML canvas via `WebGLRenderingContext.readPixels`
//! and submits them as `CapturedFrame`s through the core pipeline.
//!
//! When compiled to WASM, this crate exposes `wasm-bindgen` entry points that
//! JavaScript can call on a timer or animation frame.

use wasm_bindgen::prelude::*;
use web_sys::WebGlRenderingContext;
use radredeye_core::{CapturedFrame, PixelFormat};

/// Capture a single frame from a WebGL canvas and return raw RGBA bytes.
///
/// JavaScript must pass the `WebGLRenderingContext` and canvas dimensions.
#[wasm_bindgen]
pub fn capture_gl_frame(
    gl: &web_sys::WebGlRenderingContext,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, JsValue> {
    let len = (width * height * 4) as usize;
    let mut pixels = vec![0u8; len];

    gl.read_pixels_with_opt_u8_array(
        0,
        0,
        width as i32,
        height as i32,
        WebGlRenderingContext::RGBA,
        WebGlRenderingContext::UNSIGNED_BYTE,
        Some(&mut pixels),
    )?;

    Ok(pixels)
}

/// Build a `CapturedFrame` from raw RGBA pixel data.
///
/// This is a pure helper — it does not depend on `web-sys` and can be tested
/// in native builds.
pub fn frame_from_rgba(width: u32, height: u32, data: Vec<u8>) -> CapturedFrame {
    CapturedFrame {
        width,
        height,
        format: PixelFormat::Rgba8,
        data,
        timestamp: std::time::Instant::now(),
    }
}
