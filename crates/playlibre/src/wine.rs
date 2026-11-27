use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use bevy::window::{CursorMoved, WindowEvent, WindowMoved};

pub fn add(app: &mut App) {
    if running() {
        app.add_systems(First, unstick);
    }
}

fn unstick(
    mut moved: MessageReader<WindowMoved>,
    mut cursor: MessageReader<CursorMoved>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut buttons: MessageWriter<MouseButtonInput>,
    mut events: MessageWriter<WindowEvent>,
    mut after_move: Local<bool>,
) {
    if moved.read().count() > 0 {
        *after_move = true;
        cursor.clear();
        return;
    }
    let Some(window) = cursor.read().last().map(|c| c.window) else { return };
    if !std::mem::take(&mut *after_move) {
        return;
    }
    release_capture();
    for &button in mouse.get_pressed() {
        let input = MouseButtonInput { button, state: ButtonState::Released, window };
        buttons.write(input);
        events.write(input.into());
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn running() -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut core::ffi::c_void;
        fn GetProcAddress(module: *mut core::ffi::c_void, name: *const u8) -> *mut core::ffi::c_void;
    }
    let name: Vec<u16> = "ntdll.dll".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let ntdll = GetModuleHandleW(name.as_ptr());
        !ntdll.is_null() && !GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()).is_null()
    }
}

#[cfg(not(windows))]
fn running() -> bool {
    false
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn release_capture() {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn ReleaseCapture() -> i32;
    }
    unsafe {
        ReleaseCapture();
    }
}

#[cfg(not(windows))]
fn release_capture() {}
