//! Games with more than one place, and private and reserved servers.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use playlibre_hub::{
    AccountUpdate, DownloadProof, FleetOptions, FriendAction, FriendChange, HubClient, HubOptions,
    PlaceChange, PlayRequest, PrivateAction, PrivateServerChange, Publish, account_message, friend_message,
    place_download_id, place_message, play_message, player_id_of, private_server_message,
    private_servers_message, publish_message, sign, start_with,
};

fn ms() -> i64 {
    std::thread::sleep(Duration::from_millis(2));
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn claim(hub: &HubClient, key: &[u8; 32], name: &str) -> String {
    let id = player_id_of(key);
    let time = ms() / 1000;
    let look = serde_json::json!({});
    hub.update_account(&AccountUpdate {
        work: Some(playlibre_hub::account_work(&id, name)),
        email: None,
        signature: sign(key, &account_message(&id, name, "", &look, time)),
        player_id: id.clone(),
        name: name.into(),
        about: String::new(),
        look,
        time,
    })
    .unwrap();
    id
}

fn befriend(hub: &HubClient, a: &[u8; 32], b: &[u8; 32]) {
    for (from, to) in [(a, b), (b, a)] {
        let (me, other, time) = (player_id_of(from), player_id_of(to), ms());
        hub.change_friend(&FriendChange {
            signature: sign(from, &friend_message(&me, &other, FriendAction::Add, time)),
            player_id: me,
            other_id: other,
            action: FriendAction::Add,
            time,
        })
        .unwrap();
    }
}

fn stand_in(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let program = dir.join("fake-playlibre");
    std::fs::write(
        &program,
        format!("#!/bin/sh\n(printf '%s\\t' \"$@\"; echo) >> {}/calls\nexec sleep 20\n", dir.display()),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program
}

fn calls(dir: &Path) -> Vec<Vec<String>> {
    for _ in 0..100 {
        let text = std::fs::read_to_string(dir.join("calls")).unwrap_or_default();
        if !text.is_empty() {
            return text
                .lines()
                .map(|l| l.split('\t').filter(|a| !a.is_empty()).map(str::to_owned).collect())
                .collect();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Vec::new()
}

fn play(
    hub: &HubClient,
    key: &[u8; 32],
    request: PlayRequest,
) -> Result<Option<playlibre_hub::PlayAnswer>, String> {
    let id = player_id_of(key);
    let signature = sign(key, &play_message(&id, &request.ticket));
    hub.play(&PlayRequest { player_id: Some(id), signature: Some(signature), ..request })
}

#[test]
fn places_and_servers_of_their_own() {
    let dir = std::env::temp_dir().join(format!("playlibre-places-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fleet = FleetOptions { max_servers: 4, ..FleetOptions::new(stand_in(&dir), dir.join("servers")) };
    let options = HubOptions { no_limits: true, fleet: Some(fleet), ..Default::default() };
    let running = start_with("127.0.0.1:0".parse().unwrap(), None, options).unwrap();
    let hub = HubClient::new(&running.url());
    let (maker, friend, stranger) = ([41u8; 32], [42u8; 32], [43u8; 32]);
    let maker_id = claim(&hub, &maker, "Maker");
    claim(&hub, &friend, "Friend");
    claim(&hub, &stranger, "Stranger");
    befriend(&hub, &maker, &friend);

    let secret = "s".repeat(20);
    let game = hub
        .publish(&Publish {
            game_id: None,
            secret: secret.clone(),
            title: "Tower".into(),
            author: String::new(),
            description: String::new(),
            place: r#"{"start":1}"#.into(),
            author_id: Some(maker_id.clone()),
            author_signature: Some(sign(&maker, &publish_message(&secret))),
        })
        .unwrap()
        .game_id;

    // Every game has its start place; its creator adds another.
    let start = hub.places(&game).unwrap();
    assert_eq!(start.len(), 1);
    assert!(start[0].start);
    let mut add = PlaceChange {
        player_id: maker_id.clone(),
        time: ms(),
        place_id: 0,
        name: "Level 2".into(),
        place: Some(r#"{"level":2}"#.into()),
        signature: String::new(),
    };
    add.signature = sign(&maker, &place_message(&maker_id, &game, &add));
    let places = hub.change_place(&game, &add).unwrap();
    assert_eq!(places.len(), 2);
    let level = places[1].place_id;
    assert_eq!(hub.place(level).unwrap().game_id, game);

    // Someone else can't add places to it.
    let mut theirs = PlaceChange { player_id: player_id_of(&stranger), time: ms(), ..add.clone() };
    theirs.signature = sign(&stranger, &place_message(&theirs.player_id, &game, &theirs));
    assert!(hub.change_place(&game, &theirs).is_err());

    // Only its creator (and the Hub's servers) download a place.
    let proof = DownloadProof::new(&place_download_id(level), maker_id.clone(), |m| sign(&maker, m));
    assert_eq!(hub.download_place(level, Some(&proof)).unwrap(), r#"{"level":2}"#);
    assert!(hub.download_place(level, None).is_err());

    // A private server: its owner's friends play there, strangers don't.
    let time = ms();
    let mut make = PrivateServerChange {
        player_id: maker_id.clone(),
        action: PrivateAction::Create,
        game_id: game.clone(),
        name: "Our tower".into(),
        code: String::new(),
        time,
        signature: String::new(),
    };
    make.signature = sign(&maker, &private_server_message(&make));
    let mine = hub.change_private_server(&make).unwrap();
    assert_eq!(mine.len(), 1);
    let code = mine[0].code.clone();
    let friend_id = player_id_of(&friend);
    let time = ms();
    let seen = hub
        .private_servers(&friend_id, time, &sign(&friend, &private_servers_message(&friend_id, time)))
        .unwrap();
    assert_eq!(seen[0].code, code);

    let ask = |ticket: &str| PlayRequest {
        game_id: game.clone(),
        ticket: ticket.into(),
        access_code: Some(code.clone()),
        ..Default::default()
    };
    assert!(play(&hub, &stranger, ask("ticketstranger")).is_err());
    assert!(play(&hub, &friend, ask("ticketfriend")).unwrap().unwrap().wait);
    let started = calls(&dir);
    assert_eq!(started.len(), 1);
    let call = &started[0];
    assert_eq!(call[0], "server");
    let flag = |name: &str| call[call.iter().position(|a| a == name).unwrap() + 1].clone();
    assert_eq!(flag("--access-code"), code);
    assert_eq!(flag("--private-owner"), maker_id);
    assert_eq!(flag("--game"), game);

    // A reserved server of another place: anyone sent with its code.
    let reserved = PlayRequest {
        game_id: game.clone(),
        ticket: "ticketreserved".into(),
        place_id: Some(level),
        access_code: Some("reservedcode1234".into()),
        ..Default::default()
    };
    assert!(play(&hub, &stranger, reserved).unwrap().unwrap().wait);
    let wrong_place = PlayRequest {
        game_id: game.clone(),
        ticket: "ticketwrongplace".into(),
        place_id: Some(level + 1000),
        ..Default::default()
    };
    assert!(play(&hub, &stranger, wrong_place).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
