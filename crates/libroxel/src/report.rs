use std::path::PathBuf;

use bevy_egui::egui;

const ISSUES: &str = "https://github.com/llamasjc/libroxel/issues/new";
const MAX_URL: usize = 7500;
const LOG_LINES: usize = 20;

pub fn button(ui: &mut egui::Ui, error: &str) {
    if ui.small_button("Report").clicked() {
        open(error);
    }
}

pub fn error(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(crate::theme::RED, text);
        button(ui, text);
    });
}

pub fn status(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        crate::theme::status(ui, crate::theme::RED, text);
        button(ui, text);
    });
}

pub fn open(error: &str) {
    open_url(&url(error, &log_tail()));
}

pub fn url(error: &str, log: &str) -> String {
    let error = scrub(error.trim());
    let title = error.lines().next().unwrap_or_default();
    let title: String = if title.chars().count() > 80 {
        title.chars().take(77).chain("...".chars()).collect()
    } else {
        title.to_owned()
    };
    let mut log: Vec<String> = scrub(log).lines().map(str::to_owned).collect();
    let mut error = error;
    loop {
        let link =
            format!("{ISSUES}?title={}&body={}", encode(&title), encode(&body(&error, &log.join("\n"))));
        if link.len() <= MAX_URL {
            return link;
        }
        if !log.is_empty() {
            log.remove(0);
        } else {
            let keep = error.chars().count() * 3 / 4;
            error = error.chars().take(keep).collect();
        }
    }
}

fn body(error: &str, log: &str) -> String {
    let mut text = format!(
        "**Error**\n```\n{error}\n```\n**Version** {}\n**OS** {} {}\n",
        crate::update::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if !log.trim().is_empty() {
        text.push_str(&format!("\n**Log**\n```\n{log}\n```\n"));
    }
    text
}

fn log_tail() -> String {
    let text = std::fs::read_to_string(crate::debugging::logs_dir().join("latest.log")).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(LOG_LINES)..].join("\n")
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn scrub(text: &str) -> String {
    let mut text = text.to_owned();
    for var in ["HOME", "USERPROFILE"] {
        if let Some(home) = std::env::var_os(var).map(|h| h.to_string_lossy().into_owned())
            && home.len() > 3
        {
            text = text.replace(&home, "~");
        }
    }
    libroxel_hub::scrub::scrub(&text)
}

pub fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

fn crash_path() -> PathBuf {
    crate::server::data_dir().join("crash.txt")
}

pub fn catch_panics() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_owned());
        let place = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let trace = std::backtrace::Backtrace::force_capture().to_string();
        let frames: Vec<&str> = trace.lines().filter(|l| l.contains("libroxel")).take(24).collect();
        let mut text = format!("{message}{place}");
        if !frames.is_empty() {
            text.push_str("\n\n");
            text.push_str(&frames.join("\n"));
        }
        let _ = std::fs::create_dir_all(crate::server::data_dir());
        let _ = std::fs::write(crash_path(), text);
        before(info);
    }));
}

pub fn run(app: &mut bevy::app::App) {
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.run()));
    if ran.is_ok() {
        let _ = std::fs::remove_file(crash_path());
        return;
    }
    if !crate::MOBILE
        && std::env::var_os("LIBROXEL_CRASHED").is_none()
        && let Ok(exe) = std::env::current_exe()
    {
        let _ = std::process::Command::new(exe).env("LIBROXEL_CRASHED", "1").spawn();
    } else {
        // It crashed again straight after starting over, so the crash
        // screen can't show: say why in the computer's own way instead of
        // vanishing without a word.
        let why = std::fs::read_to_string(crash_path()).unwrap_or_default();
        tell_and_wait(&format!(
            "Libroxel couldn't start.\n\n{}\n\nThis was saved to {}.",
            why.lines().take(12).collect::<Vec<_>>().join("\n"),
            crash_path().display()
        ));
    }
    std::process::exit(101);
}

/// A message box on Windows, where the app has no terminal to print to.
#[cfg(windows)]
#[allow(unsafe_code)]
fn tell_and_wait(text: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(window: *mut core::ffi::c_void, text: *const u16, title: *const u16, kind: u32)
        -> i32;
    }
    const ERROR_ICON: u32 = 0x10;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide("Libroxel"));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call,
    // and no owner window is fine.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), ERROR_ICON);
    }
}

#[cfg(not(windows))]
fn tell_and_wait(text: &str) {
    eprintln!("{text}");
}

pub fn take_crash() -> Option<String> {
    let path = crash_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(path);
    Some(text).filter(|t| !t.trim().is_empty())
}

pub fn crash_screen(ctx: &egui::Context, crash: &mut Option<String>) {
    let Some(text) = crash.as_ref() else { return };
    let mut close = false;
    egui::Modal::new(egui::Id::new("crash")).show(ctx, |ui| {
        ui.set_max_width(520.0);
        ui.heading("Libroxel crashed");
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            ui.label(egui::RichText::new(text).monospace().small().color(crate::theme::RED));
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Report").clicked() {
                open(text);
            }
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    });
    if close {
        *crash = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_private_things() {
        let text = scrub(
            "sign in failed for bob@example.com\npassword: hunter2\nkey 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\nBearer eyJhbGciOiJIUzI1NiJ9abcDEF123",
        );
        assert!(!text.contains("bob@example.com"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
        assert!(!text.contains("9f86d081"), "{text}");
        assert!(!text.contains("eyJhbG"), "{text}");
        assert!(text.contains("sign in failed for [email]"), "{text}");
    }

    #[test]
    fn keeps_ordinary_text() {
        let line = "Could not reach the Hub: connection refused (os error 111).";
        assert_eq!(scrub(line), line);
    }

    #[test]
    fn fits_in_a_link() {
        let log = "a line of the log that is fairly long\n".repeat(2000);
        let link = url(&"x".repeat(9000), &log);
        assert!(link.len() <= MAX_URL);
        assert!(link.starts_with(ISSUES));
    }
}
