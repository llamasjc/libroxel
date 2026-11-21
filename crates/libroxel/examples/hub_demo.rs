//! A Hub full of pretend friends and servers, to try the Home and Friends
//! tabs on one computer:
//!
//! ```sh
//! cargo run -p libroxel --example hub_demo        # leave it running
//! LIBROXEL_HUB=127.0.0.1:7431 cargo run -p libroxel
//! ```
//!
//! It claims the name "AliceBuilds" for the identity on this computer (the
//! one the app uses), makes five friends (some in games, one online, one
//! offline), leaves a friend request waiting, and lists servers for the
//! starter games and one published game. The servers answer pings but run
//! no game, so joining them goes nowhere.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use libroxel_hub::{
    AccountUpdate, FriendAction, FriendChange, Heartbeat, HostKind, HubClient, PresenceUpdate, Publish,
};
use libroxel_net::{HostTunnel, Identity, Network};
use serde_json::json;

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

struct Person {
    me: Identity,
    name: &'static str,
}

impl Person {
    fn claim(&self, hub: &HubClient, look: serde_json::Value) {
        let time = now_ms() / 1000;
        let id = self.me.player_id();
        let message = libroxel_hub::account_message(&id, self.name, "", &look, time);
        let account = AccountUpdate {
            work: Some(libroxel_hub::account_work(&id, self.name)),
            email: None,
            player_id: id,
            name: self.name.into(),
            about: String::new(),
            look,
            time,
            signature: self.me.sign(&message),
        };
        if let Err(e) = hub.update_account(&account) {
            println!("{}: {e}", self.name);
        }
    }

    fn add(&self, hub: &HubClient, other: &Person) {
        std::thread::sleep(Duration::from_millis(3));
        let time = now_ms();
        let (id, other_id) = (self.me.player_id(), other.me.player_id());
        let signature = self.me.sign(&libroxel_hub::friend_message(&id, &other_id, FriendAction::Add, time));
        let change = FriendChange { player_id: id, other_id, action: FriendAction::Add, time, signature };
        if let Err(e) = hub.change_friend(&change) {
            println!("{} → {}: {e}", self.name, other.name);
        }
    }

    fn presence(&self, hub: &HubClient, online: bool, game: Option<(&str, &str)>, server: Option<String>) {
        let mut update = PresenceUpdate {
            player_id: self.me.player_id(),
            online,
            game: game.map(|g| g.0.to_owned()),
            game_id: game.map(|g| g.1.to_owned()),
            server_id: server,
            time: now_ms(),
            signature: String::new(),
        };
        update.signature = self.me.sign(&libroxel_hub::presence_message(&update));
        let _ = hub.presence(&update);
    }
}

fn look(skin: [u8; 3], shirt: [u8; 3], hat: &str) -> serde_json::Value {
    json!({ "skin": skin, "shirt": shirt, "pants": [40, 44, 60], "hat": hat, "hat_color": [240, 200, 60] })
}

fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(7431);
    let data = std::env::temp_dir().join(format!("libroxel-hub-demo-{port}.sqlite"));
    let _ = std::fs::remove_file(&data);
    let running = libroxel_hub::start_with(
        ([0, 0, 0, 0], port).into(),
        Some(&data),
        // Every pretend player signs up from this one computer.
        libroxel_hub::HubOptions { no_limits: true, ..Default::default() },
    )
    .expect("the Hub starts");
    let hub = HubClient::new(&format!("127.0.0.1:{port}"));
    println!("Demo Hub on {}", running.url());

    // The player on this computer, as the app finds them.
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(|d| PathBuf::from(d).join("libroxel"))
        .or_else(|| std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".config/libroxel")))
        .unwrap_or_else(|| PathBuf::from(".libroxel"));
    let you =
        Person { me: Identity::load_or_create(&config.join("identity.key")).unwrap(), name: "AliceBuilds" };
    you.claim(&hub, look([255, 204, 153], [255, 106, 43], "Cap"));

    let person = |name| Person { me: Identity::generate(), name };
    let friends = [
        person("AnaRacer"),
        person("BoJumps"),
        person("CyBuilds"),
        person("DeeDev"),
        person("EliOnline"),
        person("GusAway"),
    ];
    let looks = [
        look([255, 220, 180], [124, 92, 214], "Crown"),
        look([141, 85, 36], [32, 168, 150], "None"),
        look([224, 172, 105], [242, 170, 40], "Cap"),
        look([198, 134, 66], [226, 86, 150], "Top hat"),
        look([255, 219, 172], [70, 150, 230], "None"),
        look([241, 194, 125], [120, 120, 120], "None"),
    ];
    for (friend, look) in friends.iter().zip(looks) {
        friend.claim(&hub, look);
        friend.add(&hub, &you);
        you.add(&hub, friend);
    }
    let fay = person("FayAsks");
    fay.claim(&hub, look([255, 230, 200], [236, 98, 74], "None"));
    fay.add(&hub, &you);
    // Someone to find with the search.
    person("ZoeNewHere").claim(&hub, look([255, 224, 189], [47, 212, 154], "None"));

    // A published game of Dee's.
    let tower = hub
        .publish(&Publish {
            game_id: None,
            secret: libroxel_hub::new_secret(),
            title: "Tower of Doom".into(),
            author: "DeeDev".into(),
            description: "Climb 40 floors of traps. Every floor is harder.".into(),
            place: "{}".into(),
            author_id: None,
            author_signature: None,
        })
        .expect("publishing works")
        .game_id;

    // Servers, each with a real host behind it that answers pings.
    let kart = "starter:Kart race";
    let servers: Vec<(String, &str, &str, &str, u32, u32, HostKind)> = vec![
        (friends[0].me.player_id(), "AnaRacer's game", "Kart race", kart, 4, 8, HostKind::Player),
        ("speedway".into(), "Speedway 24/7", "Kart race", kart, 11, 12, HostKind::Community),
        ("pitstop".into(), "Pit stop (no ramming)", "Kart race", kart, 12, 12, HostKind::Community),
        ("official-kart".into(), "Libroxel Kart race #1", "Kart race", kart, 2, 20, HostKind::Official),
        ("tower-1".into(), "Tower of Doom: floor 20+", "Tower of Doom", &tower, 6, 10, HostKind::Community),
        ("tower-2".into(), "DeeDev's game", "Tower of Doom", &tower, 2, 6, HostKind::Player),
    ];
    let tunnels: Vec<HostTunnel> = servers
        .iter()
        .map(|_| HostTunnel::start(&Identity::generate(), Network::LocalOnly, 1).expect("networking starts"))
        .collect();
    let codes: Vec<String> = tunnels
        .iter()
        .map(|t| {
            loop {
                if let Some(code) = t.code() {
                    break code;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        })
        .collect();
    let secrets: Vec<String> = servers.iter().map(|_| libroxel_hub::new_secret()).collect();
    println!("Ready. Run the app with LIBROXEL_HUB=127.0.0.1:{port}");
    loop {
        for (i, (id, name, game, game_id, players, max, kind)) in servers.iter().enumerate() {
            let beat = Heartbeat {
                server_id: id.clone(),
                secret: secrets[i].clone(),
                name: (*name).into(),
                game: (*game).into(),
                code: codes[i].clone(),
                players: *players,
                max_players: *max,
                kind: *kind,
                game_id: Some((*game_id).into()),
                host_id: None,
                host_signature: None,
            };
            if let Err(e) = hub.heartbeat(&beat) {
                println!("{name}: {e}");
            }
        }
        friends[0].presence(&hub, true, Some(("Kart race", kart)), Some(servers[0].0.clone()));
        friends[1].presence(&hub, true, Some(("Kart race", kart)), Some(servers[1].0.clone()));
        friends[2].presence(&hub, true, Some(("Tower of Doom", &tower)), Some(servers[4].0.clone()));
        friends[3].presence(&hub, true, Some(("Tower of Doom", &tower)), Some(servers[5].0.clone()));
        friends[4].presence(&hub, true, None, None);
        friends[5].presence(&hub, false, None, None);
        std::thread::sleep(Duration::from_secs(10));
    }
}
