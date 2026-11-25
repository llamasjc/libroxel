use playlibre_hub::{
    AccountUpdate, AppealAsk, FriendAction, FriendChange, HubClient, ModAction, ModChange, NewMessage,
    Report, account_message, add_owner, appeal_message, friend_message, message_message, moderate_message,
    moderation_message, open_db, player_id_of, report_message, security_message, sign, start,
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

    fn send(&self, hub: &HubClient, to: &Player, text: &str) -> Result<playlibre_hub::Message, String> {
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
            evidence: String::new(),
            time,
            signature: sign(&self.key, &report_message(&self.id, "message", target, "rude", "", time)),
        })
        .unwrap();
    }

    fn report_player(&self, hub: &HubClient, who: &str, evidence: &str) -> Result<(), String> {
        let time = self.tick();
        hub.report(&Report {
            reporter_id: self.id.clone(),
            kind: "player".into(),
            target: who.into(),
            reason: "Bullying".into(),
            evidence: evidence.into(),
            time,
            signature: sign(&self.key, &report_message(&self.id, "player", who, "Bullying", evidence, time)),
        })
    }

    fn appeal(&self, hub: &HubClient, kind: &str, warning_id: i64) -> Result<(), String> {
        let time = self.tick();
        let text = "it was a joke between friends";
        hub.appeal(&AppealAsk {
            player_id: self.id.clone(),
            kind: kind.into(),
            warning_id,
            text: text.into(),
            time,
            signature: sign(&self.key, &appeal_message(&self.id, kind, warning_id, text, time)),
        })
    }

    fn standing(&self, hub: &HubClient) -> playlibre_hub::Standing {
        let time = self.tick();
        hub.security(&self.id, time, &sign(&self.key, &security_message(&self.id, time))).unwrap().standing
    }

    fn answer(&self, hub: &HubClient, action: ModAction, appeal_id: i64) -> Result<(), String> {
        let mut c = ModChange {
            moderator_id: self.id.clone(),
            action,
            kind: "appeal".into(),
            target: appeal_id.to_string(),
            player_id: String::new(),
            reason: "ok".into(),
            hours: 0,
            time: self.tick(),
            signature: String::new(),
        };
        c.signature = sign(&self.key, &moderate_message(&c));
        hub.moderate(&c)
    }

    fn dashboard(&self, hub: &HubClient) -> Result<playlibre_hub::Dashboard, String> {
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
    let dir = std::env::temp_dir().join(format!("playlibre-moderation-{}", std::process::id()));
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
    assert!(hub.chat_rule(&bo.id).unwrap().muted_secs.is_some());
    assert!(hub.chat_rule(&ana.id).unwrap().muted_secs.is_none());

    boss.moderate(&hub, ModAction::Unmute, "", &bo.id, 0).unwrap();
    assert!(hub.chat_rule(&bo.id).unwrap().muted_secs.is_none());
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

#[test]
fn players_report_players_and_appeal() {
    let dir = std::env::temp_dir().join(format!("playlibre-appeals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let data = dir.join("hub.sqlite");
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&data)).unwrap();
    let hub = HubClient::new(&running.url());
    let (boss, ana, bo) = (Player::new(31), Player::new(32), Player::new(33));
    boss.claim(&hub, "Chief");
    ana.claim(&hub, "Annie");
    bo.claim(&hub, "Bobo");
    ana.add(&hub, &bo);
    bo.add(&hub, &ana);
    add_owner(&open_db(Some(&data)).unwrap(), "Chief").unwrap();

    ana.send(&hub, &bo, "hi").unwrap();
    bo.send(&hub, &ana, "you're bad at this").unwrap();
    assert!(ana.report_player(&hub, "Annie", "").unwrap_err().contains("yourself"));
    assert!(ana.report_player(&hub, "Nobody", "").is_err());
    ana.report_player(&hub, "Bobo", "In Kart Racer: Bobo: quit the game").unwrap();

    let board = boss.dashboard(&hub).unwrap();
    let q = &board.reports[0];
    assert_eq!(
        (q.kind.as_str(), q.target.as_str(), q.text.as_deref()),
        ("player", bo.id.as_str(), Some("Bobo"))
    );
    assert!(!q.hidden);
    assert_eq!(q.reports[0].evidence, "In Kart Racer: Bobo: quit the game");
    let chat: Vec<&str> = q.reports[0].chat.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(chat, ["hi", "you're bad at this"]);
    assert_eq!(q.reports[0].chat[1].from.as_deref(), Some("Bobo"));

    let mut warn = ModChange {
        moderator_id: boss.id.clone(),
        action: ModAction::Warn,
        kind: "player".into(),
        target: bo.id.clone(),
        player_id: String::new(),
        reason: "be kind".into(),
        hours: 0,
        time: boss.tick(),
        signature: String::new(),
    };
    warn.signature = sign(&boss.key, &moderate_message(&warn));
    hub.moderate(&warn).unwrap();
    let warning = bo.standing(&hub).warnings[0].warning_id;
    assert!(bo.appeal(&hub, "ban", 0).unwrap_err().contains("aren't banned"));
    bo.appeal(&hub, "warning", warning).unwrap();
    assert!(bo.appeal(&hub, "warning", warning).unwrap_err().contains("already"));

    boss.moderate(&hub, ModAction::Ban, "", &bo.id, 0).unwrap();
    bo.appeal(&hub, "ban", 0).unwrap();
    let mine = bo.standing(&hub);
    assert!(mine.banned.is_some() && mine.banned.as_ref().unwrap().left_secs.is_none());
    assert_eq!(mine.appeals.len(), 2);
    assert!(mine.appeals.iter().all(|a| a.state == "open"));

    let board = boss.dashboard(&hub).unwrap();
    assert_eq!(board.appeals.len(), 2);
    let ban = board.appeals.iter().find(|a| a.kind == "ban").unwrap();
    let warned = board.appeals.iter().find(|a| a.kind == "warning").unwrap();
    assert_eq!(ban.name.as_deref(), Some("Bobo"));
    assert_eq!(warned.penalty, "be kind");
    assert_eq!(ban.history.len(), 2);

    boss.answer(&hub, ModAction::Deny, warned.appeal_id).unwrap();
    assert!(boss.answer(&hub, ModAction::Accept, warned.appeal_id).unwrap_err().contains("already"));
    assert!(bo.send(&hub, &ana, "let me back").unwrap_err().contains("banned"));
    boss.answer(&hub, ModAction::Accept, ban.appeal_id).unwrap();
    bo.send(&hub, &ana, "thanks").unwrap();

    let mine = bo.standing(&hub);
    assert!(mine.banned.is_none());
    assert_eq!(mine.warnings.len(), 1);
    let states: Vec<&str> = mine.appeals.iter().map(|a| a.state.as_str()).collect();
    assert!(states.contains(&"accepted") && states.contains(&"denied"));
    assert!(boss.dashboard(&hub).unwrap().appeals.is_empty());
    assert_eq!(boss.dashboard(&hub).unwrap().log[0].action, "accept");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}
