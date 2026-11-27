use playlibre_hub::{
    AccountUpdate, DownloadProof, GroupAction, GroupChange, GroupJoin, GroupPermission, GroupRole, HubClient,
    Publish, account_message, creator_message, group_message, group_wallet, new_secret, player_id_of,
    publish_message, sign, start,
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

    fn change(
        &self,
        hub: &HubClient,
        action: GroupAction,
        group: &str,
        edit: impl FnOnce(&mut GroupChange),
    ) -> Result<String, String> {
        let mut change = GroupChange::new(&self.id, action, group, self.tick());
        edit(&mut change);
        change.signature = sign(&self.key, &group_message(&change));
        hub.change_group(&change).map(|c| c.group_id)
    }

    fn view(&self, hub: &HubClient, group: &str) -> playlibre_hub::GroupPage {
        let time = self.tick();
        let signature = sign(&self.key, &playlibre_hub::group_view_message(&self.id, group, time));
        hub.group_as(group, &self.id, time, &signature).unwrap()
    }
}

#[test]
fn groups_have_roles_a_wall_games_and_badges() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (alice, bo, cy) = (Player::new(1), Player::new(2), Player::new(3));
    alice.claim(&hub, "Alice");
    bo.claim(&hub, "Bobby");

    let name = |n: &str, t: &str| {
        let (n, t) = (n.to_owned(), t.to_owned());
        move |c: &mut GroupChange| {
            c.name = n;
            c.tag = t;
            c.about = "We build".into();
        }
    };
    assert!(
        cy.change(&hub, GroupAction::Create, "", name("Cy Club", "CY")).unwrap_err().contains("claim a name")
    );
    assert!(alice.change(&hub, GroupAction::Create, "", name("Shit Club", "SC")).is_err());
    assert!(alice.change(&hub, GroupAction::Create, "", name("Builders", "TOOLONG")).is_err());
    let id = alice.change(&hub, GroupAction::Create, "", name("Builders", "bld")).unwrap();
    assert!(bo.change(&hub, GroupAction::Create, "", name("builders", "XX")).unwrap_err().contains("name"));

    let page = hub.group(&id).unwrap();
    assert_eq!(page.group.tag, "BLD");
    assert_eq!(page.group.owner, "Alice");
    assert_eq!(page.members.len(), 1);
    assert_eq!(hub.groups("build").unwrap().len(), 1);

    assert!(bo.change(&hub, GroupAction::Post, &id, |c| c.text = "hi".into()).is_err());
    bo.change(&hub, GroupAction::Join, &id, |_| {}).unwrap();
    bo.change(&hub, GroupAction::Post, &id, |c| c.text = "what the shit".into()).unwrap();
    let page = hub.group(&id).unwrap();
    assert!(page.posts[0].text.starts_with("what the ") && !page.posts[0].text.contains("hit"));
    assert_eq!(page.posts[0].name, "Bobby");
    let post = page.posts[0].post_id.to_string();

    let profile = hub.profile(&bo.id).unwrap().unwrap();
    assert_eq!(profile.badge.as_ref().map(|b| b.tag.as_str()), Some("BLD"));
    assert_eq!(profile.groups.len(), 1);

    assert!(bo.change(&hub, GroupAction::Kick, &id, |c| c.other_id = alice.id.clone()).is_err());
    assert!(
        bo.change(&hub, GroupAction::Role, &id, |c| {
            c.other_id = bo.id.clone();
            c.role = Some(GroupRole::Admin);
        })
        .is_err()
    );
    alice
        .change(&hub, GroupAction::Role, &id, |c| {
            c.other_id = bo.id.clone();
            c.role = Some(GroupRole::Admin);
        })
        .unwrap();
    assert_eq!(hub.group(&id).unwrap().members[1].role, GroupRole::Admin);

    alice.change(&hub, GroupAction::Unpost, &id, |c| c.other_id = post.clone()).unwrap();
    assert!(hub.group(&id).unwrap().posts.is_empty());

    let secret = new_secret();
    let game = hub
        .publish(&Publish {
            game_id: None,
            secret: secret.clone(),
            title: "Tower".into(),
            author: "Alice".into(),
            description: String::new(),
            place: "{}".into(),
            author_id: Some(alice.id.clone()),
            author_signature: Some(sign(&alice.key, &publish_message(&secret))),
        })
        .unwrap()
        .game_id;
    assert!(bo.change(&hub, GroupAction::AddGame, &id, |c| c.other_id = game.clone()).is_err());
    alice.change(&hub, GroupAction::AddGame, &id, |c| c.other_id = game.clone()).unwrap();
    assert_eq!(hub.group(&id).unwrap().games.len(), 1);
    assert_eq!(hub.game(&game).unwrap().group.map(|g| g.name), Some("Builders".into()));

    assert!(alice.change(&hub, GroupAction::Leave, &id, |_| {}).unwrap_err().contains("owner"));
    alice
        .change(&hub, GroupAction::Role, &id, |c| {
            c.other_id = bo.id.clone();
            c.role = Some(GroupRole::Owner);
        })
        .unwrap();
    assert_eq!(hub.group(&id).unwrap().group.owner, "Bobby");
    bo.change(&hub, GroupAction::Kick, &id, |c| c.other_id = alice.id.clone()).unwrap();
    assert_eq!(hub.group(&id).unwrap().members.len(), 1);

    bo.change(&hub, GroupAction::Show, "", |_| {}).unwrap();
    assert!(hub.profile(&bo.id).unwrap().unwrap().badge.is_none());
    bo.change(&hub, GroupAction::Leave, &id, |_| {}).unwrap();
    assert!(hub.group(&id).is_err());
}

#[test]
fn tags_and_colours_are_claimed_and_groups_can_be_closed() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (owner, asker, guest) = (Player::new(11), Player::new(12), Player::new(13));
    owner.claim(&hub, "Owner1");
    asker.claim(&hub, "Asker1");
    guest.claim(&hub, "Guest1");
    let make = |p: &Player, name: &str, tag: &str, color: &str, join: GroupJoin| {
        let (name, tag, color) = (name.to_owned(), tag.to_owned(), color.to_owned());
        p.change(&hub, GroupAction::Create, "", move |c| {
            c.name = name;
            c.tag = tag;
            c.color = color;
            c.join = Some(join);
        })
    };
    let id = make(&owner, "Red Team", "RT", "red", GroupJoin::Request).unwrap();
    assert!(make(&asker, "Other Team", "rt", "red", GroupJoin::Open).unwrap_err().contains("RT in red"));
    let blue = make(&asker, "Blue Team", "RT", "blue", GroupJoin::Open).unwrap();
    assert!(make(&guest, "Rainbow", "RB", "rainbow", GroupJoin::Open).is_err());
    assert!(make(&guest, "Too Long", "TOOLNG", "red", GroupJoin::Open).is_err());
    assert!(make(&guest, "PlayLibre Staff", "LS", "red", GroupJoin::Open).is_err());
    assert_eq!(hub.group(&blue).unwrap().group.color, "blue");

    asker.change(&hub, GroupAction::Join, &id, |_| {}).unwrap();
    let seen = asker.view(&hub, &id);
    assert!(seen.requested && seen.members.len() == 1 && seen.requests.is_empty());
    let mine = owner.view(&hub, &id);
    assert_eq!(mine.requests.len(), 1);
    assert!(asker.change(&hub, GroupAction::Accept, &id, |c| c.other_id = guest.id.clone()).is_err());
    owner.change(&hub, GroupAction::Accept, &id, |c| c.other_id = asker.id.clone()).unwrap();
    assert_eq!(hub.group(&id).unwrap().members.len(), 2);

    owner
        .change(&hub, GroupAction::Edit, &id, |c| {
            c.name = "Red Team".into();
            c.tag = "RT".into();
            c.color = "red".into();
            c.join = Some(GroupJoin::Invite);
        })
        .unwrap();
    assert!(guest.change(&hub, GroupAction::Join, &id, |_| {}).unwrap_err().contains("invite only"));
    owner.change(&hub, GroupAction::Invite, &id, |c| c.other_id = guest.id.clone()).unwrap();
    assert!(guest.view(&hub, &id).invited);
    let time = guest.tick();
    let invites = hub
        .group_invites(
            &guest.id,
            time,
            &sign(&guest.key, &playlibre_hub::group_invites_message(&guest.id, time)),
        )
        .unwrap();
    assert_eq!(invites.len(), 1);
    guest.change(&hub, GroupAction::Join, &id, |_| {}).unwrap();
    assert_eq!(hub.group(&id).unwrap().members.len(), 3);
}

#[test]
fn group_games_are_shared_and_funds_pay_members() {
    let dir = std::env::temp_dir().join(format!("playlibre-group-funds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("hub.sqlite");
    let running = start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let hub = HubClient::new(&running.url());
    let (alice, bo) = (Player::new(11), Player::new(12));
    alice.claim(&hub, "Alicia");
    bo.claim(&hub, "Bobo");
    let id = alice
        .change(&hub, GroupAction::Create, "", |c| {
            c.name = "Studio".into();
            c.tag = "STU".into();
        })
        .unwrap();
    bo.change(&hub, GroupAction::Join, &id, |_| {}).unwrap();

    let publish = |who: &Player, game_id: Option<String>, title: &str| {
        let secret = new_secret();
        hub.publish(&Publish {
            game_id,
            secret: secret.clone(),
            title: title.into(),
            author: String::new(),
            description: String::new(),
            place: "{}".into(),
            author_id: Some(who.id.clone()),
            author_signature: Some(sign(&who.key, &publish_message(&secret))),
        })
    };
    let game = publish(&alice, None, "Obby").unwrap().game_id;
    alice.change(&hub, GroupAction::AddGame, &id, |c| c.other_id = game.clone()).unwrap();
    let download = |who: &Player| {
        hub.download(&game, Some(&DownloadProof::new(&game, who.id.clone(), |m| sign(&who.key, m))))
    };
    let mine = |who: &Player| {
        let time = who.tick();
        hub.my_games(&who.id, time, &sign(&who.key, &creator_message(&who.id, time))).unwrap()
    };

    assert!(publish(&bo, Some(game.clone()), "Mine").unwrap_err().contains("someone else"));
    assert!(download(&bo).is_err());
    assert!(mine(&bo).is_empty());
    assert!(bo.change(&hub, GroupAction::Permissions, &id, |c| c.other_id = bo.id.clone()).is_err());
    alice
        .change(&hub, GroupAction::Permissions, &id, |c| {
            c.other_id = bo.id.clone();
            c.text = "games".into();
        })
        .unwrap();
    let page = bo.view(&hub, &id);
    assert_eq!(page.can, vec![GroupPermission::Games]);
    assert!(page.funds.is_none());
    assert!(download(&bo).is_ok());
    assert_eq!(mine(&bo).len(), 1);
    assert_eq!(publish(&bo, Some(game.clone()), "Obby 2").unwrap().version, 2);
    let info = hub.game(&game).unwrap();
    assert_eq!((info.title.as_str(), info.author_id.as_deref()), ("Obby 2", Some(alice.id.as_str())));
    assert!(bo.change(&hub, GroupAction::RemoveGame, &id, |c| c.other_id = game.clone()).is_err());

    let pay = |who: &Player, amount: &str| {
        who.change(&hub, GroupAction::Pay, &id, |c| {
            c.other_id = bo.id.clone();
            c.text = amount.into();
        })
    };
    assert!(pay(&bo, "5").is_err());
    assert!(pay(&alice, "5").unwrap_err().contains("doesn't have"));
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "INSERT INTO ledger (player_id, amount, reason, reference, time) VALUES (?1, 100, 'earned', 'sale-1', 0)",
        [group_wallet(&id)],
    )
    .unwrap();
    db.execute("INSERT INTO wallets (player_id, balance) VALUES (?1, 100)", [group_wallet(&id)]).unwrap();
    drop(db);
    assert_eq!(alice.view(&hub, &id).funds.unwrap().balance, 100);
    pay(&alice, "30").unwrap();
    assert!(pay(&alice, "0").is_err());
    let funds = alice.view(&hub, &id).funds.unwrap();
    assert_eq!(funds.balance, 70);
    assert_eq!((funds.history[0].amount, funds.history[0].to.as_str()), (-30, "Bobo"));
    let time = bo.tick();
    let wallet =
        hub.wallet(&bo.id, time, &sign(&bo.key, &playlibre_hub::wallet_message(&bo.id, time))).unwrap();
    assert_eq!(wallet.balance, 30);

    alice
        .change(&hub, GroupAction::Role, &id, |c| {
            c.other_id = bo.id.clone();
            c.role = Some(GroupRole::Owner);
        })
        .unwrap();
    bo.change(&hub, GroupAction::Kick, &id, |c| c.other_id = alice.id.clone()).unwrap();
    assert!(bo.change(&hub, GroupAction::Leave, &id, |_| {}).unwrap_err().contains("funds"));
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}
