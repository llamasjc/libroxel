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

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use bevy::prelude::World;
use bevy::render::RenderPlugin;
use bevy::render::error_handler::{ErrorType, RenderError, RenderErrorHandler, RenderErrorPolicy};
use bevy::render::settings::{Backends, RenderCreation, WgpuLimits, WgpuSettings};

/// Bevy's renderer, set up for this device.
pub fn render_plugin() -> RenderPlugin {
    RenderPlugin { render_creation: creation(), ..Default::default() }
}

fn creation() -> RenderCreation {
    RenderCreation::Automatic(Box::new(settings(cfg!(target_os = "android") && !has_vulkan())))
}

/// What to do when the graphics card stops answering or runs out of
/// memory. Bevy on its own closes the app at once. A graphics driver that
/// restarts (an update, a laptop switching cards, a phone taking the card
/// back while the app is in the background) takes the device with it, so
/// the renderer is made again instead, and the game carries on.
pub fn error_handler() -> RenderErrorHandler {
    RenderErrorHandler(on_render_error)
}

/// How many times in a row the renderer may be made again before giving
/// up, and how long it must then run without trouble for the count to start over.
const MOST_RECOVERIES: u32 = 3;
const CALM_SECS: u64 = 60;

static RECOVERIES: AtomicU32 = AtomicU32::new(0);
static LAST_RECOVERY: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Whether the renderer should be made again after `kind` of error, given
/// how many times it already was (`tries`) and how long ago (`since`).
fn should_recover(kind: &ErrorType, tries: u32, since: u64) -> bool {
    let tries = if since > CALM_SECS { 0 } else { tries };
    // A validation error is a mistake in how the card is used; making the
    // renderer again would only meet it again.
    !matches!(kind, ErrorType::Validation) && tries < MOST_RECOVERIES
}

fn on_render_error(error: &RenderError, main_world: &mut World, _: &mut World) -> RenderErrorPolicy {
    let now = now_secs();
    let since = now.saturating_sub(LAST_RECOVERY.load(Ordering::Relaxed));
    let tries = if since > CALM_SECS { 0 } else { RECOVERIES.load(Ordering::Relaxed) };
    if should_recover(&error.ty, tries, since) {
        RECOVERIES.store(tries + 1, Ordering::Relaxed);
        LAST_RECOVERY.store(now, Ordering::Relaxed);
        bevy::log::warn!("The graphics card stopped ({:?}); starting the renderer again", error.ty);
        return RenderErrorPolicy::Recover(creation());
    }
    // Say why on the next start, rather than vanishing.
    crate::report::save_crash(&format!(
        "The graphics card stopped working ({:?}) and couldn't be started again. {}\n\n\
         Updating the graphics driver often fixes this.",
        error.ty,
        error.description.trim()
    ));
    main_world.write_message(bevy::app::AppExit::error());
    RenderErrorPolicy::StopRendering
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
    fn a_lost_card_is_started_again_a_few_times() {
        assert!(should_recover(&ErrorType::DeviceLost, 0, 0));
        assert!(should_recover(&ErrorType::OutOfMemory, 2, 5));
        assert!(!should_recover(&ErrorType::DeviceLost, MOST_RECOVERIES, 5));
        // After a calm minute the count starts over.
        assert!(should_recover(&ErrorType::DeviceLost, MOST_RECOVERIES, CALM_SECS + 1));
        assert!(!should_recover(&ErrorType::Validation, 0, 0));
    }

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
