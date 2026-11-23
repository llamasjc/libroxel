use std::sync::{Arc, Mutex};

use libroxel_hub::{
    AccountUpdate, Birthday, Chat, Consent, FamilyChange, FamilySettings, FriendAction, FriendChange,
    HubClient, Mail, Messages, ParentEmail, account_message, birthday_message, consent_message,
    family_message, family_read_message, friend_message, parent_email_message, player_id_of, sign,
};

fn ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn claim(hub: &HubClient, key: &[u8; 32], name: &str) {
    let id = player_id_of(key);
    let look = serde_json::json!({});
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
    .unwrap();
}

fn years_ago(years: i64) -> String {
    let year = 1970 + ms() / 1000 / 86_400 * 400 / 146_097;
    format!("{}-01-01", year - years)
}

fn birthday(
    hub: &HubClient,
    key: &[u8; 32],
    born: &str,
    parent: Option<&str>,
) -> Result<libroxel_hub::Family, String> {
    let mut b = Birthday {
        player_id: player_id_of(key),
        born: born.into(),
        parent_email: parent.map(str::to_owned),
        utc_offset_mins: 0,
        time: ms(),
        signature: String::new(),
    };
    b.signature = sign(key, &birthday_message(&b));
    hub.set_birthday(&b)
}

fn read(hub: &HubClient, key: &[u8; 32]) -> libroxel_hub::Family {
    let (id, t) = (player_id_of(key), ms());
    hub.family(&id, t, &sign(key, &family_read_message(&id, t))).unwrap()
}

fn change(
    hub: &HubClient,
    key: &[u8; 32],
    pin: &str,
    new_pin: Option<&str>,
    settings: FamilySettings,
) -> Result<libroxel_hub::Family, String> {
    let mut c = FamilyChange {
        player_id: player_id_of(key),
        pin: pin.into(),
        new_pin: new_pin.map(str::to_owned),
        settings,
        time: ms(),
        signature: String::new(),
    };
    c.signature = sign(key, &family_message(&c));
    hub.change_family(&c)
}

#[test]
fn under_13_chats_strictly_and_a_parent_unlocks_the_rest() {
    let outbox = Arc::new(Mutex::new(Vec::new()));
    let options = libroxel_hub::HubOptions { mail: Mail::Outbox(outbox.clone()), ..Default::default() };
    let running = libroxel_hub::start_with("127.0.0.1:0".parse().unwrap(), None, options).unwrap();
    let hub = HubClient::new(&running.url());
    let (kid, teen) = ([71u8; 32], [72u8; 32]);
    claim(&hub, &kid, "kiddo");
    claim(&hub, &teen, "teenager");
    outbox.lock().unwrap().clear();
    assert!(!read(&hub, &kid).has_birthday);
    assert_eq!(hub.chat_rule(&player_id_of(&kid)).unwrap().chat, Chat::Strict);

    assert!(birthday(&hub, &kid, "2999-01-01", Some("mum@example.com")).is_err());
    let family = birthday(&hub, &kid, &years_ago(9), Some("Mum@Example.com")).unwrap();
    assert!(family.under_13 && !family.consent);
    assert_eq!(family.settings.chat, Chat::Strict);
    assert_eq!(family.settings.messages, Messages::Friends);
    assert!(!family.settings.friend_requests);
    assert!(birthday(&hub, &kid, &years_ago(20), None).unwrap_err().contains("already"));
    let (to, code) = outbox.lock().unwrap().pop().unwrap();
    assert_eq!(to, "mum@example.com");
    let open = FamilySettings {
        chat: Chat::Standard,
        messages: Messages::Everyone,
        friend_requests: true,
        spend_limit: None,
        playtime_mins: None,
    };
    assert!(change(&hub, &kid, "", Some("1234"), open).unwrap_err().contains("approve"));
    let t = ms();
    let ask = ParentEmail {
        player_id: player_id_of(&kid),
        parent_email: "dad@example.com".into(),
        time: t,
        signature: sign(&kid, &parent_email_message(&player_id_of(&kid), "dad@example.com", t)),
    };
    assert!(hub.ask_parent(&ask).unwrap_err().contains("just sent"));

    let consent = |code: &str, pin: &str| {
        let mut c = Consent {
            player_id: player_id_of(&kid),
            code: code.into(),
            pin: pin.into(),
            time: ms(),
            signature: String::new(),
        };
        c.signature = sign(&kid, &consent_message(&c));
        hub.consent(&c)
    };
    assert!(consent("000000", "1234").is_err() || code == "000000");
    let family = consent(&code, "4321").unwrap();
    assert!(family.consent && family.has_pin);
    assert_eq!(family.settings.chat, Chat::Strict);
    assert!(change(&hub, &kid, "1111", None, open).unwrap_err().contains("not right"));
    let family = change(&hub, &kid, "4321", None, open).unwrap();
    assert_eq!(family.settings.chat, Chat::Strict);
    let no_friends = FamilySettings { friend_requests: false, playtime_mins: Some(0), ..open };
    change(&hub, &kid, "4321", None, no_friends).unwrap();
    birthday(&hub, &teen, &years_ago(15), None).unwrap();
    assert_eq!(hub.chat_rule(&player_id_of(&teen)).unwrap().chat, Chat::Standard);
    let t = ms();
    let ask = FriendChange {
        player_id: player_id_of(&teen),
        other_id: player_id_of(&kid),
        action: FriendAction::Add,
        time: t,
        signature: sign(
            &teen,
            &friend_message(&player_id_of(&teen), &player_id_of(&kid), FriendAction::Add, t),
        ),
    };
    assert!(hub.change_friend(&ask).unwrap_err().contains("can't take friend requests"));
    assert!(change(&hub, &teen, "", None, open).unwrap_err().contains("PIN"));
    change(&hub, &teen, "", Some("9999"), FamilySettings { chat: Chat::Off, ..open }).unwrap();
    assert_eq!(hub.chat_rule(&player_id_of(&teen)).unwrap().chat, Chat::Off);
    assert!(change(&hub, &teen, "", None, open).is_err());
    for _ in 0..5 {
        let _ = change(&hub, &teen, "0000", None, open);
    }
    assert!(change(&hub, &teen, "9999", None, open).unwrap_err().contains("wait"));
}
