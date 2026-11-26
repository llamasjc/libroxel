//! `playlibre://` links, so Play on the website opens the app straight into
//! a game:
//!
//! - `playlibre://join/<code>` joins the server with that join code;
//! - `playlibre://play/<game>` plays a game by its id (`starter:<name>` or a
//!   published game's id): it joins the best server, or starts one;
//! - `playlibre://host/<game>` starts a new server of it.
//!
//! The parts after the action are percent-encoded. The app registers itself
//! for these links each time it starts (see [`register`]), so a copy that
//! has been opened once handles them.

use std::time::Instant;

/// What a link asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    Join(String),
    Play(String),
    Host(String),
    /// A private server: the game and the server's code.
    Private {
        game: String,
        code: String,
    },
}

/// A link waiting for the home screen, with when it came.
#[derive(Debug, Clone)]
pub struct PendingLaunch {
    pub launch: Launch,
    pub since: Instant,
}

impl PendingLaunch {
    pub fn new(launch: Launch) -> Self {
        Self { launch, since: Instant::now() }
    }
}

const SCHEMES: [&str; 2] = ["playlibre:", "libroxel:"];

fn after_scheme(arg: &str) -> Option<&str> {
    SCHEMES.iter().find_map(|scheme| {
        arg.get(..scheme.len()).filter(|s| s.eq_ignore_ascii_case(scheme)).map(|_| &arg[scheme.len()..])
    })
}

/// Whether a command-line argument is a `playlibre:` link.
pub fn is_link(arg: &str) -> bool {
    after_scheme(arg).is_some()
}

/// Reads a `playlibre://` link, if it is one the app understands.
pub fn parse(link: &str) -> Option<Launch> {
    let rest = after_scheme(link)?.trim_start_matches('/');
    // Some systems add a slash at the end; the parts themselves never hold one.
    let rest = rest.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    let (action, value) = rest.split_once('/')?;
    let value = percent_decode(value)?;
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return None;
    }
    match action.to_ascii_lowercase().as_str() {
        "join" => Some(Launch::Join(value.to_owned())),
        "play" => Some(Launch::Play(value.to_owned())),
        "host" => Some(Launch::Host(value.to_owned())),
        "private" => {
            let (game, code) = value.rsplit_once(':')?;
            Some(Launch::Private { game: game.to_owned(), code: code.to_owned() })
        }
        _ => None,
    }
}

/// Undoes `%XX` escapes; `None` if the result isn't text.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Tells the computer that `playlibre://` links open this copy of the app.
/// Quiet and best effort: it runs in the background and a failure only
/// means links don't open the app. On macOS the app bundle's Info.plist
/// registers the links instead.
pub fn register() {
    std::thread::spawn(|| {
        if let Err(why) = register_now() {
            eprintln!("note: could not set up playlibre:// links: {why}");
        }
    });
}

#[cfg(target_os = "linux")]
fn register_now() -> Result<(), String> {
    use std::path::PathBuf;
    // The AppImage itself when it runs from one, which stays put.
    let exe = crate::command::app_path().map_err(|e| e.to_string())?;
    let dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or("no home folder")?
        .join("applications");
    // Desktop files quote the program like this (the Desktop Entry spec).
    let quoted: String = exe
        .to_string_lossy()
        .chars()
        .flat_map(|c| match c {
            '"' | '`' | '$' | '\\' => vec!['\\', c],
            c => vec![c],
        })
        .collect();
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=PlayLibre\nComment=Opens games from playlibre.net\n\
         Exec=\"{quoted}\" %u\nTerminal=false\nNoDisplay=true\nMimeType=x-scheme-handler/playlibre;x-scheme-handler/libroxel;\n"
    );
    let file = dir.join("playlibre-links.desktop");
    if std::fs::read_to_string(&file).is_ok_and(|old| old == entry) {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&file, entry).map_err(|e| e.to_string())?;
    let quiet = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    };
    let _ = quiet("update-desktop-database", &[&dir.to_string_lossy()]);
    for scheme in ["x-scheme-handler/playlibre", "x-scheme-handler/libroxel"] {
        quiet("xdg-mime", &["default", "playlibre-links.desktop", scheme])
            .map_err(|e| format!("xdg-mime: {e}"))?;
    }
    Ok(())
}

#[cfg(windows)]
fn register_now() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let open = format!("\"{}\" \"%1\"", exe.display());
    for key in [r"HKCU\Software\Classes\playlibre", r"HKCU\Software\Classes\libroxel"] {
        let command = format!(r"{key}\shell\open\command");
        for args in [
            vec!["add", key, "/ve", "/d", "URL:PlayLibre", "/f"],
            vec!["add", key, "/v", "URL Protocol", "/d", "", "/f"],
            vec!["add", &command, "/ve", "/d", &open, "/f"],
        ] {
            let status = std::process::Command::new("reg")
                .args(&args)
                .creation_flags(NO_WINDOW)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("reg {} failed", args[1]));
            }
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn register_now() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_links_from_the_website() {
        assert_eq!(parse("playlibre://join/abc123"), Some(Launch::Join("abc123".into())));
        assert_eq!(parse("libroxel://join/abc123/"), Some(Launch::Join("abc123".into())));
        assert_eq!(
            parse("PLAYLIBRE://Play/starter%3AKart%20race"),
            Some(Launch::Play("starter:Kart race".into()))
        );
        assert_eq!(parse("playlibre://host/x7k2q"), Some(Launch::Host("x7k2q".into())));
        assert_eq!(
            parse("playlibre://private/x7k2q%3Aabc"),
            Some(Launch::Private { game: "x7k2q".into(), code: "abc".into() })
        );
        assert_eq!(parse("playlibre:join/a%2Fb"), Some(Launch::Join("a/b".into())));
        assert_eq!(parse("playlibre://join/caf%C3%A9"), Some(Launch::Join("café".into())));
    }

    #[test]
    fn turns_away_anything_else() {
        assert_eq!(parse("https://playlibre.net/join/abc"), None);
        assert_eq!(parse("playlibre://delete/everything"), None);
        assert_eq!(parse("playlibre://join/"), None);
        assert_eq!(parse("playlibre://join"), None);
        assert_eq!(parse("playlibre://join/%zz"), None);
        assert_eq!(parse("playlibre://join/%FF"), None);
        assert_eq!(parse("playlibre://join/a%0Ab"), None);
        assert!(!is_link("game.lrx"));
        assert!(!is_link("li"));
    }
}
