//! Bevy plugin for the radredeye capture pipeline.
//!
//! This crate wraps the engine-agnostic `radredeye-core` pipeline
//! in a Bevy `Resource` and provides a `CaptureCamera` marker component.

use bevy::prelude::*;
use bevy::render::camera::RenderTarget;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::{PrimaryWindow, WindowRef};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::time::Instant;
use radredeye_core::{CapturePipeline, CapturedFrame, PixelFormat};

/// Bevy resource wrapping the engine-agnostic [`CapturePipeline`].
#[derive(Resource)]
pub struct BevyCapturePipeline(pub CapturePipeline);

impl Default for BevyCapturePipeline {
    fn default() -> Self {
        Self(CapturePipeline::new())
    }
}

impl Deref for BevyCapturePipeline {
    type Target = CapturePipeline;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for BevyCapturePipeline {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Marker component: cameras with this component will feed captured frames
/// into the configured [`BevyCapturePipeline`].
#[derive(Component, Default, Debug, Reflect)]
#[reflect(Component)]
pub struct CaptureCamera {
    pub enabled: bool,
    pub throttle_seconds: Option<f32>,
}

impl CaptureCamera {
    /// Enable capture with a sensible 1-second throttle by default.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            throttle_seconds: Some(1.0),
        }
    }
}

/// Plugin entry point.
#[derive(Default)]
pub struct RadredeyeCapturePlugin;

impl Plugin for RadredeyeCapturePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<CaptureCamera>()
            .init_resource::<BevyCapturePipeline>()
            .add_systems(PostUpdate, capture_viewport_system);
    }
}

/// Periodically request screenshots from any camera marked with [`CaptureCamera`].
fn capture_viewport_system(
    mut commands: Commands,
    time: Res<Time>,
    primary_window: Single<Entity, With<PrimaryWindow>>,
    cameras: Query<(Entity, &Camera, &CaptureCamera)>,
    mut last_capture: Local<HashMap<Entity, f64>>,
) {
    let now = time.elapsed_secs_f64();
    let primary_window = *primary_window;

    for (entity, camera, capture) in &cameras {
        if !capture.enabled {
            continue;
        }
        if let Some(throttle) = capture.throttle_seconds {
            if let Some(last) = last_capture.get(&entity) {
                if now - *last < throttle as f64 {
                    continue;
                }
            }
        }

        match &camera.target {
            RenderTarget::Window(window_ref) => {
                let window_entity = match window_ref {
                    WindowRef::Primary => primary_window,
                    WindowRef::Entity(window_entity) => *window_entity,
                };
                commands
                    .spawn(Screenshot::window(window_entity))
                    .observe(on_screenshot);
            }
            RenderTarget::Image(handle) => {
                commands
                    .spawn(Screenshot::image(handle.clone()))
                    .observe(on_screenshot);
            }
            _ => {}
        }

        last_capture.insert(entity, now);
    }
}

/// Observer that turns a Bevy screenshot into a [`CapturedFrame`] and feeds
/// it to the pipeline.
fn on_screenshot(trigger: Trigger<ScreenshotCaptured>, pipeline: ResMut<BevyCapturePipeline>) {
    let img = trigger.event().0.clone();
    let Ok(dyn_img) = img.try_into_dynamic() else {
        bevy::log::warn!("failed to convert screenshot to dynamic image");
        return;
    };

    let rgba = dyn_img.to_rgba8();
    let frame = CapturedFrame {
        width: rgba.width(),
        height: rgba.height(),
        format: PixelFormat::Rgba8,
        data: rgba.into_raw(),
        timestamp: Instant::now(),
    };

    pipeline.submit(&frame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn capture_camera_enabled_defaults() {
        let cam = CaptureCamera::enabled();
        assert!(cam.enabled);
        assert_eq!(cam.throttle_seconds, Some(1.0));
    }

    #[test]
    fn capture_camera_disabled_default() {
        let cam = CaptureCamera::default();
        assert!(!cam.enabled);
        assert_eq!(cam.throttle_seconds, None);
    }

    #[test]
    fn bevy_capture_pipeline_deref() {
        let res = BevyCapturePipeline::default();
        assert_eq!(res.sink_count(), 0);
        res.register_sink(Arc::new(radredeye_core::sinks::StdoutSink));
        assert_eq!(res.sink_count(), 1);
    }

    #[test]
    fn plugin_registers_resource() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(RadredeyeCapturePlugin);
        // Resource should be initialized.
        assert!(app.world().get_resource::<BevyCapturePipeline>().is_some());
    }
}
