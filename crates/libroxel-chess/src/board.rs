//! The board, its pieces and the rules for moving them.
//!
//! Squares are numbered 0 to 63 from a1, b1, … h1, a2 … up to h8. A
//! [`Position`] is small and copied freely: making a move returns a new one.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn other(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Color::White => "White",
            Color::Black => "Black",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Pawn, Kind::Knight, Kind::Bishop, Kind::Rook, Kind::Queen, Kind::King];

    /// The letter used in notation (upper case).
    pub fn letter(self) -> char {
        match self {
            Kind::Pawn => 'P',
            Kind::Knight => 'N',
            Kind::Bishop => 'B',
            Kind::Rook => 'R',
            Kind::Queen => 'Q',
            Kind::King => 'K',
        }
    }

    pub fn from_letter(c: char) -> Option<Kind> {
        Some(match c.to_ascii_uppercase() {
            'P' => Kind::Pawn,
            'N' => Kind::Knight,
            'B' => Kind::Bishop,
            'R' => Kind::Rook,
            'Q' => Kind::Queen,
            'K' => Kind::King,
            _ => return None,
        })
    }

    /// The usual worth in pawns, for the material count beside the board.
    pub fn points(self) -> i32 {
        match self {
            Kind::Pawn => 1,
            Kind::Knight | Kind::Bishop => 3,
            Kind::Rook => 5,
            Kind::Queen => 9,
            Kind::King => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Pawn => "pawn",
            Kind::Knight => "knight",
            Kind::Bishop => "bishop",
            Kind::Rook => "rook",
            Kind::Queen => "queen",
            Kind::King => "king",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: Kind,
}

impl Piece {
    pub fn new(color: Color, kind: Kind) -> Self {
        Piece { color, kind }
    }

    fn fen_char(self) -> char {
        let c = self.kind.letter();
        if self.color == Color::White { c } else { c.to_ascii_lowercase() }
    }
}

pub type Square = u8;

pub fn file_of(sq: Square) -> i8 {
    (sq % 8) as i8
}

pub fn rank_of(sq: Square) -> i8 {
    (sq / 8) as i8
}

pub fn square(file: i8, rank: i8) -> Option<Square> {
    ((0..8).contains(&file) && (0..8).contains(&rank)).then(|| (rank * 8 + file) as Square)
}

/// "e4" for square 28.
pub fn square_name(sq: Square) -> String {
    format!("{}{}", (b'a' + sq % 8) as char, sq / 8 + 1)
}

pub fn parse_square(text: &str) -> Option<Square> {
    let b = text.as_bytes();
    if b.len() != 2 {
        return None;
    }
    square(b[0].wrapping_sub(b'a') as i8, b[1].wrapping_sub(b'1') as i8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Move {
    pub from: Square,
    pub to: Square,
    pub promotion: Option<Kind>,
}

impl Move {
    pub fn new(from: Square, to: Square) -> Self {
        Move { from, to, promotion: None }
    }

    /// The move as coordinates, such as `e2e4` or `e7e8q`.
    pub fn uci(self) -> String {
        let mut s = square_name(self.from) + &square_name(self.to);
        if let Some(p) = self.promotion {
            s.push(p.letter().to_ascii_lowercase());
        }
        s
    }

    pub fn from_uci(text: &str) -> Option<Move> {
        let text = text.trim();
        if !(4..=5).contains(&text.len()) || !text.is_ascii() {
            return None;
        }
        let from = parse_square(&text[0..2])?;
        let to = parse_square(&text[2..4])?;
        let promotion = match text.chars().nth(4) {
            Some(c) => Some(Kind::from_letter(c).filter(|k| !matches!(k, Kind::Pawn | Kind::King))?),
            None => None,
        };
        Some(Move { from, to, promotion })
    }
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.uci())
    }
}

pub const WHITE_SHORT: u8 = 1;
pub const WHITE_LONG: u8 = 2;
pub const BLACK_SHORT: u8 = 4;
pub const BLACK_LONG: u8 = 8;

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

const KNIGHT_STEPS: [(i8, i8); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING_STEPS: [(i8, i8); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const ROOK_DIRS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

// Random numbers for position keys, made at compile time.
const fn splitmix(state: u64) -> (u64, u64) {
    let state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (state, z ^ (z >> 31))
}

const KEYS_LEN: usize = 12 * 64 + 1 + 16 + 8;

const KEYS: [u64; KEYS_LEN] = {
    let mut keys = [0u64; KEYS_LEN];
    let mut state = 0x1234_5678_9ABC_DEF0u64;
    let mut i = 0;
    while i < KEYS_LEN {
        let (s, v) = splitmix(state);
        state = s;
        keys[i] = v;
        i += 1;
    }
    keys
};

fn piece_key(p: Piece, sq: Square) -> u64 {
    KEYS[(p.color.index() * 6 + p.kind as usize) * 64 + sq as usize]
}
const SIDE_KEY: usize = 12 * 64;
const CASTLE_KEYS: usize = SIDE_KEY + 1;
const EP_KEYS: usize = CASTLE_KEYS + 16;

/// Everything about a position: where the pieces are, whose turn it is,
/// who may still castle, the en passant square and the move counters.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub board: [Option<Piece>; 64],
    pub turn: Color,
    pub castling: u8,
    /// The square a pawn may capture onto en passant, after a double step.
    pub en_passant: Option<Square>,
    /// Half-moves since the last capture or pawn move (the 50-move rule).
    pub halfmove: u32,
    pub fullmove: u32,
}

impl Default for Position {
    fn default() -> Self {
        Position::start()
    }
}

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Position({})", self.fen())
    }
}

impl Position {
    pub fn start() -> Self {
        Position::from_fen(START_FEN).expect("the start position is valid")
    }

    pub fn from_fen(fen: &str) -> Result<Self, String> {
        let mut parts = fen.split_whitespace();
        let placement = parts.next().ok_or("empty FEN")?;
        let mut board = [None; 64];
        let ranks: Vec<&str> = placement.split('/').collect();
        if ranks.len() != 8 {
            return Err("a FEN needs 8 ranks".into());
        }
        for (i, row) in ranks.iter().enumerate() {
            let rank = 7 - i as i8;
            let mut file = 0i8;
            for c in row.chars() {
                if let Some(d) = c.to_digit(10) {
                    file += d as i8;
                } else {
                    let kind = Kind::from_letter(c).ok_or_else(|| format!("unknown piece {c}"))?;
                    let color = if c.is_ascii_uppercase() { Color::White } else { Color::Black };
                    let sq = square(file, rank).ok_or("a rank is too long")?;
                    board[sq as usize] = Some(Piece::new(color, kind));
                    file += 1;
                }
            }
            if file != 8 {
                return Err("each rank needs 8 squares".into());
            }
        }
        let turn = match parts.next().unwrap_or("w") {
            "w" => Color::White,
            "b" => Color::Black,
            other => return Err(format!("unknown side to move {other}")),
        };
        let mut castling = 0;
        for c in parts.next().unwrap_or("-").chars() {
            castling |= match c {
                'K' => WHITE_SHORT,
                'Q' => WHITE_LONG,
                'k' => BLACK_SHORT,
                'q' => BLACK_LONG,
                _ => 0,
            };
        }
        let en_passant = match parts.next().unwrap_or("-") {
            "-" => None,
            sq => Some(parse_square(sq).ok_or("bad en passant square")?),
        };
        let halfmove = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let fullmove = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
        let pos = Position { board, turn, castling, en_passant, halfmove, fullmove };
        for color in [Color::White, Color::Black] {
            let kings = board.iter().filter(|p| **p == Some(Piece::new(color, Kind::King))).count();
            if kings != 1 {
                return Err(format!("{} needs exactly one king", color.name()));
            }
        }
        if pos.attacked(pos.king(turn.other()), turn) {
            return Err("the side not to move is in check".into());
        }
        Ok(pos)
    }

    pub fn fen(&self) -> String {
        let mut out = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                match self.board[(rank * 8 + file) as usize] {
                    None => empty += 1,
                    Some(p) => {
                        if empty > 0 {
                            out.push_str(&empty.to_string());
                            empty = 0;
                        }
                        out.push(p.fen_char());
                    }
                }
            }
            if empty > 0 {
                out.push_str(&empty.to_string());
            }
            if rank > 0 {
                out.push('/');
            }
        }
        out.push_str(if self.turn == Color::White { " w " } else { " b " });
        let mut castle = String::new();
        for (bit, c) in [(WHITE_SHORT, 'K'), (WHITE_LONG, 'Q'), (BLACK_SHORT, 'k'), (BLACK_LONG, 'q')] {
            if self.castling & bit != 0 {
                castle.push(c);
            }
        }
        if castle.is_empty() {
            castle.push('-');
        }
        out.push_str(&castle);
        out.push(' ');
        out.push_str(&self.en_passant.map_or("-".into(), square_name));
        out.push_str(&format!(" {} {}", self.halfmove, self.fullmove));
        out
    }

    pub fn at(&self, sq: Square) -> Option<Piece> {
        self.board[sq as usize]
    }

    pub fn king(&self, color: Color) -> Square {
        self.board.iter().position(|p| *p == Some(Piece::new(color, Kind::King))).unwrap_or(0) as Square
    }

    /// A number that is (almost surely) different for every position, for
    /// spotting repetitions. Move counters don't count.
    pub fn key(&self) -> u64 {
        let mut k = 0;
        for (sq, p) in self.board.iter().enumerate() {
            if let Some(p) = p {
                k ^= piece_key(*p, sq as Square);
            }
        }
        if self.turn == Color::Black {
            k ^= KEYS[SIDE_KEY];
        }
        k ^= KEYS[CASTLE_KEYS + self.castling as usize];
        // En passant only counts when a capture is really possible.
        if let Some(ep) = self.en_passant
            && self
                .legal_moves()
                .iter()
                .any(|m| m.to == ep && self.at(m.from).is_some_and(|p| p.kind == Kind::Pawn))
        {
            k ^= KEYS[EP_KEYS + file_of(ep) as usize];
        }
        k
    }

    /// A quick key for the computer players' search: like [`Position::key`]
    /// but counting any en passant square.
    pub fn search_key(&self) -> u64 {
        let mut k = 0;
        for (sq, p) in self.board.iter().enumerate() {
            if let Some(p) = p {
                k ^= piece_key(*p, sq as Square);
            }
        }
        if self.turn == Color::Black {
            k ^= KEYS[SIDE_KEY];
        }
        k ^= KEYS[CASTLE_KEYS + self.castling as usize];
        if let Some(ep) = self.en_passant {
            k ^= KEYS[EP_KEYS + file_of(ep) as usize];
        }
        k
    }

    /// Whether `sq` is attacked by any piece of `by`.
    pub fn attacked(&self, sq: Square, by: Color) -> bool {
        let (f, r) = (file_of(sq), rank_of(sq));
        let is = |s: Option<Square>, kinds: &[Kind]| {
            s.and_then(|s| self.at(s)).is_some_and(|p| p.color == by && kinds.contains(&p.kind))
        };
        // Pawns attack diagonally forward, so look backward from `sq`.
        let back = if by == Color::White { -1 } else { 1 };
        if is(square(f - 1, r + back), &[Kind::Pawn]) || is(square(f + 1, r + back), &[Kind::Pawn]) {
            return true;
        }
        if KNIGHT_STEPS.iter().any(|(df, dr)| is(square(f + df, r + dr), &[Kind::Knight])) {
            return true;
        }
        if KING_STEPS.iter().any(|(df, dr)| is(square(f + df, r + dr), &[Kind::King])) {
            return true;
        }
        for (dirs, kinds) in
            [(ROOK_DIRS, [Kind::Rook, Kind::Queen]), (BISHOP_DIRS, [Kind::Bishop, Kind::Queen])]
        {
            for (df, dr) in dirs {
                let (mut x, mut y) = (f + df, r + dr);
                while let Some(s) = square(x, y) {
                    if let Some(p) = self.at(s) {
                        if p.color == by && kinds.contains(&p.kind) {
                            return true;
                        }
                        break;
                    }
                    x += df;
                    y += dr;
                }
            }
        }
        false
    }

    pub fn in_check(&self) -> bool {
        self.attacked(self.king(self.turn), self.turn.other())
    }

    /// Moves that follow how pieces move, ignoring whether the king is left
    /// in check.
    pub fn pseudo_moves(&self, out: &mut Vec<Move>, captures_only: bool) {
        let us = self.turn;
        for from in 0..64u8 {
            let Some(p) = self.at(from) else { continue };
            if p.color != us {
                continue;
            }
            let (f, r) = (file_of(from), rank_of(from));
            match p.kind {
                Kind::Pawn => {
                    let dir = if us == Color::White { 1 } else { -1 };
                    let start = if us == Color::White { 1 } else { 6 };
                    let last = if us == Color::White { 7 } else { 0 };
                    let push = |to: Square, out: &mut Vec<Move>| {
                        if rank_of(to) == last {
                            for k in [Kind::Queen, Kind::Knight, Kind::Rook, Kind::Bishop] {
                                out.push(Move { from, to, promotion: Some(k) });
                            }
                        } else {
                            out.push(Move::new(from, to));
                        }
                    };
                    if let Some(one) = square(f, r + dir)
                        && self.at(one).is_none()
                    {
                        if !captures_only || rank_of(one) == last {
                            push(one, out);
                        }
                        if !captures_only
                            && r == start
                            && let Some(two) = square(f, r + 2 * dir)
                            && self.at(two).is_none()
                        {
                            out.push(Move::new(from, two));
                        }
                    }
                    for df in [-1, 1] {
                        if let Some(to) = square(f + df, r + dir) {
                            let enemy = self.at(to).is_some_and(|q| q.color != us);
                            if enemy || self.en_passant == Some(to) {
                                push(to, out);
                            }
                        }
                    }
                }
                Kind::Knight | Kind::King => {
                    let steps = if p.kind == Kind::Knight { &KNIGHT_STEPS } else { &KING_STEPS };
                    for (df, dr) in steps {
                        if let Some(to) = square(f + df, r + dr) {
                            match self.at(to) {
                                None if !captures_only => out.push(Move::new(from, to)),
                                Some(q) if q.color != us => out.push(Move::new(from, to)),
                                _ => {}
                            }
                        }
                    }
                    if p.kind == Kind::King && !captures_only {
                        self.castle_moves(from, out);
                    }
                }
                Kind::Bishop | Kind::Rook | Kind::Queen => {
                    let dirs: &[(i8, i8)] = match p.kind {
                        Kind::Bishop => &BISHOP_DIRS,
                        Kind::Rook => &ROOK_DIRS,
                        _ => &[(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)],
                    };
                    for (df, dr) in dirs {
                        let (mut x, mut y) = (f + df, r + dr);
                        while let Some(to) = square(x, y) {
                            match self.at(to) {
                                None => {
                                    if !captures_only {
                                        out.push(Move::new(from, to));
                                    }
                                }
                                Some(q) => {
                                    if q.color != us {
                                        out.push(Move::new(from, to));
                                    }
                                    break;
                                }
                            }
                            x += df;
                            y += dr;
                        }
                    }
                }
            }
        }
    }

    fn castle_moves(&self, from: Square, out: &mut Vec<Move>) {
        let us = self.turn;
        let (home, short, long) = match us {
            Color::White => (4u8, WHITE_SHORT, WHITE_LONG),
            Color::Black => (60u8, BLACK_SHORT, BLACK_LONG),
        };
        if from != home || self.in_check() {
            return;
        }
        let them = us.other();
        let rook = Some(Piece::new(us, Kind::Rook));
        if self.castling & short != 0
            && self.at(home + 3) == rook
            && self.at(home + 1).is_none()
            && self.at(home + 2).is_none()
            && !self.attacked(home + 1, them)
            && !self.attacked(home + 2, them)
        {
            out.push(Move::new(home, home + 2));
        }
        if self.castling & long != 0
            && self.at(home - 4) == rook
            && self.at(home - 1).is_none()
            && self.at(home - 2).is_none()
            && self.at(home - 3).is_none()
            && !self.attacked(home - 1, them)
            && !self.attacked(home - 2, them)
        {
            out.push(Move::new(home, home - 2));
        }
    }

    /// Plays a move (which should come from [`Position::pseudo_moves`] or
    /// [`Position::legal_moves`]) and returns the new position.
    pub fn play(&self, m: Move) -> Position {
        let mut next = *self;
        let Some(piece) = self.at(m.from) else { return next };
        let captured = self.at(m.to);
        next.board[m.from as usize] = None;
        next.board[m.to as usize] = Some(match m.promotion {
            Some(k) if piece.kind == Kind::Pawn => Piece::new(piece.color, k),
            _ => piece,
        });
        next.en_passant = None;
        if piece.kind == Kind::Pawn {
            if Some(m.to) == self.en_passant && captured.is_none() && file_of(m.from) != file_of(m.to) {
                let taken = if piece.color == Color::White { m.to - 8 } else { m.to + 8 };
                next.board[taken as usize] = None;
            }
            if (m.to as i8 - m.from as i8).abs() == 16 {
                next.en_passant = Some((m.from + m.to) / 2);
            }
        }
        if piece.kind == Kind::King && (m.to as i8 - m.from as i8).abs() == 2 {
            let (rook_from, rook_to) =
                if m.to > m.from { (m.from + 3, m.from + 1) } else { (m.from - 4, m.from - 1) };
            next.board[rook_to as usize] = next.board[rook_from as usize].take();
        }
        for sq in [m.from, m.to] {
            next.castling &= match sq {
                0 => !WHITE_LONG,
                7 => !WHITE_SHORT,
                4 => !(WHITE_SHORT | WHITE_LONG),
                56 => !BLACK_LONG,
                63 => !BLACK_SHORT,
                60 => !(BLACK_SHORT | BLACK_LONG),
                _ => 0xFF,
            };
        }
        next.halfmove = if piece.kind == Kind::Pawn || captured.is_some() { 0 } else { self.halfmove + 1 };
        if self.turn == Color::Black {
            next.fullmove += 1;
        }
        next.turn = self.turn.other();
        next
    }

    /// Whether a pseudo-legal move leaves the mover's own king safe.
    pub fn is_legal_pseudo(&self, m: Move) -> bool {
        let next = self.play(m);
        !next.attacked(next.king(self.turn), self.turn.other())
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        let mut moves = Vec::with_capacity(48);
        self.pseudo_moves(&mut moves, false);
        moves.retain(|m| self.is_legal_pseudo(*m));
        moves
    }

    pub fn is_legal(&self, m: Move) -> bool {
        self.legal_moves().contains(&m)
    }

    pub fn is_capture(&self, m: Move) -> bool {
        self.at(m.to).is_some()
            || (Some(m.to) == self.en_passant
                && self.at(m.from).is_some_and(|p| p.kind == Kind::Pawn)
                && file_of(m.from) != file_of(m.to))
    }

    /// Neither side has enough left to ever checkmate.
    pub fn insufficient_material(&self) -> bool {
        let mut minors = Vec::new();
        for (sq, p) in self.board.iter().enumerate() {
            match p.map(|p| p.kind) {
                None | Some(Kind::King) => {}
                Some(Kind::Knight) | Some(Kind::Bishop) => minors.push((sq as Square, p.unwrap())),
                _ => return false,
            }
        }
        match minors.as_slice() {
            [] | [_] => true,
            // Only bishops, all on squares of one colour.
            many if many.iter().all(|(_, p)| p.kind == Kind::Bishop) => {
                let shade = |sq: Square| (file_of(sq) + rank_of(sq)) % 2;
                let first = shade(many[0].0);
                many.iter().all(|(sq, _)| shade(*sq) == first)
            }
            _ => false,
        }
    }

    /// Whether `color` alone could ever mate (for a flag fall: running out
    /// of time is a draw when the other side can't possibly win).
    pub fn can_mate(&self, color: Color) -> bool {
        let mut knights = 0;
        let mut bishops = 0;
        for p in self.board.iter().flatten().filter(|p| p.color == color) {
            match p.kind {
                Kind::Pawn | Kind::Rook | Kind::Queen => return true,
                Kind::Knight => knights += 1,
                Kind::Bishop => bishops += 1,
                Kind::King => {}
            }
        }
        knights + bishops >= 2
    }

    /// The move in standard notation, such as `Nf3`, `exd5`, `O-O` or `e8=Q+`.
    pub fn san(&self, m: Move) -> String {
        let Some(piece) = self.at(m.from) else { return m.uci() };
        let mut s = String::new();
        if piece.kind == Kind::King && (m.to as i8 - m.from as i8).abs() == 2 {
            s.push_str(if m.to > m.from { "O-O" } else { "O-O-O" });
        } else {
            let capture = self.is_capture(m);
            if piece.kind == Kind::Pawn {
                if capture {
                    s.push((b'a' + m.from % 8) as char);
                }
            } else {
                s.push(piece.kind.letter());
                // Tell apart other pieces of the same kind that can also go there.
                let others: Vec<Move> = self
                    .legal_moves()
                    .into_iter()
                    .filter(|o| o.to == m.to && o.from != m.from && self.at(o.from) == Some(piece))
                    .collect();
                if !others.is_empty() {
                    let same_file = others.iter().any(|o| file_of(o.from) == file_of(m.from));
                    let same_rank = others.iter().any(|o| rank_of(o.from) == rank_of(m.from));
                    if !same_file {
                        s.push((b'a' + m.from % 8) as char);
                    } else if !same_rank {
                        s.push((b'1' + m.from / 8) as char);
                    } else {
                        s.push_str(&square_name(m.from));
                    }
                }
            }
            if capture {
                s.push('x');
            }
            s.push_str(&square_name(m.to));
            if let Some(k) = m.promotion {
                s.push('=');
                s.push(k.letter());
            }
        }
        let next = self.play(m);
        if next.in_check() {
            s.push(if next.legal_moves().is_empty() { '#' } else { '+' });
        }
        s
    }

    /// Reads a move in standard notation (or coordinates).
    pub fn parse_san(&self, text: &str) -> Option<Move> {
        let clean = |s: &str| s.trim().trim_end_matches(['+', '#', '!', '?']).replace('0', "O");
        let want = clean(text);
        let legal = self.legal_moves();
        if let Some(m) = legal.iter().find(|m| clean(&self.san(**m)) == want) {
            return Some(*m);
        }
        Move::from_uci(text).filter(|m| legal.contains(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perft(pos: &Position, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        let moves = pos.legal_moves();
        if depth == 1 {
            return moves.len() as u64;
        }
        moves.iter().map(|m| perft(&pos.play(*m), depth - 1)).sum()
    }

    #[test]
    fn perft_start() {
        let p = Position::start();
        assert_eq!(perft(&p, 1), 20);
        assert_eq!(perft(&p, 2), 400);
        assert_eq!(perft(&p, 3), 8902);
        assert_eq!(perft(&p, 4), 197_281);
    }

    #[test]
    fn perft_tricky() {
        // "Kiwipete": castling, en passant, promotions and pins all at once.
        let p = Position::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
            .unwrap();
        assert_eq!(perft(&p, 1), 48);
        assert_eq!(perft(&p, 2), 2039);
        assert_eq!(perft(&p, 3), 97_862);
        let p = Position::from_fen("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1").unwrap();
        assert_eq!(perft(&p, 4), 43_238);
        let p =
            Position::from_fen("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1").unwrap();
        assert_eq!(perft(&p, 3), 9467);
        let p = Position::from_fen("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8").unwrap();
        assert_eq!(perft(&p, 3), 62_379);
    }

    #[test]
    fn fen_round_trip() {
        let fen = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
        assert_eq!(Position::from_fen(fen).unwrap().fen(), fen);
        assert_eq!(Position::start().fen(), START_FEN);
    }

    #[test]
    fn notation() {
        let p = Position::start();
        let m = p.parse_san("Nf3").unwrap();
        assert_eq!(m.uci(), "g1f3");
        assert_eq!(p.san(m), "Nf3");
        let p = Position::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
            .unwrap();
        assert_eq!(p.san(Move::new(4, 6)), "O-O");
        assert_eq!(p.san(Move::new(4, 2)), "O-O-O");
        // Both rooks can reach d1.
        let p = Position::from_fen("4k3/8/8/8/8/8/8/R4RK1 w - - 0 1").unwrap();
        assert_eq!(p.san(Move::new(0, 3)), "Rad1");
        let p = Position::from_fen("7k/P7/8/8/8/8/8/K7 w - - 0 1").unwrap();
        assert_eq!(p.san(Move { from: 48, to: 56, promotion: Some(Kind::Queen) }), "a8=Q+");
        let mate = Position::from_fen("6k1/5ppp/8/8/8/8/8/K3R3 w - - 0 1").unwrap();
        assert_eq!(mate.san(Move::new(4, 60)), "Re8#");
    }

    #[test]
    fn material() {
        assert!(Position::from_fen("8/8/8/4k3/8/8/8/4K3 w - - 0 1").unwrap().insufficient_material());
        assert!(Position::from_fen("8/8/8/4k3/8/8/8/3NK3 w - - 0 1").unwrap().insufficient_material());
        assert!(!Position::from_fen("8/8/8/4k3/8/8/8/2NNK3 w - - 0 1").unwrap().insufficient_material());
        assert!(!Position::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1").unwrap().insufficient_material());
    }
}
