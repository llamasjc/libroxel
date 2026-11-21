//! Online chess on a Hub on this computer: quick matches, challenges,
//! moves checked by the rules, draws, resigning, ratings and rematches.

use libroxel_hub::{
    AccountUpdate, ChessAction, ChessLobby, ChessRequest, HubClient, account_message, chess_message,
    player_id_of, sign, start,
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
        let message = account_message(&self.id, name, "", &look, time);
        hub.update_account(&AccountUpdate {
            player_id: self.id.clone(),
            name: name.into(),
            about: String::new(),
            look,
            time,
            signature: sign(&self.key, &message),
            work: Some(libroxel_hub::account_work(&self.id, name)),
            email: None,
        })
        .unwrap();
    }

    fn try_do(&self, hub: &HubClient, action: ChessAction) -> Result<ChessLobby, String> {
        let time = self.tick();
        let action = serde_json::to_string(&action).unwrap();
        let signature = sign(&self.key, &chess_message(&self.id, time, &action));
        hub.chess(&ChessRequest { player_id: self.id.clone(), time, action, signature })
    }

    fn act(&self, hub: &HubClient, action: ChessAction) -> ChessLobby {
        self.try_do(hub, action).unwrap()
    }

    fn mv(&self, hub: &HubClient, game: &str, uci: &str, ply: u32) -> ChessLobby {
        self.act(hub, ChessAction::Move { game_id: game.into(), uci: uci.into(), ply })
    }
}

#[test]
fn quick_match_mate_ratings_and_rematch() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (a, b) = (Player::new(11), Player::new(12));
    a.claim(&hub, "Ash");
    b.claim(&hub, "Birch");

    let lobby = a.act(&hub, ChessAction::Seek { control: "3+2".into(), rated: true });
    assert!(lobby.seeking.is_some());
    assert!(lobby.active.is_empty());
    let lobby = b.act(&hub, ChessAction::Seek { control: "3+2".into(), rated: true });
    assert_eq!(lobby.active.len(), 1, "two players looking for 3+2 are paired");
    let game = lobby.active[0].clone();
    assert_eq!(game.control, "3+2");
    assert_eq!(game.category, "blitz");
    let (white, black) = if game.white.player_id == a.id { (&a, &b) } else { (&b, &a) };
    assert_eq!(game.white.name.len() + game.black.name.len(), 8);

    // Moves out of turn, illegal moves and stale ones are refused.
    assert!(
        black
            .try_do(&hub, ChessAction::Move { game_id: game.id.clone(), uci: "e7e5".into(), ply: 0 })
            .is_err()
    );
    assert!(
        white
            .try_do(&hub, ChessAction::Move { game_id: game.id.clone(), uci: "e2e5".into(), ply: 0 })
            .is_err()
    );
    white.mv(&hub, &game.id, "f2f3", 0);
    assert!(
        white
            .try_do(&hub, ChessAction::Move { game_id: game.id.clone(), uci: "e2e4".into(), ply: 0 })
            .is_err()
    );
    black.mv(&hub, &game.id, "e7e5", 1);
    white.mv(&hub, &game.id, "g2g4", 2);
    let lobby = black.mv(&hub, &game.id, "d8h4", 3);
    let over = lobby.game.clone().unwrap();
    assert_eq!(over.status, "over");
    assert_eq!(over.winner.as_deref(), Some("black"));
    assert_eq!(over.reason.as_deref(), Some("checkmate"));
    assert_eq!(over.black_change, Some(20));
    assert_eq!(over.white_change, Some(-20));
    let blitz = |l: &ChessLobby| l.ratings.iter().find(|r| r.category == "blitz").unwrap().clone();
    assert_eq!(blitz(&lobby).rating, 1220);
    assert_eq!(blitz(&lobby).wins, 1);

    // A rematch needs both, and swaps colours.
    let lobby = white.act(&hub, ChessAction::Rematch { game_id: game.id.clone() });
    assert_eq!(lobby.game.unwrap().rematch_offer.as_deref(), Some("white"));
    let lobby = black.act(&hub, ChessAction::Rematch { game_id: game.id.clone() });
    let rematch = lobby.game.unwrap();
    assert_ne!(rematch.id, game.id);
    assert_eq!(rematch.white.player_id, black.id);
    assert_eq!(rematch.status, "playing");

    // Resigning before both have moved only calls it off.
    let lobby = black.act(&hub, ChessAction::Resign { game_id: rematch.id.clone() });
    assert_eq!(lobby.game.clone().unwrap().reason.as_deref(), Some("aborted"));
    assert_eq!(blitz(&lobby).rating, 1220);

    let board = hub.chess_leaderboard("blitz").unwrap();
    assert_eq!(board[0].player.player_id, black.id);
    assert_eq!(board.len(), 2);
    let profile = hub.chess_profile(&black.id).unwrap();
    assert_eq!(profile.recent.len(), 1);
}

#[test]
fn challenges_draws_and_resigning() {
    let running = start("127.0.0.1:0".parse().unwrap(), None).unwrap();
    let hub = HubClient::new(&running.url());
    let (a, b) = (Player::new(21), Player::new(22));
    a.claim(&hub, "Cedar");
    b.claim(&hub, "Dune");

    let lobby = a.act(
        &hub,
        ChessAction::Challenge {
            to: b.id.clone(),
            control: "10+0".into(),
            rated: false,
            color: Some("white".into()),
        },
    );
    assert_eq!(lobby.outgoing.len(), 1);
    let lobby = b.act(&hub, ChessAction::Sync { game_id: None });
    assert_eq!(lobby.incoming.len(), 1);
    assert_eq!(lobby.incoming[0].from.name, "Cedar");
    let lobby = b.act(&hub, ChessAction::AnswerChallenge { id: lobby.incoming[0].id.clone(), accept: true });
    let game = lobby.game.unwrap();
    assert_eq!(game.white.player_id, a.id, "the challenger asked for white");
    assert!(!game.rated);
    assert!(game.first_move_ms.is_some());

    a.mv(&hub, &game.id, "e2e4", 0);
    let lobby = b.mv(&hub, &game.id, "e7e5", 1);
    assert!(lobby.game.unwrap().clock_running);
    // A draw offered and declined, then offered and accepted.
    a.act(&hub, ChessAction::OfferDraw { game_id: game.id.clone() });
    let lobby = b.act(&hub, ChessAction::DeclineDraw { game_id: game.id.clone() });
    assert_eq!(lobby.game.unwrap().draw_offer, None);
    a.act(&hub, ChessAction::OfferDraw { game_id: game.id.clone() });
    let lobby = b.act(&hub, ChessAction::OfferDraw { game_id: game.id.clone() });
    let over = lobby.game.clone().unwrap();
    assert_eq!(over.winner.as_deref(), Some("draw"));
    assert_eq!(over.reason.as_deref(), Some("agreement"));
    assert_eq!(over.white_change, None, "casual games don't move ratings");

    // Chat is a fixed set of phrases.
    let lobby = a.act(&hub, ChessAction::Chat { game_id: game.id.clone(), phrase: 8 });
    assert_eq!(lobby.game.unwrap().chat.len(), 1);

    // A new challenge, resigned after it starts.
    a.act(&hub, ChessAction::Challenge { to: b.id.clone(), control: "5+0".into(), rated: true, color: None });
    let id = b.act(&hub, ChessAction::Sync { game_id: None }).incoming[0].id.clone();
    let game = b.act(&hub, ChessAction::AnswerChallenge { id, accept: true }).game.unwrap();
    let (white, black) = if game.white.player_id == a.id { (&a, &b) } else { (&b, &a) };
    white.mv(&hub, &game.id, "d2d4", 0);
    black.mv(&hub, &game.id, "d7d5", 1);
    assert!(white.try_do(&hub, ChessAction::ClaimWin { game_id: game.id.clone() }).is_err());
    let over = white.act(&hub, ChessAction::Resign { game_id: game.id.clone() }).game.unwrap();
    assert_eq!(over.reason.as_deref(), Some("resign"));
    assert_eq!(over.winner.as_deref(), Some("black"));
    assert_eq!(hub.chess_game(&game.id).unwrap().status, "over");
    assert!(hub.chess_live().unwrap().is_empty());
}
