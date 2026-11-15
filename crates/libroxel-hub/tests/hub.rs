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
        game_id: None,
        host_id: None,
        host_signature: None,
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
        author_id: None,
        author_signature: None,
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

#[test]
fn players_claim_names_and_their_signed_games_show_them() {
    use libroxel_hub::{AccountUpdate, Publish, account_message, player_id_of, publish_message, sign};
    let hub = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let client = HubClient::new(&hub.url());
    let (kai_key, ana_key) = ([7u8; 32], [9u8; 32]);
    let (kai, ana) = (player_id_of(&kai_key), player_id_of(&ana_key));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let account = |key: &[u8; 32], id: &str, name: &str, time: i64| {
        let look = serde_json::json!({ "hat": "Crown" });
        AccountUpdate {
            player_id: id.into(),
            name: name.into(),
            about: "I build obbies".into(),
            signature: sign(key, &account_message(id, name, "I build obbies", &look, time)),
            look,
            time,
        }
    };
    client.update_account(&account(&kai_key, &kai, "Kai", now)).unwrap();
    // Names are one per Hub, whatever the case, and checked.
    assert!(client.update_account(&account(&ana_key, &ana, "kai", now)).unwrap_err().contains("already"));
    assert!(client.update_account(&account(&ana_key, &ana, "a b", now)).is_err());
    // No one can change someone else's account, or bring back an old change.
    let mut forged = account(&ana_key, &kai, "Kai2", now + 1);
    forged.signature = sign(&ana_key, b"anything");
    assert!(client.update_account(&forged).is_err());
    assert!(client.update_account(&account(&kai_key, &kai, "Kai", now)).is_err());
    client.update_account(&account(&kai_key, &kai, "KaiBuilds", now + 1)).unwrap();

    // A game signed by Kai shows his account name, and his page lists it.
    let secret = new_secret();
    let game = Publish {
        game_id: None,
        secret: secret.clone(),
        title: "Lava Tower".into(),
        author: "whatever".into(),
        description: String::new(),
        place: "{}".into(),
        author_id: Some(kai.clone()),
        author_signature: Some(sign(&kai_key, &publish_message(&secret))),
    };
    client.publish(&game).unwrap();
    let listed = &client.games("KaiBuilds").unwrap()[0];
    assert_eq!(listed.author, "KaiBuilds");
    assert_eq!(listed.author_id.as_deref(), Some(kai.as_str()));
    let page = client.profile("kaibuilds").unwrap().expect("found by name");
    assert_eq!(page.player_id, kai);
    assert_eq!(page.look["hat"], "Crown");
    assert_eq!(page.games.len(), 1);
    assert_eq!(client.people("kai").unwrap().len(), 1);
    assert!(client.profile("nobody").unwrap().is_none());

    // Claiming to be Kai without his key doesn't work.
    let other = new_secret();
    let fake = Publish {
        secret: other.clone(),
        author_signature: Some(sign(&ana_key, &publish_message(&other))),
        ..game
    };
    assert!(client.publish(&fake).is_err());
}

#[test]
fn creators_share_hats_and_others_find_and_wear_them() {
    use libroxel_hub::{NewItem, item_message, player_id_of, sign, unlist_message};
    let hub = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let client = HubClient::new(&hub.url());
    let key = [3u8; 32];
    let me = player_id_of(&key);
    let hat = serde_json::json!({ "name": "Top hat", "parts": [{ "size": [1, 1, 1], "at": [0, 1, 0], "color": [0, 0, 0] }] });
    let item = |name: &str, data: serde_json::Value, signer: &[u8; 32]| NewItem {
        creator_id: me.clone(),
        name: name.into(),
        kind: "hat".into(),
        signature: sign(signer, &item_message(&me, name, "hat", &data, 5)),
        data,
        time: 5,
    };
    let shared = client.share_item(&item("Top hat", hat.clone(), &key)).unwrap();
    assert_eq!(shared.creator, "Someone");
    // Signed by someone else, or not a hat: turned away.
    assert!(client.share_item(&item("Fake", hat.clone(), &[4u8; 32])).is_err());
    assert!(client.share_item(&item("Shoe", serde_json::json!({ "laces": 2 }), &key)).is_err());

    let found = client.catalog("top").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].data, hat);
    client.wore(&shared.item_id).unwrap();
    assert_eq!(client.catalog("").unwrap()[0].wears, 1);

    // Only the creator takes it out.
    assert!(client.unlist(&shared.item_id, &sign(&[4u8; 32], &unlist_message(&shared.item_id))).is_err());
    client.unlist(&shared.item_id, &sign(&key, &unlist_message(&shared.item_id))).unwrap();
    assert!(client.catalog("").unwrap().is_empty());
}

#[test]
fn reported_games_drop_off_until_whoever_runs_the_hub_clears_them() {
    use libroxel_hub::{HIDE_AFTER_REPORTS, Publish, Report, open_db, player_id_of, report_message, sign};
    let dir = std::env::temp_dir().join(format!("libroxel-hub-reports-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let db_path = dir.join("hub.sqlite");
    let hub = start("127.0.0.1:0".parse().unwrap(), Some(&db_path)).unwrap();
    let client = HubClient::new(&hub.url());
    let game = Publish {
        game_id: None,
        secret: new_secret(),
        title: "Rude game".into(),
        author: "x".into(),
        description: String::new(),
        place: "{}".into(),
        author_id: None,
        author_signature: None,
    };
    let id = client.publish(&game).unwrap().game_id;
    let report = |key: u8, reason: &str| {
        let secret = [key; 32];
        let me = player_id_of(&secret);
        Report {
            signature: sign(&secret, &report_message(&me, "game", &id, reason, 1)),
            reporter_id: me,
            kind: "game".into(),
            target: id.clone(),
            reason: reason.into(),
            time: 1,
        }
    };
    // The same player reporting twice counts once.
    client.report(&report(1, "rude")).unwrap();
    client.report(&report(1, "very rude")).unwrap();
    assert_eq!(client.games("").unwrap().len(), 1);
    // A forged report doesn't count.
    let mut forged = report(2, "rude");
    forged.reporter_id = player_id_of(&[9; 32]);
    assert!(client.report(&forged).is_err());
    for key in 2..=HIDE_AFTER_REPORTS as u8 {
        client.report(&report(key, "rude")).unwrap();
    }
    assert!(client.games("").unwrap().is_empty(), "hidden once enough players report it");
    assert!(client.download(&id).is_err());

    // Whoever runs the Hub sees why, and can put it back.
    let db = open_db(Some(&db_path)).unwrap();
    let reported = libroxel_hub::reports(&db);
    assert_eq!(reported.len(), 1);
    assert!(reported[0].hidden);
    assert_eq!(reported[0].name.as_deref(), Some("Rude game"));
    assert!(reported[0].reasons.contains(&"very rude".to_owned()));
    libroxel_hub::clear_reports(&db, "game", &id).unwrap();
    assert_eq!(client.games("").unwrap().len(), 1);
    libroxel_hub::take_down(&db, "game", &id).unwrap();
    assert!(client.games("").unwrap().is_empty());
    drop(hub);
    let _ = std::fs::remove_dir_all(&dir);
}
