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

/// Hands this frame's typing and back button to egui.
#[cfg(target_os = "android")]
pub fn keyboard(
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
        events.push(key(egui::Key::Escape, true));
        events.push(key(egui::Key::Escape, false));
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
