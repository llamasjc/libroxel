use playlibre_hub::{HubClient, Publish, new_secret, start};

#[test]
fn big_games_publish_and_download() {
    let hub = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let client = HubClient::new(&hub.url());
    let place = format!("{{\"parts\":\"{}\"}}", "part ".repeat(5 * 1024 * 1024));
    let done = client
        .publish(&Publish {
            game_id: None,
            secret: new_secret(),
            title: "Big".into(),
            author: "Alice".into(),
            description: String::new(),
            place: place.clone(),
            author_id: None,
            author_signature: None,
        })
        .unwrap();
    assert_eq!(client.download(&done.game_id, None).unwrap(), place);
}
