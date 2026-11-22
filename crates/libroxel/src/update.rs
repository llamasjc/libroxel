//! Updates from GitHub releases, checked against a signed `latest.json`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use data_encoding::{BASE64, HEXLOWER};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

pub const VERSION: &str = match option_env!("LIBROXEL_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

const RELEASES: &str = "https://github.com/llamasjc/libroxel/releases";
const KEY: &str = "eZLyPH2I3vxefEog220wBtSrI3CxSLCMkgKWyRKv0xY=";

static READY: Mutex<Option<Staged>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Windows,
    Mac,
    AppImage,
    Deb,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Windows => "windows",
            Kind::Mac => "macos",
            Kind::AppImage => "appimage",
            Kind::Deb => "deb",
        }
    }
}

#[derive(Debug, Clone)]
struct Staged {
    version: String,
    file: PathBuf,
}

fn kind() -> Option<Kind> {
    let exe = std::env::current_exe().ok()?;
    if cfg!(windows) {
        return exe.with_file_name("unins000.exe").exists().then_some(Kind::Windows);
    }
    if cfg!(target_os = "macos") {
        return bundle(&exe).map(|_| Kind::Mac);
    }
    if std::env::var_os("APPIMAGE").is_some() {
        return Some(Kind::AppImage);
    }
    exe.starts_with("/usr/lib/libroxel").then_some(Kind::Deb)
}

fn bundle(exe: &Path) -> Option<PathBuf> {
    let app = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"))?;
    let text = app.to_string_lossy().to_ascii_lowercase();
    (!text.contains("/apptranslocation/") && !text.starts_with("/volumes/")).then(|| app.to_path_buf())
}

fn numbers(version: &str) -> Option<Vec<u64>> {
    if version.contains('-') {
        return None;
    }
    version.split('.').map(|n| n.parse().ok()).collect()
}

fn newer(theirs: &str, ours: &str) -> bool {
    match (numbers(theirs), numbers(ours)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn dir() -> PathBuf {
    crate::server::data_dir().join("update")
}

fn enabled() -> bool {
    option_env!("LIBROXEL_VERSION").is_some()
        && numbers(VERSION).is_some()
        && std::env::var_os("LIBROXEL_NO_UPDATE").is_none()
}

fn staged() -> Option<Staged> {
    let text = std::fs::read_to_string(dir().join("ready")).ok()?;
    let (version, name) = text.trim().split_once('\n')?;
    let file = dir().join(name.trim());
    file.exists().then(|| Staged { version: version.trim().to_owned(), file })
}

pub fn on_start(args: &[String]) {
    if !enabled() {
        return;
    }
    match staged() {
        Some(s) if newer(&s.version, VERSION) => {
            if args.is_empty()
                && kind().is_some_and(|k| k != Kind::Deb)
                && let Err(why) = install(&s)
            {
                eprintln!("note: could not install the update: {why}");
            }
            *READY.lock().unwrap() = Some(s);
        }
        _ => {
            let _ = std::fs::remove_dir_all(dir());
        }
    }
}

pub fn start() {
    if !enabled() || kind().is_none() {
        return;
    }
    std::thread::spawn(|| match check() {
        Ok(Some(s)) => *READY.lock().unwrap() = Some(s),
        Ok(None) => {}
        Err(why) => eprintln!("note: could not check for updates: {why}"),
    });
}

pub fn ready() -> Option<String> {
    READY.lock().unwrap().as_ref().map(|s| s.version.clone())
}

pub fn restart() -> Result<(), String> {
    let s = READY.lock().unwrap().clone().ok_or("No update is ready")?;
    install(&s)
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url).call().map_err(|e| e.to_string())?;
    response.body_mut().with_config().limit(1 << 30).read_to_vec().map_err(|e| e.to_string())
}

fn verify(manifest: &[u8], sig: &[u8]) -> Result<(), String> {
    let key: [u8; 32] =
        BASE64.decode(KEY.as_bytes()).ok().and_then(|k| k.try_into().ok()).ok_or("bad key")?;
    let key = VerifyingKey::from_bytes(&key).map_err(|e| e.to_string())?;
    let sig = BASE64.decode(sig.trim_ascii()).map_err(|e| e.to_string())?;
    let sig = Signature::from_slice(&sig).map_err(|e| e.to_string())?;
    key.verify_strict(manifest, &sig).map_err(|_| "the update's signature doesn't match".to_owned())
}

fn check() -> Result<Option<Staged>, String> {
    let kind = kind().ok_or("not installed")?;
    let manifest = get(&format!("{RELEASES}/latest/download/latest.json"))?;
    let sig = get(&format!("{RELEASES}/latest/download/latest.json.sig"))?;
    verify(&manifest, &sig)?;
    let json: serde_json::Value = serde_json::from_slice(&manifest).map_err(|e| e.to_string())?;
    let version = json["version"].as_str().ok_or("no version")?.to_owned();
    if !newer(&version, VERSION) {
        return Ok(None);
    }
    if let Some(s) = staged().filter(|s| s.version == version) {
        return Ok(Some(s));
    }
    let entry = &json["files"][kind.key()];
    let (Some(name), Some(sha)) = (entry["name"].as_str(), entry["sha256"].as_str()) else {
        return Ok(None);
    };
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Err("bad file name".into());
    }
    let bytes = get(&format!("{RELEASES}/download/v{version}/{name}"))?;
    if HEXLOWER.encode(&Sha256::digest(&bytes)) != sha.to_ascii_lowercase() {
        return Err("the download doesn't match the signed checksum".into());
    }
    let _ = std::fs::remove_dir_all(dir());
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let file = dir().join(name);
    std::fs::write(&file, &bytes).map_err(|e| e.to_string())?;
    std::fs::write(dir().join("ready"), format!("{version}\n{name}\n")).map_err(|e| e.to_string())?;
    Ok(Some(Staged { version, file }))
}

fn install(s: &Staged) -> Result<(), String> {
    let kind = kind().ok_or("not installed")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    match kind {
        Kind::Windows => {
            Command::new(&s.file)
                .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/NOCANCEL"])
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        Kind::Mac => {
            let app = bundle(&exe).ok_or("not in an app")?;
            let out = dir().join("new");
            let _ = std::fs::remove_dir_all(&out);
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            run(Command::new("tar").arg("-xzf").arg(&s.file).arg("-C").arg(&out))?;
            let new = out.join("Libroxel.app");
            let old = app.with_extension("app.old");
            let _ = std::fs::remove_dir_all(&old);
            std::fs::rename(&app, &old).map_err(|e| e.to_string())?;
            if let Err(e) = std::fs::rename(&new, &app) {
                let _ = std::fs::rename(&old, &app);
                return Err(e.to_string());
            }
            let _ = std::fs::remove_dir_all(&old);
            Command::new("open").arg("-n").arg(&app).spawn().map_err(|e| e.to_string())?;
        }
        Kind::AppImage => {
            let path = PathBuf::from(std::env::var_os("APPIMAGE").ok_or("no AppImage")?);
            let part = path.with_extension("new");
            std::fs::copy(&s.file, &part).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }
            std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
            Command::new(&path).spawn().map_err(|e| e.to_string())?;
        }
        Kind::Deb => {
            Command::new("sh")
                .arg("-c")
                .arg("pkexec apt-get install -y --allow-downgrades \"$1\" && exec /usr/bin/libroxel")
                .arg("sh")
                .arg(&s.file)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
    }
    std::process::exit(0);
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| e.to_string())?;
    status.success().then_some(()).ok_or_else(|| format!("{cmd:?} failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("0.10.0", "0.9.3"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.2.0-rc1", "0.1.0"));
        assert!(!newer("0.2.0", "dev-abc1234"));
    }

    #[test]
    fn signature() {
        let sig = b"MR3kmvTWMVPjI1erMhmu0TVppH5B08sOcVocj3DqAK39JwFcVkHps6GsOfSwstktc+8G+H8MqLvG/0TTK3DxBQ==";
        assert!(verify(br#"{"version":"0.0.0"}"#, sig).is_ok());
        assert!(verify(br#"{"version":"9.0.0"}"#, sig).is_err());
    }
}
