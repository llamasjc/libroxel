//! How much of each script the shield renames, and how long it takes.
//!
//! `cargo run --release -p playlibre-script --example shield_bench -- <files or folders of .lua/.luau>`

use std::path::{Path, PathBuf};
use std::time::Instant;

use playlibre_script::shield::{Shielded, shield};

fn scripts(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
            scripts(&entry.path(), out);
        }
    } else if path.extension().is_some_and(|e| e == "lua" || e == "luau") {
        out.push(path.to_owned());
    }
}

fn main() {
    let mut files = Vec::new();
    for arg in std::env::args().skip(1) {
        scripts(Path::new(&arg), &mut files);
    }
    let (mut renamed, mut stripped, mut as_written, mut bytes_in, mut bytes_out) = (0, 0, 0, 0, 0);
    let started = Instant::now();
    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else { continue };
        let (out, how) = shield(&source);
        bytes_in += source.len();
        bytes_out += out.len();
        match how {
            Shielded::Renamed => renamed += 1,
            Shielded::Stripped => {
                stripped += 1;
                println!("only stripped: {}", file.display());
            }
            Shielded::AsWritten => {
                as_written += 1;
                println!("as written: {}", file.display());
            }
        }
    }
    let first = started.elapsed();
    let again = Instant::now();
    for file in &files {
        if let Ok(source) = std::fs::read_to_string(file) {
            shield(&source);
        }
    }
    println!(
        "{} scripts, {:.0} KB -> {:.0} KB: {renamed} renamed, {stripped} only stripped, {as_written} as written",
        files.len(),
        bytes_in as f64 / 1024.0,
        bytes_out as f64 / 1024.0
    );
    println!(
        "first time {:.1} ms ({:.1} us per KB), again (cached) {:.2} ms",
        first.as_secs_f64() * 1000.0,
        first.as_secs_f64() * 1e6 / (bytes_in as f64 / 1024.0),
        again.elapsed().as_secs_f64() * 1000.0
    );
}
