use playlibre_hub::{
    DataChange, DataEdit, DataQuery, DataTrade, DataTraded, DataView, HubClient, Publish, creator_message,
    data_edit_message, data_message, new_secret, player_id_of, publish_message, sign,
};

fn ms() -> i64 {
    std::thread::sleep(std::time::Duration::from_millis(2));
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn publish(hub: &HubClient, key: &[u8; 32]) -> String {
    let secret = new_secret();
    hub.publish(&Publish {
        game_id: None,
        author_signature: Some(sign(key, &publish_message(&secret))),
        secret,
        title: "Coin Tower".into(),
        author: String::new(),
        description: String::new(),
        place: "{}".into(),
        author_id: Some(player_id_of(key)),
    })
    .unwrap()
    .game_id
}

fn trade(
    hub: &HubClient,
    key: &[u8; 32],
    game: &str,
    since: i64,
    changes: Vec<DataChange>,
) -> Result<DataTraded, String> {
    let server_id = player_id_of(key);
    let time = ms();
    let signature = sign(key, &data_message(&server_id, game, time, since, &changes));
    hub.trade_data(game, &DataTrade { server_id, time, since, changes, signature })
}

fn set(store: &str, key: &str, value: serde_json::Value) -> DataChange {
    DataChange { store: store.into(), key: key.into(), value: Some(value) }
}

#[test]
fn a_games_servers_share_its_saved_data() {
    let dir = std::env::temp_dir().join(format!("playlibre-saved-data-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let running =
        playlibre_hub::start("127.0.0.1:0".parse().unwrap(), Some(&dir.join("hub.sqlite"))).unwrap();
    let hub = HubClient::new(&running.url());
    let (maker, stranger) = ([31u8; 32], [32u8; 32]);
    let game = publish(&hub, &maker);

    // Only servers the Hub trusts with the game keep its data there.
    let refused = trade(&hub, &stranger, &game, 0, vec![set("coins", "1", 5.into())]).unwrap_err();
    assert!(refused.contains("only the game's own servers"), "{refused}");

    // One server saves; it gets nothing back that it already has.
    let first = trade(&hub, &maker, &game, 0, vec![set("coins", "1", 5.into()), set("coins", "2", 9.into())])
        .unwrap();
    assert!(first.changes.is_empty());
    assert_eq!(first.seq, 2);

    // A new server starts from what is there.
    let start = trade(&hub, &maker, &game, 0, Vec::new()).unwrap();
    assert_eq!(start.changes.len(), 2);
    assert_eq!(start.seq, 2);

    // A change and a removal reach the first server on its next trade.
    let second = trade(
        &hub,
        &maker,
        &game,
        start.seq,
        vec![set("coins", "1", 6.into()), DataChange { store: "coins".into(), key: "2".into(), value: None }],
    )
    .unwrap();
    assert_eq!(second.seq, 4);
    let caught_up = trade(&hub, &maker, &game, first.seq, Vec::new()).unwrap();
    assert_eq!(
        caught_up.changes,
        vec![set("coins", "1", 6.into()), DataChange { store: "coins".into(), key: "2".into(), value: None }]
    );

    // A signed trade can't be sent again.
    let server_id = player_id_of(&maker);
    let time = ms();
    let signature = sign(&maker, &data_message(&server_id, &game, time, 0, &[]));
    let once = DataTrade { server_id, time, since: 0, changes: Vec::new(), signature };
    assert!(hub.trade_data(&game, &once).is_ok());
    assert!(hub.trade_data(&game, &once).is_err());

    // Names and sizes are checked.
    assert!(trade(&hub, &maker, &game, 0, vec![set("", "1", 1.into())]).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

fn look(hub: &HubClient, key: &[u8; 32], game: &str, store: Option<&str>) -> Result<DataView, String> {
    let player_id = player_id_of(key);
    let time = ms();
    let signature = sign(key, &creator_message(&player_id, time));
    hub.data(
        game,
        &DataQuery { player_id, time, signature, store: store.map(Into::into), ..Default::default() },
    )
}

fn edit(hub: &HubClient, key: &[u8; 32], game: &str, k: &str, value: Option<&str>) -> Result<(), String> {
    let player_id = player_id_of(key);
    let time = ms();
    let signature = sign(key, &data_edit_message(&player_id, game, time, "coins", k, value));
    hub.edit_data(
        game,
        &DataEdit {
            player_id,
            time,
            store: "coins".into(),
            key: k.into(),
            value: value.map(Into::into),
            signature,
        },
    )
}

#[test]
fn creators_see_and_change_saved_data() {
    let running = playlibre_hub::start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (maker, stranger) = ([33u8; 32], [34u8; 32]);
    let game = publish(&hub, &maker);
    let first = trade(&hub, &maker, &game, 0, vec![set("coins", "1", 5.into()), set("coins", "2", 9.into())])
        .unwrap();

    assert!(look(&hub, &stranger, &game, None).is_err());
    assert!(edit(&hub, &stranger, &game, "1", Some("7")).is_err());

    let all = look(&hub, &maker, &game, None).unwrap();
    assert_eq!(all.stores.len(), 1);
    assert_eq!(all.stores[0].keys, 2);
    assert!(all.keys.is_empty());
    let coins = look(&hub, &maker, &game, Some("coins")).unwrap();
    assert_eq!(coins.keys.iter().map(|k| k.key.as_str()).collect::<Vec<_>>(), ["1", "2"]);
    assert_eq!(coins.keys[0].value, Some(5.into()));

    assert!(edit(&hub, &maker, &game, "1", Some("{oops")).is_err());
    edit(&hub, &maker, &game, "1", Some(r#"{"gold":7}"#)).unwrap();
    edit(&hub, &maker, &game, "2", None).unwrap();
    assert!(edit(&hub, &maker, &game, "2", None).is_err());

    let after = look(&hub, &maker, &game, Some("coins")).unwrap();
    assert_eq!(after.keys.len(), 1);
    assert_eq!(after.keys[0].value, Some(serde_json::json!({ "gold": 7 })));
    assert!(after.seq > coins.seq);

    let back = trade(&hub, &maker, &game, first.seq, Vec::new()).unwrap();
    assert_eq!(
        back.changes,
        vec![
            set("coins", "1", serde_json::json!({ "gold": 7 })),
            DataChange { store: "coins".into(), key: "2".into(), value: None }
        ]
    );
}
