//! Playing without a keyboard and mouse: game controllers, and the
//! on-screen controls the host app draws on touch screens.
//!
//! A controller's left stick walks, the right stick looks round, A (the
//! bottom button) jumps, the right trigger clicks (it uses the tool in
//! hand), X (the left button) uses a prompt, Y (the top one) takes out the
//! first tool, the shoulder buttons swing the camera and the d-pad zooms.

use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::prelude::*;

/// Walking and looking from the on-screen touch controls, which the host
/// app fills in each frame.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct VirtualInput {
    /// The on-screen stick: x to the right, y forwards, each -1 to 1.
    pub stick: Vec2,
    /// The on-screen jump button is held.
    pub jump: bool,
    /// How far fingers dragged to look round since it was last used, in
    /// points (x to the right, y up).
    pub look: Vec2,
    /// Pinching, in mouse-wheel lines (positive zooms in).
    pub zoom: f32,
    /// The screen has been touched since the game started.
    pub touched: bool,
}

/// How far a stick must be pushed before it counts.
pub const DEAD_ZONE: f32 = 0.15;

/// How fast the right stick turns the camera at full push, in radians a second.
pub const LOOK_SPEED: f32 = 2.6;

/// A stick's push with the dead zone in the middle taken out, scaled so
/// just past the dead zone is 0 and all the way is 1.
pub fn dead_zone(v: Vec2) -> Vec2 {
    let len = v.length();
    if len < DEAD_ZONE {
        return Vec2::ZERO;
    }
    v / len * ((len - DEAD_ZONE) / (1.0 - DEAD_ZONE)).min(1.0)
}

/// What the controllers ask for this frame.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct PadInput {
    /// Walking: x to the right, y forwards.
    pub stick: Vec2,
    /// Looking: x to the right, y up.
    pub look: Vec2,
    pub jump: bool,
    /// The click button went down (`Some(true)`) or up (`Some(false)`).
    pub click: Option<bool>,
    /// The prompt button went down or up.
    pub interact: Option<bool>,
    /// The take-out-a-tool button was pressed.
    pub tool: bool,
    /// Camera swings this frame: +1 to the left, -1 to the right.
    pub swing: f32,
    /// Zoom steps this frame: +1 in, -1 out.
    pub zoom: f32,
    /// Anything at all was used.
    pub used: bool,
}

fn edge(pad: &Gamepad, button: GamepadButton) -> Option<bool> {
    if pad.just_pressed(button) {
        Some(true)
    } else if pad.just_released(button) {
        Some(false)
    } else {
        None
    }
}

/// Reads every connected controller and adds them up.
pub fn read_pads<'a>(pads: impl IntoIterator<Item = &'a Gamepad>) -> PadInput {
    use GamepadButton as B;
    let mut out = PadInput::default();
    for pad in pads {
        out.stick += dead_zone(pad.left_stick());
        out.look += dead_zone(pad.right_stick());
        out.jump |= pad.pressed(B::South);
        out.click = out.click.or(edge(pad, B::RightTrigger2));
        out.interact = out.interact.or(edge(pad, B::West));
        out.tool |= pad.just_pressed(B::North);
        out.swing +=
            pad.just_pressed(B::LeftTrigger) as i32 as f32 - pad.just_pressed(B::RightTrigger) as i32 as f32;
        out.zoom += pad.just_pressed(B::DPadUp) as i32 as f32 - pad.just_pressed(B::DPadDown) as i32 as f32;
        out.used |= pad.get_pressed().next().is_some()
            || pad.left_stick().length() > DEAD_ZONE
            || pad.right_stick().length() > DEAD_ZONE;
    }
    out.stick = out.stick.clamp_length_max(1.0);
    out.look = out.look.clamp_length_max(1.0);
    out
}

/// A walking direction on the ground from a stick push (x right, y
/// forwards) and where the camera faces.
pub fn walk_direction(stick: Vec2, camera_forward: Vec3) -> Vec3 {
    let forward = Vec3::new(camera_forward.x, 0.0, camera_forward.z).try_normalize().unwrap_or(Vec3::NEG_Z);
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    forward * stick.y + right * stick.x
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::gamepad::{GamepadAxis, GamepadInput};

    fn pad() -> Gamepad {
        Gamepad::default()
    }

    #[test]
    fn small_pushes_do_nothing_and_full_ones_are_full() {
        assert_eq!(dead_zone(Vec2::new(0.1, 0.05)), Vec2::ZERO);
        assert!((dead_zone(Vec2::new(1.0, 0.0)).x - 1.0).abs() < 1e-6);
        let half = dead_zone(Vec2::new(0.0, (1.0 + DEAD_ZONE) / 2.0));
        assert!((half.y - 0.5).abs() < 1e-5, "{half}");
    }

    #[test]
    fn sticks_walk_and_look_and_buttons_do_their_jobs() {
        let mut p = pad();
        p.analog_mut().set(GamepadInput::Axis(GamepadAxis::LeftStickY), 1.0);
        p.analog_mut().set(GamepadInput::Axis(GamepadAxis::RightStickX), -1.0);
        p.digital_mut().press(GamepadButton::South);
        p.digital_mut().press(GamepadButton::RightTrigger2);
        p.digital_mut().press(GamepadButton::LeftTrigger);
        let input = read_pads([&p]);
        assert!((input.stick - Vec2::new(0.0, 1.0)).length() < 1e-5);
        assert!((input.look - Vec2::new(-1.0, 0.0)).length() < 1e-5);
        assert!(input.jump && input.used);
        assert_eq!(input.click, Some(true));
        assert_eq!(input.swing, 1.0);
        assert_eq!(input.interact, None);
        // Letting go of the trigger is a release.
        p.digital_mut().clear();
        p.digital_mut().release(GamepadButton::RightTrigger2);
        assert_eq!(read_pads([&p]).click, Some(false));
        // Nothing plugged in: nothing happens.
        assert_eq!(read_pads(std::iter::empty::<&Gamepad>()), PadInput::default());
    }

    #[test]
    fn the_stick_walks_the_way_the_camera_faces() {
        let east = walk_direction(Vec2::new(0.0, 1.0), Vec3::new(1.0, -0.5, 0.0));
        assert!((east - Vec3::X).length() < 1e-5, "{east}");
        let right_of_north = walk_direction(Vec2::new(1.0, 0.0), Vec3::NEG_Z);
        assert!((right_of_north - Vec3::X).length() < 1e-5, "{right_of_north}");
    }
}
