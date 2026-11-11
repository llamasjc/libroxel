//! The Libroxel Hub: one small server that lists the games being played
//! right now, so players can find one and join it with its join code.
//!
//! The Hub never runs a game or carries its traffic: hosts announce
//! themselves every few seconds with a [`Heartbeat`], and a host that stops
//! announcing drops off the list. Anyone can run a Hub (`libroxel hub`);
//! it keeps its data in one SQLite file.
//!
//! [`HubClient`] is the blocking client the editor and servers use.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// The port a Hub listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7430;

/// How often hosts should announce themselves.
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(15);

/// A host missing for this long is taken off the list.
const EXPIRES_AFTER_SECS: i64 = 45;

/// The longest name, game title or code the Hub keeps.
const MAX_TEXT: usize = 600;

// ---------------------------------------------------------------------------
// What goes over the wire
// ---------------------------------------------------------------------------

/// What kind of computer hosts a game. The list shows community and player
/// hosts first; official servers are the last resort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    /// A player hosting from the editor while they play.
    #[default]
    Player,
    /// A windowless server someone runs for their community.
    Community,
    /// A server run by whoever runs this Hub.
    Official,
}

impl HostKind {
    fn rank(self) -> i64 {
        match self {
            HostKind::Community => 0,
            HostKind::Player => 1,
            HostKind::Official => 2,
        }
    }

    fn from_rank(rank: i64) -> Self {
        match rank {
            0 => HostKind::Community,
            2 => HostKind::Official,
            _ => HostKind::Player,
        }
    }

    /// A short word for lists.
    pub fn label(self) -> &'static str {
        match self {
            HostKind::Player => "Player",
            HostKind::Community => "Community",
            HostKind::Official => "Official",
        }
    }
}

/// A host announcing itself. The first heartbeat for a `server_id` claims
/// it; later ones must carry the same `secret`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Heartbeat {
    pub server_id: String,
    pub secret: String,
    /// The name shown in the list, such as "Kai's obby".
    pub name: String,
    /// The game's title.
    pub game: String,
    /// The join code players paste into Join.
    pub code: String,
    pub players: u32,
    /// 0 means no limit.
    pub max_players: u32,
    pub kind: HostKind,
}

/// A live server as the list shows it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerInfo {
    pub server_id: String,
    pub name: String,
    pub game: String,
    pub code: String,
    pub players: u32,
    pub max_players: u32,
    pub kind: HostKind,
    /// Seconds since the host last announced itself.
    pub seen_secs_ago: i64,
}

/// What `GET /` answers, so a client can check it reached a Hub.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HubStatus {
    pub hub: String,
    pub version: String,
    pub servers: u32,
    pub players: u32,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Secret {
    secret: String,
}

// ---------------------------------------------------------------------------
// The server
// ---------------------------------------------------------------------------

type Db = Arc<Mutex<Connection>>;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn clip(text: &str) -> String {
    text.trim().chars().take(MAX_TEXT).collect()
}

/// Opens (or makes) the Hub's database. `None` keeps it in memory.
pub fn open_db(path: Option<&Path>) -> Result<Connection, String> {
    let db = match path {
        Some(path) => {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
            }
            Connection::open(path)
        }
        None => Connection::open_in_memory(),
    }
    .map_err(|e| format!("could not open the Hub database: {e}"))?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS servers (
            server_id   TEXT PRIMARY KEY,
            secret      TEXT NOT NULL,
            name        TEXT NOT NULL,
            game        TEXT NOT NULL,
            code        TEXT NOT NULL,
            players     INTEGER NOT NULL,
            max_players INTEGER NOT NULL,
            kind        INTEGER NOT NULL,
            first_seen  INTEGER NOT NULL,
            last_seen   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS servers_last_seen ON servers(last_seen);",
    )
    .map_err(|e| format!("could not set up the Hub database: {e}"))?;
    Ok(db)
}

/// The Hub's routes over `db`.
pub fn router(db: Connection) -> Router {
    let db: Db = Arc::new(Mutex::new(db));
    Router::new()
        .route("/", get(status))
        .route("/servers", get(list_servers))
        .route("/servers/heartbeat", post(heartbeat))
        .route("/servers/{id}", delete(remove_server))
        .with_state(db)
}

async fn status(State(db): State<Db>) -> Json<HubStatus> {
    let db = db.lock().unwrap();
    let (servers, players): (i64, i64) = db
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(players), 0) FROM servers WHERE last_seen >= ?1",
            params![now() - EXPIRES_AFTER_SECS],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap_or((0, 0));
    Json(HubStatus {
        hub: "libroxel".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        servers: servers as u32,
        players: players as u32,
    })
}

async fn list_servers(State(db): State<Db>, Query(query): Query<ListQuery>) -> Json<Vec<ServerInfo>> {
    let db = db.lock().unwrap();
    let now = now();
    let _ = db.execute("DELETE FROM servers WHERE last_seen < ?1", params![now - EXPIRES_AFTER_SECS]);
    let pattern = format!("%{}%", query.q.unwrap_or_default().trim().replace(['%', '_'], ""));
    let mut statement = db
        .prepare(
            "SELECT server_id, name, game, code, players, max_players, kind, last_seen FROM servers
             WHERE name LIKE ?1 OR game LIKE ?1
             ORDER BY kind ASC, players DESC, first_seen ASC LIMIT 200",
        )
        .expect("the list query is valid");
    let rows = statement
        .query_map(params![pattern], |r| {
            Ok(ServerInfo {
                server_id: r.get(0)?,
                name: r.get(1)?,
                game: r.get(2)?,
                code: r.get(3)?,
                players: r.get::<_, i64>(4)? as u32,
                max_players: r.get::<_, i64>(5)? as u32,
                kind: HostKind::from_rank(r.get(6)?),
                seen_secs_ago: now - r.get::<_, i64>(7)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default();
    Json(rows)
}

async fn heartbeat(
    State(db): State<Db>,
    Json(beat): Json<Heartbeat>,
) -> Result<StatusCode, (StatusCode, String)> {
    let bad = |why: &str| Err((StatusCode::BAD_REQUEST, why.to_owned()));
    if beat.server_id.trim().is_empty() || beat.secret.len() < 16 {
        return bad("a heartbeat needs a server_id and a secret of 16 or more characters");
    }
    if beat.code.trim().is_empty() {
        return bad("a heartbeat needs a join code");
    }
    let db = db.lock().unwrap();
    let now = now();
    let owner: Option<String> = db
        .query_row("SELECT secret FROM servers WHERE server_id = ?1", params![beat.server_id], |r| r.get(0))
        .optional()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if owner.is_some_and(|s| s != beat.secret) {
        return Err((StatusCode::FORBIDDEN, "another host owns that server_id".into()));
    }
    let name = if beat.name.trim().is_empty() { "Unnamed server".to_owned() } else { clip(&beat.name) };
    let game = if beat.game.trim().is_empty() { "Untitled game".to_owned() } else { clip(&beat.game) };
    db.execute(
        "INSERT INTO servers (server_id, secret, name, game, code, players, max_players, kind, first_seen, last_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(server_id) DO UPDATE SET name = ?3, game = ?4, code = ?5, players = ?6,
             max_players = ?7, kind = ?8, last_seen = ?9",
        params![
            clip(&beat.server_id),
            beat.secret,
            name,
            game,
            clip(&beat.code),
            beat.players,
            beat.max_players,
            beat.kind.rank(),
            now
        ],
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_server(
    State(db): State<Db>,
    UrlPath(id): UrlPath<String>,
    Json(secret): Json<Secret>,
) -> StatusCode {
    let db = db.lock().unwrap();
    match db.execute("DELETE FROM servers WHERE server_id = ?1 AND secret = ?2", params![id, secret.secret]) {
        Ok(0) => StatusCode::NOT_FOUND,
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// A Hub running in the background. Dropping it stops the Hub.
pub struct RunningHub {
    addr: SocketAddr,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl RunningHub {
    /// Where the Hub listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The address clients use, such as `http://127.0.0.1:7430`.
    pub fn url(&self) -> String {
        let host =
            if self.addr.ip().is_unspecified() { "127.0.0.1".to_owned() } else { self.addr.ip().to_string() };
        format!("http://{host}:{}", self.addr.port())
    }
}

/// Starts a Hub on `addr` in a background thread (port 0 picks a free one).
pub fn start(addr: SocketAddr, db_path: Option<&Path>) -> Result<RunningHub, String> {
    let db = open_db(db_path)?;
    let listener =
        std::net::TcpListener::bind(addr).map_err(|e| format!("could not start the Hub on {addr}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("libroxel-hub")
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    std::thread::Builder::new()
        .name("libroxel-hub".into())
        .spawn(move || {
            runtime.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else { return };
                let _ = axum::serve(listener, router(db))
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await;
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(RunningHub { addr, _stop: stop })
}

/// Runs a Hub on `addr` until Ctrl+C, printing what it does.
pub fn run(addr: SocketAddr, db_path: &Path) -> Result<(), String> {
    let db = open_db(Some(db_path))?;
    let runtime =
        tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("could not start the Hub on {addr}: {e}"))?;
        println!("Libroxel Hub listening on http://{addr} (data in {})", db_path.display());
        println!(
            "Point players at it with the Hub address in Browse, or LIBROXEL_HUB=http://<this computer>:{}",
            addr.port()
        );
        axum::serve(listener, router(db))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
                println!("Hub stopped.");
            })
            .await
            .map_err(|e| e.to_string())
    })
}

// ---------------------------------------------------------------------------
// The client
// ---------------------------------------------------------------------------

/// Talks to one Hub. Every call blocks, so call it off the UI thread.
#[derive(Clone)]
pub struct HubClient {
    base: String,
    agent: ureq::Agent,
}

impl HubClient {
    /// A client for the Hub at `url` (`http://host:port`; a bare host gets
    /// `http://` and the default port).
    pub fn new(url: &str) -> Self {
        let url = url.trim().trim_end_matches('/');
        let url = if url.contains("://") { url.to_owned() } else { format!("http://{url}") };
        let has_port = url.rsplit_once(':').is_some_and(|(head, port)| {
            !head.ends_with("http") && !head.ends_with("https") && port.parse::<u16>().is_ok()
        });
        let base = if has_port { url } else { format!("{url}:{DEFAULT_PORT}") };
        let local = base.contains("://127.") || base.contains("://localhost") || base.contains("://[::1]");
        let mut config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .http_status_as_error(true);
        if local {
            // A Hub on this computer is never behind a proxy.
            config = config.proxy(None);
        }
        Self { base, agent: config.build().into() }
    }

    /// The Hub's address.
    pub fn url(&self) -> &str {
        &self.base
    }

    /// Whether a Hub answers, and how busy it is.
    pub fn status(&self) -> Result<HubStatus, String> {
        self.agent
            .get(&self.base)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub at {}: {e}", self.base))
    }

    /// Live servers, community hosts first, matching `search` if given.
    pub fn servers(&self, search: &str) -> Result<Vec<ServerInfo>, String> {
        self.agent
            .get(format!("{}/servers", self.base))
            .query("q", search)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| format!("could not reach the Hub at {}: {e}", self.base))
    }

    /// Announces a host; repeat every [`HEARTBEAT_EVERY`].
    pub fn heartbeat(&self, beat: &Heartbeat) -> Result<(), String> {
        self.agent
            .post(format!("{}/servers/heartbeat", self.base))
            .send_json(beat)
            .map(|_| ())
            .map_err(|e| format!("the Hub at {} did not take the announcement: {e}", self.base))
    }

    /// Takes a host off the list straight away.
    pub fn remove(&self, server_id: &str, secret: &str) -> Result<(), String> {
        self.agent
            .delete(format!("{}/servers/{server_id}", self.base))
            .force_send_body()
            .send_json(serde_json::json!({ "secret": secret }))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// A random secret for claiming a server's place on the list.
pub fn new_secret() -> String {
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes).expect("the system has a random number source");
    data_encoding::BASE32_NOPAD.encode(&bytes).to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// Announcing in the background
// ---------------------------------------------------------------------------

/// What an announcer reports about the game it announces.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub name: String,
    pub game: String,
    pub code: Option<String>,
    pub players: u32,
    pub max_players: u32,
    pub kind: HostKind,
}

/// Keeps a hosted game on a Hub's list from a background thread. Update the
/// shared [`Listing`] as players come and go; dropping the announcer takes
/// the game off the list.
pub struct Announcer {
    listing: Arc<Mutex<Listing>>,
    status: Arc<Mutex<Option<String>>>,
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Announcer {
    /// Starts announcing to `hub`. Nothing is sent until the listing has a
    /// join code.
    pub fn start(hub: HubClient, server_id: String, listing: Listing) -> Self {
        let listing = Arc::new(Mutex::new(listing));
        let status = Arc::new(Mutex::new(None));
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let shared = (listing.clone(), status.clone());
        let thread = std::thread::Builder::new()
            .name("libroxel-announce".into())
            .spawn(move || {
                let (listing, status) = shared;
                let secret = new_secret();
                let mut sent = false;
                loop {
                    let now = listing.lock().unwrap().clone();
                    let wait = if let Some(code) = now.code {
                        let beat = Heartbeat {
                            server_id: server_id.clone(),
                            secret: secret.clone(),
                            name: now.name,
                            game: now.game,
                            code,
                            players: now.players,
                            max_players: now.max_players,
                            kind: now.kind,
                        };
                        let result = hub.heartbeat(&beat);
                        sent |= result.is_ok();
                        *status.lock().unwrap() = Some(match result {
                            Ok(()) => format!("Listed on {}", hub.url()),
                            Err(e) => e,
                        });
                        HEARTBEAT_EVERY
                    } else {
                        Duration::from_millis(500)
                    };
                    match stopped.recv_timeout(wait) {
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                }
                if sent {
                    let _ = hub.remove(&server_id, &secret);
                }
            })
            .ok();
        Self { listing, status, stop: Some(stop), thread }
    }

    /// Changes what the list shows; the next heartbeat carries it.
    pub fn update(&self, change: impl FnOnce(&mut Listing)) {
        change(&mut self.listing.lock().unwrap());
    }

    /// The last thing the Hub said, such as "Listed on http://…".
    pub fn status(&self) -> Option<String> {
        self.status.lock().unwrap().clone()
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        // The thread takes the game off the list on its own, so a slow Hub
        // never holds up whoever stops hosting.
        drop(self.stop.take());
        drop(self.thread.take());
    }
}
