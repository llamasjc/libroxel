//! Messages between friends on a Hub on this computer.

use playlibre_hub::{
    AccountUpdate, FriendAction, FriendChange, HubClient, Message, NewMessage, Report, account_message,
    friend_message, message_message, player_id_of, read_messages_message, report_message, sign, start,
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

    fn claim(&self, hub: &HubClient, name: &str) {
        let time = self.tick() / 1000;
        let look = serde_json::json!({});
        hub.update_account(&AccountUpdate {
            player_id: self.id.clone(),
            name: name.into(),
            about: String::new(),
            look: look.clone(),
            time,
            signature: sign(&self.key, &account_message(&self.id, name, "", &look, time)),
            work: Some(playlibre_hub::account_work(&self.id, name)),
            email: None,
        })
        .unwrap();
    }

    fn add(&self, hub: &HubClient, other: &Player) {
        let time = self.tick();
        hub.change_friend(&FriendChange {
            player_id: self.id.clone(),
            other_id: other.id.clone(),
            action: FriendAction::Add,
            time,
            signature: sign(&self.key, &friend_message(&self.id, &other.id, FriendAction::Add, time)),
        })
        .unwrap();
    }

    fn send(&self, hub: &HubClient, to: &Player, text: &str) -> Result<Message, String> {
        let time = self.tick();
        hub.send_message(&NewMessage {
            from_id: self.id.clone(),
            to_id: to.id.clone(),
            text: text.into(),
            time,
            signature: sign(&self.key, &message_message(&self.id, &to.id, text, time)),
        })
    }

    fn read_signature(&self) -> (i64, String) {
        let time = self.tick();
        (time, sign(&self.key, &read_messages_message(&self.id, time)))
    }
}

#[test]
fn friends_message_each_other_and_bad_words_are_masked() {
    let dir = std::env::temp_dir().join(format!("playlibre-messages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let data = dir.join("hub.sqlite");
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&data)).unwrap();
    let hub = HubClient::new(&running.url());
    let (alice, ana, bo) = (Player::new(11), Player::new(12), Player::new(13));
    alice.claim(&hub, "Alice");
    ana.claim(&hub, "Anna");
    bo.claim(&hub, "Bobby");

    assert!(alice.send(&hub, &ana, "hi").unwrap_err().contains("friends"));
    alice.add(&hub, &ana);
    ana.add(&hub, &alice);

    let hi = alice.send(&hub, &ana, "hi Ana").unwrap();
    assert_eq!(hi.text, "hi Ana");
    let rude = ana.send(&hub, &alice, "you are a shithead").unwrap();
    assert_ne!(rude.text, "you are a shithead");
    assert!(alice.send(&hub, &ana, "   ").is_err());
    assert!(alice.send(&hub, &ana, &"a".repeat(501)).is_err());
    assert!(bo.send(&hub, &ana, "hello").is_err());

    // Nobody else can sign for Alice.
    let time = alice.tick();
    let forged = NewMessage {
        from_id: alice.id.clone(),
        to_id: ana.id.clone(),
        text: "send me your password".into(),
        time,
        signature: sign(&bo.key, &message_message(&alice.id, &ana.id, "send me your password", time)),
    };
    assert!(hub.send_message(&forged).is_err());

    let (time, signature) = ana.read_signature();
    let list = hub.conversations(&ana.id, time, &signature).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].with.name, "Alice");
    assert_eq!(list[0].last.message_id, rude.message_id);
    assert_eq!(list[0].unread, 1);

    let (time, signature) = ana.read_signature();
    let talk = hub.conversation(&ana.id, &alice.id, time, &signature).unwrap();
    assert_eq!(
        talk.iter().map(|m| m.message_id.as_str()).collect::<Vec<_>>(),
        [&hi.message_id, &rude.message_id]
    );
    let (time, signature) = ana.read_signature();
    assert_eq!(hub.conversations(&ana.id, time, &signature).unwrap()[0].unread, 0);

    // Only the two of them can read it.
    let (time, signature) = bo.read_signature();
    assert!(hub.conversation(&ana.id, &alice.id, time, &signature).is_err());

    // Alice reports Ana's message; only messages sent to you can be reported.
    let report = |who: &Player, target: &str| {
        let time = who.tick();
        hub.report(&Report {
            reporter_id: who.id.clone(),
            kind: "message".into(),
            target: target.into(),
            reason: "rude".into(),
            evidence: String::new(),
            time,
            signature: sign(&who.key, &report_message(&who.id, "message", target, "rude", "", time)),
        })
    };
    assert!(report(&ana, &rude.message_id).is_err());
    report(&alice, &rude.message_id).unwrap();
    let db = playlibre_hub::open_db(Some(&data)).unwrap();
    let reported = playlibre_hub::reports(&db);
    assert_eq!(reported.len(), 1);
    assert!(reported[0].name.as_deref().unwrap().starts_with("Anna: "));
    playlibre_hub::take_down(&db, "message", &rude.message_id).unwrap();
    let (time, signature) = alice.read_signature();
    assert_eq!(hub.conversation(&alice.id, &ana.id, time, &signature).unwrap().len(), 1);
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}
