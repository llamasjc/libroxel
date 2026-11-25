//! Times a Hub with many listed servers answering many players at once:
//! `cargo run -p libroxel-hub --example hub_bench -- 200 8`
//! (servers listed, players asking at the same time).

use std::time::Instant;

use libroxel_hub::{Heartbeat, HostKind, HubClient, HubOptions, new_secret, start_with};

fn main() {
    let servers: usize = std::env::args().nth(1).and_then(|n| n.parse().ok()).unwrap_or(200);
    let players: usize = std::env::args().nth(2).and_then(|n| n.parse().ok()).unwrap_or(8);
    let dir = std::env::temp_dir().join(format!("libroxel-hub-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // (Every pretend player calls from this computer, so no rate limits.)
    let options = HubOptions { no_limits: true, ..Default::default() };
    let hub = start_with("127.0.0.1:0".parse().unwrap(), Some(&dir.join("hub.db")), options).unwrap();
    let url = hub.url().to_owned();
    let client = HubClient::new(&url);
    let t = Instant::now();
    for i in 0..servers {
        let beat = Heartbeat {
            server_id: format!("server{i}"),
            secret: new_secret(),
            name: format!("Server {i}"),
            game: format!("Game {}", i % 17),
            code: format!("lrx1code{i}"),
            players: (i % 10) as u32,
            max_players: 10,
            kind: HostKind::Player,
            game_id: None,
            host_id: None,
            host_signature: None,
        };
        client.heartbeat(&beat).unwrap();
    }
    println!("{servers} heartbeats: {:.0} us each", t.elapsed().as_secs_f64() * 1e6 / servers as f64);
    let asks = 200;
    let t = Instant::now();
    std::thread::scope(|s| {
        for p in 0..players {
            let url = url.clone();
            s.spawn(move || {
                let client = HubClient::new(&url);
                for i in 0..asks {
                    let search = if (i + p) % 2 == 0 { "" } else { "Game 3" };
                    client.servers(search).unwrap();
                }
            });
        }
    });
    let total = (asks * players) as f64;
    let secs = t.elapsed().as_secs_f64();
    println!(
        "{players} players listing servers: {:.0} lists a second ({:.0} us each)",
        total / secs,
        secs * 1e6 / total
    );
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}
