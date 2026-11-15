//! `libroxel server` and `libroxel hub`: the windowless parts of the app.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use libroxel_engine::net::DEFAULT_PORT;
use libroxel_engine::{CharacterInput, LogLevel, PlaySession};
use libroxel_hub::{Announcer, HostKind, HubClient, Listing};
use libroxel_instance::place;
use libroxel_net::{HostTunnel, Identity, Network};

pub const HELP: &str = "\
Libroxel: build, play and host games.

usage:
  libroxel [game.lrx]          open the editor (and its built-in Hub)
  libroxel --sample            open the editor with the sample obby
  libroxel server <game.lrx>   host a game with no window
      --port <n>               game port on this network (default 7420)
      --name <text>            name shown in Browse (default: the file name)
      --hub <address>          list the game on that Hub (default: the official
                               Hub, or one run here if there is none)
      --no-hub                 do not run or use a Hub
      --official               list it as an official server (the last resort;
                               the Hub's owners must trust this server)
      --max-players <n>        players the list shows as the limit (0: none)
      --game-id <id>           put it on that game's server list (a published
                               game's id, or starter:<name>)
  libroxel hub                 run only a Hub
      --port <n>               Hub port (default 7430)
      --data <file>            where the Hub keeps its list
      --behind-proxy           a web server here (such as Caddy) passes players
                               on; trust it for their addresses
  libroxel hub reports         list reported games and hats (and which are hidden)
  libroxel hub clear <game|item> <id>    say a reported one is fine: list it again
  libroxel hub remove <game|item> <id>   take one down for good
  libroxel hub accounts [search]         list accounts
  libroxel hub owners                    list the Hub's owners
  libroxel hub owner add|remove <name or player id>
                               owners (and servers they add by id) may list
                               Official servers
  libroxel hub ban <name> [reason]       ban an account; unban <name> lets it back
  libroxel hub rename <name> <new name>  rename an account (reserved names such as
                               admin too)
  libroxel help                show this
";

/// Handles `server`, `hub` and `help`. Returns `false` when the arguments
/// are for the editor.
pub fn dispatch(args: &[String]) -> bool {
    let result = match args.first().map(String::as_str) {
        Some("server") => server(&args[1..]),
        Some("hub") => hub(&args[1..]),
        Some("help" | "--help" | "-h") => {
            print!("{HELP}");
            Ok(())
        }
        _ => return false,
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
    true
}

/// Pulls `--flag value` pairs and bare flags out of `args`.
struct Flags {
    rest: Vec<String>,
    pairs: Vec<(String, String)>,
    bare: Vec<String>,
}

impl Flags {
    fn parse(args: &[String], with_value: &[&str]) -> Result<Self, String> {
        let mut flags = Flags { rest: Vec::new(), pairs: Vec::new(), bare: Vec::new() };
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if with_value.contains(&arg.as_str()) {
                let value = args.get(i + 1).ok_or(format!("{arg} needs a value"))?;
                flags.pairs.push((arg.clone(), value.clone()));
                i += 2;
            } else if arg.starts_with("--") {
                flags.bare.push(arg.clone());
                i += 1;
            } else {
                flags.rest.push(arg.clone());
                i += 1;
            }
        }
        Ok(flags)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.pairs.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn has(&self, name: &str) -> bool {
        self.bare.iter().any(|b| b == name)
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        self.get(name)
            .map_or(Ok(default), |v| v.parse().map_err(|_| format!("{name} needs a number, not {v}")))
    }
}

/// Where the app keeps its settings and data.
pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|d| PathBuf::from(d).join("Libroxel"))
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|d| PathBuf::from(d).join("libroxel")))
        .or_else(|| std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".config/libroxel")))
        .unwrap_or_else(|| PathBuf::from(".libroxel"))
}

fn hub(args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(args, &["--port", "--data"])?;
    let options = libroxel_hub::HubOptions { behind_proxy: flags.has("--behind-proxy") };
    let port = flags.number("--port", libroxel_hub::DEFAULT_PORT)?;
    let data = flags.get("--data").map_or_else(|| data_dir().join("hub.sqlite"), PathBuf::from);
    let words: Vec<&str> = flags.rest.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => libroxel_hub::run(SocketAddr::from(([0, 0, 0, 0], port)), &data, options),
        ["reports"] => {
            let db = libroxel_hub::open_db(Some(&data))?;
            let reported = libroxel_hub::reports(&db);
            if reported.is_empty() {
                println!("Nothing has been reported.");
            }
            for r in reported {
                let state = if r.hidden {
                    "HIDDEN"
                } else if r.cleared {
                    "cleared"
                } else {
                    "listed"
                };
                let name = r.name.as_deref().unwrap_or("(gone)");
                println!("{} {} \"{name}\" [{state}], {} report(s):", r.kind, r.target, r.reasons.len());
                for reason in r.reasons {
                    println!("    - {reason}");
                }
            }
            Ok(())
        }
        ["clear", kind, id] => {
            libroxel_hub::clear_reports(&libroxel_hub::open_db(Some(&data))?, kind, id)?;
            println!("{kind} {id} is listed again.");
            Ok(())
        }
        ["remove", kind, id] => {
            libroxel_hub::take_down(&libroxel_hub::open_db(Some(&data))?, kind, id)?;
            println!("{kind} {id} is gone.");
            Ok(())
        }
        ["accounts", search @ ..] => {
            let rows = libroxel_hub::accounts(&libroxel_hub::open_db(Some(&data))?, &search.join(" "));
            if rows.is_empty() {
                println!("No accounts match.");
            }
            for a in rows {
                let mut notes = Vec::new();
                if a.owner {
                    notes.push("owner".to_owned());
                }
                if a.has_password {
                    notes.push("password".to_owned());
                }
                if let Some(reason) = &a.banned {
                    notes.push(format!("BANNED: {reason}"));
                }
                let days = a.joined_secs_ago / 86_400;
                println!("{:<20} {}  joined {days} days ago  {}", a.name, a.player_id, notes.join(", "));
            }
            Ok(())
        }
        ["owners"] => {
            let owners = libroxel_hub::owners(&libroxel_hub::open_db(Some(&data))?);
            if owners.is_empty() {
                println!("No owners yet. Add yourself: libroxel hub owner add <your name>");
            }
            for (id, name) in owners {
                println!("{:<20} {id}", name.as_deref().unwrap_or("(a server)"));
            }
            Ok(())
        }
        ["owner", "add", who] => {
            let id = libroxel_hub::add_owner(&libroxel_hub::open_db(Some(&data))?, who)?;
            println!("{who} ({id}) is an owner of this Hub.");
            Ok(())
        }
        ["owner", "remove", who] => {
            libroxel_hub::remove_owner(&libroxel_hub::open_db(Some(&data))?, who)?;
            println!("{who} is no longer an owner.");
            Ok(())
        }
        ["ban", who, reason @ ..] => {
            let reason = if reason.is_empty() { "no reason given".to_owned() } else { reason.join(" ") };
            libroxel_hub::ban(&libroxel_hub::open_db(Some(&data))?, who, &reason)?;
            println!("{who} is banned.");
            Ok(())
        }
        ["rename", who, name] => {
            libroxel_hub::rename(&libroxel_hub::open_db(Some(&data))?, who, name)?;
            println!("{who} is now called {name}.");
            Ok(())
        }
        ["unban", who] => {
            libroxel_hub::unban(&libroxel_hub::open_db(Some(&data))?, who)?;
            println!("{who} can use this Hub again.");
            Ok(())
        }
        _ => {
            Err("usage: libroxel hub [reports | clear <game|item> <id> | remove <game|item> <id> | accounts \
                  [search] | owners | owner add|remove <who> | ban <who> [reason] | unban <who> | rename <who> <name>]"
                .into())
        }
    }
}

fn server(args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(args, &["--port", "--name", "--hub", "--max-players", "--game-id"])?;
    let Some(path) = flags.rest.first() else {
        return Err("usage: libroxel server <game.lrx> (see libroxel help)".into());
    };
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {path}: {e}"))?;
    let game =
        Path::new(path).file_stem().map_or("Untitled game".into(), |s| s.to_string_lossy().into_owned());
    let settings = ServerSettings {
        name: flags.get("--name").map_or_else(|| game.clone(), str::to_owned),
        game,
        text,
        key: Path::new(path).with_file_name("server-identity.key"),
        port: flags.number("--port", DEFAULT_PORT)?,
        hub: if flags.has("--no-hub") {
            HubUse::None
        } else if let Some(address) = flags.get("--hub") {
            HubUse::At(address.to_owned())
        } else {
            HubUse::usual()
        },
        official: flags.has("--official"),
        max_players: flags.number("--max-players", 0u32)?,
        game_id: flags.get("--game-id").map(str::to_owned),
    };
    run(settings, &mut |line| println!("{line}"), &AtomicBool::new(false))
}

/// Which Hub a server lists itself on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubUse {
    /// Run one here (or use the one this computer already runs).
    Own,
    At(String),
    None,
}

impl HubUse {
    /// The Hub `LIBROXEL_HUB` names, else the official Hub, else one run here.
    pub fn usual() -> Self {
        match std::env::var("LIBROXEL_HUB").ok().filter(|a| !a.trim().is_empty()) {
            Some(address) => HubUse::At(address),
            None => libroxel_hub::OFFICIAL_HUB.map_or(HubUse::Own, |hub| HubUse::At(hub.to_owned())),
        }
    }
}

/// Everything a server needs to know to start.
#[derive(Debug, Clone)]
pub struct ServerSettings {
    /// The game's name, as Browse shows it.
    pub game: String,
    /// The server's name in Browse.
    pub name: String,
    /// The saved place to host.
    pub text: String,
    /// Where the server keeps its identity, so its join code stays the same.
    pub key: PathBuf,
    pub port: u16,
    pub hub: HubUse,
    pub official: bool,
    pub max_players: u32,
    /// Which game it is, so it shows on that game's server list (a
    /// published game's id, or `starter:<name>`).
    pub game_id: Option<String>,
}

/// Hosts a game with no window until `stop` is set, telling `say` what happens.
pub fn run(settings: ServerSettings, say: &mut dyn FnMut(String), stop: &AtomicBool) -> Result<(), String> {
    let ServerSettings { game, name, text, key, port, hub, official, max_players, game_id } = settings;
    let (tree, report) = place::load(&text).map_err(|e| format!("could not load {game}: {e}"))?;
    for warning in report.warnings {
        say(format!("warning: {warning}"));
    }
    let mut session = PlaySession::host(Rc::new(RefCell::new(tree)), port, None, false)?;
    let identity =
        Identity::load_or_create(&key).map_err(|e| format!("could not load {}: {e}", key.display()))?;
    let tunnel = HostTunnel::start(&identity, Network::Internet, port)?;
    say(format!("Hosting {game} on port {port}. Getting a join code…"));

    // A Hub of our own unless told to use another one (or none).
    let mut _own_hub = None;
    let hub = match hub {
        HubUse::None => None,
        HubUse::At(address) => Some(HubClient::new(&address)),
        HubUse::Own => {
            let hub_port = libroxel_hub::DEFAULT_PORT;
            match libroxel_hub::start(
                SocketAddr::from(([0, 0, 0, 0], hub_port)),
                Some(&data_dir().join("hub.sqlite")),
            ) {
                Ok(running) => {
                    say(format!(
                        "Running a Hub on port {hub_port}: players add this computer's address in Browse."
                    ));
                    let client = HubClient::new(&running.url());
                    _own_hub = Some(running);
                    Some(client)
                }
                Err(_) => {
                    // Usually another Hub on this computer already has the port.
                    say("Listing it on the Hub this computer already runs.".into());
                    Some(HubClient::new(&format!("127.0.0.1:{hub_port}")))
                }
            }
        }
    };
    let kind = if official { HostKind::Official } else { HostKind::Community };
    if official {
        say(format!(
            "This server's id is {}. On the Hub's computer, `libroxel hub owner add <that id>` lets it list as Official.",
            identity.player_id()
        ));
    }
    // Signed with the server's own key, so the Hub knows which server it is.
    let signer = identity.clone();
    let host = libroxel_hub::HostSigner {
        player_id: identity.player_id(),
        sign: Arc::new(move |message: &[u8]| signer.sign(message)),
    };
    let announcer = hub.map(|hub| {
        Announcer::start_signed(
            hub,
            identity.player_id(),
            Listing { name, game, code: None, players: 0, max_players, kind, game_id },
            Some(host),
        )
    });

    let frame = Duration::from_secs_f64(1.0 / 60.0);
    let mut next = Instant::now();
    let mut announced = false;
    let mut hub_said = None;
    while !stop.load(Ordering::Relaxed) {
        session.step(frame.as_secs_f32(), CharacterInput::default());
        if !announced {
            if let Some(code) = tunnel.code() {
                say(format!("Join code (paste it into Join, or find this server in Browse):\n\n{code}\n"));
                if let Some(announcer) = &announcer {
                    announcer.update(|l| l.code = Some(code));
                }
                announced = true;
            } else if let Some(error) = tunnel.error() {
                say(format!("No join code, so only this network can join: {error}"));
                announced = true;
            }
        }
        if let Some(announcer) = &announcer {
            let players = session.joined_count() as u32;
            announcer.update(|l| l.players = players);
            let said = announcer.status();
            if said != hub_said {
                if let Some(said) = &said {
                    say(format!("Hub: {said}"));
                }
                hub_said = said;
            }
        }
        for line in session.take_log() {
            let level = match line.level {
                LogLevel::Info => "",
                LogLevel::Warn => "warning: ",
                LogLevel::Error => "error: ",
            };
            say(format!("[{:8.2}] {level}{}", line.time, line.text));
        }
        next += frame;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
    say("Server stopped.".into());
    Ok(())
}

/// A server running inside the app, on a thread of its own.
pub struct BackgroundServer {
    pub game: String,
    pub port: u16,
    lines: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}

impl BackgroundServer {
    pub fn start(settings: ServerSettings) -> Self {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let (game, port) = (settings.game.clone(), settings.port);
        let (l, s, d) = (lines.clone(), stop.clone(), done.clone());
        std::thread::spawn(move || {
            let mut say = |line: String| l.lock().unwrap_or_else(|e| e.into_inner()).push(line);
            if let Err(e) = run(settings, &mut say, &s) {
                say(format!("error: {e}"));
            }
            d.store(true, Ordering::Relaxed);
        });
        Self { game, port, lines, stop, done }
    }

    /// Everything the server has said so far.
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Whether it has stopped (asked to, or because it failed).
    pub fn finished(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for BackgroundServer {
    fn drop(&mut self) {
        self.stop();
    }
}
