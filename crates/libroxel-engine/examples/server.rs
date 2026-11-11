//! Hosts a saved game with no window, for others to join from the editor:
//! `cargo run -p libroxel-engine --example server -- mygame.lrx [port]`.
//!
//! The game runs 60 steps a second and prints what scripts print. Saved
//! data lives in memory until the server stops.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use libroxel_engine::net::DEFAULT_PORT;
use libroxel_engine::{CharacterInput, LogLevel, PlaySession};
use libroxel_instance::place;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: server <game.lrx> [port]");
        std::process::exit(2);
    };
    let port = args.next().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("could not read {path}: {e}");
        std::process::exit(1);
    });
    let (tree, report) = place::load(&text).unwrap_or_else(|e| {
        eprintln!("could not load {path}: {e}");
        std::process::exit(1);
    });
    for warning in report.warnings {
        eprintln!("warning: {warning}");
    }
    let mut session = PlaySession::host(Rc::new(RefCell::new(tree)), port, None, false).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    println!("Hosting {path} on port {port}. Join it from the editor's Join button.");
    let frame = Duration::from_secs_f64(1.0 / 60.0);
    let mut next = Instant::now();
    loop {
        session.step(frame.as_secs_f32(), CharacterInput::default());
        for line in session.take_log() {
            let level = match line.level {
                LogLevel::Info => "",
                LogLevel::Warn => "warning: ",
                LogLevel::Error => "error: ",
            };
            println!("[{:8.2}] {level}{}", line.time, line.text);
        }
        next += frame;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
}
