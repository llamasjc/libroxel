//! Puts the Libroxel icon and version details into `libroxel.exe` on
//! Windows, so Explorer, the taskbar and shortcuts show the icon. Other
//! systems take the icon from their installers instead.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/libroxel.ico");
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
