//! Minimal usage example: a Bevy app with one capture camera, a stdout sink,
//! and a file sink that writes screenshots to disk.

use bevy::prelude::*;
use std::sync::Arc;
use radredeye_bevy::{CaptureCamera, RadredeyeCapturePlugin};
use radredeye_core::{
    sinks::{file::FileSink, http::HttpSink, StdoutSink},
    CaptureSink, CapturedFrame, SinkError,
};

struct StdoutSink2;

impl CaptureSink for StdoutSink2 {
    fn submit(&self, frame: &CapturedFrame) -> Result<(), SinkError> {
        // In a real pipeline this would send the frame to an LLM vision endpoint.
        println!("captured {}x{} frame", frame.width, frame.height);
        Ok(())
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RadredeyeCapturePlugin)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, pipeline: ResMut<radredeye_bevy::BevyCapturePipeline>) {
    pipeline.register_sink(Arc::new(StdoutSink));
    pipeline.register_sink(Arc::new(StdoutSink2));
    pipeline.register_sink(Arc::new(FileSink::new("captures/bevy").unwrap())); // guardrails-allow PREVENT-013: example code

    if let Ok(url) = std::env::var("RADREDEYE_HTTP_SINK_URL") {
        pipeline.register_sink(Arc::new(HttpSink::new(url)));
    }

    // Any camera marked with CaptureCamera will be wired into the pipeline.
    commands.spawn((Camera2d, CaptureCamera::enabled()));
}
