//! Following games and creators, and notifications.

use playlibre_hub::{
    AccountUpdate, FollowChange, FollowKind, FriendAction, FriendChange, HubClient, NotificationsSeen,
    Publish, account_message, follow_message, follows_message, friend_message, notifications_message,
    notifications_seen_message, player_id_of, publish_message, sign, start,
};

fn ms() -> i64 {
    std::thread::sleep(std::time::Duration::from_millis(2));
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn claim(hub: &HubClient, key: &[u8; 32], name: &str) -> String {
    let id = player_id_of(key);
    let time = ms() / 1000;
    let look = serde_json::json!({});
    hub.update_account(&AccountUpdate {
        work: Some(playlibre_hub::account_work(&id, name)),
        email: None,
        signature: sign(key, &account_message(&id, name, "", &look, time)),
        player_id: id.clone(),
        name: name.into(),
        about: String::new(),
        look,
        time,
    })
    .unwrap();
    id
}

fn publish(hub: &HubClient, key: &[u8; 32], game_id: Option<String>, title: &str) -> String {
    let secret = "s".repeat(20);
    hub.publish(&Publish {
        game_id,
        secret: secret.clone(),
        title: title.into(),
        author: String::new(),
        description: String::new(),
        place: format!(r#"{{"t":"{title}{}"}}"#, ms()),
        author_id: Some(player_id_of(key)),
        author_signature: Some(sign(key, &publish_message(&secret))),
    })
    .unwrap()
    .game_id
}

fn follow(
    hub: &HubClient,
    key: &[u8; 32],
    kind: FollowKind,
    target: &str,
    on: bool,
) -> Result<playlibre_hub::Following, String> {
    let mut c = FollowChange {
        player_id: player_id_of(key),
        kind,
        target: target.into(),
        follow: on,
        time: ms(),
        signature: String::new(),
    };
    c.signature = sign(key, &follow_message(&c));
    hub.change_follow(&c)
}

fn notes(hub: &HubClient, key: &[u8; 32]) -> playlibre_hub::Notifications {
    let (id, time) = (player_id_of(key), ms());
    hub.notifications(&id, time, &sign(key, &notifications_message(&id, time))).unwrap()
}

#[test]
fn followers_hear_about_updates_and_new_games() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (maker, fan) = ([41u8; 32], [42u8; 32]);
    let maker_id = claim(&hub, &maker, "maker");
    claim(&hub, &fan, "fan");
    let game = publish(&hub, &maker, None, "Sky Tower");

    assert!(follow(&hub, &fan, FollowKind::Game, "no-such-game", true).is_err());
    assert!(follow(&hub, &maker, FollowKind::Creator, &maker_id, true).is_err());
    follow(&hub, &fan, FollowKind::Game, &game, true).unwrap();
    let following = follow(&hub, &fan, FollowKind::Creator, &maker_id, true).unwrap();
    assert_eq!((following.games.len(), following.creators.len()), (1, 1));
    assert_eq!(hub.game(&game).unwrap().followers, 1);
    let (fan_id, time) = (player_id_of(&fan), ms());
    assert!(hub.following(&fan_id, time, &sign(&maker, &follows_message(&fan_id, time))).is_err());
    assert_eq!(
        hub.following(&fan_id, time, &sign(&fan, &follows_message(&fan_id, time))).unwrap(),
        following
    );

    publish(&hub, &maker, Some(game.clone()), "Sky Tower");
    publish(&hub, &maker, Some(game.clone()), "Sky Tower");
    publish(&hub, &maker, None, "Cloud Kart");
    let n = notes(&hub, &fan);
    // Two updates of one game are one notification.
    assert_eq!(n.unseen, 2, "{n:?}");
    assert_eq!(n.items[0].text, "maker put out a new game: Cloud Kart");
    assert_eq!(n.items[1].text, "Sky Tower was updated");
    assert!(n.items[1].link.starts_with("/games/"));

    // A friend request is one too, and seeing them clears the count.
    let time = ms();
    hub.change_friend(&FriendChange {
        player_id: player_id_of(&maker),
        other_id: fan_id.clone(),
        action: FriendAction::Add,
        time,
        signature: sign(&maker, &friend_message(&maker_id, &fan_id, FriendAction::Add, time)),
    })
    .unwrap();
    let n = notes(&hub, &fan);
    assert_eq!(n.unseen, 3);
    assert_eq!(n.items[0].kind, "friend");
    let (up_to, time) = (n.items[0].notification_id, ms());
    let seen = hub
        .notifications_seen(&NotificationsSeen {
            player_id: fan_id.clone(),
            up_to,
            time,
            signature: sign(&fan, &notifications_seen_message(&fan_id, up_to, time)),
        })
        .unwrap();
    assert_eq!(seen.unseen, 0);

    follow(&hub, &fan, FollowKind::Game, &game, false).unwrap();
    assert_eq!(hub.game(&game).unwrap().followers, 0);
}
