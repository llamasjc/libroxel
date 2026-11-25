//! Keeping menus and buttons clear of an iPhone's notch and home bar.
//!
//! On iOS the window's inner frame is the part of the screen clear of
//! them (its "safe area") and its outer frame the whole screen, so the
//! gaps between the two are how far in to keep the app's menus.

#![cfg_attr(not(target_os = "ios"), allow(dead_code))]

/// How far in from each edge (left, top, right, bottom) the safe area
/// starts, from the outer and inner frames' corners and sizes, in pixels.
pub fn insets(outer: (i32, i32, u32, u32), inner: (i32, i32, u32, u32)) -> [f32; 4] {
    let (ox, oy, ow, oh) = outer;
    let (ix, iy, iw, ih) = inner;
    let left = (ix - ox).max(0) as f32;
    let top = (iy - oy).max(0) as f32;
    let right = (ow as f32 - iw as f32 - left).max(0.0);
    let bottom = (oh as f32 - ih as f32 - top).max(0.0);
    [left, top, right, bottom]
}

/// Tells egui the iPhone's safe area, now and then (it changes only when
/// the phone turns), when the app draws to the whole screen.
#[cfg(target_os = "ios")]
pub fn follow(
    _main_thread: bevy::ecs::system::NonSendMarker,
    mut frames: bevy::prelude::Local<u32>,
    mut last: bevy::prelude::Local<[f32; 4]>,
    windows: bevy::prelude::Query<
        (bevy::prelude::Entity, &bevy::window::Window),
        bevy::prelude::With<bevy::window::PrimaryWindow>,
    >,
    mut contexts: bevy::prelude::Query<
        &mut bevy_egui::EguiInput,
        bevy::prelude::With<bevy_egui::PrimaryEguiContext>,
    >,
) {
    *frames += 1;
    if *frames % 60 != 1 {
        return;
    }
    let Ok((entity, window)) = windows.single() else { return };
    let frames = bevy::winit::WINIT_WINDOWS.with_borrow(|all| {
        let w = all.get_window(entity)?;
        let (outer, inner) = (w.outer_position().ok()?, w.inner_position().ok()?);
        let (osize, isize) = (w.outer_size(), w.inner_size());
        Some(((outer.x, outer.y, osize.width, osize.height), (inner.x, inner.y, isize.width, isize.height)))
    });
    let Some((outer, inner)) = frames else { return };
    // Drawn only into the safe area already: nothing to keep clear.
    let whole_screen = window.physical_width() >= outer.2 && window.physical_height() >= outer.3;
    let px = if whole_screen { insets(outer, inner) } else { [0.0; 4] };
    if px == *last {
        return;
    }
    *last = px;
    let Ok(mut input) = contexts.single_mut() else { return };
    let ppp = window.scale_factor().max(0.1);
    input.0.safe_area_insets = Some(bevy_egui::egui::SafeAreaInsets(bevy_egui::egui::epaint::MarginF32 {
        left: px[0] / ppp,
        top: px[1] / ppp,
        right: px[2] / ppp,
        bottom: px[3] / ppp,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gaps_between_the_frames_are_the_insets() {
        // A phone on its side: the notch on the left, the home bar along the bottom.
        let outer = (0, 0, 2532, 1170);
        let inner = (141, 0, 2250, 1107);
        assert_eq!(insets(outer, inner), [141.0, 0.0, 141.0, 63.0]);
        // No notch: nothing to keep clear.
        assert_eq!(insets(outer, outer), [0.0; 4]);
    }
}
