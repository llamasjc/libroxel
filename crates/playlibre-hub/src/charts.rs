use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{Db, hidden_sql, now, verify};
use crate::social::{Failure, internal, now_ms};

const SESSION_GAP_SECS: i64 = 30 * 60;

const LIST_SLACK_MS: i64 = 10 * 60 * 1000;

const CHART_SIZE: usize = 30;

const DAY: i64 = 24 * 60 * 60;

const GENRES: [(&str, &[&str]); 7] = [
    ("Racing", &["kart", "race", "racing", "racer", "car", "drift", "speed"]),
    ("Fighting", &["battle", "royale", "fight", "pvp", "sword", "combat", "arena", "war"]),
    ("Obby", &["obby", "parkour", "obstacle", "tower", "climb", "jump"]),
    (
        "Board and cards",
        &["chess", "cards", "card", "kings", "board", "checkers", "table", "poker", "solitaire"],
    ),
    ("Party", &["tag", "hide", "seek", "party", "minigame", "minigames"]),
    ("Tycoon and sim", &["tycoon", "simulator", "sim", "roleplay", "town", "city", "farm", "shop"]),
    ("Adventure", &["adventure", "explore", "quest", "story", "dungeon", "survival"]),
];

pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS played (
            player_id   TEXT NOT NULL,
            game_id     TEXT NOT NULL,
            title       TEXT NOT NULL,
            first       INTEGER NOT NULL,
            last        INTEGER NOT NULL,
            sessions    INTEGER NOT NULL,
            seconds     INTEGER NOT NULL,
            PRIMARY KEY (player_id, game_id)
        );
        CREATE INDEX IF NOT EXISTS played_game ON played(game_id);
        CREATE TABLE IF NOT EXISTS game_days (
            game_id     TEXT NOT NULL,
            day         INTEGER NOT NULL,
            plays       INTEGER NOT NULL,
            PRIMARY KEY (game_id, day)
        );",
    )
    .map_err(|e| format!("could not set up charts: {e}"))
}

pub(crate) fn note_play(db: &Connection, player_id: &str, game_id: &str, title: &str) {
    let now = now();
    let last: Option<i64> = db
        .query_row(
            "SELECT last FROM played WHERE player_id = ?1 AND game_id = ?2",
            params![player_id, game_id],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten();
    let new_session = last.is_none_or(|last| now - last > SESSION_GAP_SECS);
    let added = last.map_or(0, |last| (now - last).clamp(0, 60));
    let _ = db.execute(
        "INSERT INTO played (player_id, game_id, title, first, last, sessions, seconds)
         VALUES (?1, ?2, ?3, ?4, ?4, 1, 0)
         ON CONFLICT(player_id, game_id) DO UPDATE SET title = ?3, last = ?4,
            sessions = sessions + ?5, seconds = seconds + ?6",
        params![player_id, game_id, title, now, new_session as i64, added],
    );
    if new_session {
        let _ = db.execute(
            "INSERT INTO game_days (game_id, day, plays) VALUES (?1, ?2, 1)
             ON CONFLICT(game_id, day) DO UPDATE SET plays = plays + 1",
            params![game_id, now / DAY],
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chart {
    pub id: String,
    pub title: String,
    pub game_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChartStats {
    pub game_id: String,
    pub likes: u32,
    pub dislikes: u32,
    pub favorites: u32,
    pub plays_week: u32,
    pub genre: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Charts {
    pub charts: Vec<Chart>,
    pub stats: Vec<ChartStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Played {
    pub game_id: String,
    pub title: String,
    pub last_secs_ago: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ForYou {
    #[serde(rename = "continue")]
    pub continue_playing: Vec<Played>,
    pub recommended: Vec<String>,
}

pub fn for_you_message(player_id: &str, time: i64) -> Vec<u8> {
    format!("libroxel for you\n{player_id}\n{time}").into_bytes()
}

pub fn guess_genre(title: &str, about: &str) -> Option<&'static str> {
    let words: Vec<String> = format!("{title} {about}")
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    GENRES
        .iter()
        .map(|(genre, keys)| (genre, words.iter().filter(|w| keys.contains(&w.as_str())).count()))
        .filter(|(_, n)| *n > 0)
        .max_by_key(|(_, n)| *n)
        .map(|(genre, _)| *genre)
}

pub fn wilson(up: u32, n: u32) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let (n, p, z) = (n as f64, up as f64 / n as f64, 1.96f64);
    (p + z * z / (2.0 * n) - z * ((p * (1.0 - p) + z * z / (4.0 * n)) / n).sqrt()) / (1.0 + z * z / n)
}

#[derive(Debug, Clone, Default)]
struct Game {
    id: String,
    genre: Option<&'static str>,
    age_days: Option<f64>,
    playing: u32,
    days: HashMap<i64, u32>,
    likes: u32,
    dislikes: u32,
    favorites: u32,
}

impl Game {
    fn plays(&self, from: i64, to: i64) -> u32 {
        (from..=to).map(|d| self.days.get(&d).copied().unwrap_or(0)).sum()
    }

    fn hot(&self) -> f64 {
        let decayed: f64 = self.days.iter().map(|(&d, &n)| n as f64 * 0.5f64.powf(d as f64 / 2.0)).sum();
        self.playing as f64 * 4.0 + decayed
    }

    fn freshness(&self) -> f64 {
        self.age_days.map_or(1.0, |age| 1.0 + 2.0 * (-age / 7.0).exp())
    }

    fn trend(&self) -> f64 {
        let recent = self.plays(0, 1) as f64 + self.playing as f64;
        let before = self.plays(2, 8) as f64 / 7.0;
        let growth = (recent / 2.0 + 1.0) / (before + 1.0);
        growth * (1.0 + recent).ln() * self.freshness()
    }

    fn rating(&self) -> f64 {
        wilson(self.likes, self.likes + self.dislikes)
    }

    fn popular(&self) -> f64 {
        (self.hot() + self.favorites as f64 * 0.5) * self.freshness() * (0.5 + self.rating())
    }
}

fn games(db: &Connection) -> Result<Vec<Game>, rusqlite::Error> {
    let now = now();
    let today = now / DAY;
    let mut list = Vec::new();
    let mut statement = db.prepare(&format!(
        "SELECT g.game_id, g.title, g.description, g.created FROM games g
         WHERE g.private = 0 AND NOT {} AND NOT {}",
        hidden_sql("game", "g.game_id"),
        crate::owner::banned_sql("g.author_id")
    ))?;
    let rows = statement.query_map([], |r| {
        let (title, about): (String, String) = (r.get(1)?, r.get(2)?);
        Ok(Game {
            id: r.get(0)?,
            genre: guess_genre(&title, &about),
            age_days: Some((now - r.get::<_, i64>(3)?).max(0) as f64 / DAY as f64),
            ..Game::default()
        })
    })?;
    list.extend(rows.filter_map(Result::ok));
    if let Ok(mut statement) = db.prepare("SELECT name, about FROM official") {
        let rows = statement.query_map([], |r| {
            let (name, about): (String, String) = (r.get(0)?, r.get(1)?);
            Ok(Game { id: format!("starter:{name}"), genre: guess_genre(&name, &about), ..Game::default() })
        })?;
        list.extend(rows.filter_map(Result::ok));
    }
    let index: HashMap<String, usize> = list.iter().enumerate().map(|(i, g)| (g.id.clone(), i)).collect();
    let mut statement = db.prepare(
        "SELECT game_id, SUM(players) FROM servers
         WHERE game_id IS NOT NULL AND last_seen >= ?1 GROUP BY game_id",
    )?;
    for row in
        statement.query_map(params![now - 45], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
    {
        let (id, n) = row?;
        if let Some(&i) = index.get(&id) {
            list[i].playing = n.max(0) as u32;
        }
    }
    let mut statement = db.prepare("SELECT game_id, day, plays FROM game_days WHERE day >= ?1")?;
    for row in statement.query_map(params![today - 14], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
    })? {
        let (id, day, n) = row?;
        if let Some(&i) = index.get(&id) {
            list[i].days.insert(today - day, n.max(0) as u32);
        }
    }
    if let Ok(mut statement) =
        db.prepare("SELECT game_id, SUM(vote > 0), SUM(vote < 0) FROM game_votes GROUP BY game_id")
    {
        let rows: Vec<(String, i64, i64)> = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default();
        for (id, up, down) in rows {
            if let Some(&i) = index.get(&id) {
                list[i].likes = up.max(0) as u32;
                list[i].dislikes = down.max(0) as u32;
            }
        }
    }
    if let Ok(mut statement) = db.prepare("SELECT game_id, COUNT(*) FROM game_favorites GROUP BY game_id") {
        let rows: Vec<(String, i64)> = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default();
        for (id, n) in rows {
            if let Some(&i) = index.get(&id) {
                list[i].favorites = n.max(0) as u32;
            }
        }
    }
    Ok(list)
}

fn ranked(list: &[Game], keep: impl Fn(&Game) -> bool, score: impl Fn(&Game) -> f64) -> Vec<&Game> {
    let mut picked: Vec<(&Game, f64)> = list.iter().filter(|g| keep(g)).map(|g| (g, score(g))).collect();
    picked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| b.0.hot().total_cmp(&a.0.hot())));
    picked.into_iter().take(CHART_SIZE).map(|(g, _)| g).collect()
}

pub fn charts(db: &Connection) -> Charts {
    let Ok(list) = games(db) else { return Charts::default() };
    let chart = |id: &str, title: &str, games: Vec<&Game>| Chart {
        id: id.to_owned(),
        title: title.to_owned(),
        game_ids: games.iter().map(|g| g.id.clone()).collect(),
    };
    let mut charts = vec![
        chart(
            "top-playing",
            "Top playing",
            ranked(
                &list,
                |g| g.playing > 0 || g.hot() > 0.0 || g.age_days.is_none(),
                |g| g.playing as f64 + g.hot() / 100.0 + if g.age_days.is_none() { 0.001 } else { 0.0 },
            ),
        ),
        chart("trending", "Trending", ranked(&list, |g| g.plays(0, 1) + g.playing > 0, Game::trend)),
        chart("top-rated", "Top rated", ranked(&list, |g| g.likes + g.dislikes > 0, Game::rating)),
        chart(
            "new",
            "New",
            ranked(&list, |g| g.age_days.is_some_and(|a| a <= 30.0), |g| (g.hot() + 1.0) * g.freshness()),
        ),
        chart("popular", "Popular", ranked(&list, |_| true, Game::popular)),
    ];
    for (genre, _) in GENRES {
        let games = ranked(&list, |g| g.genre == Some(genre), Game::popular);
        if !games.is_empty() {
            charts.push(chart(&format!("genre:{genre}"), genre, games));
        }
    }
    charts.retain(|c| !c.game_ids.is_empty());
    let stats = list
        .iter()
        .map(|g| ChartStats {
            game_id: g.id.clone(),
            likes: g.likes,
            dislikes: g.dislikes,
            favorites: g.favorites,
            plays_week: g.plays(0, 6),
            genre: g.genre.map(str::to_owned),
        })
        .collect();
    Charts { charts, stats }
}

pub fn recommend(db: &Connection, player_id: &str, count: usize) -> Vec<String> {
    let Ok(list) = games(db) else { return Vec::new() };
    let known: HashMap<&str, &Game> = list.iter().map(|g| (g.id.as_str(), g)).collect();
    let mine = taste(db, player_id);
    let played: HashSet<&str> = mine.keys().map(String::as_str).collect();
    let mut score: HashMap<&str, f64> = HashMap::new();
    let mut genres: HashMap<&str, f64> = HashMap::new();
    for (game, weight) in &mine {
        if let Some(genre) = known.get(game.as_str()).and_then(|g| g.genre) {
            *genres.entry(genre).or_default() += weight;
        }
    }
    if let Ok(mut statement) = db.prepare(&format!(
        "SELECT o.player_id, o.game_id, o.sessions, o.seconds, {}
             FROM played m JOIN played o ON o.game_id = m.game_id AND o.player_id != m.player_id
             WHERE m.player_id = ?1 LIMIT 5000",
        liking(db, "o")
    )) {
        let shared: Vec<(String, String, i64, i64, i64)> = statement
            .query_map(params![player_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default();
        let mut similar: HashMap<String, f64> = HashMap::new();
        for (other, game, sessions, seconds, vote) in &shared {
            let theirs = weight(*sessions, *seconds, *vote);
            let ours = mine.get(game).copied().unwrap_or(0.0);
            *similar.entry(other.clone()).or_default() += theirs.min(ours).max(0.0);
        }
        let mut others: Vec<(String, f64)> = similar.into_iter().filter(|(_, s)| *s > 0.0).collect();
        others.sort_by(|a, b| b.1.total_cmp(&a.1));
        others.truncate(50);
        for (other, similarity) in others {
            for (game, w) in taste(db, &other) {
                if let Some(g) = known.get(game.as_str()).filter(|g| !played.contains(g.id.as_str())) {
                    *score.entry(g.id.as_str()).or_default() += similarity * w;
                }
            }
        }
    }
    let top = score.values().cloned().fold(0.0f64, f64::max).max(1.0);
    let genre_top = genres.values().cloned().fold(0.0f64, f64::max).max(1.0);
    let pop_top = list.iter().map(Game::popular).fold(0.0f64, f64::max).max(1.0);
    let mut picks: Vec<(&Game, f64)> = list
        .iter()
        .filter(|g| !played.contains(g.id.as_str()))
        .map(|g| {
            let people = score.get(g.id.as_str()).copied().unwrap_or(0.0) / top;
            let genre = g.genre.and_then(|x| genres.get(x)).copied().unwrap_or(0.0) / genre_top;
            let popular = g.popular() / pop_top;
            (g, 3.0 * people + 1.5 * genre + popular + 0.25 * g.rating())
        })
        .collect();
    picks.sort_by(|a, b| b.1.total_cmp(&a.1));
    picks.into_iter().take(count).map(|(g, _)| g.id.clone()).collect()
}

fn has_table(db: &Connection, name: &str) -> bool {
    db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", params![name], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

fn liking(db: &Connection, p: &str) -> String {
    let vote = if has_table(db, "game_votes") {
        format!(
            "COALESCE((SELECT v.vote FROM game_votes v WHERE v.player_id = {p}.player_id AND v.game_id = {p}.game_id), 0)"
        )
    } else {
        "0".into()
    };
    let favorite = if has_table(db, "game_favorites") {
        format!(
            "2 * EXISTS (SELECT 1 FROM game_favorites f WHERE f.player_id = {p}.player_id AND f.game_id = {p}.game_id)"
        )
    } else {
        "0".into()
    };
    format!("{vote} + {favorite}")
}

fn weight(sessions: i64, seconds: i64, liked: i64) -> f64 {
    let played = (1.0 + sessions as f64).ln() + (1.0 + seconds as f64 / 600.0).ln();
    if liked < 0 { -2.0 } else { played + 2.0 * liked as f64 }
}

fn taste(db: &Connection, player_id: &str) -> HashMap<String, f64> {
    let Ok(mut statement) = db.prepare(&format!(
        "SELECT p.game_id, p.sessions, p.seconds, {} FROM played p
         WHERE p.player_id = ?1 ORDER BY p.last DESC LIMIT 200",
        liking(db, "p")
    )) else {
        return HashMap::new();
    };
    statement
        .query_map(params![player_id], |r| {
            Ok((r.get::<_, String>(0)?, weight(r.get(1)?, r.get(2)?, r.get(3)?)))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

pub(crate) async fn get_charts(State(db): State<Db>) -> Json<Charts> {
    let db = db.lock().unwrap();
    Json(charts(&db))
}

#[derive(Debug, Deserialize)]
pub(crate) struct Signed {
    time: i64,
    signature: String,
}

pub(crate) async fn for_you(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Query(signed): Query<Signed>,
) -> Result<Json<ForYou>, Failure> {
    if !verify(&id, &for_you_message(&id, signed.time), &signed.signature) {
        return Err((StatusCode::FORBIDDEN, "only you can see what you played".into()));
    }
    if (signed.time - now_ms()).abs() > LIST_SLACK_MS {
        return Err((StatusCode::BAD_REQUEST, "this computer's clock is too far off".into()));
    }
    let db = db.lock().unwrap();
    let now = now();
    let mut statement = db
        .prepare("SELECT game_id, title, last FROM played WHERE player_id = ?1 ORDER BY last DESC, rowid DESC LIMIT 20")
        .map_err(internal)?;
    let continue_playing = statement
        .query_map(params![id], |r| {
            Ok(Played { game_id: r.get(0)?, title: r.get(1)?, last_secs_ago: now - r.get::<_, i64>(2)? })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .map_err(internal)?;
    Ok(Json(ForYou { continue_playing, recommended: recommend(&db, &id, 20) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_favours_more_votes() {
        assert!(wilson(90, 100) > wilson(9, 10));
        assert!(wilson(1, 1) < wilson(50, 60));
        assert_eq!(wilson(0, 0), 0.0);
    }

    #[test]
    fn genres_from_words() {
        assert_eq!(guess_genre("Kart Racer", "Drift round the track"), Some("Racing"));
        assert_eq!(guess_genre("Chess", ""), Some("Board and cards"));
        assert_eq!(guess_genre("Thing", "nothing here"), None);
    }
}
