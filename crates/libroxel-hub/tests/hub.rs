//! A Hub on this computer, with hosts announcing themselves to it.

use std::time::{Duration, Instant};

use libroxel_hub::{Announcer, Heartbeat, HostKind, HubClient, Listing, new_secret, start};

fn beat(id: &str, secret: &str, kind: HostKind, players: u32) -> Heartbeat {
    Heartbeat {
        server_id: id.into(),
        secret: secret.into(),
        name: format!("{id} server"),
        game: "Obby Tower".into(),
        code: format!("lrx1{id}"),
        players,
        max_players: 10,
        kind,
    }
}

#[test]
fn hosts_announce_and_players_find_them() {
    let hub = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let client = HubClient::new(&hub.url());
    assert_eq!(client.status().unwrap().servers, 0);

    let (a, b, c) = (new_secret(), new_secret(), new_secret());
    client.heartbeat(&beat("official", &a, HostKind::Official, 9)).unwrap();
    client.heartbeat(&beat("player", &b, HostKind::Player, 2)).unwrap();
    client.heartbeat(&beat("community", &c, HostKind::Community, 1)).unwrap();

    // Community hosts come first and official servers last.
    let list = client.servers("").unwrap();
    let ids: Vec<&str> = list.iter().map(|s| s.server_id.as_str()).collect();
    assert_eq!(ids, ["community", "player", "official"]);
    assert_eq!(list[0].code, "lrx1community");
    assert_eq!(client.status().unwrap().players, 12);

    // Someone else cannot take over a listed server.
    assert!(client.heartbeat(&beat("player", &new_secret(), HostKind::Player, 0)).is_err());
    // Updates replace the listing.
    client.heartbeat(&beat("player", &b, HostKind::Player, 5)).unwrap();
    let list = client.servers("player").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].players, 5);
    assert!(client.servers("nothing like this").unwrap().is_empty());

    client.remove("player", &b).unwrap();
    assert_eq!(client.servers("").unwrap().len(), 2);
}

#[test]
fn an_announcer_lists_a_game_once_it_has_a_code() {
    let hub = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let client = HubClient::new(&hub.url());
    let announcer = Announcer::start(
        client.clone(),
        "kai".into(),
        Listing { name: "Kai's game".into(), game: "Kart race".into(), ..Default::default() },
    );
    std::thread::sleep(Duration::from_millis(700));
    assert!(client.servers("").unwrap().is_empty(), "listed before it had a code");
    announcer.update(|l| l.code = Some("lrx1abc".into()));
    let start = Instant::now();
    while client.servers("").unwrap().is_empty() {
        assert!(start.elapsed() < Duration::from_secs(5), "never listed");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(announcer.status().unwrap().starts_with("Listed on"));
    drop(announcer);
    let start = Instant::now();
    while !client.servers("").unwrap().is_empty() {
        assert!(start.elapsed() < Duration::from_secs(5), "still listed after stopping");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn addresses_are_tidied() {
    assert_eq!(HubClient::new("192.168.1.106").url(), "http://192.168.1.106:7430");
    assert_eq!(HubClient::new("http://hub.example.com/").url(), "http://hub.example.com:7430");
    assert_eq!(HubClient::new("https://hub.example.com:443").url(), "https://hub.example.com:443");
    assert_eq!(HubClient::new("localhost:9000").url(), "http://localhost:9000");
}

#[test]
fn games_are_published_found_downloaded_and_updated() {
    use libroxel_hub::Publish;
    let hub = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let client = HubClient::new(&hub.url());
    let secret = new_secret();
    let game = |id: Option<String>, secret: &str, place: &str| Publish {
        game_id: id,
        secret: secret.into(),
        title: "Lava Tower".into(),
        author: "Kai".into(),
        description: "Climb before the lava does".into(),
        place: place.into(),
    };
    let first = client.publish(&game(None, &secret, r#"{"version":1,"root":{}}"#)).unwrap();
    assert_eq!(first.version, 1);
    assert_eq!(client.status().unwrap().games, 1);

    let list = client.games("lava").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "Lava Tower");
    assert_eq!(list[0].author, "Kai");
    assert!(client.games("kart").unwrap().is_empty());

    assert_eq!(client.download(&first.game_id).unwrap(), r#"{"version":1,"root":{}}"#);
    assert_eq!(client.games("").unwrap()[0].plays, 1);

    // A new version from whoever published it; no one else can replace it.
    let second =
        client.publish(&game(Some(first.game_id.clone()), &secret, r#"{"version":1,"root":{"x":1}}"#));
    assert_eq!(second.unwrap().version, 2);
    let stolen = client.publish(&game(Some(first.game_id.clone()), &new_secret(), "{}"));
    assert!(stolen.unwrap_err().contains("someone else"));
    assert_eq!(client.download(&first.game_id).unwrap(), r#"{"version":1,"root":{"x":1}}"#);

    // Only saved games are taken.
    assert!(client.publish(&game(None, &secret, "not a game")).is_err());
    let untitled = Publish { title: " ".into(), ..game(None, &secret, "{}") };
    assert!(client.publish(&untitled).is_err());

    client.unpublish(&first.game_id, &secret).unwrap();
    assert!(client.games("").unwrap().is_empty());
    assert!(client.download(&first.game_id).is_err());
}
