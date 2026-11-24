//! How Bevy may use the graphics card.
//!
//! Some Android GPUs (Mali ones without Vulkan, for example) run OpenGL ES
//! with storage buffers in fragment and compute shaders but not in vertex
//! shaders. Bevy only checks whether storage buffers exist at all, so it
//! builds vertex layouts the driver refuses, and quits on the first frame.
//! On those devices Bevy is told there are no storage buffers, so it uses
//! the uniform buffer path it has for WebGL2 instead.
//!
//! Those devices also draw with OpenGL ES only. Android's Vulkan library
//! is there even without a Vulkan driver, and a Vulkan surface takes the
//! window for itself, so OpenGL ES could no longer draw to it.

use bevy::render::RenderPlugin;
use bevy::render::settings::{Backends, RenderCreation, WgpuLimits, WgpuSettings};

/// Bevy's renderer, set up for this device.
pub fn render_plugin() -> RenderPlugin {
    RenderPlugin {
        render_creation: RenderCreation::Automatic(Box::new(settings(
            cfg!(target_os = "android") && !has_vulkan(),
        ))),
        ..Default::default()
    }
}

/// The renderer's settings. `gles_only` draws with OpenGL ES alone and
/// keeps storage buffers out of Bevy's hands; every other limit stays as
/// the device has it.
fn settings(gles_only: bool) -> WgpuSettings {
    let mut settings = WgpuSettings::default();
    if gles_only {
        settings.backends = Some(Backends::GL);
        settings.constrained_limits = Some(WgpuLimits {
            max_storage_buffers_per_shader_stage: 0,
            max_storage_textures_per_shader_stage: 0,
            ..WgpuLimits::unlimited()
        });
    }
    settings
}

/// Whether the device has a Vulkan driver. Android keeps them as
/// `hw/vulkan.<name>.so` beside the system's or the vendor's libraries.
/// Without one, wgpu falls back to OpenGL ES.
fn has_vulkan() -> bool {
    ["/vendor/lib64/hw", "/vendor/lib/hw", "/system/lib64/hw", "/system/lib/hw"].iter().any(|dir| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.flatten().any(|e| e.file_name().to_string_lossy().starts_with("vulkan."))
        })
    })
}

/// Keeps Bevy's idea of the window's size to the size Android drew it.
/// Android can hand over the window after Bevy has sized it from the
/// settings, and without this the menus are laid out for a window of the
/// wrong size, off to one side.
#[cfg(target_os = "android")]
pub fn match_android_window(
    mut windows: bevy::prelude::Query<
        &mut bevy::window::Window,
        bevy::prelude::With<bevy::window::PrimaryWindow>,
    >,
) {
    let Some(native) = bevy::android::ANDROID_APP.get().and_then(|app| app.native_window()) else { return };
    let (width, height) = (native.width().max(0) as u32, native.height().max(0) as u32);
    let Ok(mut window) = windows.single_mut() else { return };
    if width > 0 && height > 0 && (window.physical_width() != width || window.physical_height() != height) {
        window.resolution.set_physical_resolution(width, height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without Vulkan, Bevy draws with OpenGL ES alone and sees no storage
    /// buffers, and nothing else about the device changes.
    #[test]
    fn only_storage_buffers_are_held_back() {
        let device = WgpuLimits::downlevel_defaults();
        assert!(device.max_storage_buffers_per_shader_stage > 0);
        let held = settings(true).constrained_limits.unwrap();
        let used = device.clone().or_worse_values_from(&held);
        assert_eq!(used.max_storage_buffers_per_shader_stage, 0);
        assert_eq!(used.max_storage_textures_per_shader_stage, 0);
        assert_eq!(
            WgpuLimits {
                max_storage_buffers_per_shader_stage: 0,
                max_storage_textures_per_shader_stage: 0,
                ..device
            },
            used
        );
        assert_eq!(settings(true).backends, Some(Backends::GL));
        assert!(settings(false).constrained_limits.is_none());
        assert_ne!(settings(false).backends, Some(Backends::GL));
    }
}
