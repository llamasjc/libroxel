use libroxel_hub::{
    AccountUpdate, FriendAction, FriendChange, Heartbeat, HostKind, HubClient, PartyAction, PartyChange,
    PartyView, PresenceUpdate, account_message, friend_message, party_message, party_view_message,
    player_id_of, presence_message, sign, start,
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

    fn befriend(&self, hub: &HubClient, other: &Player) {
        for (a, b) in [(self, other), (other, self)] {
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

    fn party(&self, hub: &HubClient, action: PartyAction, target: &str) -> Result<PartyView, String> {
        let time = self.tick();
        hub.change_party(&PartyChange {
            player_id: self.id.clone(),
            action,
            target: target.into(),
            time,
            signature: sign(&self.key, &party_message(&self.id, action, target, time)),
        })
    }

    fn view(&self, hub: &HubClient) -> PartyView {
        let time = self.tick();
        hub.party(&self.id, time, &sign(&self.key, &party_view_message(&self.id, time))).unwrap()
    }

    fn presence(&self, hub: &HubClient, server: &str) {
        let mut update = PresenceUpdate {
            player_id: self.id.clone(),
            online: true,
            game: Some("Kart race".into()),
            game_id: Some("starter:Kart race".into()),
            server_id: Some(server.into()),
            time: self.tick(),
            signature: String::new(),
        };
        update.signature = sign(&self.key, &presence_message(&update));
        hub.presence(&update).unwrap();
    }
}

#[test]
fn friends_party_up_and_follow_the_leader() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (alice, ana, bo) =
        (Player::new(&hub, 1, "Alice"), Player::new(&hub, 2, "Ana"), Player::new(&hub, 3, "Bob"));
    alice.befriend(&hub, &ana);

    assert!(alice.party(&hub, PartyAction::Invite, &bo.id).unwrap_err().contains("only invite friends"));
    assert!(alice.view(&hub).party.is_none());

    let sent = alice.party(&hub, PartyAction::Invite, &ana.id).unwrap();
    let party = sent.party.unwrap();
    assert_eq!(party.leader_id, alice.id);
    assert_eq!(party.invited[0].name, "Ana");

    let invites = ana.view(&hub).invites;
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0].from.name, "Alice");
    assert!(bo.party(&hub, PartyAction::Accept, &party.party_id).is_err());

    let joined = ana.party(&hub, PartyAction::Accept, &party.party_id).unwrap();
    assert!(joined.invites.is_empty());
    let members = joined.party.unwrap().members;
    assert_eq!(
        members.iter().map(|m| (m.name.as_str(), m.leader)).collect::<Vec<_>>(),
        [("Alice", true), ("Ana", false)]
    );

    hub.heartbeat(&Heartbeat {
        server_id: alice.id.clone(),
        secret: libroxel_hub::new_secret(),
        name: "Alice's game".into(),
        game: "Kart race".into(),
        code: "lrx1alice".into(),
        players: 1,
        max_players: 8,
        kind: HostKind::Player,
        game_id: Some("starter:Kart race".into()),
        host_id: None,
        host_signature: None,
    })
    .unwrap();
    alice.presence(&hub, &alice.id);
    let party = ana.view(&hub).party.unwrap();
    assert_eq!(party.leader().unwrap().server.as_ref().unwrap().code, "lrx1alice");

    let time = bo.tick();
    assert!(hub.party(&ana.id, time, &sign(&bo.key, &party_view_message(&ana.id, time))).is_err());
    assert!(ana.party(&hub, PartyAction::Remove, &alice.id).unwrap_err().contains("leader"));

    alice.party(&hub, PartyAction::Leave, "").unwrap();
    let party = ana.view(&hub).party.unwrap();
    assert_eq!(party.leader_id, ana.id);
    assert_eq!(party.members.len(), 1);
    ana.party(&hub, PartyAction::Leave, "").unwrap();
    assert!(ana.view(&hub).party.is_none());
}
