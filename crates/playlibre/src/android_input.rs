//! Typing and the back button on Android.
//!
//! A phone's on-screen keyboard doesn't send key presses: it edits a line
//! of text Android keeps for the app, a word at a time. winit leaves that
//! line alone, so here it is read each frame and what changed is handed to
//! egui as typing and backspaces. The back button acts as Esc does (it
//! opens and closes the game's menu), which egui otherwise never hears of.

#![cfg_attr(not(target_os = "android"), allow(dead_code))]

/// What changed between the keyboard's line `before` and `after`: how many
/// characters were taken off the end, then what was added.
pub fn typed(before: &str, after: &str) -> (usize, String) {
    let same = before.chars().zip(after.chars()).take_while(|(a, b)| a == b).count();
    (before.chars().count() - same, after.chars().skip(same).collect())
}

#[cfg(target_os = "android")]
fn key(key: bevy_egui::egui::Key, pressed: bool) -> bevy_egui::egui::Event {
    bevy_egui::egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: bevy_egui::egui::Modifiers::NONE,
    }
}

/// Hands this frame's typing and back button to egui. Outside a game,
/// the back button puts PlayLibre in the background, as in other apps.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
pub fn keyboard(
    engine: Option<bevy::prelude::NonSend<playlibre_engine::view::EngineState>>,
    mut last: bevy::prelude::Local<String>,
    keys: bevy::prelude::Res<bevy::input::ButtonInput<bevy::input::keyboard::Key>>,
    mut contexts: bevy::prelude::Query<
        (&mut bevy_egui::EguiInput, &mut bevy_egui::EguiContext),
        bevy::prelude::With<bevy_egui::PrimaryEguiContext>,
    >,
) {
    use bevy::android::android_activity::input::{TextInputState, TextSpan};
    use bevy_egui::egui;

    let Some(app) = bevy::android::ANDROID_APP.get() else { return };
    let Ok((mut input, mut ctx)) = contexts.single_mut() else { return };
    let events = &mut input.0.events;
    if keys.just_pressed(bevy::input::keyboard::Key::BrowserBack) {
        if engine.as_ref().is_some_and(|e| e.session.is_none()) {
            // SAFETY: the Java VM and activity Android gave the app.
            if let Ok(vm) = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) }
                && let Ok(mut env) = vm.attach_current_thread()
            {
                let activity = unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr().cast()) };
                let _ =
                    env.call_method(&activity, "moveTaskToBack", "(Z)Z", &[jni::objects::JValue::Bool(1)]);
                if env.exception_check().unwrap_or(false) {
                    let _ = env.exception_clear();
                }
            }
        } else {
            events.push(key(egui::Key::Escape, true));
            events.push(key(egui::Key::Escape, false));
        }
    }
    let typing = ctx.get_mut().memory(|m| m.focused().is_some());
    let state = app.text_input_state();
    if !typing {
        // A new text box starts from an empty line.
        if !state.text.is_empty() || !last.is_empty() {
            app.set_text_input_state(TextInputState {
                text: String::new(),
                selection: TextSpan { start: 0, end: 0 },
                compose_region: None,
            });
            last.clear();
        }
        return;
    }
    if state.text != *last {
        let (gone, added) = typed(&last, &state.text);
        for _ in 0..gone {
            events.push(key(egui::Key::Backspace, true));
            events.push(key(egui::Key::Backspace, false));
        }
        if !added.is_empty() {
            events.push(egui::Event::Text(added));
        }
        *last = state.text;
    }
}

/// Runs the phone's vibration motor at a strength a game set with
/// HapticService:SetMotor, through PlayLibre's activity.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
pub fn vibrate(mut screen: bevy::prelude::ResMut<playlibre_engine::devices::ScreenTouches>) {
    use jni::JavaVM;
    use jni::objects::{JObject, JValue};

    let Some(strength) = screen.vibration.take() else { return };
    let Some(app) = bevy::android::ANDROID_APP.get() else { return };
    // SAFETY: the pointers are the Java VM and activity Android gave the
    // app, alive for as long as it runs.
    let Ok(vm) = (unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }) else { return };
    let Ok(mut env) = vm.attach_current_thread() else { return };
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let _ = env.call_method(&activity, "setMotor", "(F)V", &[JValue::Float(strength)]);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

/// Keeps the app's menus and buttons out from under a notch or camera
/// cut-out: asks the activity how far in the screen is clear (now and then,
/// as it changes only when the screen turns) and tells egui, whose panels
/// and windows then stay inside.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
pub fn safe_area(
    mut frames: bevy::prelude::Local<u32>,
    mut last: bevy::prelude::Local<[i32; 4]>,
    window: bevy::prelude::Query<&bevy::window::Window, bevy::prelude::With<bevy::window::PrimaryWindow>>,
    mut contexts: bevy::prelude::Query<
        &mut bevy_egui::EguiInput,
        bevy::prelude::With<bevy_egui::PrimaryEguiContext>,
    >,
) {
    use jni::JavaVM;
    use jni::objects::{JIntArray, JObject};

    *frames += 1;
    if *frames % 60 != 1 {
        return;
    }
    let Some(app) = bevy::android::ANDROID_APP.get() else { return };
    let Ok(vm) = (unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }) else { return };
    let Ok(mut env) = vm.attach_current_thread() else { return };
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let mut insets = [0i32; 4];
    let got = env
        .call_method(&activity, "getSafeInsets", "()[I", &[])
        .and_then(|v| v.l())
        .and_then(|array| env.get_int_array_region(JIntArray::from(array), 0, &mut insets));
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    if got.is_err() || insets == *last {
        return;
    }
    *last = insets;
    let (Ok(window), Ok(mut input)) = (window.single(), contexts.single_mut()) else { return };
    let ppp = window.scale_factor().max(0.1);
    let points = |px: i32| px.max(0) as f32 / ppp;
    input.0.safe_area_insets = Some(bevy_egui::egui::SafeAreaInsets(bevy_egui::egui::epaint::MarginF32 {
        left: points(insets[0]),
        top: points(insets[1]),
        right: points(insets[2]),
        bottom: points(insets[3]),
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_fixing_words_comes_through() {
        assert_eq!(typed("", "h"), (0, "h".to_owned()));
        assert_eq!(typed("hel", "hello"), (0, "lo".to_owned()));
        // The keyboard fixed a word: back to where it differs, then the new end.
        assert_eq!(typed("teh", "the "), (2, "he ".to_owned()));
        assert_eq!(typed("hello", "hell"), (1, String::new()));
        assert_eq!(typed("año", "añob"), (0, "b".to_owned()));
        assert_eq!(typed("same", "same"), (0, String::new()));
    }
}
