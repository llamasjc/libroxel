use playlibre_hub::{
    Publish, creator_message, creator_stats, open_db, player_id_of, publish_message, record, sign, start,
};

const DAY: i64 = 86_400;

fn publish(hub: &playlibre_hub::HubClient, key: &[u8; 32], title: &str) -> String {
    let secret = "s".repeat(20);
    hub.publish(&Publish {
        game_id: None,
        secret: secret.clone(),
        title: title.into(),
        author: String::new(),
        description: String::new(),
        place: "{}".into(),
        author_id: Some(player_id_of(key)),
        author_signature: Some(sign(key, &publish_message(&secret))),
    })
    .unwrap()
    .game_id
}

#[test]
fn counts_visits_players_playtime_and_retention() {
    let db = open_db(None).unwrap();
    let author = player_id_of(&[1; 32]);
    db.execute(
        "INSERT INTO games (game_id, secret, title, author, description, place, version, plays, created, updated, author_id)
         VALUES ('g', 'x', 'Obby', 'a', '', '{}', 1, 0, 0, 0, ?1)",
        [&author],
    )
    .unwrap();
    let start = 1000 * DAY;
    let a = player_id_of(&[2; 32]);
    let b = player_id_of(&[3; 32]);
    for t in (0..=300).step_by(15) {
        record(&db, "g", &a, start + t).unwrap();
    }
    for t in (0..=60).step_by(15) {
        record(&db, "g", &b, start + 100 + t).unwrap();
    }
    for day in [1, 7] {
        for t in (0..=30).step_by(15) {
            record(&db, "g", &a, start + day * DAY + t).unwrap();
        }
    }
    record(&db, "starter:Obby", &a, start).unwrap();

    let stats = creator_stats(&db, &author, 30, start + 8 * DAY).unwrap();
    assert_eq!(stats.games.len(), 1);
    let g = &stats.games[0];
    assert_eq!(g.visits, 4);
    assert_eq!(g.players, 2);
    assert_eq!(g.average_playtime_secs, (300 + 60 + 30 + 30) / 4);
    assert_eq!(g.day1_retention, Some(0.5));
    assert_eq!(g.day7_retention, Some(0.5));
    assert_eq!(g.peak, 2);
    let first = g.days.iter().find(|d| d.day == 1000).unwrap();
    assert_eq!((first.visits, first.players, first.new_players), (2, 2, 2));
    assert_eq!(g.days.iter().find(|d| d.day == 1001).unwrap().new_players, 0);

    let codes: Vec<String> = db
        .prepare("SELECT DISTINCT player FROM play_sessions")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(codes.len(), 2);
    assert!(codes.iter().all(|c| c != &a && c != &b && c.len() == 24));

    record(&db, "g", &b, start + 200 * DAY).unwrap();
    let left: i64 = db.query_row("SELECT COUNT(*) FROM play_sessions", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 1);
}

#[test]
fn only_the_author_sees_the_numbers() {
    let hub = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let key = [7; 32];
    let id = player_id_of(&key);
    let game = publish(&playlibre_hub::HubClient::new(&hub.url()), &key, "Mine");
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let get = |signer: &[u8; 32]| {
        ureq::get(format!("{}/creator/{id}", hub.url()))
            .query("time", time.to_string())
            .query("signature", sign(signer, &creator_message(&id, time)))
            .query("days", "7")
            .call()
    };
    let mine: serde_json::Value = get(&key).unwrap().body_mut().read_json().unwrap();
    assert_eq!(mine["games"][0]["game_id"], game);
    assert_eq!(mine["games"][0]["days"].as_array().unwrap().len(), 7);
    assert!(get(&[8; 32]).is_err());
}
