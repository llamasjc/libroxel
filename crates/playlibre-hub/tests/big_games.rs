use playlibre_hub::{
    DownloadProof, HubClient, Publish, new_secret, player_id_of, publish_message, sign, start,
};

#[test]
fn big_games_publish_and_download() {
    let hub = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let client = HubClient::new(&hub.url());
    let place = format!("{{\"parts\":\"{}\"}}", "part ".repeat(5 * 1024 * 1024));
    let key = [7; 32];
    let secret = new_secret();
    let done = client
        .publish(&Publish {
            game_id: None,
            secret: secret.clone(),
            title: "Big".into(),
            author: "Alice".into(),
            description: String::new(),
            place: place.clone(),
            author_id: Some(player_id_of(&key)),
            author_signature: Some(sign(&key, &publish_message(&secret))),
        })
        .unwrap();
    let proof = DownloadProof::new(&done.game_id, player_id_of(&key), |m| sign(&key, m));
    assert_eq!(client.download(&done.game_id, Some(&proof)).unwrap(), place);
}
