use playlibre_hub::{
    Change, DownloadProof, HubClient, KEEP_VERSIONS, ManageGame, Publish, creator_message, player_id_of,
    publish_message, sign, start,
};

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn publish(hub: &HubClient, key: &[u8; 32], game_id: Option<String>, place: &str) -> String {
    let secret = "s".repeat(20);
    hub.publish(&Publish {
        game_id,
        secret: secret.clone(),
        title: "Obby".into(),
        author: String::new(),
        description: String::new(),
        place: place.into(),
        author_id: Some(player_id_of(key)),
        author_signature: Some(sign(key, &publish_message(&secret))),
    })
    .unwrap()
    .game_id
}

fn ask(key: &[u8; 32], game_id: &str, change: Change) -> ManageGame {
    let player_id = player_id_of(key);
    let time = now_ms();
    let signature = sign(key, &playlibre_hub::manage_message(&player_id, game_id, time, &change));
    ManageGame { player_id, time, signature, change }
}

#[test]
fn settings_versions_and_remove() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let me = [1; 32];
    let them = [2; 32];
    let id = publish(&hub, &me, None, r#"{"v":1}"#);
    for v in 2..=12 {
        publish(&hub, &me, Some(id.clone()), &format!(r#"{{"v":{v}}}"#));
    }

    let settings =
        Change::Settings { title: "Tower".into(), description: "Up".into(), private: true, max_players: 500 };
    assert!(hub.manage(&id, &ask(&them, &id, settings.clone())).is_err());
    hub.manage(&id, &ask(&me, &id, settings)).unwrap();
    let game = hub.game(&id).unwrap();
    assert_eq!((game.title.as_str(), game.private, game.max_players), ("Tower", true, 100));
    assert!(hub.games("").unwrap().is_empty());
    assert!(hub.download(&id, None).is_err());

    let time = now_ms();
    let mine = player_id_of(&me);
    let signature = sign(&me, &creator_message(&mine, time));
    assert_eq!(hub.my_games(&mine, time, &signature).unwrap().len(), 1);
    let versions = hub.versions(&id, &mine, time, &signature).unwrap();
    assert_eq!(versions.len() as i64, KEEP_VERSIONS);
    assert_eq!(versions[0].version, 12);

    hub.manage(&id, &ask(&me, &id, Change::Restore { version: 5 })).unwrap();
    assert!(hub.manage(&id, &ask(&me, &id, Change::Restore { version: 1 })).is_err());
    let open = Change::Settings {
        title: "Tower".into(),
        description: String::new(),
        private: false,
        max_players: 0,
    };
    hub.manage(&id, &ask(&me, &id, open)).unwrap();
    let proof = DownloadProof::new(&id, player_id_of(&me), |m| sign(&me, m));
    assert_eq!(hub.download(&id, Some(&proof)).unwrap(), r#"{"v":5}"#);
    assert_eq!(hub.game(&id).unwrap().version, 13);

    hub.manage(&id, &ask(&me, &id, Change::Remove)).unwrap();
    assert!(hub.game(&id).is_err());
}
