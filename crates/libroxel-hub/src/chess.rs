//! Online chess: quick matches between strangers, challenges between
//! friends, clocks the Hub keeps, ratings and a leaderboard.
//!
//! Everything a player does is one signed [`ChessRequest`] to `POST /chess`.
//! The Hub checks each move with the same rules the app uses
//! ([`libroxel_chess`]), runs the clocks itself (so a slow connection never
//! steals time from anyone), and ends games on time, by agreement, by
//! resignation or when a player has been gone too long.
//!
//! Anyone may watch a game (`GET /chess/games/{id}`), list the games being
//! played (`GET /chess/live`), see the leaderboard
//! (`GET /chess/leaderboard?category=blitz`) or a player's ratings and
//! recent games (`GET /chess/players/{id}`).

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use libroxel_chess::{Category, Clocks, Color, Game, Outcome, Position, Reason, TimeControl, Winner, elo};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::Db;
use super::social::now_ms;

/// How far a signed time may be from the Hub's clock, in milliseconds.
const SLACK_MS: i64 = 10 * 60 * 1000;

/// A quick-match search drops off when its player stops asking for this long.
const SEEK_EXPIRES_MS: i64 = 20_000;

/// How long each side has to make their first move before the game is called off.
pub const FIRST_MOVE_MS: i64 = 30_000;

/// How long a player may be gone before their opponent may claim the win.
pub const GONE_MS: i64 = 60_000;

/// How long a challenge waits for an answer.
const CHALLENGE_EXPIRES_MS: i64 = 10 * 60 * 1000;

/// What players can say to each other during a game. Only these, so chat
/// stays friendly without anyone having to watch it.
pub const CHESS_PHRASES: [&str; 10] = [
    "Hi! Good luck!",
    "Have fun!",
    "Nice move!",
    "Oops!",
    "Hmm, let me think...",
    "Well played!",
    "Thanks!",
    "Sorry, gotta go soon.",
    "Good game!",
    "Rematch?",
];

/// Something a player does in chess.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum ChessAction {
    /// Reads the player's lobby (and one game, if given). Also keeps a
    /// quick-match search alive and says the player is still there.
    Sync {
        game_id: Option<String>,
    },
    /// Looks for an opponent with this clock (such as `3+2`).
    Seek {
        control: String,
        rated: bool,
    },
    CancelSeek,
    /// Invites one player to a game. `color` is `white`, `black` or empty for random.
    Challenge {
        to: String,
        control: String,
        rated: bool,
        color: Option<String>,
    },
    /// Accepts or declines a challenge (the one who sent it declines to take it back).
    AnswerChallenge {
        id: String,
        accept: bool,
    },
    /// A move in coordinates (`e2e4`, `e7e8q`), as the `ply`-th half-move.
    Move {
        game_id: String,
        uci: String,
        ply: u32,
    },
    Resign {
        game_id: String,
    },
    /// Offers a draw, or accepts the opponent's offer.
    OfferDraw {
        game_id: String,
    },
    DeclineDraw {
        game_id: String,
    },
    /// Calls the game off before both players have moved.
    Abort {
        game_id: String,
    },
    /// Wins a game whose opponent has been gone for a minute.
    ClaimWin {
        game_id: String,
    },
    /// Offers a rematch, or accepts the opponent's offer.
    Rematch {
        game_id: String,
    },
    /// Says one of [`CHESS_PHRASES`].
    Chat {
        game_id: String,
        phrase: u8,
    },
}

/// A signed chess request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChessRequest {
    pub player_id: String,
    /// Milliseconds since 1970. Each change must be newer than the player's
    /// last (reads only need to be near now).
    pub time: i64,
    /// The [`ChessAction`] as JSON, exactly as signed.
    pub action: String,
    /// [`chess_message`] signed with the player's identity key.
    pub signature: String,
}

/// What a player signs for a chess request.
pub fn chess_message(player_id: &str, time: i64, action: &str) -> Vec<u8> {
    format!("libroxel chess\n{player_id}\n{time}\n{action}").into_bytes()
}

/// One player in a game, or on a list.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChessPlayer {
    pub player_id: String,
    pub name: String,
    #[serde(default)]
    pub number: u64,
    pub rating: i32,
}

/// A player's rating in one category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChessRating {
    pub category: String,
    pub rating: i32,
    pub games: u32,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    /// Their highest rating so far.
    pub best: i32,
}

/// A line of quick chat.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChessChat {
    /// `white` or `black`.
    pub color: String,
    pub phrase: u8,
}

/// A game, as players and watchers see it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChessGame {
    pub id: String,
    pub white: ChessPlayer,
    pub black: ChessPlayer,
    /// The clock, such as `3+2`.
    pub control: String,
    pub category: String,
    pub rated: bool,
    /// Every move so far in coordinates, separated by spaces.
    pub moves: String,
    /// Time left on each clock right now (when the Hub answered).
    pub white_ms: i64,
    pub black_ms: i64,
    /// Whether the side to move's clock is running.
    pub clock_running: bool,
    /// How long the side to move has left to make a first move, if the game
    /// hasn't properly started yet.
    pub first_move_ms: Option<i64>,
    /// `playing` or `over`.
    pub status: String,
    /// `white`, `black` or `draw`, once over.
    pub winner: Option<String>,
    /// Why it ended (see [`libroxel_chess::Reason::code`]).
    pub reason: Option<String>,
    /// Which side offered a draw that hasn't been answered.
    pub draw_offer: Option<String>,
    /// Which side asked for a rematch.
    pub rematch_offer: Option<String>,
    /// The rematch, once both agreed.
    pub rematch_game: Option<String>,
    /// How ratings moved when the game ended.
    pub white_change: Option<i32>,
    pub black_change: Option<i32>,
    /// Seconds since each player last checked in.
    pub white_away_secs: i64,
    pub black_away_secs: i64,
    pub chat: Vec<ChessChat>,
    /// Seconds since 1970.
    pub created: i64,
    pub ended: Option<i64>,
}

/// A challenge from one player to another.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChessChallenge {
    pub id: String,
    pub from: ChessPlayer,
    pub to: ChessPlayer,
    pub control: String,
    pub rated: bool,
    /// The colour the challenger plays, if they picked one.
    pub color: Option<String>,
}

/// A quick-match search under way.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChessSeek {
    pub control: String,
    pub rated: bool,
    /// How long it has been looking, in seconds.
    pub waiting_secs: i64,
}

/// Everything a player's chess screen needs, in one answer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChessLobby {
    pub me: ChessPlayer,
    pub ratings: Vec<ChessRating>,
    pub seeking: Option<ChessSeek>,
    pub incoming: Vec<ChessChallenge>,
    pub outgoing: Vec<ChessChallenge>,
    /// Games this player is in that aren't over.
    pub active: Vec<ChessGame>,
    /// The game asked for (or a new one that just started for this player).
    pub game: Option<ChessGame>,
    /// How many players are looking for a quick match right now, by clock.
    pub seeking_counts: Vec<(String, u32)>,
    /// How many games are being played right now.
    pub playing_now: u32,
}

/// A finished game in a player's history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChessHistoryEntry {
    pub id: String,
    pub white: ChessPlayer,
    pub black: ChessPlayer,
    pub control: String,
    pub rated: bool,
    pub winner: Option<String>,
    pub reason: Option<String>,
    pub moves: u32,
    pub ended: i64,
}

/// A player's chess page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChessProfile {
    pub player: ChessPlayer,
    pub ratings: Vec<ChessRating>,
    pub recent: Vec<ChessHistoryEntry>,
}

/// One row of the leaderboard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChessLeader {
    pub rank: u32,
    pub player: ChessPlayer,
    pub games: u32,
    pub wins: u32,
}

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS chess_ratings (
            player_id   TEXT NOT NULL,
            category    TEXT NOT NULL,
            rating      INTEGER NOT NULL,
            games       INTEGER NOT NULL,
            wins        INTEGER NOT NULL,
            draws       INTEGER NOT NULL,
            losses      INTEGER NOT NULL,
            best        INTEGER NOT NULL,
            PRIMARY KEY (player_id, category)
        );
        CREATE INDEX IF NOT EXISTS chess_ratings_rank ON chess_ratings(category, rating);
        CREATE TABLE IF NOT EXISTS chess_seeks (
            player_id   TEXT PRIMARY KEY,
            control     TEXT NOT NULL,
            rated       INTEGER NOT NULL,
            rating      INTEGER NOT NULL,
            since       INTEGER NOT NULL,
            seen        INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS chess_challenges (
            id          TEXT PRIMARY KEY,
            from_id     TEXT NOT NULL,
            to_id       TEXT NOT NULL,
            control     TEXT NOT NULL,
            rated       INTEGER NOT NULL,
            color       TEXT,
            created     INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS chess_games (
            id            TEXT PRIMARY KEY,
            white         TEXT NOT NULL,
            black         TEXT NOT NULL,
            white_rating  INTEGER NOT NULL,
            black_rating  INTEGER NOT NULL,
            control       TEXT NOT NULL,
            rated         INTEGER NOT NULL,
            moves         TEXT NOT NULL,
            white_ms      INTEGER NOT NULL,
            black_ms      INTEGER NOT NULL,
            turn_since    INTEGER NOT NULL,
            status        TEXT NOT NULL,
            winner        TEXT,
            reason        TEXT,
            draw_offer    TEXT,
            rematch_offer TEXT,
            rematch_game  TEXT,
            white_change  INTEGER,
            black_change  INTEGER,
            white_seen    INTEGER NOT NULL,
            black_seen    INTEGER NOT NULL,
            chat          TEXT NOT NULL,
            created       INTEGER NOT NULL,
            ended         INTEGER
        );
        CREATE INDEX IF NOT EXISTS chess_games_white ON chess_games(white, status);
        CREATE INDEX IF NOT EXISTS chess_games_black ON chess_games(black, status);
        CREATE INDEX IF NOT EXISTS chess_games_status ON chess_games(status);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))
}

type Failure = (StatusCode, String);

fn internal(e: rusqlite::Error) -> Failure {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn bad(text: &str) -> Failure {
    (StatusCode::BAD_REQUEST, text.into())
}

fn color_word(c: Color) -> &'static str {
    match c {
        Color::White => "white",
        Color::Black => "black",
    }
}

fn winner_word(w: Winner) -> &'static str {
    match w {
        Winner::White => "white",
        Winner::Black => "black",
        Winner::Draw => "draw",
    }
}

fn new_id() -> String {
    let mut bytes = [0u8; 9];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::BASE32_NOPAD.encode(&bytes).to_lowercase()
}

fn player(db: &Connection, player_id: &str, category: Option<Category>) -> Result<ChessPlayer, Failure> {
    let (name, number): (String, i64) = db
        .query_row(
            "SELECT name, COALESCE(number, 0) FROM accounts WHERE player_id = ?1",
            params![player_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(internal)?
        .unwrap_or_else(|| ("Someone".into(), 0));
    let rating = match category {
        Some(c) => rating_of(db, player_id, c)?.rating,
        None => elo::START,
    };
    Ok(ChessPlayer { player_id: player_id.into(), name, number: number as u64, rating })
}

fn rating_of(db: &Connection, player_id: &str, category: Category) -> Result<ChessRating, Failure> {
    Ok(db
        .query_row(
            "SELECT rating, games, wins, draws, losses, best FROM chess_ratings
             WHERE player_id = ?1 AND category = ?2",
            params![player_id, category.code()],
            |r| {
                Ok(ChessRating {
                    category: category.code().into(),
                    rating: r.get(0)?,
                    games: r.get(1)?,
                    wins: r.get(2)?,
                    draws: r.get(3)?,
                    losses: r.get(4)?,
                    best: r.get(5)?,
                })
            },
        )
        .optional()
        .map_err(internal)?
        .unwrap_or(ChessRating {
            category: category.code().into(),
            rating: elo::START,
            games: 0,
            wins: 0,
            draws: 0,
            losses: 0,
            best: elo::START,
        }))
}

fn save_rating(db: &Connection, player_id: &str, r: &ChessRating) -> Result<(), Failure> {
    db.execute(
        "INSERT INTO chess_ratings (player_id, category, rating, games, wins, draws, losses, best)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(player_id, category) DO UPDATE SET rating = ?3, games = ?4, wins = ?5,
            draws = ?6, losses = ?7, best = ?8",
        params![player_id, r.category, r.rating, r.games, r.wins, r.draws, r.losses, r.best],
    )
    .map_err(internal)?;
    Ok(())
}

/// A game as stored.
#[derive(Debug, Clone)]
struct Row {
    id: String,
    white: String,
    black: String,
    white_rating: i32,
    black_rating: i32,
    control: TimeControl,
    rated: bool,
    moves: String,
    clocks: Clocks,
    turn_since: i64,
    status: String,
    winner: Option<String>,
    reason: Option<String>,
    draw_offer: Option<String>,
    rematch_offer: Option<String>,
    rematch_game: Option<String>,
    white_change: Option<i32>,
    black_change: Option<i32>,
    white_seen: i64,
    black_seen: i64,
    chat: Vec<ChessChat>,
    created: i64,
    ended: Option<i64>,
}

const ROW_COLUMNS: &str = "id, white, black, white_rating, black_rating, control, rated, moves, white_ms,
    black_ms, turn_since, status, winner, reason, draw_offer, rematch_offer, rematch_game, white_change,
    black_change, white_seen, black_seen, chat, created, ended";

fn read_row(r: &rusqlite::Row) -> rusqlite::Result<Row> {
    let control: String = r.get(5)?;
    let chat: String = r.get(21)?;
    Ok(Row {
        id: r.get(0)?,
        white: r.get(1)?,
        black: r.get(2)?,
        white_rating: r.get(3)?,
        black_rating: r.get(4)?,
        control: TimeControl::parse(&control).unwrap_or(TimeControl::new(5, 0)),
        rated: r.get::<_, i64>(6)? != 0,
        moves: r.get(7)?,
        clocks: Clocks { white_ms: r.get(8)?, black_ms: r.get(9)? },
        turn_since: r.get(10)?,
        status: r.get(11)?,
        winner: r.get(12)?,
        reason: r.get(13)?,
        draw_offer: r.get(14)?,
        rematch_offer: r.get(15)?,
        rematch_game: r.get(16)?,
        white_change: r.get(17)?,
        black_change: r.get(18)?,
        white_seen: r.get(19)?,
        black_seen: r.get(20)?,
        chat: serde_json::from_str(&chat).unwrap_or_default(),
        created: r.get(22)?,
        ended: r.get(23)?,
    })
}

fn load(db: &Connection, id: &str) -> Result<Option<Row>, Failure> {
    db.query_row(&format!("SELECT {ROW_COLUMNS} FROM chess_games WHERE id = ?1"), params![id], read_row)
        .optional()
        .map_err(internal)
}

fn store(db: &Connection, g: &Row) -> Result<(), Failure> {
    db.execute(
        "INSERT OR REPLACE INTO chess_games (id, white, black, white_rating, black_rating, control, rated,
            moves, white_ms, black_ms, turn_since, status, winner, reason, draw_offer, rematch_offer,
            rematch_game, white_change, black_change, white_seen, black_seen, chat, created, ended)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
            ?20, ?21, ?22, ?23, ?24)",
        params![
            g.id,
            g.white,
            g.black,
            g.white_rating,
            g.black_rating,
            g.control.code(),
            g.rated as i64,
            g.moves,
            g.clocks.white_ms,
            g.clocks.black_ms,
            g.turn_since,
            g.status,
            g.winner,
            g.reason,
            g.draw_offer,
            g.rematch_offer,
            g.rematch_game,
            g.white_change,
            g.black_change,
            g.white_seen,
            g.black_seen,
            serde_json::to_string(&g.chat).unwrap_or_else(|_| "[]".into()),
            g.created,
            g.ended,
        ],
    )
    .map_err(internal)?;
    Ok(())
}

impl Row {
    fn game(&self) -> Game {
        Game::from_uci(Position::start(), &self.moves).unwrap_or_default()
    }

    fn ply(&self) -> usize {
        self.moves.split_whitespace().count()
    }

    fn color_of(&self, player_id: &str) -> Option<Color> {
        if self.white == player_id {
            Some(Color::White)
        } else if self.black == player_id {
            Some(Color::Black)
        } else {
            None
        }
    }

    fn playing(&self) -> bool {
        self.status == "playing"
    }

    fn turn(&self) -> Color {
        if self.ply().is_multiple_of(2) { Color::White } else { Color::Black }
    }

    /// Ends the game on time, or calls it off, if its clock says so.
    fn settle(&mut self, db: &Connection, now: i64) -> Result<(), Failure> {
        if !self.playing() {
            return Ok(());
        }
        let ply = self.ply();
        let waited = now - self.turn_since;
        if ply < 2 {
            if waited > FIRST_MOVE_MS {
                self.finish(db, Outcome { winner: Winner::Draw, reason: Reason::Aborted }, now)?;
            }
            return Ok(());
        }
        let turn = self.turn();
        let left = self.clocks.get(turn) - waited;
        if left <= 0 {
            *self.clocks.get_mut(turn) = 0;
            let game = self.game();
            let outcome = if game.position().can_mate(turn.other()) {
                Outcome { winner: Winner::of(turn.other()), reason: Reason::Timeout }
            } else {
                Outcome { winner: Winner::Draw, reason: Reason::TimeoutDraw }
            };
            self.turn_since = now;
            self.finish(db, outcome, now)?;
        }
        Ok(())
    }

    /// Ends the game and moves the ratings.
    fn finish(&mut self, db: &Connection, outcome: Outcome, now: i64) -> Result<(), Failure> {
        // Stop the running clock where it is.
        if self.ply() >= 2 && outcome.reason != Reason::Timeout && outcome.reason != Reason::TimeoutDraw {
            let turn = self.turn();
            let left = (self.clocks.get(turn) - (now - self.turn_since)).max(0);
            *self.clocks.get_mut(turn) = left;
        }
        self.turn_since = now;
        self.status = "over".into();
        self.winner = Some(winner_word(outcome.winner).into());
        self.reason = Some(outcome.reason.code().into());
        self.draw_offer = None;
        self.ended = Some(now / 1000);
        if self.rated && outcome.reason != Reason::Aborted {
            let category = self.control.category();
            let mut w = rating_of(db, &self.white, category)?;
            let mut b = rating_of(db, &self.black, category)?;
            let wp = outcome.winner.points_for(Color::White);
            let new_w = elo::update(w.rating, w.games, b.rating, wp);
            let new_b = elo::update(b.rating, b.games, w.rating, 1.0 - wp);
            self.white_change = Some(new_w - w.rating);
            self.black_change = Some(new_b - b.rating);
            for (r, new, points) in [(&mut w, new_w, wp), (&mut b, new_b, 1.0 - wp)] {
                r.rating = new;
                r.best = r.best.max(new);
                r.games += 1;
                if points == 1.0 {
                    r.wins += 1;
                } else if points == 0.0 {
                    r.losses += 1;
                } else {
                    r.draws += 1;
                }
            }
            save_rating(db, &self.white, &w)?;
            save_rating(db, &self.black, &b)?;
        }
        Ok(())
    }

    fn view(&self, db: &Connection, now: i64) -> Result<ChessGame, Failure> {
        let category = self.control.category();
        let mut white = player(db, &self.white, None)?;
        white.rating = self.white_rating;
        let mut black = player(db, &self.black, None)?;
        black.rating = self.black_rating;
        let mut clocks = self.clocks;
        let ply = self.ply();
        let running = self.playing() && ply >= 2;
        if running {
            let turn = self.turn();
            *clocks.get_mut(turn) = (clocks.get(turn) - (now - self.turn_since)).max(0);
        }
        Ok(ChessGame {
            id: self.id.clone(),
            white,
            black,
            control: self.control.code(),
            category: category.code().into(),
            rated: self.rated,
            moves: self.moves.clone(),
            white_ms: clocks.white_ms,
            black_ms: clocks.black_ms,
            clock_running: running,
            first_move_ms: (self.playing() && ply < 2)
                .then(|| (FIRST_MOVE_MS - (now - self.turn_since)).max(0)),
            status: self.status.clone(),
            winner: self.winner.clone(),
            reason: self.reason.clone(),
            draw_offer: self.draw_offer.clone(),
            rematch_offer: self.rematch_offer.clone(),
            rematch_game: self.rematch_game.clone(),
            white_change: self.white_change,
            black_change: self.black_change,
            white_away_secs: ((now - self.white_seen) / 1000).max(0),
            black_away_secs: ((now - self.black_seen) / 1000).max(0),
            chat: self.chat.clone(),
            created: self.created,
            ended: self.ended,
        })
    }
}

/// Starts a game between two players.
fn start_game(
    db: &Connection,
    white: &str,
    black: &str,
    control: TimeControl,
    rated: bool,
    now: i64,
) -> Result<Row, Failure> {
    let category = control.category();
    let row = Row {
        id: new_id(),
        white: white.into(),
        black: black.into(),
        white_rating: rating_of(db, white, category)?.rating,
        black_rating: rating_of(db, black, category)?.rating,
        control,
        rated,
        moves: String::new(),
        clocks: Clocks::new(control),
        turn_since: now,
        status: "playing".into(),
        winner: None,
        reason: None,
        draw_offer: None,
        rematch_offer: None,
        rematch_game: None,
        white_change: None,
        black_change: None,
        white_seen: now,
        black_seen: now,
        chat: Vec::new(),
        created: now / 1000,
        ended: None,
    };
    store(db, &row)?;
    // Someone who starts a game isn't looking for another.
    db.execute("DELETE FROM chess_seeks WHERE player_id IN (?1, ?2)", params![white, black])
        .map_err(internal)?;
    Ok(row)
}

fn coin() -> bool {
    let mut b = [0u8; 1];
    getrandom::fill(&mut b).expect("the system has a random number source");
    b[0] & 1 == 0
}

/// Games a player is in that aren't over, settling clocks first.
fn active_games(db: &Connection, player_id: &str, now: i64) -> Result<Vec<Row>, Failure> {
    let rows: Vec<Row> = db
        .prepare(&format!(
            "SELECT {ROW_COLUMNS} FROM chess_games WHERE status = 'playing' AND (white = ?1 OR black = ?1)"
        ))
        .and_then(|mut s| s.query_map(params![player_id], read_row)?.collect())
        .map_err(internal)?;
    let mut still = Vec::new();
    for mut row in rows {
        row.settle(db, now)?;
        store(db, &row)?;
        if row.playing() {
            still.push(row);
        }
    }
    Ok(still)
}

/// Pairs players looking for the same kind of game. Close ratings first;
/// the longer someone waits, the wider the range they'll accept.
fn pair_seekers(db: &Connection, now: i64) -> Result<(), Failure> {
    db.execute("DELETE FROM chess_seeks WHERE seen < ?1", params![now - SEEK_EXPIRES_MS])
        .map_err(internal)?;
    let seeks: Vec<(String, String, bool, i32, i64)> = db
        .prepare("SELECT player_id, control, rated, rating, since FROM chess_seeks ORDER BY since")
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0, r.get(3)?, r.get(4)?)))?
                .collect()
        })
        .map_err(internal)?;
    let range = |since: i64, rated: bool| {
        if rated { 150 + ((now - since) / 1000) as i32 * 25 } else { i32::MAX }
    };
    let mut taken = vec![false; seeks.len()];
    for i in 0..seeks.len() {
        if taken[i] {
            continue;
        }
        let a = &seeks[i];
        let mut best: Option<(usize, i32)> = None;
        for (j, b) in seeks.iter().enumerate().skip(i + 1) {
            if taken[j] || a.1 != b.1 || a.2 != b.2 {
                continue;
            }
            let gap = (a.3 - b.3).abs();
            if gap <= range(a.4, a.2) && gap <= range(b.4, b.2) && best.is_none_or(|(_, g)| gap < g) {
                best = Some((j, gap));
            }
        }
        if let Some((j, _)) = best {
            taken[i] = true;
            taken[j] = true;
            let b = &seeks[j];
            let control = TimeControl::parse(&a.1).unwrap_or(TimeControl::new(5, 0));
            let (white, black) = if coin() { (&a.0, &b.0) } else { (&b.0, &a.0) };
            start_game(db, white, black, control, a.2, now)?;
        }
    }
    Ok(())
}

fn challenges(
    db: &Connection,
    column: &str,
    player_id: &str,
    now: i64,
) -> Result<Vec<ChessChallenge>, Failure> {
    db.execute("DELETE FROM chess_challenges WHERE created < ?1", params![now - CHALLENGE_EXPIRES_MS])
        .map_err(internal)?;
    let rows: Vec<(String, String, String, String, bool, Option<String>)> = db
        .prepare(&format!(
            "SELECT id, from_id, to_id, control, rated, color FROM chess_challenges WHERE {column} = ?1
             ORDER BY created DESC"
        ))
        .and_then(|mut s| {
            s.query_map(params![player_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? != 0, r.get(5)?))
            })?
            .collect()
        })
        .map_err(internal)?;
    rows.into_iter()
        .map(|(id, from, to, control, rated, color)| {
            let category = TimeControl::parse(&control).map(|c| c.category());
            Ok(ChessChallenge {
                id,
                from: player(db, &from, category)?,
                to: player(db, &to, category)?,
                control,
                rated,
                color,
            })
        })
        .collect()
}

fn lobby(db: &Connection, player_id: &str, game_id: Option<&str>, now: i64) -> Result<ChessLobby, Failure> {
    pair_seekers(db, now)?;
    let mut active = Vec::new();
    for row in active_games(db, player_id, now)? {
        active.push(row.view(db, now)?);
    }
    let game = match game_id {
        Some(id) => match load(db, id)? {
            Some(mut row) => {
                row.settle(db, now)?;
                match row.color_of(player_id) {
                    Some(Color::White) => row.white_seen = now,
                    Some(Color::Black) => row.black_seen = now,
                    None => {}
                }
                store(db, &row)?;
                Some(row.view(db, now)?)
            }
            None => None,
        },
        None => active.first().cloned(),
    };
    let seeking = db
        .query_row(
            "SELECT control, rated, since FROM chess_seeks WHERE player_id = ?1",
            params![player_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0, r.get::<_, i64>(2)?)),
        )
        .optional()
        .map_err(internal)?
        .map(|(control, rated, since)| ChessSeek { control, rated, waiting_secs: (now - since) / 1000 });
    let seeking_counts = db
        .prepare("SELECT control, COUNT(*) FROM chess_seeks GROUP BY control")
        .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect())
        .map_err(internal)?;
    let playing_now: u32 = db
        .query_row("SELECT COUNT(*) FROM chess_games WHERE status = 'playing'", [], |r| r.get(0))
        .map_err(internal)?;
    let ratings =
        Category::ALL.iter().map(|c| rating_of(db, player_id, *c)).collect::<Result<Vec<_>, _>>()?;
    Ok(ChessLobby {
        me: player(db, player_id, None)?,
        ratings,
        seeking,
        incoming: challenges(db, "to_id", player_id, now)?,
        outgoing: challenges(db, "from_id", player_id, now)?,
        active,
        game,
        seeking_counts,
        playing_now,
    })
}

fn has_account(db: &Connection, player_id: &str) -> Result<bool, Failure> {
    db.query_row("SELECT 1 FROM accounts WHERE player_id = ?1", params![player_id], |_| Ok(()))
        .optional()
        .map(|o| o.is_some())
        .map_err(internal)
}

/// The game a player is acting on, settled and checked to be theirs.
fn my_game(db: &Connection, game_id: &str, me: &str, now: i64) -> Result<(Row, Color), Failure> {
    let mut row = load(db, game_id)?.ok_or((StatusCode::NOT_FOUND, "there is no such game".into()))?;
    let color = row.color_of(me).ok_or((StatusCode::FORBIDDEN, "you aren't playing in that game".into()))?;
    row.settle(db, now)?;
    match color {
        Color::White => row.white_seen = now,
        Color::Black => row.black_seen = now,
    }
    Ok((row, color))
}

fn act(db: &Connection, me: &str, action: ChessAction, now: i64) -> Result<Option<String>, Failure> {
    match action {
        ChessAction::Sync { game_id } => Ok(game_id),
        ChessAction::Seek { control, rated } => {
            let control =
                TimeControl::parse(&control).ok_or_else(|| bad("that clock isn't one the Hub knows"))?;
            if !active_games(db, me, now)?.is_empty() {
                return Err(bad("finish your game first"));
            }
            let rating = rating_of(db, me, control.category())?.rating;
            db.execute(
                "INSERT INTO chess_seeks (player_id, control, rated, rating, since, seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT(player_id) DO UPDATE SET control = ?2, rated = ?3, rating = ?4, since = ?5, seen = ?5",
                params![me, control.code(), rated as i64, rating, now],
            )
            .map_err(internal)?;
            pair_seekers(db, now)?;
            Ok(None)
        }
        ChessAction::CancelSeek => {
            db.execute("DELETE FROM chess_seeks WHERE player_id = ?1", params![me]).map_err(internal)?;
            Ok(None)
        }
        ChessAction::Challenge { to, control, rated, color } => {
            let control =
                TimeControl::parse(&control).ok_or_else(|| bad("that clock isn't one the Hub knows"))?;
            if to == me {
                return Err(bad("you can't challenge yourself"));
            }
            if !has_account(db, &to)? {
                return Err(bad("there's no player with that id here"));
            }
            if crate::owner::is_banned(db, &to) {
                return Err(bad("that player can't play here"));
            }
            let color = color.filter(|c| c == "white" || c == "black");
            let waiting: i64 = db
                .query_row("SELECT COUNT(*) FROM chess_challenges WHERE from_id = ?1", params![me], |r| {
                    r.get(0)
                })
                .map_err(internal)?;
            if waiting >= 10 {
                return Err(bad("you have too many challenges waiting already"));
            }
            db.execute("DELETE FROM chess_challenges WHERE from_id = ?1 AND to_id = ?2", params![me, to])
                .map_err(internal)?;
            db.execute(
                "INSERT INTO chess_challenges (id, from_id, to_id, control, rated, color, created)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![new_id(), me, to, control.code(), rated as i64, color, now],
            )
            .map_err(internal)?;
            Ok(None)
        }
        ChessAction::AnswerChallenge { id, accept } => {
            let found: Option<(String, String, String, bool, Option<String>)> = db
                .query_row(
                    "SELECT from_id, to_id, control, rated, color FROM chess_challenges WHERE id = ?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0, r.get(4)?)),
                )
                .optional()
                .map_err(internal)?;
            let (from, to, control, rated, color) =
                found.ok_or((StatusCode::NOT_FOUND, "that challenge has gone".into()))?;
            if me != from && me != to {
                return Err((StatusCode::FORBIDDEN, "that challenge isn't yours".into()));
            }
            db.execute("DELETE FROM chess_challenges WHERE id = ?1", params![id]).map_err(internal)?;
            if !accept || me == from {
                return Ok(None);
            }
            let control = TimeControl::parse(&control).unwrap_or(TimeControl::new(10, 0));
            let from_white = match color.as_deref() {
                Some("white") => true,
                Some("black") => false,
                _ => coin(),
            };
            let (white, black) = if from_white { (&from, &to) } else { (&to, &from) };
            let row = start_game(db, white, black, control, rated, now)?;
            Ok(Some(row.id))
        }
        ChessAction::Move { game_id, uci, ply } => {
            let (mut row, color) = my_game(db, &game_id, me, now)?;
            if !row.playing() {
                store(db, &row)?;
                return Err(bad("the game is over"));
            }
            if row.ply() != ply as usize {
                return Err((StatusCode::CONFLICT, "the board changed; look again".into()));
            }
            if row.turn() != color {
                return Err(bad("it isn't your turn"));
            }
            let mut game = row.game();
            let m = libroxel_chess::Move::from_uci(&uci).ok_or_else(|| bad("that isn't a move"))?;
            game.play(m).map_err(|e| bad(&e))?;
            if row.ply() >= 2 {
                let left = row.clocks.get(color) - (now - row.turn_since);
                *row.clocks.get_mut(color) = left + row.control.increment_ms();
            }
            row.moves = game.uci_moves();
            row.turn_since = now;
            // Moving answers "no" to a draw offered to you.
            if row.draw_offer.as_deref() == Some(color_word(color.other())) {
                row.draw_offer = None;
            }
            if let Some(outcome) = game.board_outcome() {
                row.finish(db, outcome, now)?;
            }
            store(db, &row)?;
            Ok(Some(game_id))
        }
        ChessAction::Resign { game_id } => {
            let (mut row, color) = my_game(db, &game_id, me, now)?;
            if row.playing() {
                let outcome = if row.ply() < 2 {
                    Outcome { winner: Winner::Draw, reason: Reason::Aborted }
                } else {
                    Outcome { winner: Winner::of(color.other()), reason: Reason::Resigned }
                };
                row.finish(db, outcome, now)?;
            }
            store(db, &row)?;
            Ok(Some(game_id))
        }
        ChessAction::Abort { game_id } => {
            let (mut row, _) = my_game(db, &game_id, me, now)?;
            if row.playing() {
                if row.ply() >= 2 {
                    return Err(bad("the game has started; resign instead"));
                }
                row.finish(db, Outcome { winner: Winner::Draw, reason: Reason::Aborted }, now)?;
            }
            store(db, &row)?;
            Ok(Some(game_id))
        }
        ChessAction::OfferDraw { game_id } => {
            let (mut row, color) = my_game(db, &game_id, me, now)?;
            if row.playing() {
                if row.draw_offer.as_deref() == Some(color_word(color.other())) {
                    row.finish(db, Outcome { winner: Winner::Draw, reason: Reason::Agreement }, now)?;
                } else if row.ply() >= 2 {
                    row.draw_offer = Some(color_word(color).into());
                }
            }
            store(db, &row)?;
            Ok(Some(game_id))
        }
        ChessAction::DeclineDraw { game_id } => {
            let (mut row, color) = my_game(db, &game_id, me, now)?;
            if row.draw_offer.as_deref() == Some(color_word(color.other())) {
                row.draw_offer = None;
            }
            store(db, &row)?;
            Ok(Some(game_id))
        }
        ChessAction::ClaimWin { game_id } => {
            let (mut row, color) = my_game(db, &game_id, me, now)?;
            if row.playing() {
                let seen = match color {
                    Color::White => row.black_seen,
                    Color::Black => row.white_seen,
                };
                if now - seen < GONE_MS {
                    return Err(bad("your opponent is still here"));
                }
                let game = row.game();
                let outcome = if row.ply() < 2 {
                    Outcome { winner: Winner::Draw, reason: Reason::Aborted }
                } else if game.position().can_mate(color) {
                    Outcome { winner: Winner::of(color), reason: Reason::Abandoned }
                } else {
                    Outcome { winner: Winner::Draw, reason: Reason::Abandoned }
                };
                row.finish(db, outcome, now)?;
            }
            store(db, &row)?;
            Ok(Some(game_id))
        }
        ChessAction::Rematch { game_id } => {
            let (mut row, color) = my_game(db, &game_id, me, now)?;
            if row.playing() {
                return Err(bad("the game isn't over yet"));
            }
            if let Some(next) = row.rematch_game.clone() {
                store(db, &row)?;
                return Ok(Some(next));
            }
            if row.rematch_offer.as_deref() == Some(color_word(color.other())) {
                if !active_games(db, &row.white, now)?.is_empty()
                    || !active_games(db, &row.black, now)?.is_empty()
                {
                    return Err(bad("one of you is already in another game"));
                }
                // Colours swap.
                let next = start_game(db, &row.black, &row.white, row.control, row.rated, now)?;
                row.rematch_game = Some(next.id.clone());
                row.rematch_offer = None;
                store(db, &row)?;
                return Ok(Some(next.id));
            }
            row.rematch_offer = Some(color_word(color).into());
            store(db, &row)?;
            Ok(Some(game_id))
        }
        ChessAction::Chat { game_id, phrase } => {
            let (mut row, color) = my_game(db, &game_id, me, now)?;
            if (phrase as usize) < CHESS_PHRASES.len() {
                row.chat.push(ChessChat { color: color_word(color).into(), phrase });
                if row.chat.len() > 50 {
                    row.chat.remove(0);
                }
            }
            store(db, &row)?;
            Ok(Some(game_id))
        }
    }
}

pub(crate) async fn chess(
    State(db): State<Db>,
    Json(request): Json<ChessRequest>,
) -> Result<Json<ChessLobby>, Failure> {
    let message = chess_message(&request.player_id, request.time, &request.action);
    if !crate::verify(&request.player_id, &message, &request.signature) {
        return Err((StatusCode::FORBIDDEN, "the signature does not match that player".into()));
    }
    let action: ChessAction = serde_json::from_str(&request.action).map_err(|e| bad(&e.to_string()))?;
    let now = now_ms();
    let db = db.lock().unwrap();
    if crate::owner::is_banned(&db, &request.player_id) {
        return Err((StatusCode::FORBIDDEN, "this account is banned from this Hub".into()));
    }
    if !has_account(&db, &request.player_id)? {
        return Err(bad("claim a name on the Avatar tab first"));
    }
    let me = request.player_id.as_str();
    if matches!(action, ChessAction::Sync { .. }) {
        if (request.time - now).abs() > SLACK_MS {
            return Err(bad("this computer's clock is too far off"));
        }
        db.execute("UPDATE chess_seeks SET seen = ?2 WHERE player_id = ?1", params![me, now])
            .map_err(internal)?;
    } else {
        super::social::fresh(&db, me, "chess", request.time)?;
    }
    let game_id = act(&db, me, action, now)?;
    Ok(Json(lobby(&db, me, game_id.as_deref(), now)?))
}

pub(crate) async fn game(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<ChessGame>, Failure> {
    let now = now_ms();
    let db = db.lock().unwrap();
    let mut row = load(&db, &id)?.ok_or((StatusCode::NOT_FOUND, "there is no such game".into()))?;
    if row.playing() {
        row.settle(&db, now)?;
        store(&db, &row)?;
    }
    Ok(Json(row.view(&db, now)?))
}

pub(crate) async fn live(State(db): State<Db>) -> Result<Json<Vec<ChessGame>>, Failure> {
    let now = now_ms();
    let db = db.lock().unwrap();
    let rows: Vec<Row> = db
        .prepare(&format!(
            "SELECT {ROW_COLUMNS} FROM chess_games WHERE status = 'playing'
             ORDER BY white_rating + black_rating DESC LIMIT 40"
        ))
        .and_then(|mut s| s.query_map([], read_row)?.collect())
        .map_err(internal)?;
    let mut out = Vec::new();
    for mut row in rows {
        row.settle(&db, now)?;
        store(&db, &row)?;
        if row.playing() {
            out.push(row.view(&db, now)?);
        }
    }
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
pub(crate) struct BoardQuery {
    category: Option<String>,
}

pub(crate) async fn leaderboard(
    State(db): State<Db>,
    Query(query): Query<BoardQuery>,
) -> Result<Json<Vec<ChessLeader>>, Failure> {
    let category = query.category.as_deref().and_then(Category::from_code).unwrap_or(Category::Blitz);
    let db = db.lock().unwrap();
    let rows: Vec<(String, String, i64, i32, u32, u32)> = db
        .prepare(&format!(
            "SELECT r.player_id, a.name, COALESCE(a.number, 0), r.rating, r.games, r.wins
             FROM chess_ratings r JOIN accounts a ON a.player_id = r.player_id
             WHERE r.category = ?1 AND r.games > 0 AND NOT {}
             ORDER BY r.rating DESC, r.games DESC LIMIT 100",
            crate::hidden_sql("account", "r.player_id")
        ))
        .and_then(|mut s| {
            s.query_map(params![category.code()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?
            .collect()
        })
        .map_err(internal)?;
    Ok(Json(
        rows.into_iter()
            .enumerate()
            .map(|(i, (player_id, name, number, rating, games, wins))| ChessLeader {
                rank: i as u32 + 1,
                player: ChessPlayer { player_id, name, number: number as u64, rating },
                games,
                wins,
            })
            .collect(),
    ))
}

pub(crate) async fn profile(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<ChessProfile>, Failure> {
    let db = db.lock().unwrap();
    if !has_account(&db, &id)? {
        return Err((StatusCode::NOT_FOUND, "there's no such player".into()));
    }
    let ratings = Category::ALL.iter().map(|c| rating_of(&db, &id, *c)).collect::<Result<Vec<_>, _>>()?;
    let rows: Vec<Row> = db
        .prepare(&format!(
            "SELECT {ROW_COLUMNS} FROM chess_games WHERE status = 'over' AND (white = ?1 OR black = ?1)
             AND reason != 'aborted' ORDER BY ended DESC LIMIT 20"
        ))
        .and_then(|mut s| s.query_map(params![id], read_row)?.collect())
        .map_err(internal)?;
    let mut recent = Vec::new();
    for row in rows {
        let mut white = player(&db, &row.white, None)?;
        white.rating = row.white_rating;
        let mut black = player(&db, &row.black, None)?;
        black.rating = row.black_rating;
        recent.push(ChessHistoryEntry {
            id: row.id.clone(),
            white,
            black,
            control: row.control.code(),
            rated: row.rated,
            winner: row.winner.clone(),
            reason: row.reason.clone(),
            moves: row.ply() as u32,
            ended: row.ended.unwrap_or(row.created),
        });
    }
    Ok(Json(ChessProfile { player: player(&db, &id, None)?, ratings, recent }))
}
