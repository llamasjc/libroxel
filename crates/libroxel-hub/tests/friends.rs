//! Friends, presence and per-game server lists on a Hub on this computer.

use libroxel_hub::{
    AccountUpdate, FriendAction, FriendChange, FriendState, Heartbeat, HostKind, HubClient, PresenceUpdate,
    account_message, friend_message, friends_list_message, new_secret, player_id_of, presence_message, sign,
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

    /// A time newer than the last one this player signed.
    fn tick(&self) -> i64 {
        self.clock.set(self.clock.get() + 1);
        self.clock.get()
    }

    fn claim(&self, hub: &HubClient, name: &str) {
        let time = self.tick() / 1000;
        let look = serde_json::json!({});
        let message = account_message(&self.id, name, "", &look, time);
        hub.update_account(&AccountUpdate {
            player_id: self.id.clone(),
            name: name.into(),
            about: String::new(),
            look,
            time,
            signature: sign(&self.key, &message),
            work: Some(libroxel_hub::account_work(&self.id, name)),
            email: None,
        })
        .unwrap();
    }

    fn friend(&self, hub: &HubClient, other: &Player, action: FriendAction) -> Result<FriendState, String> {
        let time = self.tick();
        hub.change_friend(&FriendChange {
            player_id: self.id.clone(),
            other_id: other.id.clone(),
            action,
            time,
            signature: sign(&self.key, &friend_message(&self.id, &other.id, action, time)),
        })
    }

    fn friends(&self, hub: &HubClient) -> libroxel_hub::Friends {
        let time = self.tick();
        hub.friends(&self.id, time, &sign(&self.key, &friends_list_message(&self.id, time))).unwrap()
    }

    fn presence(&self, hub: &HubClient, online: bool, server: Option<&str>) {
        let mut update = PresenceUpdate {
            player_id: self.id.clone(),
            online,
            game: server.map(|_| "Kart race".into()),
            game_id: server.map(|_| "starter:Kart race".into()),
            server_id: server.map(str::to_owned),
            time: self.tick(),
            signature: String::new(),
        };
        update.signature = sign(&self.key, &presence_message(&update));
        hub.presence(&update).unwrap();
    }
}

#[test]
fn friends_ask_accept_see_each_other_and_join() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (alice, ana, bo) = (Player::new(1), Player::new(2), Player::new(3));
    alice.claim(&hub, "Alice");
    ana.claim(&hub, "Ana");

    // Bo has no name yet, so can't send requests.
    assert!(bo.friend(&hub, &alice, FriendAction::Add).unwrap_err().contains("claim a name"));

    // Alice asks Ana; Ana sees it waiting and accepts by adding back.
    assert_eq!(alice.friend(&hub, &ana, FriendAction::Add).unwrap(), FriendState::Requested);
    assert_eq!(alice.friends(&hub).outgoing[0].name, "Ana");
    let waiting = ana.friends(&hub);
    assert_eq!(waiting.incoming.len(), 1);
    assert_eq!(waiting.incoming[0].name, "Alice");
    assert!(waiting.friends.is_empty());
    assert_eq!(ana.friend(&hub, &alice, FriendAction::Add).unwrap(), FriendState::Friends);

    // Offline until Ana says she's online.
    let list = alice.friends(&hub);
    assert!(list.incoming.is_empty() && list.outgoing.is_empty());
    assert_eq!(list.friends.len(), 1);
    assert!(!list.friends[0].online);
    assert_eq!(list.friends[0].last_online_secs_ago, None);

    // Ana hosts a Kart race server and says she's in it.
    let secret = new_secret();
    hub.heartbeat(&Heartbeat {
        server_id: ana.id.clone(),
        secret: secret.clone(),
        name: "Ana's game".into(),
        game: "Kart race".into(),
        code: "lrx1anascode".into(),
        players: 1,
        max_players: 8,
        kind: HostKind::Player,
        game_id: Some("starter:Kart race".into()),
        host_id: None,
        host_signature: None,
    })
    .unwrap();
    ana.presence(&hub, true, Some(&ana.id));
    let friend = &alice.friends(&hub).friends[0];
    assert!(friend.online);
    assert_eq!(friend.game.as_deref(), Some("Kart race"));
    let server = friend.server.as_ref().expect("her server is listed");
    assert_eq!(server.code, "lrx1anascode");
    assert_eq!(server.game_id.as_deref(), Some("starter:Kart race"));

    // Her server is on the Kart race list and no other.
    assert_eq!(hub.game_servers("starter:Kart race").unwrap().len(), 1);
    assert!(hub.game_servers("starter:Sample obby").unwrap().is_empty());

    // Signing off shows her offline, with when she was last on.
    ana.presence(&hub, false, None);
    let friend = &alice.friends(&hub).friends[0];
    assert!(!friend.online && friend.server.is_none() && friend.game.is_none());
    assert!(friend.last_online_secs_ago.is_some());

    // Only Alice can read Alice's list.
    let time = ana.tick();
    let forged = sign(&ana.key, &friends_list_message(&alice.id, time));
    assert!(hub.friends(&alice.id, time, &forged).is_err());

    // An old change can't be replayed.
    let old = FriendChange {
        player_id: alice.id.clone(),
        other_id: ana.id.clone(),
        action: FriendAction::Remove,
        time: 5,
        signature: sign(&alice.key, &friend_message(&alice.id, &ana.id, FriendAction::Remove, 5)),
    };
    assert!(hub.change_friend(&old).is_err());

    // Unfriending ends it for both.
    assert_eq!(ana.friend(&hub, &alice, FriendAction::Remove).unwrap(), FriendState::None);
    assert!(alice.friends(&hub).friends.is_empty());
    assert!(ana.friends(&hub).friends.is_empty());
}

#[test]
fn published_games_count_their_players() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let published = hub
        .publish(&libroxel_hub::Publish {
            game_id: None,
            secret: new_secret(),
            title: "Tower".into(),
            author: "Alice".into(),
            description: String::new(),
            place: "{}".into(),
            author_id: None,
            author_signature: None,
        })
        .unwrap();
    for (i, players) in [3, 4].into_iter().enumerate() {
        hub.heartbeat(&Heartbeat {
            server_id: format!("s{i}"),
            secret: new_secret(),
            name: format!("Server {i}"),
            game: "Tower".into(),
            code: format!("lrx1s{i}"),
            players,
            max_players: 10,
            kind: HostKind::Community,
            game_id: Some(published.game_id.clone()),
            host_id: None,
            host_signature: None,
        })
        .unwrap();
    }
    let game = &hub.games("").unwrap()[0];
    assert_eq!((game.playing, game.servers), (7, 2));
}
