// On Windows the app opens as a window with no console behind it.
// `playlibre server` and `playlibre help` borrow the terminal they were
// typed in instead (see `attach_terminal`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    playlibre::run();
}
