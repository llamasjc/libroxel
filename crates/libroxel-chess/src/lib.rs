//! Chess for Libroxel.
//!
//! - [`Position`] and [`Move`]: the board and every rule for moving on it
//!   (castling, en passant, promotion), standard notation and FEN.
//! - [`Game`]: a game's moves, and how it ends (checkmate, stalemate,
//!   threefold repetition, the 50-move rule, too few pieces, or something
//!   off the board such as resigning or a clock running out).
//! - [`TimeControl`] and [`Category`]: clocks, such as 3 minutes plus 2
//!   seconds a move, and which rating they count for.
//! - The [`elo`] module: ratings.
//! - [`Bot`]: the computer players, from a beginner to a strong one.
//!
//! The Hub uses the same rules to check every move of an online game, so
//! the app and the Hub always agree.

mod board;
mod bot;
mod game;

pub use board::{
    Color, Kind, Move, Piece, Position, START_FEN, Square, file_of, parse_square, rank_of, square,
    square_name,
};
pub use bot::{BOTS, Bot, BotLevel, Search, evaluate};
pub use game::{Game, Outcome, Reason, Winner};

/// A game's clock: minutes each, plus seconds added after every move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeControl {
    pub base_secs: u32,
    pub increment_secs: u32,
}

/// Which rating a game counts toward, by how long it lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Bullet,
    Blitz,
    Rapid,
    Classical,
}

impl Category {
    pub const ALL: [Category; 4] = [Category::Bullet, Category::Blitz, Category::Rapid, Category::Classical];

    pub fn code(self) -> &'static str {
        match self {
            Category::Bullet => "bullet",
            Category::Blitz => "blitz",
            Category::Rapid => "rapid",
            Category::Classical => "classical",
        }
    }

    pub fn from_code(code: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.code() == code)
    }

    pub fn name(self) -> &'static str {
        match self {
            Category::Bullet => "Bullet",
            Category::Blitz => "Blitz",
            Category::Rapid => "Rapid",
            Category::Classical => "Classical",
        }
    }
}

impl TimeControl {
    pub const fn new(minutes: u32, increment_secs: u32) -> Self {
        TimeControl { base_secs: minutes * 60, increment_secs }
    }

    /// The clocks offered for quick matches.
    pub const PRESETS: [TimeControl; 10] = [
        TimeControl::new(1, 0),
        TimeControl::new(2, 1),
        TimeControl::new(3, 0),
        TimeControl::new(3, 2),
        TimeControl::new(5, 0),
        TimeControl::new(5, 3),
        TimeControl::new(10, 0),
        TimeControl::new(10, 5),
        TimeControl::new(15, 10),
        TimeControl::new(30, 0),
    ];

    /// "3+2", or "0.5+0" for 30 seconds.
    pub fn code(self) -> String {
        if self.base_secs.is_multiple_of(60) {
            format!("{}+{}", self.base_secs / 60, self.increment_secs)
        } else {
            format!("{}+{}", self.base_secs as f32 / 60.0, self.increment_secs)
        }
    }

    pub fn parse(code: &str) -> Option<TimeControl> {
        let (base, inc) = code.trim().split_once('+')?;
        let minutes: f32 = base.trim().parse().ok()?;
        let increment_secs: u32 = inc.trim().parse().ok()?;
        let base_secs = (minutes * 60.0).round() as u32;
        // Between 15 seconds and 3 hours, with at most a minute a move.
        ((15..=3 * 3600).contains(&base_secs) && increment_secs <= 60)
            .then_some(TimeControl { base_secs, increment_secs })
    }

    /// Roughly how long a game lasts for each player, counting 40 moves.
    pub fn estimate_secs(self) -> u32 {
        self.base_secs + 40 * self.increment_secs
    }

    pub fn category(self) -> Category {
        match self.estimate_secs() {
            0..180 => Category::Bullet,
            180..480 => Category::Blitz,
            480..1500 => Category::Rapid,
            _ => Category::Classical,
        }
    }

    pub fn base_ms(self) -> i64 {
        self.base_secs as i64 * 1000
    }

    pub fn increment_ms(self) -> i64 {
        self.increment_secs as i64 * 1000
    }
}

/// Ratings: everyone starts at [`elo::START`]; a win against a stronger
/// player gains more than one against a weaker one.
pub mod elo {
    pub const START: i32 = 1200;
    pub const FLOOR: i32 = 100;

    /// The chance `a` beats `b`, from 0 to 1.
    pub fn expected(a: i32, b: i32) -> f64 {
        1.0 / (1.0 + 10f64.powf((b - a) as f64 / 400.0))
    }

    /// How much a rating moves: more while a player is new.
    pub fn k_factor(games: u32) -> f64 {
        if games < 30 { 40.0 } else { 24.0 }
    }

    /// The new rating of a player rated `mine` with `games` played, after
    /// scoring `points` (1, 0.5 or 0) against `theirs`.
    pub fn update(mine: i32, games: u32, theirs: i32, points: f64) -> i32 {
        let change = k_factor(games) * (points - expected(mine, theirs));
        (mine + change.round() as i32).max(FLOOR)
    }
}

/// Clocks in milliseconds, ticking for the side to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clocks {
    pub white_ms: i64,
    pub black_ms: i64,
}

impl Clocks {
    pub fn new(control: TimeControl) -> Self {
        Clocks { white_ms: control.base_ms(), black_ms: control.base_ms() }
    }

    pub fn get(&self, color: Color) -> i64 {
        match color {
            Color::White => self.white_ms,
            Color::Black => self.black_ms,
        }
    }

    pub fn get_mut(&mut self, color: Color) -> &mut i64 {
        match color {
            Color::White => &mut self.white_ms,
            Color::Black => &mut self.black_ms,
        }
    }
}

/// How a clock reads: `4:59`, `0:09.4` under ten seconds, `1:02:00` over an hour.
pub fn clock_text(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 10_000 {
        return format!("0:{:02}.{}", ms / 1000, (ms % 1000) / 100);
    }
    let secs = (ms + 999) / 1000;
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls() {
        assert_eq!(TimeControl::parse("3+2"), Some(TimeControl::new(3, 2)));
        assert_eq!(TimeControl::new(3, 2).code(), "3+2");
        assert_eq!(TimeControl::new(1, 0).category(), Category::Bullet);
        assert_eq!(TimeControl::new(3, 2).category(), Category::Blitz);
        assert_eq!(TimeControl::new(10, 0).category(), Category::Rapid);
        assert_eq!(TimeControl::new(30, 0).category(), Category::Classical);
        assert_eq!(TimeControl::parse("0+0"), None);
        assert_eq!(clock_text(299_500), "5:00");
        assert_eq!(clock_text(9_400), "0:09.4");
    }

    #[test]
    fn ratings() {
        assert_eq!(elo::update(1200, 0, 1200, 1.0), 1220);
        assert_eq!(elo::update(1200, 0, 1200, 0.0), 1180);
        assert_eq!(elo::update(1200, 50, 1200, 0.5), 1200);
        assert!(elo::update(1200, 50, 1600, 1.0) > elo::update(1200, 50, 1000, 1.0));
    }
}
