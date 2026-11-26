//! Game servers the Hub runs itself, so pressing Play puts players on a
//! server on the Hub's own computer instead of someone's home computer.
//!
//! A player's app asks `POST /play` with the game's id and a ticket (a
//! random word it keeps while it waits). The Hub:
//!
//! 1. puts them on the fullest of its servers for that game that still has
//!    room (counting players already sent there but not yet in), or on one
//!    that is still starting;
//! 2. otherwise starts a new one (`playlibre server --game <id> …`) and
//!    answers "wait"; the app asks again every second or two until the new
//!    server has its join code;
//! 3. starts the next server early when one fills up, so the next player
//!    doesn't wait.
//!
//! Each server stops on its own once it has been empty for a while, and the
//! Hub never runs more than its limit at once. Servers run as children of
//! the Hub, each with its own identity key and UDP port, and announce
//! themselves on the Hub's list like any other server; the Hub trusts them
//! to list as Official. They outlive a restart of the Hub (it finds them
//! again on its list).
//!
//! Players can still host a server themselves; this only decides where
//! Play sends them by default.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// How a Hub runs game servers of its own.
#[derive(Debug, Clone)]
pub struct FleetOptions {
    /// The PlayLibre app (`playlibre`), run as `playlibre server` for each one.
    pub program: PathBuf,
    /// Where servers keep their identity keys and logs.
    pub dir: PathBuf,
    /// The most servers running at once.
    pub max_servers: u32,
    /// Players on each server.
    pub players_per_server: u32,
    /// The first of the UDP ports servers use (one each, counting up, as
    /// many as `max_servers`). Open them in the firewall so players connect
    /// straight to the server rather than through a relay.
    pub first_udp_port: u16,
    /// A server with nobody in it stops after this long.
    pub stop_when_empty: Duration,
    /// The address servers announce themselves to: this Hub, as seen from
    /// its own computer. Filled in when the Hub starts listening.
    pub hub_url: String,
}

impl FleetOptions {
    /// The usual settings, running `program` and keeping files in `dir`.
    pub fn new(program: PathBuf, dir: PathBuf) -> Self {
        Self {
            program,
            dir,
            max_servers: 8,
            players_per_server: 12,
            first_udp_port: 7500,
            stop_when_empty: Duration::from_secs(120),
            hub_url: String::new(),
        }
    }
}

/// What the app sends to `POST /play`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PlayRequest {
    /// A published game's id, or `starter:<name>`.
    pub game_id: String,
    /// Any random word of 8 to 64 letters and digits, the same each time
    /// the app asks for the same press of Play.
    pub ticket: String,
    #[serde(default)]
    pub player_id: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    /// One of the game's places (`game.PlaceId`), when it isn't the start place.
    #[serde(default)]
    pub place_id: Option<i64>,
    /// A server of its own: a private server's code, or a code a game got
    /// from `TeleportService:ReserveServer`. Only players sent with the
    /// code play there, and it isn't on the list.
    #[serde(default)]
    pub access_code: Option<String>,
}

pub fn play_message(player_id: &str, ticket: &str) -> Vec<u8> {
    format!("libroxel play\n{player_id}\n{ticket}").into_bytes()
}

/// What `POST /play` answers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PlayAnswer {
    /// The join code of the server to join, once it is ready.
    #[serde(default)]
    pub code: Option<String>,
    /// That server's id.
    #[serde(default)]
    pub server_id: Option<String>,
    /// A server is starting: ask again in a second or two.
    #[serde(default)]
    pub wait: bool,
}

/// A server that has been started but hasn't announced itself yet is given
/// up on after this long.
const START_TIMEOUT: Duration = Duration::from_secs(90);

/// Players sent to a server count against its room for this long, until
/// its own heartbeat counts them.
const TICKET_HOLD: Duration = Duration::from_secs(30);

/// After a server for a game fails to start, Play for that game says so
/// for this long instead of trying again and again.
const FAILED_FOR: Duration = Duration::from_secs(60);

/// Servers the Hub started, and players on their way to them.
pub(crate) struct Fleet {
    pub(crate) options: FleetOptions,
    state: std::sync::Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Servers started by this run of the Hub, by id.
    children: HashMap<String, Running>,
    /// Ticket → (server, when it was handed out).
    tickets: HashMap<String, (String, Instant)>,
    /// Games whose last server failed to start, and when.
    failed: HashMap<String, Instant>,
}

struct Running {
    child: Child,
    game_id: String,
    started: Instant,
    /// Whether it has announced itself on the list yet.
    seen: bool,
}

/// The table of servers the Hub runs. It exists in every Hub's database, so
/// the list can say which servers are the Hub's own.
pub(crate) fn create_tables(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS fleet (
            server_id   TEXT PRIMARY KEY,
            game_id     TEXT NOT NULL,
            udp_port    INTEGER NOT NULL,
            started     INTEGER NOT NULL
        );",
    )
    .map_err(|e| format!("could not set up the Hub's game server table: {e}"))
}

/// Whether `server_id` is one of the servers this Hub runs.
pub(crate) fn is_fleet(db: &Connection, server_id: &str) -> bool {
    db.query_row("SELECT 1 FROM fleet WHERE server_id = ?1", params![server_id], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

/// What a server of the fleet runs: a game, and maybe one of its other
/// places (`#p<number>`) and a server of its own (`@<code>`).
pub(crate) fn target(game_id: &str, place_id: Option<i64>, code: Option<&str>) -> String {
    let mut t = game_id.to_owned();
    if let Some(p) = place_id {
        t.push_str(&format!("#p{p}"));
    }
    if let Some(c) = code {
        t.push('@');
        t.push_str(c);
    }
    t
}

/// The game a fleet target runs.
pub(crate) fn game_of(target: &str) -> &str {
    target.split(['#', '@']).next().unwrap_or(target)
}

/// Whether `server_id` is a fleet server running `game_id` (any of its
/// places, on any server of its own).
pub(crate) fn runs(db: &Connection, server_id: &str, game_id: &str) -> bool {
    db.query_row(
        "SELECT 1 FROM fleet WHERE server_id = ?1
             AND (game_id = ?2 OR substr(game_id, 1, length(?2) + 1) IN (?2 || '#', ?2 || '@'))",
        params![server_id, game_id],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

/// Whether `game_id` names a game the Hub can run: a starter game, or a
/// published one that hasn't been taken down.
fn runnable(db: &Connection, game_id: &str) -> bool {
    if let Some(name) = game_id.strip_prefix("starter:") {
        return !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_alphanumeric() || c == ' ');
    }
    db.query_row(
        &format!(
            "SELECT 1 FROM games WHERE game_id = ?1 AND private = 0 AND NOT {}",
            crate::hidden_sql("game", "game_id")
        ),
        params![game_id],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

/// One of the Hub's servers as the list sees it.
struct Known {
    server_id: String,
    game_id: String,
    udp_port: u16,
    started: i64,
    /// From its last heartbeat, if it has announced itself.
    live: Option<(String, u32, u32)>,
}

fn known(db: &Connection, now: i64) -> Vec<Known> {
    let Ok(mut statement) = db.prepare(
        "SELECT f.server_id, f.game_id, f.udp_port, f.started, s.code, s.players, s.max_players
         FROM fleet f LEFT JOIN servers s ON s.server_id = f.server_id AND s.last_seen >= ?1",
    ) else {
        return Vec::new();
    };
    statement
        .query_map(params![now - crate::EXPIRES_AFTER_SECS], |r| {
            let code: Option<String> = r.get(4)?;
            Ok(Known {
                server_id: r.get(0)?,
                game_id: r.get(1)?,
                udp_port: r.get::<_, i64>(2)? as u16,
                started: r.get(3)?,
                live: match code {
                    Some(code) => Some((code, r.get::<_, i64>(5)? as u32, r.get::<_, i64>(6)? as u32)),
                    None => None,
                },
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

/// How a new server runs its game.
struct Run<'a> {
    place_id: Option<i64>,
    code: Option<&'a str>,
    private_owner: Option<&'a str>,
}

pub(crate) enum PlayError {
    BadRequest(&'static str),
    NotAllowed,
    UnknownGame,
    Busy,
    Failed,
}

impl Fleet {
    pub(crate) fn new(options: FleetOptions) -> Self {
        Self { options, state: Default::default() }
    }

    /// Finds or starts a server of `request.game_id` for one player, who
    /// proved they are `who` (if they did).
    pub(crate) fn play(
        &self,
        db: &Connection,
        request: &PlayRequest,
        who: Option<&str>,
    ) -> Result<PlayAnswer, PlayError> {
        let ticket = request.ticket.trim();
        if !(8..=64).contains(&ticket.len()) || !ticket.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(PlayError::BadRequest("a ticket is 8 to 64 letters and digits"));
        }
        let game_id = request.game_id.trim();
        if !runnable(db, game_id) {
            return Err(PlayError::UnknownGame);
        }
        let place_id = match request.place_id {
            Some(p) if crate::places::start_place(db, game_id) != Some(p) => {
                if crate::places::place(db, p).is_none_or(|found| found.game_id != game_id) {
                    return Err(PlayError::BadRequest("that game has no such place"));
                }
                Some(p)
            }
            _ => None,
        };
        let code = request.access_code.as_deref().map(str::trim).filter(|c| !c.is_empty());
        let mut private_owner = None;
        if let Some(c) = code {
            if !(8..=64).contains(&c.len()) || !c.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(PlayError::BadRequest("a server's code is 8 to 64 letters and digits"));
            }
            if let Some((private_game, owner)) = crate::places::private_owner(db, c) {
                if private_game != game_id {
                    return Err(PlayError::BadRequest("that private server is for another game"));
                }
                if !crate::places::may_join(db, c, who) {
                    return Err(PlayError::NotAllowed);
                }
                private_owner = Some(owner);
            }
        }
        let target = target(game_id, place_id, code);
        let game_id = target.as_str();
        let run = Run { place_id, code, private_owner: private_owner.as_deref() };
        let now = crate::now();
        let mut state = self.state.lock().unwrap();
        state.tickets.retain(|_, (_, at)| at.elapsed() < TICKET_HOLD);
        let servers = known(db, now);
        let starting = |k: &Known| k.live.is_none() && now - k.started < START_TIMEOUT.as_secs() as i64;

        // Asked before: the same server, if it's still there.
        if let Some((server_id, _)) = state.tickets.get(ticket).cloned() {
            match servers.iter().find(|k| k.server_id == server_id) {
                Some(Known { live: Some((code, ..)), .. }) => {
                    return Ok(PlayAnswer {
                        code: Some(code.clone()),
                        server_id: Some(server_id),
                        wait: false,
                    });
                }
                Some(k) if starting(k) => return Ok(PlayAnswer { wait: true, ..Default::default() }),
                _ => {
                    state.tickets.remove(ticket);
                }
            }
        }

        // Players already on their way to each server.
        let mut coming: HashMap<&str, u32> = HashMap::new();
        for (server_id, _) in state.tickets.values() {
            *coming.entry(server_id.as_str()).or_default() += 1;
        }
        let room = self.options.players_per_server.max(1);
        let ours = servers.iter().filter(|k| k.game_id == game_id);
        // The fullest live server with room, so servers fill up before new ones start.
        let live = ours
            .clone()
            .filter_map(|k| {
                let (code, players, max) = k.live.as_ref()?;
                let max = if *max == 0 { room } else { *max };
                let filled = players + coming.get(k.server_id.as_str()).copied().unwrap_or(0);
                (filled < max).then_some((k, code.clone(), filled, max))
            })
            .max_by_key(|(_, _, filled, _)| *filled);
        if let Some((k, code, filled, max)) = live {
            let server_id = k.server_id.clone();
            state.tickets.insert(ticket.to_owned(), (server_id.clone(), Instant::now()));
            // That one is now full: start the next one before anyone needs it.
            if filled + 1 >= max && !ours.clone().any(starting) {
                let _ = self.start(db, &mut state, &servers, game_id, &run);
            }
            return Ok(PlayAnswer { code: Some(code), server_id: Some(server_id), wait: false });
        }
        // One that is starting, with room.
        if let Some(k) = ours
            .clone()
            .filter(|k| starting(k))
            .find(|k| coming.get(k.server_id.as_str()).copied().unwrap_or(0) < room)
        {
            state.tickets.insert(ticket.to_owned(), (k.server_id.clone(), Instant::now()));
            return Ok(PlayAnswer { wait: true, ..Default::default() });
        }
        if state.failed.get(game_id).is_some_and(|at| at.elapsed() < FAILED_FOR) {
            return Err(PlayError::Failed);
        }
        let server_id = self.start(db, &mut state, &servers, game_id, &run)?;
        state.tickets.insert(ticket.to_owned(), (server_id, Instant::now()));
        Ok(PlayAnswer { wait: true, ..Default::default() })
    }

    /// Starts a new server of `game_id`, if there is room for one.
    fn start(
        &self,
        db: &Connection,
        state: &mut State,
        servers: &[Known],
        game_id: &str,
        run: &Run,
    ) -> Result<String, PlayError> {
        let o = &self.options;
        let now = crate::now();
        let alive =
            servers.iter().filter(|k| k.live.is_some() || now - k.started < START_TIMEOUT.as_secs() as i64);
        if alive.count() as u32 >= o.max_servers {
            return Err(PlayError::Busy);
        }
        let used: Vec<u16> = servers.iter().map(|k| k.udp_port).collect();
        let port = (0..o.max_servers.max(1) as u16 * 2)
            .map(|i| o.first_udp_port.saturating_add(i))
            .find(|p| !used.contains(p))
            .ok_or(PlayError::Busy)?;
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|_| PlayError::Failed)?;
        let server_id = crate::player_id_of(&key);
        let start = || -> std::io::Result<Child> {
            std::fs::create_dir_all(&o.dir)?;
            let key_file = o.dir.join(format!("{server_id}.key"));
            write_private(&key_file, &key)?;
            let log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(o.dir.join(format!("{}.log", log_name(game_of(game_id)))))?;
            let mut command = Command::new(&o.program);
            command.arg("server");
            if let Some(p) = run.place_id {
                command.args(["--place", &p.to_string()]);
            }
            if let Some(c) = run.code {
                command.args(["--access-code", c]);
            }
            if let Some(owner) = run.private_owner {
                command.args(["--private-owner", owner]);
            }
            command
                .args(["--game", game_of(game_id)])
                .arg("--key")
                .arg(&key_file)
                .args(["--port", "0"])
                .args(["--udp-port", &port.to_string()])
                .args(["--max-players", &o.players_per_server.to_string()])
                .args(["--stop-when-empty", &o.stop_when_empty.as_secs().to_string()])
                .arg("--official")
                .env("PLAYLIBRE_HUB", &o.hub_url)
                // A game server built before the rename reads this name.
                .env("LIBROXEL_HUB", &o.hub_url)
                .stdin(Stdio::null())
                .stdout(log.try_clone()?)
                .stderr(log)
                .spawn()
        };
        match start() {
            Ok(child) => {
                let _ = db.execute(
                    "INSERT OR REPLACE INTO fleet (server_id, game_id, udp_port, started) VALUES (?1, ?2, ?3, ?4)",
                    params![server_id, game_id, port, now],
                );
                println!("Starting a server of {game_id} ({server_id}) on UDP port {port}");
                state.children.insert(
                    server_id.clone(),
                    Running { child, game_id: game_id.to_owned(), started: Instant::now(), seen: false },
                );
                Ok(server_id)
            }
            Err(e) => {
                eprintln!("could not start a server of {game_id} with {}: {e}", o.program.display());
                let _ = std::fs::remove_file(o.dir.join(format!("{server_id}.key")));
                state.failed.insert(game_id.to_owned(), Instant::now());
                Err(PlayError::Failed)
            }
        }
    }

    /// Notices servers that stopped, stops ones that never came up, and
    /// forgets servers from an earlier run of the Hub that are gone. Run it
    /// every few seconds.
    pub(crate) fn tidy(&self, db: &Connection) {
        let now = crate::now();
        let servers = known(db, now);
        let mut state = self.state.lock().unwrap();
        let mut gone = Vec::new();
        for (server_id, running) in &mut state.children {
            let live = servers.iter().any(|k| k.server_id == *server_id && k.live.is_some());
            running.seen |= live;
            match running.child.try_wait() {
                Ok(Some(status)) => {
                    println!("Server {server_id} of {} stopped ({status})", running.game_id);
                    gone.push((server_id.clone(), running.game_id.clone(), running.seen));
                }
                Ok(None) if !running.seen && running.started.elapsed() > START_TIMEOUT => {
                    eprintln!("Server {server_id} of {} never came up; stopping it", running.game_id);
                    let _ = running.child.kill();
                }
                _ => {}
            }
        }
        for (server_id, game_id, seen) in gone {
            state.children.remove(&server_id);
            if !seen {
                state.failed.insert(game_id, Instant::now());
            }
            self.forget(db, &server_id);
        }
        // Servers from before the Hub last restarted are known only by their heartbeats.
        for k in &servers {
            let ours_now = state.children.contains_key(&k.server_id);
            if !ours_now && k.live.is_none() && now - k.started > START_TIMEOUT.as_secs() as i64 {
                self.forget(db, &k.server_id);
            }
        }
    }

    fn forget(&self, db: &Connection, server_id: &str) {
        let _ = db.execute("DELETE FROM fleet WHERE server_id = ?1", params![server_id]);
        let _ = std::fs::remove_file(self.options.dir.join(format!("{server_id}.key")));
    }
}

/// A file name for a game's log: its id, with anything odd made `_`.
fn log_name(game_id: &str) -> String {
    game_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

/// Writes a secret only the Hub's own user can read.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}
