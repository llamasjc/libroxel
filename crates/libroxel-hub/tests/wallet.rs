use std::sync::{Arc, Mutex};

use libroxel_hub::{
    AccountUpdate, Birthday, Buy, Consent, Daily, FamilyChange, HubClient, Mail, NewItem, SetPrice,
    account_message, birthday_message, buy_message, consent_message, daily_message, family_message,
    item_message, player_id_of, price_message, sign, wallet_message,
};

fn ms() -> i64 {
    std::thread::sleep(std::time::Duration::from_millis(2));
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn save_look(hub: &HubClient, key: &[u8; 32], name: &str, look: serde_json::Value) -> Result<(), String> {
    let id = player_id_of(key);
    let time = ms() / 1000;
    hub.update_account(&AccountUpdate {
        work: Some(libroxel_hub::account_work(&id, name)),
        email: Some(format!("{name}@example.com")),
        signature: sign(key, &account_message(&id, name, "", &look, time)),
        player_id: id,
        name: name.into(),
        about: String::new(),
        look,
        time,
    })
    .map(|_| ())
}

fn wallet(hub: &HubClient, key: &[u8; 32]) -> libroxel_hub::Wallet {
    let (id, t) = (player_id_of(key), ms());
    hub.wallet(&id, t, &sign(key, &wallet_message(&id, t))).unwrap()
}

fn daily(hub: &HubClient, key: &[u8; 32]) -> Result<libroxel_hub::Wallet, String> {
    let (player_id, time) = (player_id_of(key), ms());
    let signature = sign(key, &daily_message(&player_id, time));
    hub.claim_daily(&Daily { player_id, time, signature })
}

fn buy(hub: &HubClient, key: &[u8; 32], item_id: &str, price: u32) -> Result<libroxel_hub::Wallet, String> {
    let (player_id, time) = (player_id_of(key), ms());
    let signature = sign(key, &buy_message(&player_id, item_id, price, time));
    hub.buy_item(&Buy { player_id, item_id: item_id.into(), price, time, signature })
}

fn price(hub: &HubClient, key: &[u8; 32], item_id: &str, price: u32) -> Result<(), String> {
    let (player_id, time) = (player_id_of(key), ms());
    let signature = sign(key, &price_message(&player_id, item_id, price, time));
    hub.set_price(item_id, &SetPrice { player_id, price, time, signature })
}

fn share_hat(hub: &HubClient, key: &[u8; 32]) -> libroxel_hub::Item {
    let data = serde_json::json!({ "name": "Box", "parts": [
        { "size": [1.0, 1.0, 1.0], "at": [0.0, 1.2, 0.0], "turn": [0.0, 0.0, 0.0], "shape": "Block", "color": [9, 9, 9], "neon": false }
    ] });
    let (creator_id, time) = (player_id_of(key), ms() / 1000);
    let signature = sign(key, &item_message(&creator_id, "Box", "hat", &data, time));
    hub.share_item(&NewItem { creator_id, name: "Box".into(), kind: "hat".into(), data, time, signature })
        .unwrap()
}

#[test]
fn daily_sparks_buy_a_hat_once() {
    let running = libroxel_hub::start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (maker, me) = ([90u8; 32], [91u8; 32]);
    save_look(&hub, &maker, "hatmaker", serde_json::json!({})).unwrap();
    save_look(&hub, &me, "shopper", serde_json::json!({})).unwrap();
    assert_eq!(wallet(&hub, &me).balance, 0);
    assert!(wallet(&hub, &me).daily_ready);

    let party =
        hub.catalog("Party hat").unwrap().into_iter().find(|i| i.item_id == "shop-party-hat").unwrap();
    assert_eq!((party.price, party.creator.as_str()), (50, "Libroxel"));
    assert!(
        save_look(&hub, &me, "shopper", serde_json::json!({ "item": party.data }))
            .unwrap_err()
            .contains("buy")
    );
    assert!(buy(&hub, &me, "shop-party-hat", 50).unwrap_err().contains("enough"));

    let hat = share_hat(&hub, &maker);
    assert!(price(&hub, &me, &hat.item_id, 10).unwrap_err().contains("creator"));
    price(&hub, &maker, &hat.item_id, 10).unwrap();
    save_look(&hub, &maker, "hatmaker", serde_json::json!({ "item": hat.data })).unwrap();
    assert!(
        save_look(&hub, &me, "shopper", serde_json::json!({ "item": hat.data })).unwrap_err().contains("buy")
    );

    let w = daily(&hub, &me).unwrap();
    assert_eq!(w.balance, libroxel_hub::DAILY);
    assert!(!w.daily_ready);
    assert!(daily(&hub, &me).unwrap_err().contains("tomorrow"));

    assert!(buy(&hub, &me, &hat.item_id, 5).unwrap_err().contains("price"));
    let w = buy(&hub, &me, &hat.item_id, 10).unwrap();
    assert_eq!(w.balance, libroxel_hub::DAILY - 10);
    assert_eq!(w.owned, vec![hat.item_id.clone()]);
    assert_eq!((w.history[0].reason.as_str(), w.history[0].amount), ("buy", -10));
    assert!(buy(&hub, &me, &hat.item_id, 10).unwrap_err().contains("already"));
    save_look(&hub, &me, "shopper", serde_json::json!({ "item": hat.data })).unwrap();
}

#[test]
fn a_parent_can_turn_spending_off() {
    let outbox = Arc::new(Mutex::new(Vec::new()));
    let options = libroxel_hub::HubOptions { mail: Mail::Outbox(outbox.clone()), ..Default::default() };
    let running = libroxel_hub::start_with("127.0.0.1:0".parse().unwrap(), None, options).unwrap();
    let hub = HubClient::new(&running.url());
    let (maker, kid) = ([93u8; 32], [92u8; 32]);
    save_look(&hub, &maker, "hatmaker2", serde_json::json!({})).unwrap();
    save_look(&hub, &kid, "kidshopper", serde_json::json!({})).unwrap();
    let hat = share_hat(&hub, &maker);
    price(&hub, &maker, &hat.item_id, 10).unwrap();
    outbox.lock().unwrap().clear();
    let year = 1970 + ms() / 1000 / 86_400 * 400 / 146_097;
    let mut b = Birthday {
        player_id: player_id_of(&kid),
        born: format!("{}-01-01", year - 9),
        parent_email: Some("mum@example.com".into()),
        utc_offset_mins: 0,
        time: ms(),
        signature: String::new(),
    };
    b.signature = sign(&kid, &birthday_message(&b));
    hub.set_birthday(&b).unwrap();
    let (_, code) = outbox.lock().unwrap().pop().unwrap();
    let mut c = Consent {
        player_id: player_id_of(&kid),
        code,
        pin: "4321".into(),
        time: ms(),
        signature: String::new(),
    };
    c.signature = sign(&kid, &consent_message(&c));
    let mut settings = hub.consent(&c).unwrap().settings;
    settings.spend_limit = Some(0);
    let mut change = FamilyChange {
        player_id: player_id_of(&kid),
        pin: "4321".into(),
        new_pin: None,
        settings,
        time: ms(),
        signature: String::new(),
    };
    change.signature = sign(&kid, &family_message(&change));
    hub.change_family(&change).unwrap();

    daily(&hub, &kid).unwrap();
    assert_eq!(wallet(&hub, &kid).may_spend, Some(0));
    assert!(buy(&hub, &kid, &hat.item_id, 10).unwrap_err().contains("parent"));
    assert_eq!(wallet(&hub, &kid).balance, libroxel_hub::DAILY);
}
