//! The computer's own "open a file" window, so a game can be picked by
//! browsing rather than by typing its path.
//!
//! The window runs on its own thread so the game keeps drawing; the
//! choice is collected with [`Picker::poll`]. On Linux, KDE's picker
//! (`kdialog`) is tried first, then GNOME's (`zenity`), then the desktop
//! portal; on Windows and macOS the system's own window is used.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// What the window ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    File(PathBuf),
    Cancelled,
    /// No picker could be shown on this computer.
    Unavailable,
}

/// File endings the picker offers, as a name and a list.
const GAMES: (&str, &[&str]) = ("Games", &["lrx", "rbxl", "rbxlx", "rbxm", "rbxmx"]);

/// An "open a file" window that may be showing.
#[derive(Default)]
pub struct Picker {
    waiting: Option<Receiver<Picked>>,
}

impl Picker {
    /// Whether the window is showing now.
    pub fn is_open(&self) -> bool {
        self.waiting.is_some()
    }

    /// Shows the window, starting in `folder`. Does nothing if it's already showing.
    pub fn open(&mut self, folder: &Path) {
        if self.waiting.is_some() {
            return;
        }
        let (send, receive) = channel();
        let folder = folder.to_path_buf();
        std::thread::spawn(move || {
            let _ = send.send(pick(&folder));
        });
        self.waiting = Some(receive);
    }

    /// What was picked, once the window has closed.
    pub fn poll(&mut self) -> Option<Picked> {
        let result = match self.waiting.as_ref()?.try_recv() {
            Ok(picked) => picked,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Picked::Unavailable,
        };
        self.waiting = None;
        Some(result)
    }
}

fn pick(folder: &Path) -> Picked {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    if let Some(picked) = pick_with_command(folder) {
        return picked;
    }
    let (name, endings) = GAMES;
    let dialog =
        rfd::FileDialog::new().set_title("Open a game").add_filter(name, endings).set_directory(folder);
    let started = std::time::Instant::now();
    match dialog.pick_file() {
        Some(path) => Picked::File(path),
        // Nobody closes a window that fast: it never showed.
        None if started.elapsed() < std::time::Duration::from_millis(400) => Picked::Unavailable,
        None => Picked::Cancelled,
    }
}

/// Runs KDE's or GNOME's picker. None when neither is installed.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn pick_with_command(folder: &Path) -> Option<Picked> {
    use std::process::Command;
    let (name, endings) = GAMES;
    let patterns: Vec<String> = endings.iter().map(|e| format!("*.{e}")).collect();
    let start = format!("{}/", folder.display());
    let filter = patterns.join(" ");
    let kde = || {
        Command::new("kdialog")
            .arg("--title")
            .arg("Open a game")
            .arg("--getopenfilename")
            .arg(&start)
            .arg(format!("{name} ({filter})"))
            .output()
    };
    let gnome = || {
        Command::new("zenity")
            .arg("--file-selection")
            .arg("--title=Open a game")
            .arg(format!("--filename={start}"))
            .arg(format!("--file-filter={name} | {filter}"))
            .output()
    };
    // The second is only tried when the first isn't installed.
    let tries: [&dyn Fn() -> std::io::Result<std::process::Output>; 2] = [&kde, &gnome];
    for run in tries {
        let Ok(output) = run() else { continue };
        let chosen = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Some(if output.status.success() && !chosen.is_empty() {
            Picked::File(PathBuf::from(chosen))
        } else {
            Picked::Cancelled
        });
    }
    None
}
