//! The Chess tab: play the computer at eight strengths, a friend on the
//! same computer, or anyone online through the Hub (quick matches by
//! clock, challenges to friends), watch live games, and climb the
//! leaderboard.
//!
//! The rules come from `libroxel-chess`; online games live on the Hub,
//! which checks every move and keeps the clocks. This screen polls the Hub
//! about once a second while a game is on, so closing the app and opening
//! it again picks the game back up where it was.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy_egui::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use libroxel_chess::{
    BOTS, Bot, Category, Color, Game, Kind, Move, Outcome, Piece, Position, Reason, Square, TimeControl,
    Winner, clock_text, file_of, rank_of, square,
};
use libroxel_hub::{
    CHESS_PHRASES, ChessAction, ChessGame, ChessLeader, ChessLobby, ChessProfile, ChessRequest, HubClient,
};

use crate::browse::Platform;
use crate::chess_pieces::{paint_piece, paint_small};
use crate::theme::{self, icon};

const SOFT: Color32 = theme::MUTED;

/// How often the Hub is asked about a game being played or watched.
const GAME_POLL: Duration = Duration::from_millis(900);
/// How often the lobby is refreshed while the Chess tab is open.
const LOBBY_POLL: Duration = Duration::from_secs(4);
/// How often challenges are checked while another tab is open.
const BACKGROUND_POLL: Duration = Duration::from_secs(10);

/// Board colours to pick from: light squares, dark squares.
const BOARDS: [(&str, Color32, Color32); 5] = [
    ("Sky", Color32::from_rgb(232, 244, 253), Color32::from_rgb(112, 166, 214)),
    ("Lagoon", Color32::from_rgb(226, 246, 240), Color32::from_rgb(84, 168, 160)),
    ("Meadow", Color32::from_rgb(238, 246, 220), Color32::from_rgb(120, 170, 92)),
    ("Maple", Color32::from_rgb(244, 228, 200), Color32::from_rgb(178, 128, 86)),
    ("Slate", Color32::from_rgb(214, 220, 230), Color32::from_rgb(108, 120, 140)),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Online,
    Computer,
    Watch,
    Leaderboard,
    Me,
}

/// Who the other side is. There is only ever one, so its size doesn't matter.
#[allow(clippy::large_enum_variant)]
enum Opponent {
    /// The computer, playing `BOTS[level]`; the player has `human`.
    Bot {
        level: usize,
        human: Color,
        brain: Arc<Mutex<Bot>>,
        thinking: Option<(MoveSlot, Instant)>,
        hint: Option<Move>,
        hint_slot: Option<MoveSlot>,
        takebacks: u32,
    },
    /// Two people taking turns on this computer.
    Local,
    /// A game on the Hub. `mine` is None while watching.
    Online { game: ChessGame, received: Instant, mine: Option<Color>, sent_ply: Option<usize> },
}

/// The game on the board.
struct Match {
    game: Game,
    opponent: Opponent,
    /// Looking back at an earlier move (the ply shown).
    review: Option<usize>,
    /// A move queued while it's the opponent's turn.
    premove: Option<Move>,
    /// Whether the result card was closed.
    result_closed: bool,
}

impl Match {
    fn new(game: Game, opponent: Opponent) -> Self {
        Match { game, opponent, review: None, premove: None, result_closed: false }
    }

    fn online_id(&self) -> Option<&str> {
        match &self.opponent {
            Opponent::Online { game, .. } => Some(&game.id),
            _ => None,
        }
    }

    /// Which colours the person at this computer may move now.
    fn movable(&self) -> Option<Color> {
        if self.game.outcome().is_some() || self.review.is_some_and(|p| p < self.game.ply()) {
            return None;
        }
        match &self.opponent {
            Opponent::Bot { human, .. } => Some(*human),
            Opponent::Local => Some(self.game.turn()),
            Opponent::Online { mine, game, .. } => (game.status == "playing").then_some(*mine).flatten(),
        }
    }

    /// The side at the bottom of the board.
    fn bottom(&self) -> Color {
        match &self.opponent {
            Opponent::Bot { human, .. } => *human,
            Opponent::Local => Color::White,
            Opponent::Online { mine, .. } => mine.unwrap_or(Color::White),
        }
    }

    fn playing(&self) -> bool {
        self.game.outcome().is_none()
            && match &self.opponent {
                Opponent::Online { game, .. } => game.status == "playing",
                _ => true,
            }
    }
}

/// Where a thread thinking about a move leaves it (None inside: no move).
type MoveSlot = Arc<Mutex<Option<Option<Move>>>>;

/// Work for the thread that talks to the Hub.
enum Job {
    Act { hub: String, action: ChessAction, sync: bool },
    Watch { hub: String, id: String },
    Live { hub: String },
    Leaders { hub: String, category: Category },
    Profile { hub: String, id: String },
}

#[allow(clippy::large_enum_variant)]
enum Reply {
    Lobby { result: Result<ChessLobby, String>, sync: bool },
    Watch(Result<ChessGame, String>),
    Live(Result<Vec<ChessGame>, String>),
    Leaders(Category, Result<Vec<ChessLeader>, String>),
    Profile(Result<ChessProfile, String>),
}

/// One thread sends every request in order, so each signed time is newer
/// than the one before.
struct Net {
    jobs: Sender<Job>,
    replies: Arc<Mutex<Vec<Reply>>>,
    /// Requests sent and not answered yet (syncs, watches).
    pending_sync: Arc<Mutex<u32>>,
}

impl Net {
    fn start() -> Self {
        let (jobs, rx): (Sender<Job>, Receiver<Job>) = channel();
        let replies: Arc<Mutex<Vec<Reply>>> = Arc::default();
        let pending_sync: Arc<Mutex<u32>> = Arc::default();
        let (out, pending) = (replies.clone(), pending_sync.clone());
        std::thread::spawn(move || {
            let mut clock = 0i64;
            while let Ok(job) = rx.recv() {
                let reply = match job {
                    Job::Act { hub, action, sync } => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis() as i64);
                        clock = now.max(clock + 1);
                        let time = clock;
                        let result = crate::identity().and_then(|me| {
                            let player_id = me.player_id();
                            let action = libroxel_instance::serde_json::to_string(&action)
                                .map_err(|e| e.to_string())?;
                            let signature = me.sign(&libroxel_hub::chess_message(&player_id, time, &action));
                            HubClient::new(&hub).chess(&ChessRequest { player_id, time, action, signature })
                        });
                        if sync {
                            let mut p = pending.lock().unwrap();
                            *p = p.saturating_sub(1);
                        }
                        Reply::Lobby { result, sync }
                    }
                    Job::Watch { hub, id } => {
                        let r = HubClient::new(&hub).chess_game(&id);
                        let mut p = pending.lock().unwrap();
                        *p = p.saturating_sub(1);
                        Reply::Watch(r)
                    }
                    Job::Live { hub } => Reply::Live(HubClient::new(&hub).chess_live()),
                    Job::Leaders { hub, category } => {
                        Reply::Leaders(category, HubClient::new(&hub).chess_leaderboard(category.code()))
                    }
                    Job::Profile { hub, id } => Reply::Profile(HubClient::new(&hub).chess_profile(&id)),
                };
                out.lock().unwrap().push(reply);
            }
        });
        Net { jobs, replies, pending_sync }
    }
}

/// The board's pointer state.
#[derive(Default)]
struct Pointer {
    selected: Option<Square>,
    /// The square a press started on, and whether it was already selected.
    pressed: Option<(Square, bool)>,
    /// A pawn reaching the end, waiting for the player to pick what it becomes.
    promotion: Option<Move>,
    /// Arrows and circles drawn with the right mouse button.
    arrows: Vec<(Square, Square)>,
    marks: Vec<Square>,
    right_from: Option<Square>,
    /// The last move, sliding into place.
    slide: Option<(Move, Instant)>,
}

/// Everything the Chess tab keeps between frames.
pub struct ChessScreen {
    page: Page,
    board_theme: usize,
    show_coords: bool,
    show_moves: bool,
    auto_queen: bool,
    flipped: bool,
    rated: bool,
    challenge_control: usize,
    challenge_color: usize,
    challenge_rated: bool,
    bot_level: usize,
    bot_color: usize,
    phrase: usize,
    pointer: Pointer,
    current: Option<Match>,
    /// The lobby is shown beside a finished game.
    show_lobby: bool,
    net: Option<Net>,
    lobby: Option<ChessLobby>,
    lobby_at: Option<Instant>,
    game_poll_at: Option<Instant>,
    lobby_error: Option<String>,
    status: Option<Result<String, String>>,
    live: Vec<ChessGame>,
    live_at: Option<Instant>,
    leader_category: Category,
    leaders: Vec<ChessLeader>,
    leaders_at: Option<Instant>,
    profile: Option<ChessProfile>,
    profile_at: Option<Instant>,
    /// Challenges already shown as a pop-up on other tabs.
    toasted: Vec<String>,
    /// Games the player left on purpose, so the lobby doesn't reopen them.
    closed_games: Vec<String>,
    copied: Option<(String, Instant)>,
    /// A queued move whose turn has come, played on the next frame.
    pending_premove: Option<Move>,
    /// Set when "Stop watching" is pressed, to clear the board after the panel.
    stop_watching: bool,
    /// A game to look at next (a rematch that just started).
    follow: Option<String>,
}

impl Default for ChessScreen {
    fn default() -> Self {
        ChessScreen {
            page: Page::Online,
            board_theme: 0,
            show_coords: true,
            show_moves: true,
            auto_queen: false,
            flipped: false,
            rated: true,
            challenge_control: 6,
            challenge_color: 0,
            challenge_rated: false,
            bot_level: 2,
            bot_color: 0,
            phrase: 0,
            pointer: Pointer::default(),
            current: None,
            show_lobby: true,
            net: None,
            lobby: None,
            lobby_at: None,
            game_poll_at: None,
            lobby_error: None,
            status: None,
            live: Vec::new(),
            live_at: None,
            leader_category: Category::Blitz,
            leaders: Vec::new(),
            leaders_at: None,
            profile: None,
            profile_at: None,
            toasted: Vec::new(),
            closed_games: Vec::new(),
            copied: None,
            pending_premove: None,
            stop_watching: false,
            follow: None,
        }
    }
}

fn color_of(word: &str) -> Option<Color> {
    match word {
        "white" => Some(Color::White),
        "black" => Some(Color::Black),
        _ => None,
    }
}

fn outcome_of(game: &ChessGame) -> Option<Outcome> {
    let winner = match game.winner.as_deref()? {
        "white" => Winner::White,
        "black" => Winner::Black,
        _ => Winner::Draw,
    };
    let reason = Reason::from_code(game.reason.as_deref()?)?;
    Some(Outcome { winner, reason })
}

impl ChessScreen {
    fn send(&mut self, platform: &Platform, action: ChessAction) {
        let sync = matches!(action, ChessAction::Sync { .. });
        let net = self.net.get_or_insert_with(Net::start);
        if sync {
            let mut pending = net.pending_sync.lock().unwrap();
            if *pending > 0 {
                return;
            }
            *pending += 1;
        }
        let _ = net.jobs.send(Job::Act { hub: platform.hub_address.clone(), action, sync });
    }

    fn fetch(&mut self, job: Job) {
        let net = self.net.get_or_insert_with(Net::start);
        if let Job::Watch { .. } = job {
            let mut pending = net.pending_sync.lock().unwrap();
            if *pending > 0 {
                return;
            }
            *pending += 1;
        }
        let _ = net.jobs.send(job);
    }

    /// Keeps up with the Hub; call every frame the home screen shows.
    /// Returns a challenge to pop up about when Chess isn't open.
    pub fn tick(&mut self, platform: &Platform, open: bool) {
        let replies: Vec<Reply> =
            self.net.as_ref().map(|n| std::mem::take(&mut *n.replies.lock().unwrap())).unwrap_or_default();
        for reply in replies {
            self.take(reply);
        }
        self.tick_bot();
        if platform.account.is_none() {
            return;
        }
        let watching =
            matches!(&self.current, Some(Match { opponent: Opponent::Online { mine: None, .. }, .. }));
        let in_game = self.current.as_ref().is_some_and(|m| m.online_id().is_some() && m.playing());
        let seeking = self.lobby.as_ref().is_some_and(|l| l.seeking.is_some());
        if open && watching && self.game_poll_at.is_none_or(|t| t.elapsed() > GAME_POLL) {
            self.game_poll_at = Some(Instant::now());
            let id = self.current.as_ref().and_then(|m| m.online_id()).unwrap_or_default().to_owned();
            self.fetch(Job::Watch { hub: platform.hub_address.clone(), id });
        }
        let every = if in_game || seeking {
            GAME_POLL
        } else if open {
            LOBBY_POLL
        } else {
            BACKGROUND_POLL
        };
        if self.lobby_at.is_none_or(|t| t.elapsed() > every) {
            self.lobby_at = Some(Instant::now());
            let game_id = self.follow.clone().or_else(|| {
                self.current.as_ref().filter(|_| !watching).and_then(|m| m.online_id()).map(str::to_owned)
            });
            self.send(platform, ChessAction::Sync { game_id });
        }
    }

    fn take(&mut self, reply: Reply) {
        match reply {
            Reply::Lobby { result, sync } => match result {
                Ok(lobby) => {
                    self.lobby_error = None;
                    if !sync {
                        self.status = None;
                    }
                    if let Some(game) = lobby.game.clone() {
                        self.update_game(game, &lobby);
                    }
                    // A game that just started (a quick match, or a challenge
                    // someone accepted) while another was on the board.
                    for game in lobby.active.clone() {
                        if self.current.as_ref().and_then(|m| m.online_id()) != Some(game.id.as_str()) {
                            self.update_game(game, &lobby);
                        }
                    }
                    self.lobby = Some(lobby);
                }
                Err(e) => {
                    if sync {
                        self.lobby_error = Some(e);
                    } else {
                        self.status = Some(Err(e));
                        // Look again at once, so a refused move goes back.
                        self.lobby_at = None;
                        if let Some(Match { opponent: Opponent::Online { sent_ply, .. }, .. }) =
                            &mut self.current
                        {
                            *sent_ply = None;
                        }
                    }
                }
            },
            Reply::Watch(Ok(game)) => {
                let lobby = self.lobby.clone().unwrap_or_default();
                self.update_game(game, &lobby);
            }
            Reply::Watch(Err(e)) => self.lobby_error = Some(e),
            Reply::Live(Ok(games)) => self.live = games,
            Reply::Leaders(category, Ok(list)) if category == self.leader_category => self.leaders = list,
            Reply::Profile(Ok(profile)) => self.profile = Some(profile),
            Reply::Live(Err(e)) | Reply::Leaders(_, Err(e)) | Reply::Profile(Err(e)) => {
                self.lobby_error = Some(e)
            }
            Reply::Leaders(..) => {}
        }
    }

    /// Takes the Hub's view of a game: the one on the board, or a new one
    /// this player just started.
    fn update_game(&mut self, game: ChessGame, lobby: &ChessLobby) {
        let me = &lobby.me.player_id;
        let mine = if game.white.player_id == *me {
            Some(Color::White)
        } else if game.black.player_id == *me {
            Some(Color::Black)
        } else {
            None
        };
        let showing = self.current.as_ref().and_then(|m| m.online_id()) == Some(game.id.as_str());
        if !showing {
            // A game of ours that is on: open it (a new match, or picking up
            // one from before the app was closed).
            let start = game.status == "playing" && mine.is_some() && !self.closed_games.contains(&game.id);
            if !start {
                return;
            }
            self.open_online(game, mine);
            return;
        }
        let Some(current) = &mut self.current else { return };
        let Opponent::Online { game: old, received, sent_ply, mine: was_mine } = &mut current.opponent else {
            return;
        };
        let server_ply = game.moves.split_whitespace().count();
        // Keep a move made here on the board until the Hub has it.
        if let Some(sent) = *sent_ply {
            if server_ply < sent && game.status == "playing" {
                *old = ChessGame { moves: old.moves.clone(), ..game };
                *received = Instant::now();
                return;
            }
            *sent_ply = None;
        }
        let before = current.game.ply();
        if let Ok(mut rebuilt) = Game::from_uci(Position::start(), &game.moves) {
            if let Some(outcome) = outcome_of(&game) {
                rebuilt.end(outcome);
            }
            if rebuilt.ply() > before
                && let Some(m) = rebuilt.last_move()
            {
                self.pointer.slide = Some((m, Instant::now()));
            }
            if rebuilt.ply() != before {
                self.pointer.selected = None;
            }
            current.game = rebuilt;
        }
        let ended_now = old.status == "playing" && game.status == "over";
        *old = game;
        *received = Instant::now();
        *was_mine = mine.or(*was_mine);
        if ended_now {
            current.result_closed = false;
        }
        // A rematch both agreed to: go to it.
        if let Some(next) = old.rematch_game.clone()
            && mine.is_some()
            && !self.closed_games.contains(&next)
            && self.follow.is_none()
        {
            self.follow = Some(next);
            self.lobby_at = None;
        }
        self.play_premove();
    }

    fn open_online(&mut self, game: ChessGame, mine: Option<Color>) {
        let mut rebuilt = Game::from_uci(Position::start(), &game.moves).unwrap_or_default();
        if let Some(outcome) = outcome_of(&game) {
            rebuilt.end(outcome);
        }
        if self.follow.as_deref() == Some(game.id.as_str()) {
            self.follow = None;
        }
        self.pointer = Pointer::default();
        self.flipped = false;
        self.show_lobby = false;
        self.current = Some(Match::new(
            rebuilt,
            Opponent::Online { game, received: Instant::now(), mine, sent_ply: None },
        ));
    }

    fn start_bot(&mut self) {
        let level = self.bot_level.min(BOTS.len() - 1);
        let human = match self.bot_color {
            1 => Color::White,
            2 => Color::Black,
            _ => {
                if rand_bit() {
                    Color::Black
                } else {
                    Color::White
                }
            }
        };
        self.pointer = Pointer::default();
        self.flipped = false;
        self.show_lobby = false;
        self.current = Some(Match::new(
            Game::default(),
            Opponent::Bot {
                level,
                human,
                brain: Arc::new(Mutex::new(Bot::new())),
                thinking: None,
                hint: None,
                hint_slot: None,
                takebacks: 0,
            },
        ));
    }

    fn start_local(&mut self) {
        self.pointer = Pointer::default();
        self.flipped = false;
        self.show_lobby = false;
        self.current = Some(Match::new(Game::default(), Opponent::Local));
    }

    /// Lets the computer think, and plays its move when it has one.
    fn tick_bot(&mut self) {
        let Some(current) = &mut self.current else { return };
        let Opponent::Bot { level, human, brain, thinking, hint, hint_slot, .. } = &mut current.opponent
        else {
            return;
        };
        let found = hint_slot.as_ref().and_then(|slot| slot.lock().unwrap().take());
        if let Some(found) = found {
            *hint = found;
            *hint_slot = None;
        }
        if current.game.outcome().is_some() {
            *hint = None;
        }
        if current.game.outcome().is_some() || current.game.turn() == *human {
            *thinking = None;
            return;
        }
        match thinking {
            None => {
                let slot: MoveSlot = Arc::default();
                let (game, brain, out, bot) =
                    (current.game.clone(), brain.clone(), slot.clone(), BOTS[*level]);
                std::thread::spawn(move || {
                    let m = brain.lock().unwrap().choose(&game, &bot);
                    *out.lock().unwrap() = Some(m);
                });
                *thinking = Some((slot, Instant::now()));
            }
            Some((slot, since)) => {
                // Never answer instantly: a short pause feels like thinking.
                if since.elapsed() < Duration::from_millis(450) {
                    return;
                }
                let found = slot.lock().unwrap().take();
                if let Some(m) = found {
                    *thinking = None;
                    *hint = None;
                    if let Some(m) = m
                        && current.game.play(m).is_ok()
                    {
                        self.pointer.slide = Some((m, Instant::now()));
                        current.review = None;
                    }
                }
            }
        }
    }

    /// Plays a move made on the board.
    fn make_move(&mut self, platform: &Platform, m: Move) {
        let Some(current) = &mut self.current else { return };
        let Some(movable) = current.movable() else { return };
        let pos = *current.game.position();
        if pos.turn != movable {
            // Not our turn: keep it for when it is.
            current.premove = Some(m);
            return;
        }
        let m = fill_promotion(&pos, m);
        if current.game.play(m).is_err() {
            return;
        }
        current.review = None;
        self.pointer.slide = None;
        self.pointer.arrows.clear();
        self.pointer.marks.clear();
        match &mut current.opponent {
            Opponent::Online { game, sent_ply, .. } => {
                let ply = current.game.ply() - 1;
                *sent_ply = Some(ply + 1);
                let action = ChessAction::Move { game_id: game.id.clone(), uci: m.uci(), ply: ply as u32 };
                self.send(platform, action);
            }
            Opponent::Bot { hint, .. } => *hint = None,
            Opponent::Local => {}
        }
    }

    fn play_premove(&mut self) {
        let Some(current) = &mut self.current else { return };
        let Some(m) = current.premove else { return };
        let Some(movable) = current.movable() else {
            current.premove = None;
            return;
        };
        if current.game.turn() != movable {
            return;
        }
        current.premove = None;
        let pos = *current.game.position();
        let m = fill_promotion(&pos, m);
        if pos.is_legal(m) {
            // Sent on the next frame through the usual path.
            self.pointer.promotion = None;
            self.pending_premove = Some(m);
        }
    }
}

fn rand_bit() -> bool {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos())
        .is_multiple_of(2)
}

/// A pawn reaching the last rank without a piece picked becomes a queen.
fn fill_promotion(pos: &Position, m: Move) -> Move {
    match pos.at(m.from) {
        Some(p)
            if p.kind == Kind::Pawn
                && m.promotion.is_none()
                && (rank_of(m.to) == 0 || rank_of(m.to) == 7) =>
        {
            Move { promotion: Some(Kind::Queen), ..m }
        }
        _ => m,
    }
}

fn needs_promotion_pick(pos: &Position, m: Move) -> bool {
    pos.at(m.from).is_some_and(|p| p.kind == Kind::Pawn) && (rank_of(m.to) == 0 || rank_of(m.to) == 7)
}

/// What the Chess tab asks the home screen to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Open the Avatar tab to claim a name.
    ClaimName,
}

/// A square's spot on screen.
fn square_rect(board: Rect, sq: Square, flipped: bool) -> Rect {
    let cell = board.width() / 8.0;
    let (f, r) = (file_of(sq) as f32, rank_of(sq) as f32);
    let (x, y) = if flipped { (7.0 - f, r) } else { (f, 7.0 - r) };
    Rect::from_min_size(board.min + Vec2::new(x * cell, y * cell), Vec2::splat(cell))
}

fn square_at(board: Rect, at: Pos2, flipped: bool) -> Option<Square> {
    if !board.contains(at) {
        return None;
    }
    let cell = board.width() / 8.0;
    let x = ((at.x - board.min.x) / cell).floor() as i8;
    let y = ((at.y - board.min.y) / cell).floor() as i8;
    let (f, r) = if flipped { (7 - x, y) } else { (x, 7 - y) };
    square(f, r)
}

fn arrow(painter: &egui::Painter, from: Pos2, to: Pos2, width: f32, colour: Color32) {
    let dir = (to - from).normalized();
    let side = Vec2::new(-dir.y, dir.x);
    let head = width * 2.4;
    let tip = to;
    let neck = tip - dir * head;
    painter.line_segment([from, neck], Stroke::new(width, colour));
    painter.add(egui::Shape::convex_polygon(
        vec![tip, neck + side * head * 0.6, neck - side * head * 0.6],
        colour,
        Stroke::NONE,
    ));
}

fn initials_orb(painter: &egui::Painter, centre: Pos2, r: f32, name: &str, colour: Color32) {
    theme::orb(painter, centre, r, colour);
    let letter: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    painter.text(centre, egui::Align2::CENTER_CENTER, letter, theme::display_bold(r * 1.05), Color32::WHITE);
}

/// A player's strip above or below the board.
struct Strip {
    name: String,
    detail: String,
    colour: Color,
    clock: Option<(i64, bool)>,
    lost: Vec<Kind>,
    lead: i32,
    thinking: bool,
    note: Option<(String, Color32)>,
}

fn player_strip(ui: &mut egui::Ui, rect: Rect, strip: &Strip) {
    let painter = ui.painter();
    theme::glass(painter, rect, 8.0, theme::see_through(236, 246, 255, 200));
    let orb_colour = if strip.colour == Color::White { theme::ACCENT } else { Color32::from_rgb(40, 52, 76) };
    initials_orb(painter, Pos2::new(rect.min.x + 24.0, rect.center().y), 15.0, &strip.name, orb_colour);
    let x = rect.min.x + 48.0;
    let name = painter.layout_no_wrap(strip.name.clone(), theme::display(16.0), theme::TEXT);
    let name_w = name.size().x;
    painter.galley(Pos2::new(x, rect.min.y + 6.0), name, theme::TEXT);
    painter.text(
        Pos2::new(x + name_w + 8.0, rect.min.y + 9.0),
        egui::Align2::LEFT_TOP,
        &strip.detail,
        egui::FontId::proportional(13.0),
        SOFT,
    );
    // Pieces this player has taken (the other side's losses), and the lead.
    let mut cx = x + 6.0;
    let taken_colour = strip.colour.other();
    for &kind in &strip.lost {
        paint_small(painter, Pos2::new(cx, rect.max.y - 11.0), 17.0, Piece::new(taken_colour, kind));
        cx += 11.0;
    }
    if strip.lead > 0 {
        painter.text(
            Pos2::new(cx + 8.0, rect.max.y - 11.0),
            egui::Align2::LEFT_CENTER,
            format!("+{}", strip.lead),
            egui::FontId::proportional(12.0),
            SOFT,
        );
    }
    if let Some((text, colour)) = &strip.note {
        painter.text(
            Pos2::new(rect.max.x - 150.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            text,
            egui::FontId::proportional(13.0),
            *colour,
        );
    }
    if strip.thinking {
        let t = ui.input(|i| i.time) as f32;
        for i in 0..3 {
            let a = ((t * 4.0 - i as f32 * 0.7).sin() * 0.5 + 0.5) * 200.0 + 40.0;
            painter.circle_filled(
                Pos2::new(rect.max.x - 170.0 + i as f32 * 10.0, rect.center().y),
                3.0,
                theme::see_through(36, 128, 222, a as u8),
            );
        }
        ui.ctx().request_repaint();
    }
    if let Some((ms, running)) = strip.clock {
        let pill = Rect::from_min_max(
            Pos2::new(rect.max.x - 130.0, rect.min.y + 6.0),
            rect.max - Vec2::new(8.0, 6.0),
        );
        let low = ms < 20_000;
        let tint = match (running, low) {
            (true, true) => Color32::from_rgb(220, 70, 60),
            (true, false) => Color32::from_rgb(40, 140, 220),
            _ => Color32::from_rgb(232, 238, 246),
        };
        theme::glass(painter, pill, 7.0, tint);
        let ink = if running { Color32::WHITE } else { theme::TEXT };
        painter.text(
            pill.center(),
            egui::Align2::CENTER_CENTER,
            clock_text(ms),
            theme::display_bold(22.0),
            ink,
        );
    }
}

impl ChessScreen {
    /// Draws the Chess tab over `page`.
    pub fn show(&mut self, ui: &mut egui::Ui, page: Rect, platform: &mut Platform) -> Option<Ask> {
        if std::mem::take(&mut self.stop_watching) {
            self.current = None;
            self.pointer = Pointer::default();
        }
        if let Some(m) = self.pending_premove.take() {
            self.make_move(platform, m);
        }
        self.keys(ui);
        let mut ask = None;
        let strip_h = 46.0;
        let side_w = (page.width() * 0.36).clamp(330.0, 460.0);
        let size = (page.height() - 2.0 * strip_h - 20.0).min(page.width() - side_w - 30.0).max(200.0);
        let size = (size / 8.0).floor() * 8.0;
        let left = Rect::from_min_size(page.min, Vec2::new(size, page.height()));
        let top_strip = Rect::from_min_size(left.min, Vec2::new(size, strip_h));
        let board = Rect::from_min_size(top_strip.left_bottom() + Vec2::new(0.0, 10.0), Vec2::splat(size));
        let bottom_strip =
            Rect::from_min_size(board.left_bottom() + Vec2::new(0.0, 10.0), Vec2::new(size, strip_h));
        let side = Rect::from_min_max(Pos2::new(board.max.x + 28.0, page.min.y), page.max);

        self.strips(ui, top_strip, bottom_strip);
        self.board(ui, board, platform);

        ui.scope_builder(egui::UiBuilder::new().max_rect(side), |ui| {
            let in_game = self.current.is_some() && !self.show_lobby;
            if in_game {
                self.game_panel(ui, platform);
            } else {
                ask = self.lobby_panel(ui, platform);
            }
        });
        if self.pointer.slide.is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(200))
            || self.current.as_ref().is_some_and(|m| m.playing())
        {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        ask
    }

    fn keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let Some(current) = &mut self.current else { return };
        let ply = current.game.ply();
        let shown = current.review.unwrap_or(ply);
        let (left, right, home, end) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::End),
            )
        });
        let to = if left {
            Some(shown.saturating_sub(1))
        } else if right {
            Some((shown + 1).min(ply))
        } else if home {
            Some(0)
        } else if end {
            Some(ply)
        } else {
            None
        };
        if let Some(to) = to {
            current.review = (to < ply).then_some(to);
            self.pointer.selected = None;
        }
    }

    fn strips(&mut self, ui: &mut egui::Ui, top: Rect, bottom: Rect) {
        let Some(current) = &self.current else {
            let name = self.lobby.as_ref().map_or("You".to_owned(), |l| l.me.name.clone());
            let empty = Strip {
                name: "Opponent".into(),
                detail: "Pick a game on the right".into(),
                colour: Color::Black,
                clock: None,
                lost: Vec::new(),
                lead: 0,
                thinking: false,
                note: None,
            };
            player_strip(ui, top, &empty);
            player_strip(
                ui,
                bottom,
                &Strip {
                    name,
                    detail: String::new(),
                    colour: Color::White,
                    ..Strip { lost: Vec::new(), ..empty }
                },
            );
            return;
        };
        let bottom_colour = if self.flipped { current.bottom().other() } else { current.bottom() };
        let (lost, lead) = current.game.captured();
        let make = |colour: Color| -> Strip {
            let lead_for = if colour == Color::White { lead } else { -lead };
            let lost_by_other = lost[colour.other().index()].clone();
            let mut strip = Strip {
                name: colour.name().into(),
                detail: String::new(),
                colour,
                clock: None,
                lost: Vec::new(),
                lead: lead_for,
                thinking: false,
                note: None,
            };
            match &current.opponent {
                Opponent::Bot { level, human, thinking, .. } => {
                    if colour == *human {
                        strip.name = self.lobby.as_ref().map_or("You".into(), |l| l.me.name.clone());
                    } else {
                        strip.name = BOTS[*level].name.into();
                        strip.detail = format!("computer · about {}", BOTS[*level].rating);
                        strip.thinking = thinking.is_some();
                    }
                }
                Opponent::Local => {
                    strip.detail = "on this computer".into();
                }
                Opponent::Online { game, received, mine, .. } => {
                    let p = if colour == Color::White { &game.white } else { &game.black };
                    strip.name = p.name.clone();
                    let change = if colour == Color::White { game.white_change } else { game.black_change };
                    strip.detail = match change {
                        Some(c) => format!("{} ({}{c})", p.rating, if c >= 0 { "+" } else { "" }),
                        None => format!("{}{}", p.rating, if game.rated { "" } else { " · casual" }),
                    };
                    let mut ms = if colour == Color::White { game.white_ms } else { game.black_ms };
                    let turn = current.game.turn();
                    let running = game.clock_running && game.status == "playing" && turn == colour;
                    if running {
                        ms -= received.elapsed().as_millis() as i64;
                    }
                    strip.clock = Some((ms.max(0), running));
                    let away =
                        if colour == Color::White { game.white_away_secs } else { game.black_away_secs }
                            + received.elapsed().as_secs() as i64;
                    if game.status == "playing" && mine.is_some() && Some(colour) != *mine && away >= 8 {
                        strip.note = Some((format!("Reconnecting… {away}s"), theme::AMBER));
                    } else if let Some(left) = game.first_move_ms
                        && turn == colour
                        && game.status == "playing"
                    {
                        let left = (left - received.elapsed().as_millis() as i64).max(0);
                        strip.note = Some((format!("First move in {}s", left / 1000), theme::AMBER));
                    }
                }
            }
            Strip { lost: lost_by_other, ..strip }
        };
        let (a, b) = (make(bottom_colour.other()), make(bottom_colour));
        player_strip(ui, top, &a);
        player_strip(ui, bottom, &b);
    }
}

impl ChessScreen {
    fn board(&mut self, ui: &mut egui::Ui, board: Rect, platform: &Platform) {
        let (_, light, dark) = BOARDS[self.board_theme % BOARDS.len()];
        let frame = board.expand(8.0);
        theme::soft_shadow(ui.painter(), frame, 10.0, 70);
        theme::glass(ui.painter(), frame, 10.0, theme::see_through(120, 170, 220, 210));
        let flipped =
            self.flipped != (self.current.as_ref().map_or(Color::White, |m| m.bottom()) == Color::Black);
        let start = Game::default();
        let (game, review) = match &self.current {
            Some(m) => (&m.game, m.review),
            None => (&start, None),
        };
        let shown_ply = review.unwrap_or(game.ply());
        let pos = *game.position_at(shown_ply);
        let last = if shown_ply > 0 { Some(game.moves()[shown_ply - 1]) } else { None };
        let movable = self.current.as_ref().and_then(|m| m.movable());
        let premove = self.current.as_ref().and_then(|m| m.premove);
        let hint = match &self.current {
            Some(Match { opponent: Opponent::Bot { hint, .. }, .. }) => *hint,
            _ => None,
        };

        // What may be picked up, and where it may go.
        let targets_from = |from: Square| -> Vec<Move> {
            let Some(colour) = movable else { return Vec::new() };
            if pos.at(from).is_none_or(|p| p.color != colour) {
                return Vec::new();
            }
            if pos.turn == colour {
                pos.legal_moves().into_iter().filter(|m| m.from == from).collect()
            } else {
                // A premove: any move the piece could make if the other side passed.
                let mut theirs = pos;
                theirs.turn = colour;
                theirs.en_passant = None;
                let mut moves = Vec::new();
                theirs.pseudo_moves(&mut moves, false);
                // Pawns may premove a capture onto any diagonal square.
                if let Some(p) = pos.at(from)
                    && p.kind == Kind::Pawn
                {
                    let dir = if colour == Color::White { 1 } else { -1 };
                    for df in [-1, 1] {
                        if let Some(to) = square(file_of(from) + df, rank_of(from) + dir) {
                            moves.push(Move::new(from, to));
                        }
                    }
                }
                moves.into_iter().filter(|m| m.from == from).collect()
            }
        };

        let id = ui.id().with("chess-board");
        let response = ui.interact(board, id, Sense::click_and_drag());
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let hover_sq = pointer.and_then(|p| square_at(board, p, flipped));
        let (pressed, released, right_pressed, right_released) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.secondary_pressed(),
                i.pointer.secondary_released(),
            )
        });
        let mut chosen: Option<Move> = None;
        if self.pointer.promotion.is_none() {
            if pressed && response.hovered() {
                self.pointer.arrows.clear();
                self.pointer.marks.clear();
                if let Some(sq) = hover_sq {
                    let from_selected = self
                        .pointer
                        .selected
                        .map(&targets_from)
                        .unwrap_or_default()
                        .into_iter()
                        .find(|m| m.to == sq);
                    if let Some(m) = from_selected {
                        chosen = Some(m);
                        self.pointer.pressed = None;
                    } else if pos.at(sq).is_some_and(|p| Some(p.color) == movable) {
                        let was = self.pointer.selected == Some(sq);
                        self.pointer.selected = Some(sq);
                        self.pointer.pressed = Some((sq, was));
                    } else {
                        self.pointer.selected = None;
                        if let Some(c) = self.current.as_mut() {
                            c.premove = None;
                        }
                    }
                }
            }
            if released && let Some((from, was)) = self.pointer.pressed.take() {
                match hover_sq {
                    Some(sq) if sq != from => {
                        if let Some(m) = targets_from(from).into_iter().find(|m| m.to == sq) {
                            chosen = Some(m);
                        }
                    }
                    Some(_) if was => self.pointer.selected = None,
                    _ => {}
                }
            }
            if right_pressed && response.hovered() {
                self.pointer.right_from = hover_sq;
                self.pointer.selected = None;
            }
            if right_released && let Some(from) = self.pointer.right_from.take() {
                match hover_sq {
                    Some(to) if to != from => {
                        if let Some(i) = self.pointer.arrows.iter().position(|a| *a == (from, to)) {
                            self.pointer.arrows.remove(i);
                        } else {
                            self.pointer.arrows.push((from, to));
                        }
                    }
                    Some(sq) => {
                        if let Some(i) = self.pointer.marks.iter().position(|m| *m == sq) {
                            self.pointer.marks.remove(i);
                        } else {
                            self.pointer.marks.push(sq);
                        }
                    }
                    None => {}
                }
            }
        }
        if let Some(m) = chosen {
            self.pointer.selected = None;
            if pos.turn == movable.unwrap_or(pos.turn) && needs_promotion_pick(&pos, m) && !self.auto_queen {
                self.pointer.promotion = Some(m);
            } else {
                self.make_move(platform, m);
            }
        }

        let painter = ui.painter_at(frame);
        // Squares, highlights and coordinates.
        let in_check = pos.in_check().then(|| pos.king(pos.turn));
        let selected_targets = self.pointer.selected.map(targets_from).unwrap_or_default();
        for sq in 0..64u8 {
            let r = square_rect(board, sq, flipped);
            let is_light = (file_of(sq) + rank_of(sq)) % 2 == 1;
            painter.rect_filled(r, 0.0, if is_light { light } else { dark });
            if last.is_some_and(|m| m.from == sq || m.to == sq) {
                painter.rect_filled(r, 0.0, theme::see_through(170, 240, 110, 130));
            }
            if premove.is_some_and(|m| m.from == sq || m.to == sq) {
                painter.rect_filled(r, 0.0, theme::see_through(190, 80, 220, 110));
            }
            if self.pointer.selected == Some(sq) {
                painter.rect_filled(r, 0.0, theme::see_through(40, 150, 255, 120));
            }
            if self.pointer.marks.contains(&sq) {
                painter.circle_stroke(
                    r.center(),
                    r.width() * 0.44,
                    Stroke::new(r.width() * 0.07, theme::see_through(40, 170, 90, 200)),
                );
            }
            if in_check == Some(sq) {
                theme::radial(
                    &painter,
                    r.center(),
                    r.width() * 0.62,
                    Color32::from_rgba_unmultiplied(255, 40, 30, 230),
                    Color32::from_rgba_unmultiplied(255, 40, 30, 0),
                );
            }
            if self.show_coords {
                let ink = if is_light { dark } else { light };
                let font = egui::FontId::proportional((r.width() * 0.16).clamp(9.0, 14.0));
                let bottom_rank = if flipped { 7 } else { 0 };
                let left_file = if flipped { 7 } else { 0 };
                if rank_of(sq) == bottom_rank {
                    painter.text(
                        r.right_bottom() + Vec2::new(-3.0, -2.0),
                        egui::Align2::RIGHT_BOTTOM,
                        ((b'a' + file_of(sq) as u8) as char).to_string(),
                        font.clone(),
                        ink,
                    );
                }
                if file_of(sq) == left_file {
                    painter.text(
                        r.left_top() + Vec2::new(3.0, 2.0),
                        egui::Align2::LEFT_TOP,
                        (rank_of(sq) + 1).to_string(),
                        font,
                        ink,
                    );
                }
            }
        }
        // Where the picked-up piece may go.
        if self.show_moves {
            for m in &selected_targets {
                let r = square_rect(board, m.to, flipped);
                if pos.at(m.to).is_some() {
                    painter.circle_stroke(
                        r.center(),
                        r.width() * 0.43,
                        Stroke::new(r.width() * 0.08, theme::see_through(20, 60, 120, 90)),
                    );
                } else {
                    painter.circle_filled(r.center(), r.width() * 0.15, theme::see_through(20, 60, 120, 90));
                }
            }
            if let Some(sq) = hover_sq
                && selected_targets.iter().any(|m| m.to == sq)
            {
                let r = square_rect(board, sq, flipped);
                painter.rect_stroke(
                    r.shrink(1.5),
                    0.0,
                    Stroke::new(3.0, theme::see_through(255, 255, 255, 200)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        // The pieces. The one being dragged follows the pointer; the last
        // move slides into place.
        let dragging = self.pointer.pressed.and_then(|(from, _)| {
            let start = ui.input(|i| i.pointer.press_origin())?;
            let now = pointer?;
            ((now - start).length() > 4.0).then_some((from, now))
        });
        let slide = self.pointer.slide.filter(|(m, t)| {
            last == Some(*m) && t.elapsed() < Duration::from_millis(180) && review.is_none()
        });
        for sq in 0..64u8 {
            let Some(piece) = pos.at(sq) else { continue };
            let mut r = square_rect(board, sq, flipped);
            if let Some((m, t)) = slide
                && m.to == sq
            {
                let k = (t.elapsed().as_secs_f32() / 0.18).clamp(0.0, 1.0);
                let k = 1.0 - (1.0 - k) * (1.0 - k);
                let from = square_rect(board, m.from, flipped);
                r = r.translate((from.min - r.min) * (1.0 - k));
            }
            if dragging.is_some_and(|(from, _)| from == sq) {
                paint_piece(&painter, r, piece, 0.35);
                continue;
            }
            paint_piece(&painter, r, piece, 1.0);
        }
        // Arrows: a hint, and ones drawn by hand.
        let cell = board.width() / 8.0;
        if let Some(m) = hint {
            arrow(
                &painter,
                square_rect(board, m.from, flipped).center(),
                square_rect(board, m.to, flipped).center(),
                cell * 0.16,
                theme::see_through(30, 170, 80, 190),
            );
        }
        for (a, b) in &self.pointer.arrows {
            arrow(
                &painter,
                square_rect(board, *a, flipped).center(),
                square_rect(board, *b, flipped).center(),
                cell * 0.15,
                theme::see_through(240, 140, 20, 190),
            );
        }
        if let Some((from, at)) = dragging
            && let Some(piece) = pos.at(from)
        {
            let r = Rect::from_center_size(at, Vec2::splat(cell * 1.1));
            let top = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, id));
            paint_piece(&top, r, piece, 1.0);
        }
        if movable.is_some()
            && hover_sq.is_some_and(|sq| pos.at(sq).is_some_and(|p| Some(p.color) == movable))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        self.promotion_picker(ui, board, flipped, platform);
        self.result_card(ui, board, platform);
    }

    fn promotion_picker(&mut self, ui: &mut egui::Ui, board: Rect, flipped: bool, platform: &Platform) {
        let Some(m) = self.pointer.promotion else { return };
        let Some(colour) = self.current.as_ref().and_then(|c| c.game.position().at(m.from)).map(|p| p.color)
        else {
            self.pointer.promotion = None;
            return;
        };
        let painter =
            ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("promo")));
        painter.rect_filled(board, 0.0, theme::see_through(10, 30, 60, 110));
        let target = square_rect(board, m.to, flipped);
        let cell = target.width();
        let down = target.min.y < board.center().y;
        let mut picked = None;
        for (i, kind) in [Kind::Queen, Kind::Rook, Kind::Bishop, Kind::Knight].into_iter().enumerate() {
            let offset = if down { i as f32 } else { -(i as f32) };
            let r = target.translate(Vec2::new(0.0, offset * cell));
            let hovered = ui.input(|inp| inp.pointer.hover_pos()).is_some_and(|p| r.contains(p));
            theme::glass(
                &painter,
                r.shrink(2.0),
                cell * 0.5,
                if hovered { Color32::from_rgb(120, 200, 255) } else { Color32::from_rgb(236, 246, 255) },
            );
            paint_piece(&painter, r.shrink(cell * 0.06), Piece::new(colour, kind), 1.0);
            if hovered && ui.input(|inp| inp.pointer.primary_clicked()) {
                picked = Some(kind);
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.pointer.promotion = None;
        }
        if let Some(kind) = picked {
            self.pointer.promotion = None;
            self.make_move(platform, Move { promotion: Some(kind), ..m });
        }
    }

    /// The card over the board when a game ends.
    fn result_card(&mut self, ui: &mut egui::Ui, board: Rect, platform: &Platform) {
        let Some(current) = &self.current else { return };
        if current.result_closed || current.review.is_some() || self.show_lobby {
            return;
        }
        let Some(outcome) = current.game.outcome() else { return };
        let me = match &current.opponent {
            Opponent::Bot { human, .. } => Some(*human),
            Opponent::Online { mine, .. } => *mine,
            Opponent::Local => None,
        };
        let (title, colour) = match (outcome.reason, outcome.winner, me) {
            (Reason::Aborted, ..) => ("Game aborted".to_owned(), theme::MUTED),
            (_, Winner::Draw, _) => ("Draw".to_owned(), theme::ACCENT_DEEP),
            (_, w, Some(me)) if w == Winner::of(me) => ("You won!".to_owned(), theme::GREEN_DEEP),
            (_, _, Some(_)) => ("You lost".to_owned(), theme::RED),
            (_, w, None) => {
                (format!("{} won", if w == Winner::White { "White" } else { "Black" }), theme::ACCENT_DEEP)
            }
        };
        let detail = match outcome.reason {
            Reason::Aborted => "Nobody moved in time, so it doesn't count.".to_owned(),
            _ => outcome.headline(),
        };
        let change = match &current.opponent {
            Opponent::Online { game, mine: Some(mine), .. } => {
                if *mine == Color::White {
                    game.white_change
                } else {
                    game.black_change
                }
            }
            _ => None,
        };
        let online = matches!(current.opponent, Opponent::Online { .. });
        let rematch_offered = match &current.opponent {
            Opponent::Online { game, mine: Some(mine), .. } => {
                game.rematch_offer.as_deref().and_then(color_of).map(|c| c != *mine)
            }
            _ => None,
        };
        let card = Rect::from_center_size(board.center(), Vec2::new((board.width() * 0.7).min(340.0), 190.0));
        let mut close = false;
        let mut action: Option<&str> = None;
        egui::Area::new(ui.id().with("result-area")).order(egui::Order::Foreground).fixed_pos(card.min).show(
            ui.ctx(),
            |ui| {
                let painter = ui.painter();
                theme::soft_shadow(painter, card, 12.0, 90);
                painter.rect_filled(card, 12.0, Color32::from_rgb(246, 251, 255));
                theme::glass(painter, card, 12.0, theme::see_through(236, 246, 255, 200));
                ui.set_min_size(card.size());
                ui.set_max_size(card.size());
                ui.vertical_centered(|ui| {
                    ui.add_space(14.0);
                    ui.label(RichText::new(&title).font(theme::display_bold(26.0)).color(colour));
                    ui.label(RichText::new(&detail).color(SOFT));
                    if let Some(c) = change {
                        ui.label(
                            RichText::new(format!("Rating {}{c}", if c >= 0 { "+" } else { "" }))
                                .font(theme::display(16.0))
                                .color(if c >= 0 { theme::GREEN_DEEP } else { theme::RED }),
                        );
                    } else {
                        ui.add_space(20.0);
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        let label = match rematch_offered {
                            Some(true) => "Accept rematch",
                            Some(false) => "Rematch asked…",
                            None => "Rematch",
                        };
                        let can =
                            rematch_offered != Some(false) && outcome.reason != Reason::Aborted || !online;
                        if ui
                            .add_enabled_ui(can, |ui| {
                                theme::block_button(ui, theme::Kind::Go, icon::ARROWS_CLOCKWISE, label, 32.0)
                            })
                            .inner
                            .clicked()
                        {
                            action = Some("rematch");
                        }
                        if theme::block_button(ui, theme::Kind::Plain, icon::PLUS, "New game", 32.0).clicked()
                        {
                            action = Some("new");
                        }
                    });
                    ui.add_space(4.0);
                    if ui.link("Look over the game").clicked() {
                        close = true;
                    }
                });
            },
        );
        match action {
            Some("rematch") => self.rematch(platform),
            Some("new") => {
                self.show_lobby = true;
                close = true;
            }
            _ => {}
        }
        if close && let Some(c) = &mut self.current {
            c.result_closed = true;
        }
    }

    fn rematch(&mut self, platform: &Platform) {
        let Some(current) = &mut self.current else { return };
        match &current.opponent {
            Opponent::Bot { level, human, .. } => {
                self.bot_level = *level;
                self.bot_color = if *human == Color::White { 2 } else { 1 };
                self.start_bot();
            }
            Opponent::Local => self.start_local(),
            Opponent::Online { game, .. } => {
                let id = game.id.clone();
                self.send(platform, ChessAction::Rematch { game_id: id });
            }
        }
    }
}

impl ChessScreen {
    fn game_panel(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        let Some(current) = &self.current else { return };
        let (title, subtitle) = match &current.opponent {
            Opponent::Bot { level, .. } => (
                format!("You vs {}", BOTS[*level].name),
                format!("Computer, about {} · {}", BOTS[*level].rating, BOTS[*level].about),
            ),
            Opponent::Local => {
                ("Pass and play".to_owned(), "Two players taking turns on this computer.".to_owned())
            }
            Opponent::Online { game, mine, .. } => {
                let control = TimeControl::parse(&game.control);
                let kind = control.map_or("", |c| c.category().name());
                let what =
                    format!("{} {kind} · {}", if game.rated { "Rated" } else { "Casual" }, game.control);
                if mine.is_some() {
                    (format!("{} vs {}", game.white.name, game.black.name), what)
                } else {
                    ("Watching".to_owned(), format!("{} vs {} · {what}", game.white.name, game.black.name))
                }
            }
        };
        theme::heading(ui, &title);
        ui.label(RichText::new(subtitle).color(SOFT));
        ui.add_space(6.0);
        let game = &current.game;
        let status = match game.outcome() {
            Some(o) => (o.headline(), theme::ACCENT_DEEP),
            None if current.review.is_some() => {
                (format!("Looking back at move {}", current.review.unwrap_or(0).div_ceil(2)), theme::AMBER)
            }
            None => {
                let turn = game.turn();
                let whose = match (&current.opponent, current.movable()) {
                    (Opponent::Local, _) => format!("{} to move", turn.name()),
                    (_, Some(me)) if me == turn => "Your move".to_owned(),
                    (Opponent::Bot { .. }, _) => "The computer is thinking…".to_owned(),
                    _ => format!("{} to move", turn.name()),
                };
                if game.position().in_check() {
                    (format!("Check! {whose}"), theme::RED)
                } else {
                    (whose, theme::GREEN_DEEP)
                }
            }
        };
        theme::status(ui, status.1, &status.0);
        if let Some(Err(e)) = &self.status {
            theme::status(ui, theme::RED, e);
        }
        ui.add_space(8.0);
        self.move_list(ui);
        ui.add_space(6.0);
        self.nav_row(ui);
        ui.add_space(10.0);
        self.actions(ui, platform);
        ui.add_space(10.0);
        self.chat(ui, platform);
        self.settings(ui);
    }

    fn move_list(&mut self, ui: &mut egui::Ui) {
        let Some(current) = &mut self.current else { return };
        let sans = current.game.sans().to_vec();
        let shown = current.review.unwrap_or(sans.len());
        let black_first = current.game.start().turn == Color::Black;
        let mut jump = None;
        theme::panel().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let height = (ui.available_height() - 260.0).clamp(90.0, 300.0);
            egui::ScrollArea::vertical()
                .id_salt("chess-moves")
                .max_height(height)
                .min_scrolled_height(height)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if sans.is_empty() {
                        ui.label(RichText::new("Moves show up here.").color(SOFT));
                    }
                    let offset = usize::from(black_first);
                    let rows = (sans.len() + offset).div_ceil(2);
                    for row in 0..rows {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [34.0, 20.0],
                                egui::Label::new(RichText::new(format!("{}.", row + 1)).color(SOFT)),
                            );
                            for col in 0..2 {
                                let i = row * 2 + col;
                                if i < offset || i - offset >= sans.len() {
                                    ui.add_sized([86.0, 20.0], egui::Label::new(""));
                                    continue;
                                }
                                let ply = i - offset + 1;
                                let on = ply == shown;
                                if theme::tool_button(ui, "", &sans[ply - 1], on).clicked() {
                                    jump = Some(ply);
                                }
                                ui.add_space((80.0 - sans[ply - 1].len() as f32 * 8.0).max(4.0));
                            }
                        });
                    }
                });
        });
        if let Some(ply) = jump {
            current.review = (ply < sans.len()).then_some(ply);
            self.pointer.selected = None;
        }
    }

    fn nav_row(&mut self, ui: &mut egui::Ui) {
        let Some(current) = &mut self.current else { return };
        let ply = current.game.ply();
        let shown = current.review.unwrap_or(ply);
        ui.horizontal(|ui| {
            let mut to = None;
            if theme::tool_button(ui, icon::CARET_DOUBLE_LEFT, "", false)
                .on_hover_text("First move")
                .clicked()
            {
                to = Some(0);
            }
            if theme::tool_button(ui, icon::CARET_LEFT, "", false).on_hover_text("Back (←)").clicked() {
                to = Some(shown.saturating_sub(1));
            }
            if theme::tool_button(ui, icon::CARET_RIGHT, "", false).on_hover_text("Forward (→)").clicked() {
                to = Some((shown + 1).min(ply));
            }
            if theme::tool_button(ui, icon::CARET_DOUBLE_RIGHT, "", false)
                .on_hover_text("Latest move")
                .clicked()
            {
                to = Some(ply);
            }
            if let Some(to) = to {
                current.review = (to < ply).then_some(to);
            }
            ui.add_space(8.0);
            if theme::tool_button(ui, icon::ARROWS_DOWN_UP, "Flip", self.flipped).clicked() {
                self.flipped = !self.flipped;
            }
            let fen = current.game.position_at(shown).fen();
            if theme::tool_button(ui, icon::COPY, "FEN", false).on_hover_text("Copy this position").clicked()
            {
                ui.ctx().copy_text(fen);
                self.copied = Some(("Position copied".into(), Instant::now()));
            }
            if theme::tool_button(ui, icon::COPY, "PGN", false)
                .on_hover_text("Copy the whole game record")
                .clicked()
            {
                let pgn = pgn_of(current);
                ui.ctx().copy_text(pgn);
                self.copied = Some(("Game record copied".into(), Instant::now()));
            }
            if let Some((text, at)) = &self.copied
                && at.elapsed() < Duration::from_secs(3)
            {
                ui.label(RichText::new(text).small().color(theme::GREEN_DEEP));
            }
        });
    }

    fn actions(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        let Some(current) = &mut self.current else { return };
        let playing = current.playing();
        let mut act: Option<ChessAction> = None;
        ui.horizontal_wrapped(|ui| {
            match &mut current.opponent {
                Opponent::Bot { hint, hint_slot, takebacks, brain: _, human, .. } => {
                    if playing {
                        let can_take = current.game.ply() > 0;
                        if ui
                            .add_enabled_ui(can_take, |ui| {
                                theme::block_button(
                                    ui,
                                    theme::Kind::Plain,
                                    icon::ARROW_U_UP_LEFT,
                                    "Take back",
                                    32.0,
                                )
                            })
                            .inner
                            .clicked()
                        {
                            // Back to the player's own turn.
                            current.game.undo();
                            if current.game.turn() != *human {
                                current.game.undo();
                            }
                            *takebacks += 1;
                            *hint = None;
                            current.review = None;
                            self.pointer = Pointer::default();
                        }
                        let my_turn = current.game.turn() == *human;
                        if ui
                            .add_enabled_ui(my_turn && hint_slot.is_none(), |ui| {
                                theme::block_button(ui, theme::Kind::Plain, icon::LIGHTBULB, "Hint", 32.0)
                            })
                            .inner
                            .clicked()
                        {
                            let slot: MoveSlot = Arc::default();
                            let (out, pos) = (slot.clone(), *current.game.position());
                            std::thread::spawn(move || {
                                let found = Bot::new().think(&pos, 64, Duration::from_millis(900)).best;
                                *out.lock().unwrap() = Some(found);
                            });
                            *hint_slot = Some(slot);
                        }
                        if theme::block_button(ui, theme::Kind::Danger, icon::FLAG, "Resign", 32.0).clicked()
                        {
                            let winner = Winner::of(human.other());
                            current.game.end(Outcome { winner, reason: Reason::Resigned });
                            *hint = None;
                            current.result_closed = false;
                        }
                    } else if theme::block_button(ui, theme::Kind::Primary, icon::PLUS, "New game", 32.0)
                        .clicked()
                    {
                        self.show_lobby = true;
                    }
                }
                Opponent::Local => {
                    if playing {
                        if ui
                            .add_enabled_ui(current.game.ply() > 0, |ui| {
                                theme::block_button(
                                    ui,
                                    theme::Kind::Plain,
                                    icon::ARROW_U_UP_LEFT,
                                    "Take back",
                                    32.0,
                                )
                            })
                            .inner
                            .clicked()
                        {
                            current.game.undo();
                            current.review = None;
                        }
                        if theme::block_button(ui, theme::Kind::Plain, icon::HANDSHAKE, "Agree a draw", 32.0)
                            .clicked()
                        {
                            current.game.end(Outcome { winner: Winner::Draw, reason: Reason::Agreement });
                        }
                        let turn = current.game.turn();
                        if theme::block_button(
                            ui,
                            theme::Kind::Danger,
                            icon::FLAG,
                            &format!("{} resigns", turn.name()),
                            32.0,
                        )
                        .clicked()
                        {
                            current
                                .game
                                .end(Outcome { winner: Winner::of(turn.other()), reason: Reason::Resigned });
                        }
                    } else if theme::block_button(ui, theme::Kind::Primary, icon::PLUS, "New game", 32.0)
                        .clicked()
                    {
                        self.show_lobby = true;
                    }
                }
                Opponent::Online { game, mine, received, .. } => {
                    let id = game.id.clone();
                    match mine {
                        Some(me) if playing => {
                            let started = current.game.ply() >= 2;
                            let offered_to_me =
                                game.draw_offer.as_deref().and_then(color_of) == Some(me.other());
                            let i_offered = game.draw_offer.as_deref().and_then(color_of) == Some(*me);
                            if !started {
                                if theme::block_button(ui, theme::Kind::Plain, icon::X, "Abort", 32.0)
                                    .clicked()
                                {
                                    act = Some(ChessAction::Abort { game_id: id.clone() });
                                }
                            } else if offered_to_me {
                                if theme::block_button(
                                    ui,
                                    theme::Kind::Go,
                                    icon::HANDSHAKE,
                                    "Accept draw",
                                    32.0,
                                )
                                .clicked()
                                {
                                    act = Some(ChessAction::OfferDraw { game_id: id.clone() });
                                }
                                if theme::block_button(ui, theme::Kind::Plain, icon::X, "Decline", 32.0)
                                    .clicked()
                                {
                                    act = Some(ChessAction::DeclineDraw { game_id: id.clone() });
                                }
                            } else if ui
                                .add_enabled_ui(!i_offered, |ui| {
                                    theme::block_button(
                                        ui,
                                        theme::Kind::Plain,
                                        icon::HANDSHAKE,
                                        if i_offered { "Draw offered" } else { "Offer draw" },
                                        32.0,
                                    )
                                })
                                .inner
                                .clicked()
                            {
                                act = Some(ChessAction::OfferDraw { game_id: id.clone() });
                            }
                            if started
                                && theme::block_button(ui, theme::Kind::Danger, icon::FLAG, "Resign", 32.0)
                                    .clicked()
                            {
                                act = Some(ChessAction::Resign { game_id: id.clone() });
                            }
                            let away =
                                if *me == Color::White { game.black_away_secs } else { game.white_away_secs }
                                    + received.elapsed().as_secs() as i64;
                            if away * 1000 >= libroxel_hub::GONE_MS
                                && theme::block_button(
                                    ui,
                                    theme::Kind::Go,
                                    icon::TROPHY,
                                    "Claim the win",
                                    32.0,
                                )
                                .clicked()
                            {
                                act = Some(ChessAction::ClaimWin { game_id: id.clone() });
                            }
                        }
                        Some(_) => {
                            if theme::block_button(
                                ui,
                                theme::Kind::Go,
                                icon::ARROWS_CLOCKWISE,
                                "Rematch",
                                32.0,
                            )
                            .clicked()
                            {
                                act = Some(ChessAction::Rematch { game_id: id.clone() });
                            }
                            if theme::block_button(ui, theme::Kind::Primary, icon::PLUS, "New game", 32.0)
                                .clicked()
                            {
                                self.closed_games.push(id.clone());
                                self.show_lobby = true;
                            }
                        }
                        None => {
                            if theme::block_button(ui, theme::Kind::Plain, icon::X, "Stop watching", 32.0)
                                .clicked()
                            {
                                self.closed_games.push(id.clone());
                                self.show_lobby = true;
                                self.page = Page::Watch;
                                self.stop_watching = true;
                            }
                        }
                    }
                }
            }
        });
        if let Some(a) = act {
            self.send(platform, a);
        }
    }

    fn chat(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        let Some(Match { opponent: Opponent::Online { game, mine, .. }, .. }) = &self.current else { return };
        let (id, chat, white, black, playing_in) = (
            game.id.clone(),
            game.chat.clone(),
            game.white.name.clone(),
            game.black.name.clone(),
            mine.is_some(),
        );
        if chat.is_empty() && !playing_in {
            return;
        }
        theme::label_caps(ui, "Chat");
        for line in chat.iter().rev().take(4).rev() {
            let who = if line.color == "white" { &white } else { &black };
            let text = CHESS_PHRASES.get(line.phrase as usize).copied().unwrap_or("…");
            ui.label(RichText::new(format!("{who}: {text}")).color(theme::TEXT));
        }
        if playing_in {
            let mut send = false;
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("chess-phrase")
                    .width(200.0)
                    .selected_text(CHESS_PHRASES[self.phrase % CHESS_PHRASES.len()])
                    .show_ui(ui, |ui| {
                        for (i, p) in CHESS_PHRASES.iter().enumerate() {
                            ui.selectable_value(&mut self.phrase, i, *p);
                        }
                    });
                send = theme::tool_button(ui, icon::CHAT_CIRCLE, "Say", false).clicked();
            });
            if send {
                self.send(platform, ChessAction::Chat { game_id: id, phrase: self.phrase as u8 });
            }
        }
        ui.add_space(8.0);
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("Board settings").color(theme::HEADING)).id_salt("chess-settings").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (i, (name, light, dark)) in BOARDS.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(44.0, 30.0), Sense::click());
                    let half = Rect::from_min_size(r.min, Vec2::new(22.0, 30.0));
                    ui.painter().rect_filled(half, 0.0, *light);
                    ui.painter().rect_filled(half.translate(Vec2::new(22.0, 0.0)), 0.0, *dark);
                    let stroke = if self.board_theme == i { Stroke::new(2.5, theme::ACCENT) } else { Stroke::new(1.0, theme::LINE) };
                    ui.painter().rect_stroke(r, 3.0, stroke, egui::StrokeKind::Outside);
                    if resp.on_hover_text(*name).clicked() {
                        self.board_theme = i;
                    }
                }
            });
            ui.checkbox(&mut self.show_coords, "Show coordinates");
            ui.checkbox(&mut self.show_moves, "Show where a piece can go");
            ui.checkbox(&mut self.auto_queen, "Pawns always become queens");
            ui.label(RichText::new("Right-click and drag on the board to draw arrows; right-click a square to circle it.").small().color(SOFT));
        });
    }
}

fn pgn_of(m: &Match) -> String {
    let date = {
        let secs =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let days = secs / 86_400;
        // Civil date from days since 1970 (Howard Hinnant's method).
        let z = days as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(mo <= 2);
        format!("{y}.{mo:02}.{d:02}")
    };
    let (white, black, event, control) = match &m.opponent {
        Opponent::Bot { level, human, .. } => {
            let bot = format!("{} (computer)", BOTS[*level].name);
            if *human == Color::White {
                ("You".into(), bot, "Libroxel game vs computer", "-".into())
            } else {
                (bot, "You".into(), "Libroxel game vs computer", "-".into())
            }
        }
        Opponent::Local => ("White".into(), "Black".into(), "Libroxel pass and play", "-".into()),
        Opponent::Online { game, .. } => (
            game.white.name.clone(),
            game.black.name.clone(),
            if game.rated { "Libroxel rated game" } else { "Libroxel casual game" },
            game.control.clone(),
        ),
    };
    let mut tags = vec![
        ("Event", event.to_owned()),
        ("Site", "Libroxel".to_owned()),
        ("Date", date),
        ("White", white),
        ("Black", black),
        ("TimeControl", control),
    ];
    if let Some(o) = m.game.outcome() {
        tags.push(("Result", if o.reason == Reason::Aborted { "*".into() } else { o.winner.score().into() }));
        tags.push(("Termination", o.reason.describe().into()));
    }
    m.game.pgn(&tags)
}

impl ChessScreen {
    fn lobby_panel(&mut self, ui: &mut egui::Ui, platform: &mut Platform) -> Option<Ask> {
        let mut ask = None;
        ui.horizontal(|ui| {
            theme::heading(ui, "Chess");
            if let Some(l) = &self.lobby {
                ui.add_space(10.0);
                ui.label(RichText::new(format!("{} playing now", l.playing_now)).color(SOFT));
            }
        });
        if self.current.is_some()
            && theme::tool_button(ui, icon::ARROW_LEFT, "Back to the game", false).clicked()
        {
            self.show_lobby = false;
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (page, glyph, label) in [
                (Page::Online, icon::GLOBE_HEMISPHERE_WEST, "Online"),
                (Page::Computer, icon::ROBOT, "Computer"),
                (Page::Watch, icon::EYE, "Watch"),
                (Page::Leaderboard, icon::TROPHY, "Leaderboard"),
                (Page::Me, icon::USER, "My games"),
            ] {
                if theme::tool_button(ui, glyph, label, self.page == page).clicked() {
                    self.page = page;
                    self.live_at = None;
                    self.leaders_at = None;
                    self.profile_at = None;
                }
            }
        });
        ui.separator();
        if let Some(e) = &self.lobby_error {
            theme::status(ui, theme::RED, e);
        }
        if let Some(Err(e)) = &self.status {
            theme::status(ui, theme::RED, e);
        }
        egui::ScrollArea::vertical().id_salt("chess-lobby").auto_shrink([false, false]).show(ui, |ui| {
            match self.page {
                Page::Computer => self.computer_page(ui),
                _ if platform.account.is_none() => {
                    theme::panel().show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(RichText::new("Claim a name to play online").font(theme::display(17.0)).color(theme::TEXT));
                        ui.label(RichText::new("Online games, ratings and the leaderboard go by your name on the Hub. It takes a second, on the Avatar tab. You can play the computer without one.").color(SOFT));
                        ui.add_space(6.0);
                        if theme::block_button(ui, theme::Kind::Primary, icon::T_SHIRT, "Claim a name", 36.0).clicked() {
                            ask = Some(Ask::ClaimName);
                        }
                    });
                }
                Page::Online => self.online_page(ui, platform),
                Page::Watch => self.watch_page(ui, platform),
                Page::Leaderboard => self.leader_page(ui, platform),
                Page::Me => self.me_page(ui, platform),
            }
        });
        ask
    }

    fn online_page(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        let lobby = self.lobby.clone().unwrap_or_default();
        let mut act: Option<ChessAction> = None;
        // Ratings at a glance.
        ui.horizontal(|ui| {
            for r in &lobby.ratings {
                let name = Category::from_code(&r.category).map_or("", |c| c.name());
                theme::panel().inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(name).small().color(SOFT));
                        ui.label(
                            RichText::new(r.rating.to_string())
                                .font(theme::display_bold(18.0))
                                .color(theme::HEADING),
                        );
                    });
                });
            }
        });
        ui.add_space(8.0);
        if let Some(seek) = &lobby.seeking {
            theme::panel().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!(
                            "Looking for a {} {} opponent…",
                            if seek.rated { "rated" } else { "casual" },
                            seek.control
                        ))
                        .font(theme::display(16.0))
                        .color(theme::TEXT),
                    );
                });
                ui.label(
                    RichText::new(format!(
                        "{}s so far. The longer it takes, the wider the range of ratings it accepts.",
                        seek.waiting_secs
                    ))
                    .color(SOFT),
                );
                ui.horizontal(|ui| {
                    if theme::block_button(ui, theme::Kind::Plain, icon::X, "Stop looking", 32.0).clicked() {
                        act = Some(ChessAction::CancelSeek);
                    }
                    if seek.waiting_secs >= 20
                        && theme::block_button(
                            ui,
                            theme::Kind::Plain,
                            icon::ROBOT,
                            "Play the computer instead",
                            32.0,
                        )
                        .clicked()
                    {
                        act = Some(ChessAction::CancelSeek);
                        self.page = Page::Computer;
                    }
                });
            });
            ui.add_space(8.0);
        }
        theme::label_caps(ui, "Quick match");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.rated, "Rated");
            ui.label(
                RichText::new("Pick a clock; you'll be paired with someone close to your rating.")
                    .small()
                    .color(SOFT),
            );
        });
        ui.add_space(4.0);
        let counts = lobby.seeking_counts.clone();
        egui::Grid::new("chess-quick").spacing(Vec2::new(8.0, 8.0)).show(ui, |ui| {
            for (i, control) in TimeControl::PRESETS.iter().enumerate() {
                let code = control.code();
                let waiting = counts.iter().find(|(c, _)| *c == code).map_or(0, |(_, n)| *n);
                let on = lobby.seeking.as_ref().is_some_and(|s| s.control == code);
                if quick_tile(ui, &code, control.category().name(), waiting, on).clicked() {
                    act = Some(if on {
                        ChessAction::CancelSeek
                    } else {
                        ChessAction::Seek { control: code, rated: self.rated }
                    });
                }
                if i % 5 == 4 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(10.0);
        // Challenges waiting for this player, and ones they sent.
        if !lobby.incoming.is_empty() {
            theme::label_caps(ui, "Challenges for you");
            for c in &lobby.incoming {
                theme::panel().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} ({}) · {} {}",
                                c.from.name,
                                c.from.rating,
                                if c.rated { "rated" } else { "casual" },
                                c.control
                            ))
                            .color(theme::TEXT),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::tool_button(ui, icon::X, "Decline", false).clicked() {
                                act = Some(ChessAction::AnswerChallenge { id: c.id.clone(), accept: false });
                            }
                            if theme::block_button(ui, theme::Kind::Go, icon::CHECK, "Accept", 28.0).clicked()
                            {
                                act = Some(ChessAction::AnswerChallenge { id: c.id.clone(), accept: true });
                            }
                        });
                    });
                });
            }
            ui.add_space(8.0);
        }
        if !lobby.outgoing.is_empty() {
            theme::label_caps(ui, "Challenges you sent");
            for c in &lobby.outgoing {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!("Waiting for {} · {}", c.to.name, c.control))
                            .color(theme::TEXT),
                    );
                    if theme::tool_button(ui, icon::X, "Cancel", false).clicked() {
                        act = Some(ChessAction::AnswerChallenge { id: c.id.clone(), accept: false });
                    }
                });
            }
            ui.add_space(8.0);
        }
        theme::label_caps(ui, "Challenge a friend");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("chess-challenge-clock")
                .width(70.0)
                .selected_text(TimeControl::PRESETS[self.challenge_control].code())
                .show_ui(ui, |ui| {
                    for (i, c) in TimeControl::PRESETS.iter().enumerate() {
                        ui.selectable_value(&mut self.challenge_control, i, c.code());
                    }
                });
            egui::ComboBox::from_id_salt("chess-challenge-colour")
                .width(90.0)
                .selected_text(["Random side", "I'm White", "I'm Black"][self.challenge_color])
                .show_ui(ui, |ui| {
                    for (i, c) in ["Random side", "I'm White", "I'm Black"].iter().enumerate() {
                        ui.selectable_value(&mut self.challenge_color, i, *c);
                    }
                });
            ui.checkbox(&mut self.challenge_rated, "Rated");
        });
        let friends = platform.social.friends.friends.clone();
        if friends.is_empty() {
            ui.label(RichText::new("Add friends on the Friends tab to challenge them here.").color(SOFT));
        }
        for f in &friends {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                theme::orb(
                    ui.painter(),
                    r.center(),
                    5.0,
                    if f.online { theme::GREEN } else { Color32::from_rgb(170, 178, 190) },
                );
                ui.label(RichText::new(&f.name).color(theme::TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::tool_button(ui, icon::SWORD, "Challenge", false).clicked() {
                        act = Some(ChessAction::Challenge {
                            to: f.player_id.clone(),
                            control: TimeControl::PRESETS[self.challenge_control].code(),
                            rated: self.challenge_rated,
                            color: match self.challenge_color {
                                1 => Some("white".into()),
                                2 => Some("black".into()),
                                _ => None,
                            },
                        });
                    }
                });
            });
        }
        ui.add_space(10.0);
        if !lobby.active.is_empty() {
            theme::label_caps(ui, "Your games on now");
            for g in &lobby.active {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} vs {} · {}", g.white.name, g.black.name, g.control))
                            .color(theme::TEXT),
                    );
                    if theme::tool_button(ui, icon::PLAY, "Go to it", false).clicked() {
                        self.closed_games.retain(|c| *c != g.id);
                        let me = lobby.me.player_id.clone();
                        let mine =
                            if g.white.player_id == me { Some(Color::White) } else { Some(Color::Black) };
                        self.open_online(g.clone(), mine);
                    }
                });
            }
        }
        if let Some(a) = act {
            self.send(platform, a);
            self.lobby_at = None;
        }
    }

    fn computer_page(&mut self, ui: &mut egui::Ui) {
        theme::label_caps(ui, "Play the computer");
        ui.label(
            RichText::new(
                "Eight players from beginner to expert. Take moves back and ask for hints whenever you like.",
            )
            .color(SOFT),
        );
        ui.add_space(6.0);
        egui::Grid::new("chess-bots").spacing(Vec2::new(8.0, 8.0)).show(ui, |ui| {
            for (i, bot) in BOTS.iter().enumerate() {
                if bot_tile(ui, bot.name, bot.rating, i, self.bot_level == i)
                    .on_hover_text(bot.about)
                    .clicked()
                {
                    self.bot_level = i;
                }
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(6.0);
        let bot = BOTS[self.bot_level];
        ui.label(
            RichText::new(format!("{} · about {}", bot.name, bot.rating))
                .font(theme::display(16.0))
                .color(theme::TEXT),
        );
        ui.label(RichText::new(bot.about).color(SOFT));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            theme::label_caps(ui, "Play as");
            for (i, name) in ["Random", "White", "Black"].iter().enumerate() {
                if theme::tool_button(ui, "", name, self.bot_color == i).clicked() {
                    self.bot_color = i;
                }
            }
        });
        ui.add_space(8.0);
        if theme::block_button(ui, theme::Kind::Go, icon::PLAY, &format!("Play {}", bot.name), 40.0).clicked()
        {
            self.start_bot();
        }
        ui.add_space(16.0);
        theme::label_caps(ui, "Pass and play");
        ui.label(RichText::new("Two people on one computer, taking turns.").color(SOFT));
        if theme::block_button(ui, theme::Kind::Plain, icon::USERS, "Start", 34.0).clicked() {
            self.start_local();
        }
    }

    fn watch_page(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        if self.live_at.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.live_at = Some(Instant::now());
            self.fetch(Job::Live { hub: platform.hub_address.clone() });
        }
        theme::label_caps(ui, "Games being played now");
        if self.live.is_empty() {
            ui.label(
                RichText::new("Nobody is playing right now. Start a quick match and others can watch you!")
                    .color(SOFT),
            );
        }
        let mut watch = None;
        for g in &self.live {
            theme::panel().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} ({})  vs  {} ({})",
                                g.white.name, g.white.rating, g.black.name, g.black.rating
                            ))
                            .color(theme::TEXT),
                        );
                        let moves = g.moves.split_whitespace().count();
                        ui.label(
                            RichText::new(format!(
                                "{} · {} · move {}",
                                g.control,
                                if g.rated { "rated" } else { "casual" },
                                moves / 2 + 1
                            ))
                            .small()
                            .color(SOFT),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::tool_button(ui, icon::EYE, "Watch", false).clicked() {
                            watch = Some(g.clone());
                        }
                    });
                });
            });
        }
        if let Some(g) = watch {
            self.closed_games.retain(|c| *c != g.id);
            self.open_online(g, None);
        }
    }

    fn leader_page(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        ui.horizontal(|ui| {
            for c in Category::ALL {
                if theme::tool_button(ui, "", c.name(), self.leader_category == c).clicked()
                    && self.leader_category != c
                {
                    self.leader_category = c;
                    self.leaders.clear();
                    self.leaders_at = None;
                }
            }
        });
        if self.leaders_at.is_none_or(|t| t.elapsed() > Duration::from_secs(20)) {
            self.leaders_at = Some(Instant::now());
            self.fetch(Job::Leaders { hub: platform.hub_address.clone(), category: self.leader_category });
        }
        ui.add_space(6.0);
        if self.leaders.is_empty() {
            ui.label(
                RichText::new("No rated games yet in this category. Be the first on the board!").color(SOFT),
            );
        }
        let me = self.lobby.as_ref().map(|l| l.me.player_id.clone()).unwrap_or_default();
        egui::Grid::new("chess-leaders").striped(true).spacing(Vec2::new(18.0, 6.0)).show(ui, |ui| {
            if !self.leaders.is_empty() {
                for h in ["#", "Player", "Rating", "Games", "Wins"] {
                    ui.label(RichText::new(h).small().color(SOFT));
                }
                ui.end_row();
            }
            for l in &self.leaders {
                let colour = if l.player.player_id == me { theme::ACCENT_DEEP } else { theme::TEXT };
                let medal = match l.rank {
                    1 => Color32::from_rgb(230, 180, 30),
                    2 => Color32::from_rgb(170, 180, 194),
                    3 => Color32::from_rgb(200, 130, 70),
                    _ => Color32::TRANSPARENT,
                };
                ui.horizontal(|ui| {
                    if medal != Color32::TRANSPARENT {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        theme::orb(ui.painter(), r.center(), 6.0, medal);
                    }
                    ui.label(RichText::new(l.rank.to_string()).color(colour));
                });
                ui.label(RichText::new(&l.player.name).color(colour));
                ui.label(
                    RichText::new(l.player.rating.to_string()).font(theme::display_bold(15.0)).color(colour),
                );
                ui.label(RichText::new(l.games.to_string()).color(SOFT));
                ui.label(RichText::new(l.wins.to_string()).color(SOFT));
                ui.end_row();
            }
        });
    }

    fn me_page(&mut self, ui: &mut egui::Ui, platform: &mut Platform) {
        let Some(me) = self.lobby.as_ref().map(|l| l.me.player_id.clone()) else {
            ui.spinner();
            return;
        };
        if self.profile_at.is_none_or(|t| t.elapsed() > Duration::from_secs(15)) {
            self.profile_at = Some(Instant::now());
            self.fetch(Job::Profile { hub: platform.hub_address.clone(), id: me.clone() });
        }
        let Some(profile) = self.profile.clone() else {
            ui.spinner();
            return;
        };
        egui::Grid::new("chess-my-ratings").striped(true).spacing(Vec2::new(18.0, 6.0)).show(ui, |ui| {
            for h in ["", "Rating", "Best", "Won", "Drawn", "Lost"] {
                ui.label(RichText::new(h).small().color(SOFT));
            }
            ui.end_row();
            for r in &profile.ratings {
                ui.label(
                    RichText::new(Category::from_code(&r.category).map_or("", |c| c.name()))
                        .color(theme::TEXT),
                );
                ui.label(
                    RichText::new(r.rating.to_string()).font(theme::display_bold(15.0)).color(theme::HEADING),
                );
                ui.label(r.best.to_string());
                ui.label(r.wins.to_string());
                ui.label(r.draws.to_string());
                ui.label(r.losses.to_string());
                ui.end_row();
            }
        });
        ui.add_space(10.0);
        theme::label_caps(ui, "Recent games");
        if profile.recent.is_empty() {
            ui.label(RichText::new("No finished online games yet.").color(SOFT));
        }
        let mut open = None;
        for g in &profile.recent {
            let white = g.white.player_id == me;
            let result = match (g.winner.as_deref(), white) {
                (Some("draw"), _) => ("Draw", theme::ACCENT_DEEP),
                (Some("white"), true) | (Some("black"), false) => ("Won", theme::GREEN_DEEP),
                _ => ("Lost", theme::RED),
            };
            let other = if white { &g.black } else { &g.white };
            ui.horizontal(|ui| {
                ui.label(RichText::new(result.0).font(theme::display_bold(14.0)).color(result.1));
                ui.label(
                    RichText::new(format!(
                        "vs {} ({}) · {} · {} moves · {}",
                        other.name,
                        other.rating,
                        g.control,
                        g.moves.div_ceil(2),
                        Reason::from_code(g.reason.as_deref().unwrap_or("")).map_or("", |r| r.describe())
                    ))
                    .color(theme::TEXT),
                );
                if theme::tool_button(ui, icon::EYE, "Review", false).clicked() {
                    open = Some(g.id.clone());
                }
            });
        }
        if let Some(id) = open {
            self.closed_games.retain(|c| *c != id);
            // Fetched like a watched game, then shown from your side.
            let mine = profile
                .recent
                .iter()
                .find(|g| g.id == id)
                .map(|g| if g.white.player_id == me { Color::White } else { Color::Black });
            let placeholder = ChessGame {
                id: id.clone(),
                white: Default::default(),
                black: Default::default(),
                control: String::new(),
                category: String::new(),
                rated: false,
                moves: String::new(),
                white_ms: 0,
                black_ms: 0,
                clock_running: false,
                first_move_ms: None,
                status: "over".into(),
                winner: None,
                reason: None,
                draw_offer: None,
                rematch_offer: None,
                rematch_game: None,
                white_change: None,
                black_change: None,
                white_away_secs: 0,
                black_away_secs: 0,
                chat: Vec::new(),
                created: 0,
                ended: None,
            };
            self.open_online(placeholder, mine);
            self.lobby_at = None;
        }
    }

    /// A pop-up about a new challenge while another tab is open. Returns
    /// true when the player wants to go to Chess.
    pub fn toast(&mut self, ctx: &egui::Context, platform: &mut Platform) -> bool {
        let Some(lobby) = &self.lobby else { return false };
        let Some(c) = lobby.incoming.iter().find(|c| !self.toasted.contains(&c.id)).cloned() else {
            return false;
        };
        let mut go = false;
        let mut answer = None;
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("chess-toast"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(screen.max.x - 360.0, screen.max.y - 130.0))
            .show(ctx, |ui| {
                theme::panel().show(ui, |ui| {
                    ui.set_width(310.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon::HORSE).size(22.0).color(theme::ACCENT));
                        ui.label(
                            RichText::new(format!("{} challenges you to chess", c.from.name))
                                .font(theme::display(15.0))
                                .color(theme::TEXT),
                        );
                    });
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            c.control,
                            if c.rated { "rated" } else { "casual" }
                        ))
                        .color(SOFT),
                    );
                    ui.horizontal(|ui| {
                        if theme::block_button(ui, theme::Kind::Go, icon::CHECK, "Accept", 28.0).clicked() {
                            answer = Some(true);
                        }
                        if theme::tool_button(ui, icon::X, "Decline", false).clicked() {
                            answer = Some(false);
                        }
                        if theme::tool_button(ui, "", "Later", false).clicked() {
                            self.toasted.push(c.id.clone());
                        }
                    });
                });
            });
        if let Some(accept) = answer {
            self.toasted.push(c.id.clone());
            self.send(platform, ChessAction::AnswerChallenge { id: c.id, accept });
            self.lobby_at = None;
            go = accept;
        }
        go
    }
}

fn quick_tile(ui: &mut egui::Ui, code: &str, kind: &str, waiting: u32, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(74.0, 62.0), Sense::click());
    let painter = ui.painter();
    let tint = if on {
        Color32::from_rgb(70, 160, 236)
    } else if response.hovered() {
        theme::see_through(200, 232, 255, 230)
    } else {
        theme::see_through(236, 246, 255, 220)
    };
    theme::glass(painter, rect, 9.0, tint);
    let ink = if on { Color32::WHITE } else { theme::HEADING };
    painter.text(
        rect.center() - Vec2::new(0.0, 9.0),
        egui::Align2::CENTER_CENTER,
        code,
        theme::display_bold(19.0),
        ink,
    );
    painter.text(
        rect.center() + Vec2::new(0.0, 12.0),
        egui::Align2::CENTER_CENTER,
        kind,
        egui::FontId::proportional(11.5),
        if on { Color32::WHITE } else { SOFT },
    );
    if waiting > 0 && !on {
        theme::orb(painter, rect.right_top() + Vec2::new(-9.0, 9.0), 5.0, theme::GREEN);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn bot_tile(ui: &mut egui::Ui, name: &str, rating: i32, level: usize, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(92.0, 92.0), Sense::click());
    let painter = ui.painter();
    let hue = [
        Color32::from_rgb(120, 210, 250),
        Color32::from_rgb(110, 220, 200),
        Color32::from_rgb(150, 210, 120),
        Color32::from_rgb(250, 160, 140),
        Color32::from_rgb(170, 140, 240),
        Color32::from_rgb(120, 180, 230),
        Color32::from_rgb(90, 110, 170),
        Color32::from_rgb(240, 190, 60),
    ][level % 8];
    let tint = if on {
        Color32::from_rgb(214, 236, 255)
    } else if response.hovered() {
        Color32::from_rgb(234, 246, 255)
    } else {
        Color32::from_rgb(248, 251, 255)
    };
    theme::glass(painter, rect, 10.0, tint);
    if on {
        painter.rect_stroke(rect, 10.0, Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    theme::orb(painter, rect.center_top() + Vec2::new(0.0, 30.0), 19.0, hue);
    painter.text(
        rect.center_top() + Vec2::new(0.0, 30.0),
        egui::Align2::CENTER_CENTER,
        icon::ROBOT,
        egui::FontId::proportional(20.0),
        Color32::WHITE,
    );
    painter.text(
        rect.center_top() + Vec2::new(0.0, 62.0),
        egui::Align2::CENTER_CENTER,
        name,
        theme::display(14.0),
        theme::TEXT,
    );
    painter.text(
        rect.center_top() + Vec2::new(0.0, 79.0),
        egui::Align2::CENTER_CENTER,
        rating.to_string(),
        egui::FontId::proportional(12.0),
        SOFT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
