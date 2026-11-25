//! `playlibre-hub`: only the Hub, for a server with no screen. It builds
//! without the game engine, and is the only program that runs or looks after the Hub.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("help" | "--help" | "-h")) {
        print!(
            "playlibre-hub: the PlayLibre Hub on its own.\n\nusage (as playlibre-hub ...):\n{}",
            playlibre_hub::HUB_HELP
        );
        return;
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(|d| PathBuf::from(d).join("playlibre"))
        .or_else(|| std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".local/share/playlibre")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hub.sqlite");
    if let Err(e) = playlibre_hub::cli(&args, &data) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
