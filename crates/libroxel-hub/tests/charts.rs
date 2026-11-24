use libroxel_hub::{
    HubClient, PresenceUpdate, Publish, for_you_message, new_secret, player_id_of, presence_message, sign,
    start,
};

struct Player {
    key: [u8; 32],
    id: String,
    clock: std::cell::Cell<i64>,
}

impl Player {
    fn new(seed: u8) -> Self {
        let key = [seed; 32];
        let clock = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        Self { key, id: player_id_of(&key), clock: std::cell::Cell::new(clock as i64) }
    }

    fn tick(&self) -> i64 {
        self.clock.set(self.clock.get() + 1);
        self.clock.get()
    }

    fn play(&self, hub: &HubClient, id: &str, title: &str) {
        let mut update = PresenceUpdate {
            player_id: self.id.clone(),
            online: true,
            game: Some(title.into()),
            game_id: Some(id.into()),
            server_id: None,
            time: self.tick(),
            signature: String::new(),
        };
        update.signature = sign(&self.key, &presence_message(&update));
        hub.presence(&update).unwrap();
    }

    fn for_you(&self, hub: &HubClient) -> libroxel_hub::ForYou {
        let time = self.tick();
        hub.for_you(&self.id, time, &sign(&self.key, &for_you_message(&self.id, time))).unwrap()
    }
}

fn publish(hub: &HubClient, title: &str, about: &str) -> String {
    hub.publish(&Publish {
        game_id: None,
        secret: new_secret(),
        title: title.into(),
        author: "Alice".into(),
        description: about.into(),
        place: r#"{"version":1,"root":{}}"#.into(),
        author_id: None,
        author_signature: None,
    })
    .unwrap()
    .game_id
}

#[test]
fn charts_continue_and_recommended() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let kart = publish(&hub, "Kart Racer", "Drift round the track");
    let drift = publish(&hub, "Drift Kings", "A car race in the hills");
    let chess = publish(&hub, "Chess", "The board game");
    let (ana, bo, cy) = (Player::new(1), Player::new(2), Player::new(3));

    ana.play(&hub, &kart, "Kart Racer");
    ana.play(&hub, &chess, "Chess");
    bo.play(&hub, &kart, "Kart Racer");
    bo.play(&hub, &drift, "Drift Kings");
    cy.play(&hub, &kart, "Kart Racer");

    let charts = hub.charts().unwrap();
    let chart =
        |id: &str| charts.charts.iter().find(|c| c.id == id).map(|c| c.game_ids.clone()).unwrap_or_default();
    assert_eq!(chart("trending")[0], kart);
    assert_eq!(chart("new").len(), 3);
    assert!(chart("genre:Racing").contains(&drift));
    assert!(chart("genre:Board and cards").contains(&chess));
    assert_eq!(charts.stats.iter().find(|s| s.game_id == kart).unwrap().plays_week, 3);

    let mine = ana.for_you(&hub);
    assert_eq!(mine.continue_playing[0].game_id, chess);
    assert_eq!(mine.continue_playing.len(), 2);
    assert_eq!(mine.recommended[0], drift);

    let theirs = cy.for_you(&hub);
    assert_eq!(theirs.recommended[0], drift);

    let time = bo.tick();
    assert!(hub.for_you(&ana.id, time, &sign(&bo.key, &for_you_message(&ana.id, time))).is_err());
}

#[test]
fn starter_games_are_in_top_playing() {
    let dir = std::env::temp_dir().join(format!("libroxel-charts-{}", new_secret()));
    let path = dir.join("hub.sqlite");
    let db = libroxel_hub::open_db(Some(&path)).unwrap();
    let starter = |name: &str| libroxel_hub::StarterPlace {
        name: name.into(),
        about: String::new(),
        symbol: String::new(),
        place: r#"{"format":"libroxel-place","children":[]}"#.into(),
    };
    libroxel_hub::set_official(&db, &[starter("Kart race"), starter("Chess")]).unwrap();
    drop(db);
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let hub = HubClient::new(&running.url());
    publish(&hub, "Quiet Game", "Nobody plays this");
    let ana = Player::new(1);
    ana.play(&hub, "starter:Chess", "Chess");
    let charts = hub.charts().unwrap();
    let top = &charts.charts.iter().find(|c| c.id == "top-playing").unwrap().game_ids;
    assert_eq!(top[0], "starter:Chess");
    assert!(top.contains(&"starter:Kart race".to_owned()));
    drop(running);
    let _ = std::fs::remove_dir_all(dir);
}
