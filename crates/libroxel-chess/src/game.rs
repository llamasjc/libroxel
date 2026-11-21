//! A whole game: the moves so far, how it ended, and the game record.

use crate::board::{Color, Move, Position};

/// Who won.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Winner {
    White,
    Black,
    Draw,
}

impl Winner {
    pub fn of(color: Color) -> Winner {
        match color {
            Color::White => Winner::White,
            Color::Black => Winner::Black,
        }
    }

    /// The result as written in a game record: `1-0`, `0-1` or `1/2-1/2`.
    pub fn score(self) -> &'static str {
        match self {
            Winner::White => "1-0",
            Winner::Black => "0-1",
            Winner::Draw => "1/2-1/2",
        }
    }

    pub fn from_score(text: &str) -> Option<Winner> {
        Some(match text {
            "1-0" => Winner::White,
            "0-1" => Winner::Black,
            "1/2-1/2" => Winner::Draw,
            _ => return None,
        })
    }

    /// 1 for a win, 0.5 for a draw and 0 for a loss, from `color`'s side.
    pub fn points_for(self, color: Color) -> f64 {
        match (self, color) {
            (Winner::Draw, _) => 0.5,
            (Winner::White, Color::White) | (Winner::Black, Color::Black) => 1.0,
            _ => 0.0,
        }
    }
}

/// Why a game ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    Checkmate,
    Stalemate,
    Repetition,
    FiftyMoves,
    InsufficientMaterial,
    Resigned,
    /// A player's clock ran out.
    Timeout,
    /// A clock ran out, but the other side had nothing left to mate with.
    TimeoutDraw,
    Agreement,
    /// A player left and didn't come back.
    Abandoned,
    /// Called off before both players had moved; nobody wins or loses.
    Aborted,
}

impl Reason {
    pub const ALL: [Reason; 11] = [
        Reason::Checkmate,
        Reason::Stalemate,
        Reason::Repetition,
        Reason::FiftyMoves,
        Reason::InsufficientMaterial,
        Reason::Resigned,
        Reason::Timeout,
        Reason::TimeoutDraw,
        Reason::Agreement,
        Reason::Abandoned,
        Reason::Aborted,
    ];

    /// A short stable name, for saving and sending.
    pub fn code(self) -> &'static str {
        match self {
            Reason::Checkmate => "checkmate",
            Reason::Stalemate => "stalemate",
            Reason::Repetition => "repetition",
            Reason::FiftyMoves => "fifty",
            Reason::InsufficientMaterial => "material",
            Reason::Resigned => "resign",
            Reason::Timeout => "timeout",
            Reason::TimeoutDraw => "timeout-draw",
            Reason::Agreement => "agreement",
            Reason::Abandoned => "abandoned",
            Reason::Aborted => "aborted",
        }
    }

    pub fn from_code(code: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.code() == code)
    }

    /// How it reads under the result, such as "by checkmate".
    pub fn describe(self) -> &'static str {
        match self {
            Reason::Checkmate => "by checkmate",
            Reason::Stalemate => "by stalemate",
            Reason::Repetition => "by threefold repetition",
            Reason::FiftyMoves => "by the 50-move rule",
            Reason::InsufficientMaterial => "with too few pieces left to mate",
            Reason::Resigned => "by resignation",
            Reason::Timeout => "on time",
            Reason::TimeoutDraw => "on time, with nothing left to mate with",
            Reason::Agreement => "by agreement",
            Reason::Abandoned => "because a player left",
            Reason::Aborted => "before it started",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub winner: Winner,
    pub reason: Reason,
}

impl Outcome {
    /// "White won by checkmate", "Draw by stalemate".
    pub fn headline(self) -> String {
        match (self.winner, self.reason) {
            (_, Reason::Aborted) => "Game aborted".into(),
            (Winner::Draw, r) => format!("Draw {}", r.describe()),
            (Winner::White, r) => format!("White won {}", r.describe()),
            (Winner::Black, r) => format!("Black won {}", r.describe()),
        }
    }
}

/// A game from some start position, with every position along the way.
#[derive(Debug, Clone)]
pub struct Game {
    positions: Vec<Position>,
    moves: Vec<Move>,
    sans: Vec<String>,
    keys: Vec<u64>,
    /// Set when the game ended some way the board can't show (resigning,
    /// a clock, an agreed draw).
    ended: Option<Outcome>,
}

impl Default for Game {
    fn default() -> Self {
        Game::new(Position::start())
    }
}

impl Game {
    pub fn new(start: Position) -> Self {
        Game {
            keys: vec![start.key()],
            positions: vec![start],
            moves: Vec::new(),
            sans: Vec::new(),
            ended: None,
        }
    }

    /// A game from its moves in coordinates (`e2e4 e7e5 …`), checking each.
    pub fn from_uci(start: Position, moves: &str) -> Result<Self, String> {
        let mut game = Game::new(start);
        for text in moves.split_whitespace() {
            let m = Move::from_uci(text).ok_or_else(|| format!("{text} is not a move"))?;
            game.play(m)?;
        }
        Ok(game)
    }

    pub fn position(&self) -> &Position {
        self.positions.last().expect("a game always has a position")
    }

    /// The position after `ply` half-moves (0 is the start).
    pub fn position_at(&self, ply: usize) -> &Position {
        &self.positions[ply.min(self.moves.len())]
    }

    pub fn start(&self) -> &Position {
        &self.positions[0]
    }

    pub fn moves(&self) -> &[Move] {
        &self.moves
    }

    pub fn sans(&self) -> &[String] {
        &self.sans
    }

    pub fn ply(&self) -> usize {
        self.moves.len()
    }

    pub fn turn(&self) -> Color {
        self.position().turn
    }

    pub fn last_move(&self) -> Option<Move> {
        self.moves.last().copied()
    }

    /// The moves in coordinates, separated by spaces.
    pub fn uci_moves(&self) -> String {
        self.moves.iter().map(|m| m.uci()).collect::<Vec<_>>().join(" ")
    }

    /// Plays a move if it is legal and the game hasn't ended.
    pub fn play(&mut self, m: Move) -> Result<(), String> {
        if self.outcome().is_some() {
            return Err("the game is over".into());
        }
        let pos = *self.position();
        // A pawn reaching the end without saying what it becomes is a queen.
        let m = match (m.promotion, pos.at(m.from)) {
            (None, Some(p)) if p.kind == crate::Kind::Pawn && (m.to / 8 == 0 || m.to / 8 == 7) => {
                Move { promotion: Some(crate::Kind::Queen), ..m }
            }
            _ => m,
        };
        if !pos.is_legal(m) {
            return Err(format!("{} is not a legal move here", m.uci()));
        }
        self.sans.push(pos.san(m));
        let next = pos.play(m);
        self.keys.push(next.key());
        self.positions.push(next);
        self.moves.push(m);
        Ok(())
    }

    /// Takes back the last move (for games against the computer).
    pub fn undo(&mut self) -> Option<Move> {
        if self.moves.is_empty() {
            return None;
        }
        self.ended = None;
        self.positions.pop();
        self.keys.pop();
        self.sans.pop();
        self.moves.pop()
    }

    /// Ends the game some way the board can't show.
    pub fn end(&mut self, outcome: Outcome) {
        if self.board_outcome().is_none() {
            self.ended = Some(outcome);
        }
    }

    /// How the game ended, if it has.
    pub fn outcome(&self) -> Option<Outcome> {
        self.board_outcome().or(self.ended)
    }

    /// How the position on the board ends the game, if it does.
    pub fn board_outcome(&self) -> Option<Outcome> {
        let pos = self.position();
        if pos.legal_moves().is_empty() {
            return Some(if pos.in_check() {
                Outcome { winner: Winner::of(pos.turn.other()), reason: Reason::Checkmate }
            } else {
                Outcome { winner: Winner::Draw, reason: Reason::Stalemate }
            });
        }
        let draw = |reason| Some(Outcome { winner: Winner::Draw, reason });
        if pos.insufficient_material() {
            return draw(Reason::InsufficientMaterial);
        }
        let key = *self.keys.last().unwrap();
        if self.keys.iter().filter(|k| **k == key).count() >= 3 {
            return draw(Reason::Repetition);
        }
        if pos.halfmove >= 100 {
            return draw(Reason::FiftyMoves);
        }
        None
    }

    /// The game record (PGN) with the given header tags.
    pub fn pgn(&self, tags: &[(&str, String)]) -> String {
        let mut out = String::new();
        let result =
            self.outcome().map_or("*", |o| if o.reason == Reason::Aborted { "*" } else { o.winner.score() });
        for (name, value) in tags {
            out.push_str(&format!("[{name} \"{}\"]\n", value.replace('"', "'")));
        }
        if !tags.iter().any(|(n, _)| *n == "Result") {
            out.push_str(&format!("[Result \"{result}\"]\n"));
        }
        if self.start().fen() != crate::START_FEN {
            out.push_str(&format!("[SetUp \"1\"]\n[FEN \"{}\"]\n", self.start().fen()));
        }
        out.push('\n');
        let mut line = String::new();
        let mut words: Vec<String> = Vec::new();
        let start = self.start();
        let mut number = start.fullmove;
        for (i, san) in self.sans.iter().enumerate() {
            let white = (i % 2 == 0) == (start.turn == Color::White);
            if white {
                words.push(format!("{number}. {san}"));
            } else {
                if i == 0 {
                    words.push(format!("{number}... {san}"));
                } else {
                    words.push(san.clone());
                }
                number += 1;
            }
        }
        words.push(result.into());
        for w in words {
            if line.len() + w.len() + 1 > 80 {
                out.push_str(line.trim_end());
                out.push('\n');
                line.clear();
            }
            line.push_str(&w);
            line.push(' ');
        }
        out.push_str(line.trim_end());
        out.push('\n');
        out
    }

    /// Pieces each side has lost, and how many points `White` is ahead.
    pub fn captured(&self) -> ([Vec<crate::Kind>; 2], i32) {
        let start = [8, 2, 2, 2, 1, 1];
        let mut lost: [Vec<crate::Kind>; 2] = [Vec::new(), Vec::new()];
        let mut lead = 0;
        for color in [Color::White, Color::Black] {
            for kind in crate::Kind::ALL {
                let have = self
                    .position()
                    .board
                    .iter()
                    .flatten()
                    .filter(|p| p.color == color && p.kind == kind)
                    .count() as i32;
                let sign = if color == Color::White { 1 } else { -1 };
                lead += sign * have * kind.points();
                for _ in have..start[kind as usize] {
                    lost[color.index()].push(kind);
                }
            }
        }
        (lost, lead)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fools_mate() {
        let g = Game::from_uci(Position::start(), "f2f3 e7e5 g2g4 d8h4").unwrap();
        assert_eq!(g.outcome(), Some(Outcome { winner: Winner::Black, reason: Reason::Checkmate }));
        assert_eq!(g.sans(), ["f3", "e5", "g4", "Qh4#"]);
        assert!(g.pgn(&[]).contains("1. f3 e5 2. g4 Qh4# 0-1"));
    }

    #[test]
    fn repetition() {
        let g = Game::from_uci(Position::start(), "g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1 f6g8").unwrap();
        assert_eq!(g.outcome().map(|o| o.reason), Some(Reason::Repetition));
    }

    #[test]
    fn stalemate() {
        let g = Game::from_uci(Position::from_fen("7k/8/6Q1/8/8/8/8/K7 w - - 0 1").unwrap(), "g6f7").unwrap();
        assert_eq!(g.outcome().map(|o| o.reason), Some(Reason::Stalemate));
    }

    #[test]
    fn promotion_defaults_to_queen() {
        let mut g = Game::new(Position::from_fen("7k/P7/8/8/8/8/8/K7 w - - 0 1").unwrap());
        g.play(Move::new(48, 56)).unwrap();
        assert_eq!(g.sans()[0], "a8=Q+");
    }
}
