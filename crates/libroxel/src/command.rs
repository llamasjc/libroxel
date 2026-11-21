//! The `libroxel` terminal command. Each time a downloaded copy of the app
//! starts, it makes sure typing `libroxel` in a new terminal opens that
//! copy:
//!
//! - **Linux:** a link at `~/.local/bin/libroxel`, and `~/.local/bin` added
//!   to the shell's PATH if it isn't there already;
//! - **macOS:** a link at `/usr/local/bin/libroxel` when that folder can be
//!   written without a password, otherwise at `~/.local/bin/libroxel` with
//!   that folder added to zsh's and bash's PATH. The More menu can also put
//!   the link in `/usr/local/bin` with the computer's password;
//! - **Windows:** the app's folder added to the user's PATH.
//!
//! The installers already do this; it's for a copy opened some other way
//! (a Mac app dragged out of its disk image, or a build copied by hand),
//! and it does nothing when `libroxel` already opens this copy.
//!
//! Quiet and best effort, like [`crate::url_handler::register`]. It is
//! skipped in debug builds (so `cargo run` doesn't take the command over)
//! and when `LIBROXEL_NO_COMMAND` is set. A file at the link's place that
//! isn't a link to a Libroxel app is left alone.

use std::path::{Path, PathBuf};

/// The lines added to a shell's startup file to put `~/.local/bin` on PATH.
#[cfg(unix)]
const PATH_LINES: &str = "\n# Added by Libroxel, so the `libroxel` command works.\n\
case \":$PATH:\" in *\":$HOME/.local/bin:\"*) ;; *) export PATH=\"$HOME/.local/bin:$PATH\" ;; esac\n";

/// Sets the command up in the background.
pub fn install() {
    if cfg!(debug_assertions) || std::env::var_os("LIBROXEL_NO_COMMAND").is_some() {
        return;
    }
    std::thread::spawn(|| {
        if let Err(why) = install_now() {
            eprintln!("note: could not set up the libroxel command: {why}");
        }
    });
}

/// Whether `path` is a place the app shouldn't point the command at: a
/// copy macOS runs from a random read-only folder (App Translocation), or
/// one opened straight out of a zip into a temporary folder.
fn temporary(path: &Path) -> bool {
    let text = path.to_string_lossy().to_ascii_lowercase().replace('\\', "/");
    text.contains("/apptranslocation/") || text.contains("/appdata/local/temp/") || text.starts_with("/tmp/")
}

/// Points a link at `exe`, unless something else already sits there.
/// Returns whether the link now points at `exe`.
#[cfg(unix)]
fn link(at: &Path, exe: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(at) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let old = std::fs::read_link(at)?;
            if old == exe {
                return Ok(true);
            }
            // Only take over a link to another copy of Libroxel (an older
            // download, or one that has since moved).
            let ours = old.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("libroxel"));
            if !ours {
                return Ok(false);
            }
            std::fs::remove_file(at)?;
        }
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    if let Some(dir) = at.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::os::unix::fs::symlink(exe, at)?;
    Ok(true)
}

/// Whether `libroxel` on PATH already opens `exe` (an installer put it
/// there, such as the .deb's /usr/bin/libroxel).
#[cfg(unix)]
fn command_opens(exe: &Path) -> bool {
    let Ok(exe) = exe.canonicalize() else { return false };
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path)
            .any(|dir| dir.join("libroxel").canonicalize().is_ok_and(|found| found == exe))
    })
}

/// Whether `dir` is one of the folders in this program's PATH.
fn on_path(dir: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|p| p == dir))
}

/// Adds the PATH lines to each of `files` that exists (or the first one,
/// if none does), unless they are there already.
#[cfg(unix)]
fn add_to_path(home: &Path, files: &[&str]) -> std::io::Result<()> {
    use std::io::Write;
    let mut targets: Vec<PathBuf> = files.iter().map(|f| home.join(f)).filter(|f| f.exists()).collect();
    if targets.is_empty() {
        targets.extend(files.last().map(|f| home.join(f)));
    }
    for file in targets {
        let old = std::fs::read_to_string(&file).unwrap_or_default();
        if !old.contains("Added by Libroxel") {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file)?
                .write_all(PATH_LINES.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| "no home folder".into())
}

#[cfg(target_os = "linux")]
fn install_now() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if temporary(&exe) || command_opens(&exe) {
        return Ok(());
    }
    let home = home()?;
    let bin = home.join(".local/bin");
    if exe.parent() == Some(bin.as_path()) {
        return Ok(());
    }
    if !link(&bin.join("libroxel"), &exe).map_err(|e| e.to_string())? {
        return Ok(());
    }
    if !on_path(&bin) {
        add_to_path(&home, &[".bashrc", ".zshrc", ".profile"]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_now() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if temporary(&exe) || command_opens(&exe) {
        return Ok(());
    }
    // Homebrew on Intel Macs leaves /usr/local/bin writable.
    let system = Path::new("/usr/local/bin");
    let writable = system.is_dir()
        && std::fs::metadata(system).is_ok_and(|m| !m.permissions().readonly())
        && tempfile_in(system);
    if writable && link(&system.join("libroxel"), &exe).unwrap_or(false) {
        return Ok(());
    }
    if std::fs::read_link(system.join("libroxel")).is_ok_and(|old| old == exe) {
        return Ok(());
    }
    let home = home()?;
    let bin = home.join(".local/bin");
    if !link(&bin.join("libroxel"), &exe).map_err(|e| e.to_string())? {
        return Ok(());
    }
    if !on_path(&bin) {
        add_to_path(&home, &[".zprofile", ".bash_profile"]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Whether a file can be made in `dir` (the permission bits don't say for
/// a folder owned by someone else).
#[cfg(target_os = "macos")]
fn tempfile_in(dir: &Path) -> bool {
    let probe = dir.join(format!(".libroxel-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Links `/usr/local/bin/libroxel` to this app, asking for the computer's
/// password. For the More menu on macOS.
#[cfg(target_os = "macos")]
pub fn link_for_everyone() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if temporary(&exe) {
        return Err("Move Libroxel to your Applications folder and open it from there first.".into());
    }
    let quoted = exe.to_string_lossy().replace('\'', "'\\''");
    let shell = format!("mkdir -p /usr/local/bin && ln -sf '{quoted}' /usr/local/bin/libroxel");
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        shell.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let status =
        std::process::Command::new("osascript").args(["-e", &script]).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("The link wasn't made.".into()) }
}

#[cfg(windows)]
fn install_now() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if temporary(&exe) {
        return Ok(());
    }
    let dir = exe.parent().ok_or("the app has no folder")?;
    if on_path(dir) {
        return Ok(());
    }
    // PowerShell's SetEnvironmentVariable also tells open windows (and so
    // new terminals) that PATH changed.
    let quoted = dir.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$d = '{quoted}'; $p = [Environment]::GetEnvironmentVariable('Path', 'User'); \
         if ($p -eq $null) {{ $p = '' }}; \
         if (-not (($p -split ';') -contains $d)) {{ \
           $n = (($p.TrimEnd(';')) + ';' + $d).TrimStart(';'); \
           [Environment]::SetEnvironmentVariable('Path', $n, 'User') }}"
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("powershell could not change PATH".into()) }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn install_now() -> Result<(), String> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("libroxel-command-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn links_to_the_app_and_follows_it_when_it_moves() {
        let dir = scratch("link");
        let at = dir.join("bin/libroxel");
        let first = dir.join("old/libroxel");
        let second = dir.join("new/libroxel");
        assert!(link(&at, &first).unwrap());
        assert_eq!(std::fs::read_link(&at).unwrap(), first);
        assert!(link(&at, &first).unwrap());
        assert!(link(&at, &second).unwrap());
        assert_eq!(std::fs::read_link(&at).unwrap(), second);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn leaves_other_programs_alone() {
        let dir = scratch("other");
        let at = dir.join("libroxel");
        std::fs::write(&at, "#!/bin/sh\n").unwrap();
        assert!(!link(&at, &dir.join("app/libroxel")).unwrap());
        assert_eq!(std::fs::read_to_string(&at).unwrap(), "#!/bin/sh\n");
        let other = dir.join("other-link");
        std::os::unix::fs::symlink(dir.join("something-else"), &other).unwrap();
        assert!(!link(&other, &dir.join("app/libroxel")).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn adds_path_lines_once_to_the_files_there() {
        let home = scratch("path");
        std::fs::write(home.join(".bashrc"), "alias ll='ls -l'\n").unwrap();
        add_to_path(&home, &[".bashrc", ".zshrc", ".profile"]).unwrap();
        add_to_path(&home, &[".bashrc", ".zshrc", ".profile"]).unwrap();
        let bashrc = std::fs::read_to_string(home.join(".bashrc")).unwrap();
        assert_eq!(bashrc.matches("Added by Libroxel").count(), 1, "{bashrc}");
        assert!(bashrc.starts_with("alias ll"));
        assert!(!home.join(".zshrc").exists() && !home.join(".profile").exists());
        // With none of them there, the last one is made.
        let empty = scratch("path-empty");
        add_to_path(&empty, &[".bashrc", ".zshrc", ".profile"]).unwrap();
        assert!(std::fs::read_to_string(empty.join(".profile")).unwrap().contains(".local/bin"));
        assert!(!empty.join(".bashrc").exists());
        std::fs::remove_dir_all(&home).unwrap();
        std::fs::remove_dir_all(&empty).unwrap();
    }

    #[test]
    fn skips_temporary_copies() {
        assert!(temporary(Path::new(
            "/private/var/folders/x/AppTranslocation/ABC/d/Libroxel.app/Contents/MacOS/libroxel"
        )));
        assert!(temporary(Path::new(r"C:\Users\a\AppData\Local\Temp\Temp1_libroxel.zip\libroxel.exe")));
        assert!(!temporary(Path::new("/home/a/Apps/libroxel/libroxel")));
    }
}
