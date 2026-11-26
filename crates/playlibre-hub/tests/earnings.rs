use playlibre_hub::{
    AccountUpdate, BuyPass, CashOut, HubClient, PassAction, PassChange, Publish, account_message,
    buy_pass_message, cash_out_message, earnings_message, open_db, pass_message, player_id_of,
    publish_message, sign, top_up,
};

fn ms() -> i64 {
    std::thread::sleep(std::time::Duration::from_millis(2));
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn join(hub: &HubClient, key: &[u8; 32], name: &str) {
    let id = player_id_of(key);
    let time = ms() / 1000;
    let look = serde_json::json!({});
    hub.update_account(&AccountUpdate {
        work: Some(playlibre_hub::account_work(&id, name)),
        email: Some(format!("{name}@example.com")),
        signature: sign(key, &account_message(&id, name, "", &look, time)),
        player_id: id,
        name: name.into(),
        about: String::new(),
        look,
        time,
    })
    .unwrap();
}

fn publish(hub: &HubClient, key: &[u8; 32]) -> String {
    let secret = "s".repeat(20);
    hub.publish(&Publish {
        game_id: None,
        secret: secret.clone(),
        title: "Obby".into(),
        author: String::new(),
        description: String::new(),
        place: "{}".into(),
        author_id: Some(player_id_of(key)),
        author_signature: Some(sign(key, &publish_message(&secret))),
    })
    .unwrap()
    .game_id
}

fn change(
    key: &[u8; 32],
    game_id: &str,
    action: PassAction,
    pass_id: i64,
    name: &str,
    price: u32,
) -> PassChange {
    let mut c = PassChange {
        player_id: player_id_of(key),
        action,
        pass_id,
        name: name.into(),
        about: String::new(),
        price,
        time: ms(),
        signature: String::new(),
    };
    c.signature = sign(key, &pass_message(game_id, &c));
    c
}

fn buy(hub: &HubClient, key: &[u8; 32], pass_id: i64, price: u32) -> Result<playlibre_hub::Pass, String> {
    let (player_id, time) = (player_id_of(key), ms());
    let signature = sign(key, &buy_pass_message(&player_id, pass_id, price, time));
    hub.buy_pass(&BuyPass { player_id, pass_id, price, time, signature })
}

fn earnings(hub: &HubClient, key: &[u8; 32]) -> playlibre_hub::Earnings {
    let (id, t) = (player_id_of(key), ms());
    hub.earnings(&id, t, &sign(key, &earnings_message(&id, t))).unwrap()
}

#[test]
fn passes_sell_and_earnings_show_up() {
    let dir = std::env::temp_dir().join(format!("playlibre-earnings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("hub.sqlite");
    let running = playlibre_hub::start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let hub = HubClient::new(&running.url());
    let (maker, fan) = ([70u8; 32], [71u8; 32]);
    join(&hub, &maker, "maker");
    join(&hub, &fan, "fan");
    let game = publish(&hub, &maker);

    assert!(hub.change_pass(&game, &change(&fan, &game, PassAction::Add, 0, "VIP", 20)).is_err());
    assert!(hub.change_pass(&game, &change(&maker, &game, PassAction::Add, 0, "VIP", 0)).is_err());
    let list = hub.change_pass(&game, &change(&maker, &game, PassAction::Add, 0, "VIP", 20)).unwrap();
    let vip = list[0].pass_id;
    assert!(list[0].owned);

    let fan_id = player_id_of(&fan);
    assert!(top_up(&open_db(Some(&path)).unwrap(), &fan_id, 25, "pay_fan").unwrap());

    assert!(!hub.owns_pass(vip, &fan_id).unwrap());
    assert!(buy(&hub, &fan, vip, 19).unwrap_err().contains("price"));
    assert!(buy(&hub, &fan, vip, 20).unwrap().owned);
    assert!(buy(&hub, &fan, vip, 20).is_err());
    assert!(hub.owns_pass(vip, &fan_id).unwrap());
    assert_eq!(hub.passes(&game, None).unwrap()[0].sold, 1);

    let e = earnings(&hub, &maker);
    assert_eq!((e.pending, e.balance, e.share_percent), (14, 0, 70));
    assert_eq!(e.sales[0].name, "VIP");
    assert!(!e.payouts_on);

    let (maker_id, time) = (player_id_of(&maker), ms());
    let mut ask = CashOut {
        player_id: maker_id,
        credits: 10,
        details: "me@example.com".into(),
        time,
        signature: String::new(),
    };
    ask.signature = sign(&maker, &cash_out_message(&ask));
    assert!(hub.cash_out(&ask).unwrap_err().contains("isn't open"));

    hub.change_pass(&game, &change(&maker, &game, PassAction::Remove, vip, "", 0)).unwrap();
    assert!(hub.passes(&game, None).unwrap().is_empty());
    assert!(hub.owns_pass(vip, &fan_id).unwrap());
}
