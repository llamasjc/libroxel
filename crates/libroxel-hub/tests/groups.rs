use libroxel_hub::{
    AccountUpdate, GroupAction, GroupChange, GroupRole, HubClient, Publish, account_message, group_message,
    new_secret, player_id_of, publish_message, sign, start,
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
