//! Hosts a saved game with no window and prints a join code that friends
//! anywhere paste into the editor's Join box:
//! `cargo run --release -p libroxel-net --example server -- mygame.lrx [port]`.
//!
//! The server keeps its identity in `server-identity.key` next to the game,
//! so its join code stays the same between restarts. Players on the same
//! network can still join with this computer's address and the port.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use libroxel_engine::net::DEFAULT_PORT;
use libroxel_engine::{CharacterInput, LogLevel, PlaySession};
use libroxel_instance::place;
use libroxel_net::{HostTunnel, Identity, Network};

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
    let key = Path::new(&path).with_file_name("server-identity.key");
    let identity = Identity::load_or_create(&key).unwrap_or_else(|e| {
        eprintln!("could not load {}: {e}", key.display());
        std::process::exit(1);
    });
    let tunnel = HostTunnel::start(&identity, Network::Internet, port).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    println!("Hosting {path} on port {port}. Getting a join code…");

    let frame = Duration::from_secs_f64(1.0 / 60.0);
    let mut next = Instant::now();
    let mut announced = false;
    loop {
        session.step(frame.as_secs_f32(), CharacterInput::default());
        if !announced {
            if let Some(code) = tunnel.code() {
                println!("Join code (paste it into the editor's Join box):\n\n{code}\n");
                announced = true;
            } else if let Some(error) = tunnel.error() {
                println!("No join code, so only this network can join: {error}");
                announced = true;
            }
        }
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
