use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use playlibre_hub::{Crash, HubClient, PING_EVERY_SECS, Ping, hub_address};

fn off_path() -> PathBuf {
    crate::server::data_dir().join("no-stats")
}

fn flag() -> &'static AtomicBool {
    static ON: OnceLock<AtomicBool> = OnceLock::new();
    ON.get_or_init(|| AtomicBool::new(!off_path().exists()))
}

pub fn enabled() -> bool {
    flag().load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    flag().store(on, Ordering::Relaxed);
    if on {
        let _ = std::fs::remove_file(off_path());
    } else {
        let _ = std::fs::create_dir_all(crate::server::data_dir());
        let _ = std::fs::write(off_path(), "");
    }
}

fn install() -> String {
    let path = crate::server::data_dir().join("install-id");
    if let Ok(id) = std::fs::read_to_string(&path)
        && !id.trim().is_empty()
    {
        return id.trim().to_owned();
    }
    let id = playlibre_hub::new_secret();
    let _ = std::fs::create_dir_all(crate::server::data_dir());
    let _ = std::fs::write(&path, &id);
    id
}

fn platform() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

pub fn start(crash: Option<&str>) {
    let crash = crash.map(crate::report::scrub);
    let _ = std::thread::Builder::new().name("stats".into()).spawn(move || {
        let hub = HubClient::new(&hub_address());
        let install = install();
        let version = crate::update::VERSION.to_owned();
        if let Some(text) = crash
            && enabled()
        {
            let _ = hub.send_crash(&Crash {
                install: install.clone(),
                version: version.clone(),
                platform: platform(),
                text,
            });
        }
        loop {
            if enabled() {
                let _ = hub.ping(&Ping {
                    install: install.clone(),
                    version: version.clone(),
                    platform: platform(),
                });
            }
            std::thread::sleep(Duration::from_secs(PING_EVERY_SECS));
        }
    });
}
