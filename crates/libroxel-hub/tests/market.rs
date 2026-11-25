use libroxel_hub::{
    AccountUpdate, AssetKind, HubClient, NewAsset, Publish, Report, Unmarket, account_message, asset_message,
    new_secret, player_id_of, report_message, sign, start, unmarket_message,
};

struct Player {
    key: [u8; 32],
    id: String,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

impl Player {
    fn new(seed: u8, hub: &HubClient, name: &str) -> Self {
        let key = [seed; 32];
        let id = player_id_of(&key);
        let look = serde_json::json!({});
        let time = now();
        hub.update_account(&AccountUpdate {
            player_id: id.clone(),
            name: name.into(),
            about: String::new(),
            look: look.clone(),
            time,
            signature: sign(&key, &account_message(&id, name, "", &look, time)),
            work: Some(libroxel_hub::account_work(&id, name)),
            email: None,
        })
        .unwrap();
        Self { key, id }
    }

    #[allow(clippy::too_many_arguments)]
    fn publish(
        &self,
        hub: &HubClient,
        asset_id: Option<&str>,
        kind: AssetKind,
        name: &str,
        data: &str,
        permissions: &[&str],
        time: i64,
    ) -> Result<libroxel_hub::Asset, String> {
        let permissions: Vec<String> = permissions.iter().map(|p| (*p).to_owned()).collect();
        let signature =
            sign(&self.key, &asset_message(&self.id, asset_id, kind, name, "", data, &permissions, 0, time));
        hub.publish_asset(&NewAsset {
            asset_id: asset_id.map(str::to_owned),
            creator_id: self.id.clone(),
            kind,
            name: name.into(),
            about: String::new(),
            data: data.into(),
            permissions,
            price: 0,
            time,
            signature,
        })
    }

    fn report(&self, hub: &HubClient, target: &str) {
        let time = now();
        let signature = sign(&self.key, &report_message(&self.id, "market", target, "bad", "", time));
        hub.report(&Report {
            reporter_id: self.id.clone(),
            kind: "market".into(),
            target: target.into(),
            reason: "bad".into(),
            evidence: String::new(),
            time,
            signature,
        })
        .unwrap();
    }
}

fn model(scripts: std::ops::Range<usize>) -> String {
    let children: Vec<serde_json::Value> = scripts
        .map(|n| {
            serde_json::json!({ "class": "script", "name": "S", "properties": {
                "source": format!("local value{n} = {n}\n{}", "print(value) ".repeat(30)) } })
        })
        .collect();
    serde_json::json!({ "format": "libroxel-place", "version": 1,
        "children": [{ "class": "model", "name": "Tower", "children": children }] })
    .to_string()
}

#[test]
fn creators_share_models_and_plugins() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let alice = Player::new(1, &hub, "Alice");
    let bob = Player::new(2, &hub, "Bob");
    let time = now();

    let tower = alice.publish(&hub, None, AssetKind::Model, "Tower", &model(0..6), &[], time).unwrap();
    assert_eq!((tower.version, tower.creator.as_str()), (1, "Alice"));
    let tool =
        alice.publish(&hub, None, AssetKind::Plugin, "Stairs", "print('stairs')", &["add"], time).unwrap();
    assert_eq!(tool.permissions, ["add"]);

    assert_eq!(hub.market("tow", None, None).unwrap().len(), 1);
    assert_eq!(hub.market("", None, Some(&bob.id)).unwrap().len(), 0);
    assert_eq!(hub.market("", None, Some(&alice.id)).unwrap().len(), 2);
    assert_eq!(hub.market("", Some(AssetKind::Plugin), None).unwrap()[0].asset_id, tool.asset_id);

    let got = hub.market_data(&tower.asset_id).unwrap();
    assert_eq!(got.data, model(0..6));
    assert_eq!(hub.market_asset(&tower.asset_id).unwrap().installs, 1);

    let copied = bob.publish(&hub, None, AssetKind::Model, "My tower", &model(0..6), &[], time).unwrap_err();
    assert!(copied.contains("Tower"), "{copied}");
    let copied =
        bob.publish(&hub, None, AssetKind::Plugin, "Steps", "print( 'stairs' )", &[], time).unwrap_err();
    assert!(copied.contains("Stairs"), "{copied}");
    let stolen = bob.publish(&hub, Some(&tool.asset_id), AssetKind::Plugin, "Stairs", "x()", &[], time + 1);
    assert!(stolen.is_err());

    let bad = alice.publish(&hub, None, AssetKind::Plugin, "Net", "x()", &["internet"], time);
    assert!(bad.unwrap_err().contains("no permission"));
    let bad = alice.publish(
        &hub,
        None,
        AssetKind::Model,
        "Two",
        r#"{"format":"libroxel-place","children":[]}"#,
        &[],
        time,
    );
    assert!(bad.is_err());

    let newer = alice
        .publish(
            &hub,
            Some(&tool.asset_id),
            AssetKind::Plugin,
            "Stairs",
            "print('v2')",
            &["add", "change"],
            time + 1,
        )
        .unwrap();
    assert_eq!((newer.version, newer.permissions.len()), (2, 2));
    let replay =
        alice.publish(&hub, Some(&tool.asset_id), AssetKind::Plugin, "Stairs", "print('v2')", &[], time + 1);
    assert!(replay.is_err());

    let game = hub.publish(&Publish {
        game_id: None,
        secret: new_secret(),
        title: "Bob's game".into(),
        author: "Bob".into(),
        description: String::new(),
        place: model(0..6),
        author_id: None,
        author_signature: None,
    });
    assert!(game.is_ok(), "a game using a marketplace model isn't a copy: {game:?}");

    for seed in 10..13 {
        Player::new(seed, &hub, &format!("Rep{seed}")).report(&hub, &tower.asset_id);
    }
    assert!(hub.market("", None, None).unwrap().iter().all(|a| a.asset_id != tower.asset_id));
    assert!(hub.market_data(&tower.asset_id).is_err());

    let t = now();
    let wrong = Unmarket { time: t, signature: sign(&bob.key, &unmarket_message(&tool.asset_id, t)) };
    assert!(hub.unmarket(&tool.asset_id, &wrong).is_err());
    let right = Unmarket { time: t, signature: sign(&alice.key, &unmarket_message(&tool.asset_id, t)) };
    hub.unmarket(&tool.asset_id, &right).unwrap();
    assert!(hub.market("", None, None).unwrap().is_empty());
}
