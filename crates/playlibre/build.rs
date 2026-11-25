//! Puts the PlayLibre icon and version details into `playlibre.exe` on
//! Windows, so Explorer, the taskbar and shortcuts show the icon. Other
//! systems take the icon from their installers instead.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/playlibre.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        link_builtins();
    }
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/icons/playlibre.ico")
            .set("ProductName", "PlayLibre")
            .set("FileDescription", "PlayLibre")
            .set("CompanyName", "PlayLibre")
            .set("LegalCopyright", "MIT OR Apache-2.0");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not add the icon to playlibre.exe: {e}");
        }
    }
}

fn link_builtins() {
    let target = std::env::var("TARGET").unwrap();
    let key = format!("CARGO_TARGET_{}_LINKER", target.to_uppercase().replace('-', "_"));
    println!("cargo:rerun-if-env-changed={key}");
    let cc = format!("CC_{}", target.replace('-', "_"));
    println!("cargo:rerun-if-env-changed={cc}");
    // Without the builtins the library names `__clear_cache`, which phones
    // don't have, and the app closes the moment it starts.
    let Some(linker) = std::env::var(&key).ok().or_else(|| std::env::var(&cc).ok()) else {
        println!(
            "cargo:warning=neither {key} nor {cc} is set, so the compiler-rt builtins aren't linked \
             and the Android app may not start; packaging/ci/check-android-lib.sh will tell"
        );
        return;
    };
    let out = std::process::Command::new(linker)
        .arg("-print-libgcc-file-name")
        .output()
        .expect("run the Android linker");
    let path = String::from_utf8(out.stdout).unwrap();
    let path = path.trim();
    assert!(std::path::Path::new(path).exists(), "no compiler-rt builtins at {path}");
    println!("cargo:rustc-link-arg={path}");
}
