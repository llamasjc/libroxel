//! Every game has a number, for its page on the website
//! (`/games/<number>/<name>`).

use playlibre_hub::{
    GameInfo, HubClient, Publish, StarterPlace, game_page, game_slug, new_secret, open_db, player_id_of,
    publish_message, set_official, sign, start,
};

fn starter(name: &str) -> StarterPlace {
    StarterPlace {
        name: name.into(),
        about: name.into(),
        symbol: String::new(),
        place: r#"{"format":"libroxel-place","children":[]}"#.into(),
    }
}

fn game(title: &str, secret: &str) -> Publish {
    let alice = [7; 32];
    Publish {
        game_id: None,
        secret: secret.into(),
        title: title.into(),
        author: "Alice".into(),
        description: String::new(),
        place: r#"{"version":1,"root":{}}"#.into(),
        author_id: Some(player_id_of(&alice)),
        author_signature: Some(sign(&alice, &publish_message(secret))),
    }
}

fn info(hub: &str, id: &str) -> Option<GameInfo> {
    let mut answer =
        ureq::get(format!("{hub}/games/{id}")).config().http_status_as_error(false).build().call().unwrap();
    (answer.status() == 200).then(|| answer.body_mut().read_json().unwrap())
}

#[test]
fn games_get_the_next_number_and_keep_it() {
    let dir = std::env::temp_dir().join(format!("playlibre-numbers-{}", new_secret()));
    let path = dir.join("hub.sqlite");
    let db = open_db(Some(&path)).unwrap();
    set_official(&db, &[starter("Chess"), starter("Kart")]).unwrap();
    drop(db);
    let hub = start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    let client = HubClient::new(&hub.url());
    let starters: Vec<_> = client.official().unwrap().iter().map(|g| (g.name.clone(), g.number)).collect();
    assert_eq!(starters, [("Chess".to_owned(), Some(1)), ("Kart".to_owned(), Some(2))]);

    let secret = new_secret();
    let lava = client.publish(&game("Lava Tower", &secret)).unwrap().game_id;
    let obby = client.publish(&game("Obby", &secret)).unwrap().game_id;
    let numbers =
        |list: Vec<GameInfo>, id: &str| list.into_iter().find(|g| g.game_id == id).and_then(|g| g.number);
    assert_eq!(numbers(client.games("").unwrap(), &lava), Some(3));
    assert_eq!(numbers(client.games("").unwrap(), &obby), Some(4));

    // A game's number finds it, as its id does.
    assert_eq!(info(&hub.url(), "3").unwrap().game_id, lava);
    assert_eq!(info(&hub.url(), &obby).unwrap().number, Some(4));
    assert!(info(&hub.url(), "99").is_none());

    // A new version keeps the number; a game taken down keeps it from others.
    let again = Publish { game_id: Some(lava.clone()), ..game("Lava Tower 2", &secret) };
    client.publish(&again).unwrap();
    assert_eq!(info(&hub.url(), "3").unwrap().title, "Lava Tower 2");
    client.unpublish(&obby, &secret).unwrap();
    assert!(info(&hub.url(), "4").is_none());
    let next = client.publish(&game("Kart Racer", &secret)).unwrap().game_id;
    assert_eq!(info(&hub.url(), &next).unwrap().number, Some(5));
    drop(hub);

    // Starters keep theirs when the Hub refreshes them on start.
    let db = open_db(Some(&path)).unwrap();
    set_official(&db, &[starter("Kart"), starter("Chess"), starter("Tag")]).unwrap();
    let starters: Vec<_> = playlibre_hub::official(&db).iter().map(|g| (g.name.clone(), g.number)).collect();
    assert_eq!(
        starters,
        [("Kart".to_owned(), Some(2)), ("Chess".to_owned(), Some(1)), ("Tag".to_owned(), Some(6))]
    );
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_hub_from_before_numbers_numbers_its_games_on_start() {
    let dir = std::env::temp_dir().join(format!("playlibre-old-numbers-{}", new_secret()));
    let path = dir.join("hub.sqlite");
    let db = open_db(Some(&path)).unwrap();
    set_official(&db, &[starter("Chess")]).unwrap();
    // As it was before numbers: games, and nothing numbering them.
    db.execute_batch(
        "DROP TRIGGER games_get_numbers; DROP TRIGGER starters_get_numbers; DROP TABLE game_numbers;
         INSERT INTO games (game_id, secret, title, author, description, place, version, plays, created, updated)
         VALUES ('newer', 's', 'Newer', 'A', '', '{}', 1, 0, 200, 200),
                ('older', 's', 'Older', 'A', '', '{}', 1, 0, 100, 100);",
    )
    .unwrap();
    drop(db);
    let hub = start("127.0.0.1:0".parse().unwrap(), Some(&path)).unwrap();
    assert_eq!(HubClient::new(&hub.url()).official().unwrap()[0].number, Some(1));
    assert_eq!(info(&hub.url(), "older").unwrap().number, Some(2));
    assert_eq!(info(&hub.url(), "newer").unwrap().number, Some(3));
    assert_eq!(info(&hub.url(), "3").unwrap().game_id, "newer");
    drop(hub);
    // Opening it again changes nothing.
    let db = open_db(Some(&path)).unwrap();
    let count: i64 = db.query_row("SELECT COUNT(*) FROM game_numbers", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 3);
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn page_addresses_read_like_the_title() {
    assert_eq!(game_slug("My Cool Game!"), "my-cool-game");
    assert_eq!(game_slug("  Obby -- 2: The   Return "), "obby-2-the-return");
    assert_eq!(game_slug("!!!"), "");
    assert_eq!(game_slug(&"ab ".repeat(40)).len(), 59);
    assert_eq!(
        game_page("https://playlibre.net", 12, "My Cool Game!"),
        "https://playlibre.net/games/12/my-cool-game"
    );
    assert_eq!(game_page("https://playlibre.net", 3, "!!!"), "https://playlibre.net/games/3");
}
