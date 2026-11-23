use libroxel_hub::{
    AccountUpdate, FriendAction, FriendChange, HubClient, ModAction, ModChange, NewMessage, Report,
    account_message, add_owner, friend_message, message_message, moderate_message, moderation_message,
    open_db, player_id_of, report_message, security_message, sign, start,
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
            work: Some(libroxel_hub::account_work(&self.id, name)),
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

    fn send(&self, hub: &HubClient, to: &Player, text: &str) -> Result<libroxel_hub::Message, String> {
        let time = self.tick();
        hub.send_message(&NewMessage {
            from_id: self.id.clone(),
            to_id: to.id.clone(),
            text: text.into(),
            time,
            signature: sign(&self.key, &message_message(&self.id, &to.id, text, time)),
        })
    }

    fn report(&self, hub: &HubClient, target: &str) {
        let time = self.tick();
        hub.report(&Report {
            reporter_id: self.id.clone(),
            kind: "message".into(),
            target: target.into(),
            reason: "rude".into(),
            time,
            signature: sign(&self.key, &report_message(&self.id, "message", target, "rude", time)),
        })
        .unwrap();
    }

    fn dashboard(&self, hub: &HubClient) -> Result<libroxel_hub::Dashboard, String> {
        let time = self.tick();
        hub.dashboard(&self.id, time, &sign(&self.key, &moderation_message(&self.id, time)))
    }

    fn moderate(
        &self,
        hub: &HubClient,
        action: ModAction,
        target: &str,
        player: &str,
        hours: u32,
    ) -> Result<(), String> {
        let mut c = ModChange {
            moderator_id: self.id.clone(),
            action,
            kind: if target.is_empty() { String::new() } else { "message".into() },
            target: target.into(),
            player_id: player.into(),
            reason: "be kind".into(),
            hours,
            time: self.tick(),
            signature: String::new(),
        };
        c.signature = sign(&self.key, &moderate_message(&c));
        hub.moderate(&c)
    }
}

#[test]
fn owners_work_through_reports() {
    let dir = std::env::temp_dir().join(format!("libroxel-moderation-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let data = dir.join("hub.sqlite");
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&data)).unwrap();
    let hub = HubClient::new(&running.url());
    let (boss, ana, bo) = (Player::new(21), Player::new(22), Player::new(23));
    boss.claim(&hub, "Boss");
    ana.claim(&hub, "Anna");
    bo.claim(&hub, "Bobby");
    ana.add(&hub, &bo);
    bo.add(&hub, &ana);

    let rude = bo.send(&hub, &ana, "go away").unwrap();
    let worse = bo.send(&hub, &ana, "really go away").unwrap();
    ana.report(&hub, &rude.message_id);
    ana.report(&hub, &worse.message_id);

    assert!(ana.dashboard(&hub).unwrap_err().contains("owners"));
    assert!(ana.moderate(&hub, ModAction::Ban, "", &bo.id, 0).is_err());
    add_owner(&open_db(Some(&data)).unwrap(), "Boss").unwrap();

    let board = boss.dashboard(&hub).unwrap();
    assert_eq!(board.reports.len(), 2);
    let item = board.reports.iter().find(|q| q.target == rude.message_id).unwrap();
    assert_eq!(item.author.as_deref(), Some("Bobby"));
    assert_eq!(item.text.as_deref(), Some("go away"));
    assert_eq!(item.reports[0].reporter.as_deref(), Some("Anna"));

    boss.moderate(&hub, ModAction::Dismiss, &worse.message_id, "", 0).unwrap();
    boss.moderate(&hub, ModAction::Remove, &rude.message_id, "", 0).unwrap();
    boss.moderate(&hub, ModAction::Warn, "", "Bobby", 0).unwrap();
    boss.moderate(&hub, ModAction::Mute, "", &bo.id, 2).unwrap();
    assert!(bo.send(&hub, &ana, "hello").unwrap_err().contains("muted"));

    let time = bo.tick();
    let mine = hub.security(&bo.id, time, &sign(&bo.key, &security_message(&bo.id, time))).unwrap();
    assert_eq!(mine.warnings.len(), 1);
    assert!(mine.muted_secs.is_some());

    boss.moderate(&hub, ModAction::Unmute, "", &bo.id, 0).unwrap();
    bo.send(&hub, &ana, "sorry").unwrap();
    boss.moderate(&hub, ModAction::Ban, "", &bo.id, 24).unwrap();
    assert!(bo.send(&hub, &ana, "hello").unwrap_err().contains("banned"));
    assert!(boss.moderate(&hub, ModAction::Ban, "", &boss.id, 0).is_err());

    let board = boss.dashboard(&hub).unwrap();
    assert!(board.reports.is_empty());
    assert_eq!(board.banned.len(), 1);
    assert!(board.banned[0].left_secs.unwrap() <= 24 * 3600);
    assert!(board.muted.is_empty());
    let actions: Vec<&str> = board.log.iter().map(|l| l.action.as_str()).collect();
    assert_eq!(actions, ["ban", "unmute", "mute", "warn", "remove", "dismiss"]);
    assert_eq!(board.log[4].text, "go away");
    assert_eq!(board.log[0].moderator.as_deref(), Some("Boss"));
    assert_eq!(board.log[0].hours, Some(24));

    let mut again = ModChange {
        moderator_id: boss.id.clone(),
        action: ModAction::Unban,
        kind: String::new(),
        target: String::new(),
        player_id: bo.id.clone(),
        reason: String::new(),
        hours: 0,
        time: boss.clock.get() - 5,
        signature: String::new(),
    };
    again.signature = sign(&boss.key, &moderate_message(&again));
    assert!(hub.moderate(&again).is_err());

    let db = open_db(Some(&data)).unwrap();
    db.execute("UPDATE banned SET until = 1", []).unwrap();
    drop(db);
    bo.send(&hub, &ana, "back").unwrap();
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}
