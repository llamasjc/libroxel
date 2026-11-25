use libroxel_hub::{
    AccountUpdate, BadgeAction, BadgeAward, BadgeChange, FriendAction, FriendChange, HubClient,
    PresenceUpdate, Publish, account_message, award_message, badge_message, friend_message, player_id_of,
    presence_message, publish_message, sign, start,
};

struct Player {
    key: [u8; 32],
    id: String,
    clock: std::cell::Cell<i64>,
}

impl Player {
    fn new(hub: &HubClient, seed: u8, name: &str) -> Self {
        let key = [seed; 32];
        let clock = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        let player = Self { key, id: player_id_of(&key), clock: std::cell::Cell::new(clock as i64) };
        let time = player.tick() / 1000;
        let look = serde_json::json!({});
        hub.update_account(&AccountUpdate {
            player_id: player.id.clone(),
            name: name.into(),
            about: String::new(),
            look: look.clone(),
            time,
            signature: sign(&key, &account_message(&player.id, name, "", &look, time)),
            work: Some(libroxel_hub::account_work(&player.id, name)),
            email: None,
        })
        .unwrap();
        player
    }

    fn tick(&self) -> i64 {
        self.clock.set(self.clock.get() + 1);
        self.clock.get()
    }

    fn play(&self, hub: &HubClient, title: &str, game_id: &str) {
        let mut update = PresenceUpdate {
            player_id: self.id.clone(),
            online: true,
            game: Some(title.into()),
            game_id: Some(game_id.into()),
            server_id: None,
            time: self.tick(),
            signature: String::new(),
        };
        update.signature = sign(&self.key, &presence_message(&update));
        hub.presence(&update).unwrap();
    }

    fn award(&self, hub: &HubClient, game_id: &str, badge_id: &str) -> Result<bool, String> {
        let time = self.tick();
        hub.award_badge(&BadgeAward {
            player_id: self.id.clone(),
            game_id: game_id.into(),
            badge_id: badge_id.into(),
            time,
            signature: sign(&self.key, &award_message(&self.id, game_id, badge_id, time)),
        })
        .map(|a| a.new)
    }

    fn change(
        &self,
        hub: &HubClient,
        game_id: &str,
        action: BadgeAction,
        badge_id: &str,
        name: &str,
    ) -> Result<Vec<libroxel_hub::Badge>, String> {
        let mut change = BadgeChange {
            player_id: self.id.clone(),
            action,
            badge_id: badge_id.into(),
            name: name.into(),
            description: "Got to the top".into(),
            icon: "trophy".into(),
            color: "yellow".into(),
            time: self.tick(),
            signature: String::new(),
        };
        change.signature = sign(&self.key, &badge_message(game_id, &change));
        hub.change_badge(game_id, &change)
    }

    fn badges(&self, hub: &HubClient) -> Vec<String> {
        hub.badges(&self.id).unwrap().into_iter().map(|b| b.badge_id).collect()
    }
}

fn publish(hub: &HubClient, key: &[u8; 32]) -> String {
    let secret = "s".repeat(20);
    hub.publish(&Publish {
        game_id: None,
        secret: secret.clone(),
        title: "Tower".into(),
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
fn platform_badges_come_from_what_players_do() {
    let options = libroxel_hub::HubOptions { no_limits: true, ..Default::default() };
    let running = libroxel_hub::start_with("127.0.0.1:0".parse().unwrap(), None, options).unwrap();
    let hub = HubClient::new(&running.url());
    let ada = Player::new(&hub, 1, "ada");
    assert_eq!(ada.badges(&hub), vec!["early-adopter"]);

    ada.play(&hub, "Kart race", "starter:Kart race");
    assert!(ada.badges(&hub).contains(&"first-steps".to_owned()));
    for n in 0..10 {
        ada.play(&hub, &format!("Game {n}"), &format!("starter:Game {n}"));
    }
    assert!(ada.badges(&hub).contains(&"explorer".to_owned()));

    let friends: Vec<Player> = (0..5).map(|n| Player::new(&hub, 10 + n, &format!("pal{n}"))).collect();
    for f in &friends {
        for (a, b) in [(&ada, f), (f, &ada)] {
            let time = a.tick();
            hub.change_friend(&FriendChange {
                player_id: a.id.clone(),
                other_id: b.id.clone(),
                action: FriendAction::Add,
                time,
                signature: sign(&a.key, &friend_message(&a.id, &b.id, FriendAction::Add, time)),
            })
            .unwrap();
        }
    }
    assert!(ada.badges(&hub).contains(&"social-butterfly".to_owned()));
    assert!(!friends[0].badges(&hub).contains(&"social-butterfly".to_owned()));

    publish(&hub, &ada.key);
    assert!(ada.badges(&hub).contains(&"creator".to_owned()));
}

#[test]
fn chess_master_comes_from_chess() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let ada = Player::new(&hub, 1, "ada");
    assert!(ada.award(&hub, "starter:Chess", "chess-master").is_err(), "not playing");
    ada.play(&hub, "Chess", "starter:Chess");
    assert!(ada.award(&hub, "starter:Kart race", "chess-master").is_err(), "another game");
    assert_eq!(ada.award(&hub, "starter:Chess", "chess-master"), Ok(true));
    assert_eq!(ada.award(&hub, "starter:Chess", "chess-master"), Ok(false));
    assert!(ada.badges(&hub).contains(&"chess-master".to_owned()));
    assert!(ada.award(&hub, "starter:Chess", "supporter").is_err());
    let t = ada.tick();
    let wallet = hub.wallet(&ada.id, t, &sign(&ada.key, &libroxel_hub::wallet_message(&ada.id, t))).unwrap();
    assert_eq!(wallet.balance, 3 * libroxel_hub::BADGE_SPARKS as i64);
}

#[test]
fn games_make_and_award_their_own_badges() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let ada = Player::new(&hub, 1, "ada");
    let bo = Player::new(&hub, 2, "bob");
    let game = publish(&hub, &ada.key);

    assert!(bo.change(&hub, &game, BadgeAction::Create, "", "Top").is_err(), "not their game");
    let list = ada.change(&hub, &game, BadgeAction::Create, "", "Top").unwrap();
    assert_eq!(list.len(), 1);
    let badge = list[0].badge_id.clone();
    assert_eq!(list[0].game_title.as_deref(), Some("Tower"));

    bo.play(&hub, "Tower", &game);
    assert_eq!(bo.award(&hub, &game, &badge), Ok(true));
    assert_eq!(bo.award(&hub, &game, &badge), Ok(false));
    assert!(bo.badges(&hub).contains(&badge));
    assert_eq!(hub.game_badges(&game).unwrap()[0].earned_by, 1);

    let list = ada.change(&hub, &game, BadgeAction::Update, &badge, "Summit").unwrap();
    assert_eq!(list[0].name, "Summit");
    let list = ada.change(&hub, &game, BadgeAction::Create, "", "fuck you").unwrap();
    assert!(!list[1].name.contains("uck"));
    ada.change(&hub, &game, BadgeAction::Remove, &badge, "").unwrap();
    assert_eq!(hub.game_badges(&game).unwrap().len(), 1);
    assert!(!bo.badges(&hub).contains(&badge));
}
