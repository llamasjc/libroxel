//! Updates from GitHub releases, checked against a signed `latest.json`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use data_encoding::{BASE64, HEXLOWER};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

pub const VERSION: &str = match option_env!("PLAYLIBRE_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

pub fn tag() -> String {
    if VERSION.starts_with(|c: char| c.is_ascii_digit()) { format!("v{VERSION}") } else { VERSION.to_owned() }
}

const RELEASES: &str = "https://github.com/llamasjc/playlibre/releases";
const KEY: &str = "eZLyPH2I3vxefEog220wBtSrI3CxSLCMkgKWyRKv0xY=";

static READY: Mutex<Option<Staged>> = Mutex::new(None);
static CHECKING: AtomicBool = AtomicBool::new(false);
static ASKED: AtomicBool = AtomicBool::new(false);
static TRIED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(target_os = "android", allow(dead_code))]
enum Kind {
    Windows,
    Mac,
    AppImage,
    Deb,
    Android,
}

impl Kind {
    /// The name of this kind of copy in a release's `latest.json`. ARM
    /// Windows and Linux copies have their own, so they never update to an
    /// x86-64 build (the Mac app and the Android app hold both).
    fn key(self) -> &'static str {
        let arm = cfg!(target_arch = "aarch64");
        match self {
            Kind::Windows if arm => "windows-aarch64",
            Kind::Windows => "windows",
            Kind::Mac => "macos",
            Kind::AppImage if arm => "appimage-aarch64",
            Kind::AppImage => "appimage",
            Kind::Deb if arm => "deb-aarch64",
            Kind::Deb => "deb",
            Kind::Android => "android",
        }
    }
}

#[derive(Debug, Clone)]
struct Staged {
    version: String,
    file: PathBuf,
}

#[cfg(target_os = "android")]
fn kind() -> Option<Kind> {
    (!android::from_play()).then_some(Kind::Android)
}

#[cfg(not(target_os = "android"))]
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
    exe.starts_with("/usr/lib/playlibre").then_some(Kind::Deb)
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
    option_env!("PLAYLIBRE_VERSION").is_some()
        && numbers(VERSION).is_some()
        && std::env::var_os("PLAYLIBRE_NO_UPDATE").is_none()
}

fn skipped() -> PathBuf {
    crate::server::data_dir().join("update-skipped")
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
            let tried = dir().join("tried");
            if std::fs::read_to_string(&tried).is_ok_and(|v| v.trim() == s.version) {
                let _ = std::fs::write(skipped(), &s.version);
                let _ = std::fs::remove_dir_all(dir());
                return;
            }
            if args.is_empty() && kind().is_some_and(|k| k != Kind::Deb) {
                let _ = std::fs::write(&tried, &s.version);
                if let Err(why) = install(&s) {
                    eprintln!("note: could not install the update: {why}");
                }
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
    std::thread::spawn(|| {
        loop {
            fetch();
            std::thread::sleep(Duration::from_secs(30 * 60));
        }
    });
}

fn fetch() {
    if CHECKING.swap(true, Ordering::SeqCst) {
        return;
    }
    match check() {
        Ok(Some(s)) => *READY.lock().unwrap() = Some(s),
        Ok(None) => {}
        Err(why) => eprintln!("note: could not check for updates: {why}"),
    }
    CHECKING.store(false, Ordering::SeqCst);
}

pub fn now() {
    if !enabled() || kind().is_none() || ready().is_some() || ASKED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(fetch);
}

pub fn checking() -> bool {
    CHECKING.load(Ordering::SeqCst)
}

pub fn apply(asked: bool) -> Result<(), String> {
    if ready().is_none() || (!asked && kind() == Some(Kind::Deb)) || TRIED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    restart()
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
    if !newer(&version, VERSION) || std::fs::read_to_string(skipped()).is_ok_and(|v| v.trim() == version) {
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
    let url = format!("{RELEASES}/download/v{version}/{name}");
    if kind == Kind::Android {
        return Ok(Some(Staged { version, file: PathBuf::from(url) }));
    }
    let bytes = get(&url)?;
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
    #[cfg(target_os = "android")]
    if kind == Kind::Android {
        return android::open(&s.file.to_string_lossy());
    }
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
            let new = std::fs::read_dir(&out)
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .ok_or("no app in the update")?;
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
        Kind::Android => {}
        Kind::Deb => {
            Command::new("sh")
                .arg("-c")
                .arg("pkexec apt-get install -y --allow-downgrades \"$1\" && exec /usr/bin/playlibre")
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

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
mod android {
    use jni::objects::{JObject, JString, JValue};
    use jni::{JNIEnv, JavaVM};

    fn with_activity<T>(
        f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>,
    ) -> Result<T, String> {
        let app = bevy::android::ANDROID_APP.get().ok_or("no app")?;
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(|e| e.to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let result = f(&mut env, &activity);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        result.map_err(|e| e.to_string())
    }

    pub fn from_play() -> bool {
        with_activity(|env, activity| {
            let manager = env
                .call_method(activity, "getPackageManager", "()Landroid/content/pm/PackageManager;", &[])?
                .l()?;
            let name = env.call_method(activity, "getPackageName", "()Ljava/lang/String;", &[])?.l()?;
            let installer = env
                .call_method(
                    &manager,
                    "getInstallerPackageName",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&name)],
                )?
                .l()?;
            if installer.is_null() {
                return Ok(false);
            }
            let installer: String = env.get_string(&JString::from(installer))?.into();
            Ok(installer == "com.android.vending")
        })
        .unwrap_or(false)
    }

    pub fn open(url: &str) -> Result<(), String> {
        with_activity(|env, activity| {
            let url = env.new_string(url)?;
            let uri = env
                .call_static_method(
                    "android/net/Uri",
                    "parse",
                    "(Ljava/lang/String;)Landroid/net/Uri;",
                    &[JValue::Object(&url)],
                )?
                .l()?;
            let action = env.new_string("android.intent.action.VIEW")?;
            let intent = env.new_object(
                "android/content/Intent",
                "(Ljava/lang/String;Landroid/net/Uri;)V",
                &[JValue::Object(&action), JValue::Object(&uri)],
            )?;
            env.call_method(
                activity,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[JValue::Object(&intent)],
            )?;
            Ok(())
        })
    }
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

    /// Every kind of copy looks for a name the release workflow writes into
    /// `latest.json`, so none of them stops getting updates.
    #[test]
    fn every_kind_has_a_file_in_the_release_manifest() {
        let workflow = include_str!("../../../.github/workflows/release.yml");
        let start = workflow.find("> latest.json").expect("the workflow writes latest.json");
        let manifest = &workflow[..start];
        let manifest = &manifest[manifest.rfind("jq -n").expect("built with jq")..];
        for kind in [Kind::Windows, Kind::Mac, Kind::AppImage, Kind::Deb, Kind::Android] {
            let key = kind.key();
            assert!(
                manifest.contains(&format!("{key}: $")) || manifest.contains(&format!("\"{key}\": $")),
                "latest.json has no {key:?} entry"
            );
        }
    }
}
