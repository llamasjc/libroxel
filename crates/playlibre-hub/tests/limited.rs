use playlibre_hub::{
    AccountUpdate, Buy, HubClient, MakeLimited, NewItem, Piece, ResaleAction, ResaleChange, SetPrice,
    TradeAction, TradeChange, account_message, buy_message, item_message, limited_message, open_db,
    player_id_of, price_message, resale_message, set_hat_maker, sign, top_up, trade_message, trades_message,
    wallet_message,
};

fn ms() -> i64 {
    std::thread::sleep(std::time::Duration::from_millis(2));
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn save_look(hub: &HubClient, key: &[u8; 32], name: &str, look: serde_json::Value) -> Result<(), String> {
    let id = player_id_of(key);
    let time = ms() / 1000;
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
    .map(|_| ())
}

fn wallet(hub: &HubClient, key: &[u8; 32]) -> playlibre_hub::Wallet {
    let (id, t) = (player_id_of(key), ms());
    hub.wallet(&id, t, &sign(key, &wallet_message(&id, t))).unwrap()
}

/// A Hub with its database at a fresh path, so tests can do what whoever
/// runs it does (name owners, add Credits as a payment would).
fn hub_at(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("playlibre-limited-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("hub.sqlite")
}

fn pay(path: &std::path::Path, key: &[u8; 32], credits: u32, payment: &str) {
    assert!(top_up(&open_db(Some(path)).unwrap(), &player_id_of(key), credits, payment).unwrap());
}

fn hat_maker(path: &std::path::Path, key: &[u8; 32]) {
    set_hat_maker(&open_db(Some(path)).unwrap(), &player_id_of(key), true).unwrap();
}

fn buy(hub: &HubClient, key: &[u8; 32], item_id: &str, price: u32) -> Result<playlibre_hub::Wallet, String> {
    let (player_id, time) = (player_id_of(key), ms());
    let signature = sign(key, &buy_message(&player_id, item_id, price, time));
    hub.buy_item(&Buy { player_id, item_id: item_id.into(), price, time, signature })
}

fn price(hub: &HubClient, key: &[u8; 32], item_id: &str, price: u32) -> Result<(), String> {
    let (player_id, time) = (player_id_of(key), ms());
    let signature = sign(key, &price_message(&player_id, item_id, price, time));
    hub.set_price(item_id, &SetPrice { player_id, price, time, signature })
}

fn share_hat(hub: &HubClient, key: &[u8; 32]) -> playlibre_hub::Item {
    try_share_hat(hub, key).unwrap()
}

fn try_share_hat(hub: &HubClient, key: &[u8; 32]) -> Result<playlibre_hub::Item, String> {
    let data = serde_json::json!({ "name": "Box", "parts": [
        { "size": [1.0, 1.0, 1.0], "at": [0.0, 1.2, 0.0], "turn": [0.0, 0.0, 0.0], "shape": "Block", "color": [9, 9, 9], "neon": false }
    ] });
    let (creator_id, time) = (player_id_of(key), ms() / 1000);
    let signature = sign(key, &item_message(&creator_id, "Box", "hat", &data, time));
    hub.share_item(&NewItem { creator_id, name: "Box".into(), kind: "hat".into(), data, time, signature })
}

fn make_limited(
    hub: &HubClient,
    key: &[u8; 32],
    item_id: &str,
    stock: Option<u32>,
    days: Option<u32>,
) -> Result<playlibre_hub::Limited, String> {
    let (player_id, time) = (player_id_of(key), ms());
    let signature = sign(key, &limited_message(&player_id, item_id, stock, days, time));
    hub.make_limited(item_id, &MakeLimited { player_id, stock, days, time, signature })
}

fn resale(
    hub: &HubClient,
    key: &[u8; 32],
    action: ResaleAction,
    item_id: &str,
    serial: u32,
    price: u32,
) -> Result<playlibre_hub::Wallet, String> {
    let (player_id, time) = (player_id_of(key), ms());
    let signature = sign(key, &resale_message(&player_id, action, item_id, serial, price, time));
    hub.resale(&ResaleChange { player_id, action, item_id: item_id.into(), serial, price, time, signature })
}

fn trade(
    hub: &HubClient,
    key: &[u8; 32],
    action: TradeAction,
    trade_id: i64,
    to: &[u8; 32],
    give: Vec<Piece>,
    get: Vec<Piece>,
) -> Result<playlibre_hub::Trade, String> {
    let (player_id, time) = (player_id_of(key), ms());
    let to_id = if action == TradeAction::Offer { player_id_of(to) } else { String::new() };
    let signature = sign(key, &trade_message(&player_id, action, trade_id, &to_id, &give, &get, time));
    hub.trade(&TradeChange { player_id, action, trade_id, to_id, give, get, time, signature })
}

fn trades(hub: &HubClient, key: &[u8; 32]) -> Vec<playlibre_hub::Trade> {
    let (id, t) = (player_id_of(key), ms());
    hub.trades(&id, t, &sign(key, &trades_message(&id, t))).unwrap()
}

fn serials(hub: &HubClient, key: &[u8; 32]) -> Vec<u32> {
    hub.limited_owned(&player_id_of(key)).unwrap().into_iter().map(|o| o.serial).collect()
}

#[test]
fn limited_items_sell_out_resell_and_trade() {
    let path = hub_at("all");
    let running = playlibre_hub::start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let hub = HubClient::new(&running.url());
    let (maker, ann, bob, cat) = ([80u8; 32], [81u8; 32], [82u8; 32], [83u8; 32]);
    for (key, name) in [(&maker, "maker"), (&ann, "ann"), (&bob, "bob"), (&cat, "cat")] {
        save_look(&hub, key, name, serde_json::json!({})).unwrap();
    }
    hat_maker(&path, &maker);
    let hat = share_hat(&hub, &maker);
    let id = hat.item_id.clone();
    assert!(make_limited(&hub, &maker, &id, Some(2), None).unwrap_err().contains("price"));
    price(&hub, &maker, &id, 100).unwrap();
    assert!(make_limited(&hub, &ann, &id, Some(2), None).unwrap_err().contains("creator"));
    assert!(make_limited(&hub, &maker, &id, Some(0), None).is_err());
    let l = make_limited(&hub, &maker, &id, Some(2), Some(7)).unwrap();
    assert_eq!((l.stock, l.sold, l.on_sale), (Some(2), 0, true));
    assert!(make_limited(&hub, &maker, &id, Some(3), None).unwrap_err().contains("already"));
    assert!(price(&hub, &maker, &id, 0).unwrap_err().contains("free"));

    for (key, pay_id) in [(&ann, "p_ann"), (&bob, "p_bob"), (&cat, "p_cat")] {
        pay(&path, key, 500, pay_id);
    }
    buy(&hub, &ann, &id, 100).unwrap();
    buy(&hub, &bob, &id, 100).unwrap();
    assert!(buy(&hub, &cat, &id, 100).unwrap_err().contains("sold out"));
    assert_eq!((serials(&hub, &ann), serials(&hub, &bob)), (vec![1], vec![2]));
    let listed = hub.catalog("").unwrap().into_iter().find(|i| i.item_id == id).unwrap();
    assert_eq!(listed.limited.unwrap().sold, 2);

    assert!(resale(&hub, &cat, ResaleAction::List, &id, 1, 300).unwrap_err().contains("isn't yours"));
    resale(&hub, &ann, ResaleAction::List, &id, 1, 300).unwrap();
    assert!(resale(&hub, &cat, ResaleAction::Buy, &id, 1, 200).unwrap_err().contains("price"));
    let w = resale(&hub, &cat, ResaleAction::Buy, &id, 1, 300).unwrap();
    assert_eq!(w.balance, 200);
    assert!(w.owned.contains(&id));
    assert_eq!(wallet(&hub, &ann).balance, 400 + 210);
    assert!(!wallet(&hub, &ann).owned.contains(&id));
    let sales = hub.item_sales(&id).unwrap();
    assert_eq!((sales.history.len(), sales.history[0].price, sales.limited.average), (1, 300, Some(300)));
    assert!(sales.listings.is_empty());
    assert!(resale(&hub, &bob, ResaleAction::Buy, &id, 1, 300).is_err());

    let one = Piece { item_id: id.clone(), serial: 1 };
    let two = Piece { item_id: id.clone(), serial: 2 };
    assert!(
        trade(&hub, &cat, TradeAction::Offer, 0, &bob, vec![two.clone()], vec![])
            .unwrap_err()
            .contains("don't have")
    );
    let t = trade(&hub, &cat, TradeAction::Offer, 0, &bob, vec![one.clone()], vec![two.clone()]).unwrap();
    let stale = trade(&hub, &bob, TradeAction::Offer, 0, &ann, vec![two.clone()], vec![]).unwrap();
    assert_eq!(t.state, "offered");
    assert!(
        trade(&hub, &cat, TradeAction::Accept, t.trade_id, &bob, vec![], vec![])
            .unwrap_err()
            .contains("offered to")
    );
    assert!(trade(&hub, &cat, TradeAction::Confirm, t.trade_id, &bob, vec![], vec![]).is_err());
    trade(&hub, &bob, TradeAction::Accept, t.trade_id, &cat, vec![], vec![]).unwrap();
    let half = trade(&hub, &cat, TradeAction::Confirm, t.trade_id, &bob, vec![], vec![]).unwrap();
    assert_eq!((half.from_confirmed, half.to_confirmed, half.state.as_str()), (true, false, "accepted"));
    assert_eq!(serials(&hub, &cat), vec![1]);
    let done = trade(&hub, &bob, TradeAction::Confirm, t.trade_id, &cat, vec![], vec![]).unwrap();
    assert_eq!(done.state, "done");
    assert_eq!((serials(&hub, &cat), serials(&hub, &bob)), (vec![2], vec![1]));
    let gone = trades(&hub, &ann).into_iter().find(|t| t.trade_id == stale.trade_id).unwrap();
    assert_eq!(gone.state, "gone");

    let d = trade(&hub, &bob, TradeAction::Offer, 0, &cat, vec![one.clone()], vec![]).unwrap();
    assert_eq!(
        trade(&hub, &cat, TradeAction::Decline, d.trade_id, &bob, vec![], vec![]).unwrap().state,
        "declined"
    );
    assert!(trade(&hub, &cat, TradeAction::Accept, d.trade_id, &bob, vec![], vec![]).is_err());
    assert!(trade(&hub, &bob, TradeAction::Offer, 0, &bob, vec![one], vec![]).is_err());
}
