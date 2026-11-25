//! Puts the Libroxel icon and version details into `libroxel.exe` on
//! Windows, so Explorer, the taskbar and shortcuts show the icon. Other
//! systems take the icon from their installers instead.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/libroxel.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        link_builtins();
    }
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/icons/libroxel.ico")
            .set("ProductName", "Libroxel")
            .set("FileDescription", "Libroxel")
            .set("CompanyName", "Libroxel")
            .set("LegalCopyright", "MIT OR Apache-2.0");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not add the icon to libroxel.exe: {e}");
        }
    }
}

fn link_builtins() {
    let target = std::env::var("TARGET").unwrap();
    let key = format!("CARGO_TARGET_{}_LINKER", target.to_uppercase().replace('-', "_"));
    println!("cargo:rerun-if-env-changed={key}");
    let Ok(linker) = std::env::var(&key) else {
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
