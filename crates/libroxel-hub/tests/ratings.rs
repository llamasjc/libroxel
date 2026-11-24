//! Likes, favorites and comments on games.

use libroxel_hub::{
    AccountUpdate, HubClient, NewComment, Publish, Rate, RemoveComment, Report, Vote, account_message,
    comment_message, new_secret, player_id_of, rate_message, remove_comment_message, report_message, sign,
    start,
};

struct Player {
    key: [u8; 32],
    id: String,
    clock: std::cell::Cell<i64>,
}

impl Player {
    fn new(seed: u8) -> Self {
        let key = [seed; 32];
        let clock = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        Self { key, id: player_id_of(&key), clock: std::cell::Cell::new(clock as i64) }
    }

    fn tick(&self) -> i64 {
        self.clock.set(self.clock.get() + 1);
        self.clock.get()
    }

    fn claim(&self, hub: &HubClient, name: &str) {
        let time = self.tick() / 1000;
        let look = serde_json::json!({});
        hub.update_account(&AccountUpdate {
            player_id: self.id.clone(),
            name: name.into(),
            about: String::new(),
            look: look.clone(),
            time,
            signature: sign(&self.key, &account_message(&self.id, name, "", &look, time)),
            work: Some(libroxel_hub::account_work(&self.id, name)),
            email: None,
        })
        .unwrap();
    }

    fn rate(
        &self,
        hub: &HubClient,
        game: &str,
        vote: Vote,
        favorite: bool,
    ) -> Result<libroxel_hub::Ratings, String> {
        let time = self.tick();
        let signature = sign(&self.key, &rate_message(&self.id, game, vote, favorite, time));
        hub.rate(game, &Rate { player_id: self.id.clone(), vote, favorite, time, signature })
    }

    fn comment(&self, hub: &HubClient, game: &str, text: &str) -> Result<libroxel_hub::Comment, String> {
        let time = self.tick();
        let signature = sign(&self.key, &comment_message(&self.id, game, text, time));
        hub.comment(game, &NewComment { player_id: self.id.clone(), text: text.into(), time, signature })
    }
}

#[test]
fn players_like_favorite_and_comment() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (alice, bo, cy, nobody) = (Player::new(1), Player::new(2), Player::new(3), Player::new(4));
    alice.claim(&hub, "Alice");
    bo.claim(&hub, "Bob");
    cy.claim(&hub, "Cyd");
    let secret = new_secret();
    let publish_secret = secret.clone();
    let game = hub
        .publish(&Publish {
            game_id: None,
            secret: publish_secret.clone(),
            title: "Lava Tower".into(),
            author: "Alice".into(),
            description: String::new(),
            place: "{}".into(),
            author_id: Some(alice.id.clone()),
            author_signature: Some(sign(&alice.key, &libroxel_hub::publish_message(&publish_secret))),
        })
        .unwrap()
        .game_id;

    assert!(nobody.rate(&hub, &game, Vote::Like, false).unwrap_err().contains("account"));
    assert!(bo.rate(&hub, "nothing", Vote::Like, false).is_err());

    let got = bo.rate(&hub, &game, Vote::Like, true).unwrap();
    assert_eq!((got.likes, got.dislikes, got.favorites, got.vote, got.favorite), (1, 0, 1, Vote::Like, true));
    cy.rate(&hub, &game, Vote::Dislike, false).unwrap();
    let got = bo.rate(&hub, &game, Vote::None, true).unwrap();
    assert_eq!((got.likes, got.dislikes, got.favorites, got.vote), (0, 1, 1, Vote::None));

    let info = hub.game(&game).unwrap();
    assert_eq!((info.likes, info.dislikes, info.favorites), (0, 1, 1));
    assert_eq!(hub.ratings(&game, Some(&cy.id)).unwrap().vote, Vote::Dislike);
    assert_eq!(hub.favorites(&bo.id).unwrap(), vec![game.clone()]);

    let time = bo.tick();
    let rate = Rate {
        player_id: bo.id.clone(),
        vote: Vote::Like,
        favorite: false,
        time,
        signature: sign(&bo.key, &rate_message(&bo.id, &game, Vote::Like, false, time)),
    };
    hub.rate(&game, &rate).unwrap();
    assert!(hub.rate(&game, &rate).is_err());

    let said = bo.comment(&hub, &game, "fun game, you are a shithead").unwrap();
    assert_eq!(said.name, "Bob");
    assert!(!said.text.contains("shithead"));
    assert!(bo.comment(&hub, &game, " ").is_err());
    assert!(bo.comment(&hub, &game, &"a".repeat(400)).is_err());
    cy.comment(&hub, &game, "Too hard").unwrap();
    let list = hub.comments(&game).unwrap();
    assert_eq!(list.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Cyd", "Bob"]);

    let remove = |who: &Player, id: i64| {
        let time = who.tick();
        hub.remove_comment(
            id,
            &RemoveComment {
                player_id: who.id.clone(),
                time,
                signature: sign(&who.key, &remove_comment_message(&who.id, id, time)),
            },
        )
    };
    assert!(remove(&cy, said.comment_id).is_err());
    remove(&alice, said.comment_id).unwrap();
    assert_eq!(hub.comments(&game).unwrap().len(), 1);

    let target = hub.comments(&game).unwrap()[0].comment_id.to_string();
    for who in [&alice, &bo, &cy] {
        let time = 1;
        hub.report(&Report {
            reporter_id: who.id.clone(),
            kind: "comment".into(),
            target: target.clone(),
            reason: "mean".into(),
            evidence: String::new(),
            time,
            signature: sign(&who.key, &report_message(&who.id, "comment", &target, "mean", "", time)),
        })
        .unwrap();
    }
    assert!(hub.comments(&game).unwrap().is_empty());
}
