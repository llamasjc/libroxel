//! A game's own pictures, genre and tags.

use playlibre_hub::{
    Change, HubClient, ICON_SIZE, ManageGame, MediaChange, Publish, manage_message, media_message,
    player_id_of, publish_message, sign, start,
};

fn ms() -> i64 {
    std::thread::sleep(std::time::Duration::from_millis(2));
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn publish(hub: &HubClient, key: &[u8; 32]) -> String {
    let secret = "s".repeat(20);
    hub.publish(&Publish {
        game_id: None,
        secret: secret.clone(),
        title: "Sky Tower".into(),
        author: String::new(),
        description: String::new(),
        place: "{}".into(),
        author_id: Some(player_id_of(key)),
        author_signature: Some(sign(key, &publish_message(&secret))),
    })
    .unwrap()
    .game_id
}

fn picture(key: &[u8; 32], game: &str, slot: u32, bytes: Option<&[u8]>) -> MediaChange {
    let player_id = player_id_of(key);
    let time = ms();
    MediaChange {
        signature: sign(key, &media_message(&player_id, game, time, slot, bytes)),
        player_id,
        time,
        slot,
        image: bytes.map(|b| data_encoding::BASE64.encode(b)),
    }
}

/// A tiny PNG: one red pixel.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
    0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18,
    0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn ask(key: &[u8; 32], game: &str, change: Change) -> ManageGame {
    let player_id = player_id_of(key);
    let time = ms();
    let signature = sign(key, &manage_message(&player_id, game, time, &change));
    ManageGame { player_id, time, signature, change }
}

#[test]
fn creators_put_up_pictures_and_pick_a_genre_and_tags() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (me, them) = ([5u8; 32], [6u8; 32]);
    let game = publish(&hub, &me);

    assert!(hub.change_media(&game, &picture(&them, &game, 0, Some(PIXEL))).is_err());
    assert!(hub.change_media(&game, &picture(&me, &game, 0, Some(b"not a picture"))).is_err());
    assert!(hub.change_media(&game, &picture(&me, &game, 9, Some(PIXEL))).is_err());
    hub.change_media(&game, &picture(&me, &game, 0, Some(PIXEL))).unwrap();
    let refs = hub.change_media(&game, &picture(&me, &game, 2, Some(PIXEL))).unwrap();
    assert_eq!(refs.iter().map(|r| r.slot).collect::<Vec<_>>(), [0, 2]);
    let info = hub.game(&game).unwrap();
    assert_eq!(info.media, refs);
    let icon = hub.media(&game, &info.media[0]).unwrap();
    assert_eq!(&icon[1..4], b"PNG");
    let size = u32::from_be_bytes(icon[16..20].try_into().unwrap());
    assert_eq!(size, ICON_SIZE);
    let refs = hub.change_media(&game, &picture(&me, &game, 2, None)).unwrap();
    assert_eq!(refs.len(), 1);

    let details = |genre: &str, tags: &[&str]| Change::Details {
        genre: genre.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
    };
    assert!(hub.manage(&game, &ask(&them, &game, details("Obby", &[]))).is_err());
    assert!(hub.manage(&game, &ask(&me, &game, details("Not a genre", &[]))).is_err());
    assert!(hub.manage(&game, &ask(&me, &game, details("Obby", &["a", "b", "c", "d", "e", "f"]))).is_err());
    hub.manage(&game, &ask(&me, &game, details("obby", &["Hard  Mode", "tower", "tower"]))).unwrap();
    let info = hub.game(&game).unwrap();
    assert_eq!(info.genre.as_deref(), Some("Obby"));
    assert_eq!(info.tags, ["hard mode", "tower"]);
    assert_eq!(hub.games_with("", "tower", "").unwrap().len(), 1);
    assert_eq!(hub.games_with("", "towe", "").unwrap().len(), 0);
    assert_eq!(hub.games_with("", "", "Obby").unwrap().len(), 1);
    assert_eq!(hub.games_with("", "", "Racing").unwrap().len(), 0);
    assert_eq!(hub.games("hard mode").unwrap().len(), 1);
}
