//! Drawing the 3D view with fewer pixels than the screen has, on phones
//! and tablets (see [`GraphicsQuality::render_density`]).
//!
//! The 3D camera then draws into a picture the shape of the window but
//! smaller, which the UI lays under itself stretched to fill the 3D view.
//! Menus and text are drawn at the screen's own sharpness on top. The
//! picture's points are the window's points, so the 3D camera turns a tap
//! into a ray (and a part into a place on screen) as it does when it draws
//! straight to the window.

use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::window::PrimaryWindow;
use bevy_egui::PrimaryEguiContext;
use libroxel_engine::view::{CameraRig, GraphicsQuality};

/// How the 3D view is drawn this frame.
#[derive(Resource, Default)]
pub struct RenderScale {
    /// The picture the 3D view is drawn into, while it draws with fewer
    /// pixels than the window has.
    pub image: Option<Handle<Image>>,
    /// Pixels for each point of the window, for the 3D camera's viewport.
    pub pixels_per_point: f32,
}

/// The share of the window's pixels the 3D view draws, for a window of
/// `scale_factor` pixels a point: 1 is all of them.
pub fn share(density: Option<f32>, scale_factor: f32) -> f32 {
    match density {
        Some(d) if scale_factor > 0.0 => (d / scale_factor).clamp(0.35, 1.0),
        _ => 1.0,
    }
}

/// Picks how the 3D view is drawn, and points the 3D camera there.
pub fn apply(
    mut commands: Commands,
    mut scale: ResMut<RenderScale>,
    quality: Option<Res<GraphicsQuality>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    world_cameras: Query<Entity, With<CameraRig>>,
    mut ui_cameras: Query<&mut Camera, With<PrimaryEguiContext>>,
) {
    let Ok(window) = window.single() else { return };
    let ppp = window.scale_factor();
    scale.pixels_per_point = ppp;
    let share = share(quality.as_ref().and_then(|q| q.render_density()), ppp);
    let size = UVec2::new(
        ((window.physical_width() as f32 * share).ceil() as u32).max(1),
        ((window.physical_height() as f32 * share).ceil() as u32).max(1),
    );
    // Phones always draw through the picture, even at every pixel: it is
    // what clears the strip beside a notch, outside the 3D view.
    let phone = quality.as_ref().is_some_and(|q| q.render_density().is_some());
    let scaled = (share < 0.99 || phone) && window.physical_width() > 0;
    let changed = if scaled {
        let fits = scale.image.as_ref().and_then(|h| images.get(h)).is_some_and(|i| {
            i.texture_descriptor.size.width == size.x && i.texture_descriptor.size.height == size.y
        });
        if fits {
            false
        } else {
            let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
            image.texture_descriptor.size =
                Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 };
            // Stretched smoothly, not in blocks.
            image.sampler = bevy::image::ImageSampler::linear();
            scale.image = Some(images.add(image));
            true
        }
    } else {
        std::mem::take(&mut scale.image).is_some()
    };
    if scaled {
        // The picture's points are the window's points.
        scale.pixels_per_point = ppp * share;
    }
    if !changed {
        return;
    }
    let target = match &scale.image {
        Some(handle) => {
            RenderTarget::Image(ImageRenderTarget { handle: handle.clone(), scale_factor: ppp * share })
        }
        None => RenderTarget::Window(bevy::window::WindowRef::Primary),
    };
    for camera in &world_cameras {
        commands.entity(camera).insert(target.clone());
    }
    // The 3D camera no longer paints the window, so the UI's camera clears it.
    for mut camera in &mut ui_cameras {
        if let bevy::camera::CameraOutputMode::Write { clear_color, .. } = &mut camera.output_mode {
            *clear_color = if scale.image.is_some() {
                ClearColorConfig::Custom(Color::BLACK)
            } else {
                ClearColorConfig::None
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phones_draw_the_3d_view_with_fewer_pixels() {
        // A computer draws every pixel.
        assert_eq!(share(None, 2.0), 1.0);
        // A phone with three pixels a point draws half of them at 1.5.
        assert!((share(Some(1.5), 3.0) - 0.5).abs() < 1e-6);
        // Never more than the screen has, nor so few it looks smeared.
        assert_eq!(share(Some(2.0), 1.0), 1.0);
        assert_eq!(share(Some(1.0), 4.0), 0.35);
    }
}
