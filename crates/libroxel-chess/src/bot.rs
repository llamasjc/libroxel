//! The computer players: one search, played at different strengths.
//!
//! The search looks ahead with alpha-beta and iterative deepening, follows
//! captures to the end so it doesn't stop in the middle of a trade, and
//! remembers positions it has seen. Weaker players look less far ahead and
//! sometimes pick a worse move on purpose.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::board::{Color, Kind, Move, Position, file_of, rank_of};

/// One computer player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BotLevel {
    pub name: &'static str,
    /// About how strong it plays, on the same scale as player ratings.
    pub rating: i32,
    pub about: &'static str,
    /// How many half-moves it looks ahead at most.
    pub depth: u32,
    /// The longest it thinks about one move.
    pub think_ms: u64,
    /// How often it plays a random move instead of thinking.
    pub random: f64,
    /// How much (in hundredths of a pawn) it misjudges each move.
    pub noise: i32,
}

/// Every computer player, weakest first.
pub const BOTS: [BotLevel; 8] = [
    BotLevel {
        name: "Droplet",
        rating: 250,
        about: "Just learned how the pieces move.",
        depth: 1,
        think_ms: 200,
        random: 0.45,
        noise: 250,
    },
    BotLevel {
        name: "Breeze",
        rating: 500,
        about: "Grabs anything left hanging, and leaves things hanging too.",
        depth: 1,
        think_ms: 300,
        random: 0.2,
        noise: 120,
    },
    BotLevel {
        name: "Pebble",
        rating: 800,
        about: "Sees one move ahead, most of the time.",
        depth: 2,
        think_ms: 400,
        random: 0.1,
        noise: 70,
    },
    BotLevel {
        name: "Coral",
        rating: 1100,
        about: "A careful club beginner who likes trading pieces.",
        depth: 3,
        think_ms: 500,
        random: 0.04,
        noise: 40,
    },
    BotLevel {
        name: "Aurora",
        rating: 1400,
        about: "Spots most short tactics and punishes loose pieces.",
        depth: 4,
        think_ms: 700,
        random: 0.0,
        noise: 20,
    },
    BotLevel {
        name: "Glacier",
        rating: 1700,
        about: "Slow and solid. Hard to trick.",
        depth: 5,
        think_ms: 1000,
        random: 0.0,
        noise: 8,
    },
    BotLevel {
        name: "Nimbus",
        rating: 2000,
        about: "Calculates deep and plays for the long game.",
        depth: 64,
        think_ms: 1800,
        random: 0.0,
        noise: 0,
    },
    BotLevel {
        name: "Zenith",
        rating: 2300,
        about: "The strongest player in Libroxel. Good luck.",
        depth: 64,
        think_ms: 3500,
        random: 0.0,
        noise: 0,
    },
];

const MATE: i32 = 100_000;
const INF: i32 = 1_000_000;

fn value(kind: Kind) -> i32 {
    match kind {
        Kind::Pawn => 100,
        Kind::Knight => 320,
        Kind::Bishop => 330,
        Kind::Rook => 500,
        Kind::Queen => 900,
        Kind::King => 0,
    }
}

// Where each piece likes to stand, from White's side, a8 first (as the
// board looks from White).
#[rustfmt::skip]
const PAWN: [i32; 64] = [
     0,  0,  0,  0,  0,  0,  0,  0,
    50, 50, 50, 50, 50, 50, 50, 50,
    10, 10, 20, 30, 30, 20, 10, 10,
     5,  5, 10, 25, 25, 10,  5,  5,
     0,  0,  0, 20, 20,  0,  0,  0,
     5, -5,-10,  0,  0,-10, -5,  5,
     5, 10, 10,-20,-20, 10, 10,  5,
     0,  0,  0,  0,  0,  0,  0,  0,
];
#[rustfmt::skip]
const KNIGHT: [i32; 64] = [
    -50,-40,-30,-30,-30,-30,-40,-50,
    -40,-20,  0,  0,  0,  0,-20,-40,
    -30,  0, 10, 15, 15, 10,  0,-30,
    -30,  5, 15, 20, 20, 15,  5,-30,
    -30,  0, 15, 20, 20, 15,  0,-30,
    -30,  5, 10, 15, 15, 10,  5,-30,
    -40,-20,  0,  5,  5,  0,-20,-40,
    -50,-40,-30,-30,-30,-30,-40,-50,
];
#[rustfmt::skip]
const BISHOP: [i32; 64] = [
    -20,-10,-10,-10,-10,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5, 10, 10,  5,  0,-10,
    -10,  5,  5, 10, 10,  5,  5,-10,
    -10,  0, 10, 10, 10, 10,  0,-10,
    -10, 10, 10, 10, 10, 10, 10,-10,
    -10,  5,  0,  0,  0,  0,  5,-10,
    -20,-10,-10,-10,-10,-10,-10,-20,
];
#[rustfmt::skip]
const ROOK: [i32; 64] = [
     0,  0,  0,  0,  0,  0,  0,  0,
     5, 10, 10, 10, 10, 10, 10,  5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
     0,  0,  0,  5,  5,  0,  0,  0,
];
#[rustfmt::skip]
const QUEEN: [i32; 64] = [
    -20,-10,-10, -5, -5,-10,-10,-20,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -10,  0,  5,  5,  5,  5,  0,-10,
     -5,  0,  5,  5,  5,  5,  0, -5,
      0,  0,  5,  5,  5,  5,  0, -5,
    -10,  5,  5,  5,  5,  5,  0,-10,
    -10,  0,  5,  0,  0,  0,  0,-10,
    -20,-10,-10, -5, -5,-10,-10,-20,
];
#[rustfmt::skip]
const KING_MIDDLE: [i32; 64] = [
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -20,-30,-30,-40,-40,-30,-30,-20,
    -10,-20,-20,-20,-20,-20,-20,-10,
     20, 20,  0,  0,  0,  0, 20, 20,
     20, 30, 10,  0,  0, 10, 30, 20,
];
#[rustfmt::skip]
const KING_END: [i32; 64] = [
    -50,-40,-30,-20,-20,-30,-40,-50,
    -30,-20,-10,  0,  0,-10,-20,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 30, 40, 40, 30,-10,-30,
    -30,-10, 20, 30, 30, 20,-10,-30,
    -30,-30,  0,  0,  0,  0,-30,-30,
    -50,-30,-30,-30,-30,-30,-30,-50,
];

/// How good the position is for the side to move, in hundredths of a pawn.
pub fn evaluate(pos: &Position) -> i32 {
    let mut score = [0i32; 2];
    let mut non_pawn = 0;
    let mut bishops = [0; 2];
    let mut pawn_files = [[0u8; 8]; 2];
    for p in pos.board.iter().flatten() {
        if !matches!(p.kind, Kind::Pawn | Kind::King) {
            non_pawn += value(p.kind);
        }
    }
    let endgame = non_pawn <= 1300;
    for (sq, p) in pos.board.iter().enumerate() {
        let Some(p) = p else { continue };
        let (f, r) = (sq % 8, sq / 8);
        // The tables are drawn with rank 8 first, from White's side.
        let i = if p.color == Color::White { (7 - r) * 8 + f } else { r * 8 + f };
        let table = match p.kind {
            Kind::Pawn => PAWN[i],
            Kind::Knight => KNIGHT[i],
            Kind::Bishop => BISHOP[i],
            Kind::Rook => ROOK[i],
            Kind::Queen => QUEEN[i],
            Kind::King if endgame => KING_END[i],
            Kind::King => KING_MIDDLE[i],
        };
        let c = p.color.index();
        score[c] += value(p.kind) + table;
        match p.kind {
            Kind::Bishop => bishops[c] += 1,
            Kind::Pawn => pawn_files[c][f] += 1,
            _ => {}
        }
    }
    for c in 0..2 {
        if bishops[c] >= 2 {
            score[c] += 30;
        }
        for f in 0..8 {
            let n = pawn_files[c][f] as i32;
            if n > 1 {
                score[c] -= 15 * (n - 1);
            }
            let left = f > 0 && pawn_files[c][f - 1] > 0;
            let right = f < 7 && pawn_files[c][f + 1] > 0;
            if n > 0 && !left && !right {
                score[c] -= 12 * n;
            }
        }
    }
    // Passed pawns grow stronger the closer they are to promoting.
    for (sq, p) in pos.board.iter().enumerate() {
        let Some(p) = p else { continue };
        if p.kind != Kind::Pawn {
            continue;
        }
        let (f, r) = (file_of(sq as u8), rank_of(sq as u8));
        let ahead = |rr: i8| if p.color == Color::White { rr > r } else { rr < r };
        let blocked = pos.board.iter().enumerate().any(|(s2, q)| {
            q.is_some_and(|q| q.kind == Kind::Pawn && q.color != p.color)
                && (file_of(s2 as u8) - f).abs() <= 1
                && ahead(rank_of(s2 as u8))
        });
        if !blocked {
            let progress = if p.color == Color::White { r } else { 7 - r } as i32;
            score[p.color.index()] += progress * progress * if endgame { 6 } else { 3 };
        }
    }
    // In a won ending, push the losing king to the edge and bring ours close.
    if endgame {
        let diff = score[0] - score[1];
        if diff.abs() > 300 {
            let (strong, weak) =
                if diff > 0 { (Color::White, Color::Black) } else { (Color::Black, Color::White) };
            let wk = pos.king(weak);
            let sk = pos.king(strong);
            let centre = |s: u8| ((file_of(s) as i32 * 2 - 7).abs() + (rank_of(s) as i32 * 2 - 7).abs()) / 2;
            let apart = (file_of(wk) - file_of(sk)).abs() as i32 + (rank_of(wk) - rank_of(sk)).abs() as i32;
            score[strong.index()] += centre(wk) * 10 + (14 - apart) * 4;
        }
    }
    let total = score[0] - score[1];
    if pos.turn == Color::White { total } else { -total }
}

#[derive(Clone, Copy, Default)]
struct Entry {
    key: u64,
    depth: i8,
    score: i32,
    flag: u8,
    best: Option<Move>,
}

const EXACT: u8 = 0;
const LOWER: u8 = 1;
const UPPER: u8 = 2;
const TABLE_SIZE: usize = 1 << 18;

/// What a search found.
#[derive(Debug, Clone)]
pub struct Search {
    pub best: Option<Move>,
    /// For the side to move, in hundredths of a pawn (big numbers are mates).
    pub score: i32,
    pub depth: u32,
    pub nodes: u64,
    /// The line it expects.
    pub line: Vec<Move>,
}

impl Search {
    /// Mate in this many moves (negative: being mated), if the score is a mate.
    pub fn mate_in(&self) -> Option<i32> {
        (self.score.abs() > MATE - 1000).then(|| {
            let plies = MATE - self.score.abs();
            let moves = (plies + 1) / 2;
            if self.score > 0 { moves } else { -moves }
        })
    }
}

/// A computer player's brain. Keep one per game so it remembers what it
/// worked out on earlier moves.
pub struct Bot {
    table: Vec<Entry>,
    killers: Vec<[Option<Move>; 2]>,
    nodes: u64,
    deadline: Instant,
    stop: Arc<AtomicBool>,
    stopped: bool,
    rng: u64,
    /// Keys of the positions already played, for repetitions.
    history: Vec<u64>,
}

impl Default for Bot {
    fn default() -> Self {
        Bot::new()
    }
}

impl Bot {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
            | 1;
        Bot {
            table: vec![Entry::default(); TABLE_SIZE],
            killers: vec![[None; 2]; 128],
            nodes: 0,
            deadline: Instant::now(),
            stop: Arc::new(AtomicBool::new(false)),
            stopped: false,
            rng: seed,
            history: Vec::new(),
        }
    }

    /// Something to set from another thread to stop thinking early.
    pub fn stopper(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn chance(&mut self) -> f64 {
        (self.random() % 1_000_000) as f64 / 1_000_000.0
    }

    /// Picks a move for the game's side to move, playing like `level`.
    pub fn choose(&mut self, game: &crate::Game, level: &BotLevel) -> Option<Move> {
        let pos = *game.position();
        let legal = pos.legal_moves();
        if legal.is_empty() {
            return None;
        }
        if legal.len() == 1 {
            return Some(legal[0]);
        }
        if level.random > 0.0 && self.chance() < level.random {
            // Even a beginner takes a free queen now and then.
            let i = (self.random() % legal.len() as u64) as usize;
            return Some(legal[i]);
        }
        self.history = (0..game.ply()).map(|i| game.position_at(i).search_key()).collect();
        if level.noise > 0 {
            return self.noisy(&pos, level);
        }
        self.think(&pos, level.depth, Duration::from_millis(level.think_ms)).best
    }

    /// Scores every move a little wrong, and plays the best-looking one.
    fn noisy(&mut self, pos: &Position, level: &BotLevel) -> Option<Move> {
        self.start(Duration::from_millis(level.think_ms));
        let mut best = None;
        let mut best_score = -INF;
        for m in pos.legal_moves() {
            let next = pos.play(m);
            self.history.push(pos.search_key());
            let score = -self.negamax(&next, level.depth as i32 - 1, 1, -INF, INF);
            self.history.pop();
            let jitter = (self.random() % (2 * level.noise as u64 + 1)) as i32 - level.noise;
            if score + jitter > best_score {
                best_score = score + jitter;
                best = Some(m);
            }
        }
        best
    }

    fn start(&mut self, time: Duration) {
        self.nodes = 0;
        self.stopped = false;
        self.stop.store(false, Ordering::Relaxed);
        self.deadline = Instant::now() + time;
        for k in &mut self.killers {
            *k = [None; 2];
        }
    }

    /// Searches `pos` up to `depth` half-moves or until `time` runs out,
    /// whichever comes first, and returns the best it found.
    pub fn think(&mut self, pos: &Position, depth: u32, time: Duration) -> Search {
        self.start(time);
        let legal = pos.legal_moves();
        let mut result =
            Search { best: legal.first().copied(), score: 0, depth: 0, nodes: 0, line: Vec::new() };
        for d in 1..=depth.min(64) as i32 {
            let score = self.negamax(pos, d, 0, -INF, INF);
            if self.stopped && d > 1 {
                break;
            }
            let entry = self.table[(pos.search_key() as usize) & (TABLE_SIZE - 1)];
            if entry.key == pos.search_key()
                && let Some(best) = entry.best
            {
                result.best = Some(best);
            }
            result.score = score;
            result.depth = d as u32;
            if self.stopped
                || score.abs() > MATE - 1000
                || Instant::now() + (Instant::now() - (self.deadline - time)) > self.deadline
            {
                break;
            }
        }
        result.nodes = self.nodes;
        result.line = self.line(pos);
        result
    }

    fn line(&self, pos: &Position) -> Vec<Move> {
        let mut line = Vec::new();
        let mut pos = *pos;
        for _ in 0..12 {
            let key = pos.search_key();
            let e = self.table[(key as usize) & (TABLE_SIZE - 1)];
            match e.best.filter(|m| e.key == key && pos.is_legal(*m)) {
                Some(m) => {
                    line.push(m);
                    pos = pos.play(m);
                }
                None => break,
            }
        }
        line
    }

    fn out_of_time(&mut self) -> bool {
        if self.nodes & 2047 == 0 && (Instant::now() >= self.deadline || self.stop.load(Ordering::Relaxed)) {
            self.stopped = true;
        }
        self.stopped
    }

    fn order(&self, pos: &Position, moves: &mut [Move], best: Option<Move>, ply: usize) {
        let killers = self.killers.get(ply).copied().unwrap_or_default();
        moves.sort_by_cached_key(|m| {
            if Some(*m) == best {
                return -1_000_000;
            }
            let mut s = 0;
            if let Some(victim) = pos.at(m.to) {
                let attacker = pos.at(m.from).map_or(0, |p| value(p.kind));
                s -= 10 * value(victim.kind) - attacker + 100_000;
            }
            if let Some(k) = m.promotion {
                s -= value(k) + 50_000;
            }
            if killers.contains(&Some(*m)) {
                s -= 20_000;
            }
            s
        });
    }

    fn negamax(&mut self, pos: &Position, mut depth: i32, ply: usize, mut alpha: i32, beta: i32) -> i32 {
        self.nodes += 1;
        if self.out_of_time() {
            return 0;
        }
        let key = pos.search_key();
        if ply > 0 {
            if pos.halfmove >= 100 || pos.insufficient_material() {
                return 0;
            }
            // Coming back to a position is as good as a draw.
            let back = pos.halfmove as usize;
            if self.history.iter().rev().take(back).any(|k| *k == key) {
                return 0;
            }
        }
        let in_check = pos.in_check();
        if in_check {
            depth += 1;
        }
        if depth <= 0 {
            return self.quiesce(pos, alpha, beta, ply);
        }
        let slot = (key as usize) & (TABLE_SIZE - 1);
        let entry = self.table[slot];
        let mut best_move = None;
        if entry.key == key {
            best_move = entry.best;
            if ply > 0 && entry.depth as i32 >= depth {
                let s = entry.score;
                match entry.flag {
                    EXACT => return s,
                    LOWER if s >= beta => return s,
                    UPPER if s <= alpha => return s,
                    _ => {}
                }
            }
        }
        let mut moves = Vec::with_capacity(48);
        pos.pseudo_moves(&mut moves, false);
        self.order(pos, &mut moves, best_move, ply);
        let original_alpha = alpha;
        let mut best = -INF;
        let mut legal = 0;
        self.history.push(key);
        for m in moves {
            let next = pos.play(m);
            if next.attacked(next.king(pos.turn), next.turn) {
                continue;
            }
            legal += 1;
            let score = if legal == 1 {
                -self.negamax(&next, depth - 1, ply + 1, -beta, -alpha)
            } else {
                // Look again with a full window only when it might be better.
                let s = -self.negamax(&next, depth - 1, ply + 1, -alpha - 1, -alpha);
                if s > alpha && s < beta {
                    -self.negamax(&next, depth - 1, ply + 1, -beta, -alpha)
                } else {
                    s
                }
            };
            if self.stopped {
                self.history.pop();
                return 0;
            }
            if score > best {
                best = score;
                best_move = Some(m);
            }
            if score > alpha {
                alpha = score;
            }
            if alpha >= beta {
                if !pos.is_capture(m)
                    && let Some(k) = self.killers.get_mut(ply)
                    && k[0] != Some(m)
                {
                    k[1] = k[0];
                    k[0] = Some(m);
                }
                break;
            }
        }
        self.history.pop();
        if legal == 0 {
            return if in_check { -MATE + ply as i32 } else { 0 };
        }
        let flag = if best <= original_alpha {
            UPPER
        } else if best >= beta {
            LOWER
        } else {
            EXACT
        };
        self.table[slot] = Entry { key, depth: depth.min(127) as i8, score: best, flag, best: best_move };
        best
    }

    fn quiesce(&mut self, pos: &Position, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.nodes += 1;
        if self.out_of_time() {
            return 0;
        }
        let stand = evaluate(pos);
        if stand >= beta {
            return stand;
        }
        alpha = alpha.max(stand);
        if ply > 100 {
            return stand;
        }
        let mut moves = Vec::with_capacity(16);
        pos.pseudo_moves(&mut moves, true);
        self.order(pos, &mut moves, None, ply);
        for m in moves {
            let next = pos.play(m);
            if next.attacked(next.king(pos.turn), next.turn) {
                continue;
            }
            let score = -self.quiesce(&next, -beta, -alpha, ply + 1);
            if self.stopped {
                return 0;
            }
            if score >= beta {
                return score;
            }
            alpha = alpha.max(score);
        }
        alpha
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Game;

    #[test]
    fn finds_mate_in_one() {
        let pos = Position::from_fen("6k1/5ppp/8/8/8/8/8/K3R3 w - - 0 1").unwrap();
        let s = Bot::new().think(&pos, 4, Duration::from_secs(5));
        assert_eq!(s.best.map(|m| m.uci()), Some("e1e8".into()));
        assert_eq!(s.mate_in(), Some(1));
    }

    #[test]
    fn takes_a_hanging_queen() {
        let pos =
            Position::from_fen("rnb1kbnr/pppp1ppp/8/4p1q1/3P4/2N5/PPP1PPPP/R1BQKBNR w KQkq - 0 1").unwrap();
        let s = Bot::new().think(&pos, 4, Duration::from_secs(5));
        assert_eq!(s.best.map(|m| m.uci()), Some("c1g5".into()));
    }

    #[test]
    fn mate_in_two() {
        // Queen sacrifice then mate.
        let pos = Position::from_fen("r1b2k1r/ppp1bppp/8/1B1Q4/5q2/2P5/PPP2PPP/R3R1K1 w - - 1 0").unwrap();
        let s = Bot::new().think(&pos, 6, Duration::from_secs(10));
        assert_eq!(s.best.map(|m| m.uci()), Some("d5d8".into()));
    }

    #[test]
    fn every_level_plays_legal_moves() {
        for level in BOTS {
            let mut game = Game::default();
            let mut bot = Bot::new();
            for _ in 0..6 {
                let quick = BotLevel { think_ms: 100, ..level };
                let m = bot.choose(&game, &quick).unwrap();
                game.play(m).unwrap();
            }
        }
    }

    #[test]
    fn strong_beats_weak() {
        let strong = BotLevel { think_ms: 60, depth: 3, ..BOTS[4] };
        let weak = BOTS[0];
        let mut points = 0.0;
        for round in 0..2 {
            let mut game = Game::default();
            let (mut a, mut b) = (Bot::new(), Bot::new());
            while game.outcome().is_none() && game.ply() < 300 {
                let strong_turn = (game.turn() == Color::White) == (round == 0);
                let m = if strong_turn { a.choose(&game, &strong) } else { b.choose(&game, &weak) };
                game.play(m.unwrap()).unwrap();
            }
            let color = if round == 0 { Color::White } else { Color::Black };
            points += game.outcome().map_or(0.5, |o| o.winner.points_for(color));
        }
        assert!(points >= 1.5, "the strong bot scored {points} of 2");
    }
}
