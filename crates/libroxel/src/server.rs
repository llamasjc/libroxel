//! `libroxel server`: the windowless part of the app.

use std::cell::RefCell;
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
  libroxel [game.lrx]          open the app (or the editor on that game)
  libroxel server <game.lrx>   host a game with no window
  libroxel server --game <id>  host a published game (or starter:<name>) from
                               the Hub, with no window
      --port <n>               game port on this network (default 7420)
      --name <text>            name shown in Online (default: the file name)
      --no-hub                 don't list it on the Hub (join codes still work)
      --official               list it as an official server (the Hub's owners
                               must trust this server)
      --max-players <n>        players the list shows as the limit (0: none)
      --game-id <id>           put it on that game's server list (a published
                               game's id, or starter:<name>)
      --key <file>             the server's identity (default: next to the game)
      --udp-port <n>           take players on this UDP port (open it in the
                               firewall so they connect directly)
      --stop-when-empty <s>    stop once nobody has been in it for s seconds
  libroxel help                show this
";

/// Handles `server` and `help`. Returns `false` when the arguments
/// are for the editor.
pub fn dispatch(args: &[String]) -> bool {
    let result = match args.first().map(String::as_str) {
        Some("server") => server(&args[1..]),
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

fn server(args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(
        args,
        &[
            "--port",
            "--name",
            "--max-players",
            "--game-id",
            "--game",
            "--key",
            "--udp-port",
            "--stop-when-empty",
        ],
    )?;
    let hub = if flags.has("--no-hub") { HubUse::None } else { HubUse::usual() };
    let (game, text, key, game_id) = match (flags.rest.first(), flags.get("--game")) {
        (Some(path), _) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {path}: {e}"))?;
            let game = Path::new(path)
                .file_stem()
                .map_or("Untitled game".into(), |s| s.to_string_lossy().into_owned());
            (game, text, Path::new(path).with_file_name("server-identity.key"), flags.get("--game-id"))
        }
        (None, Some(id)) => {
            let (game, text) = game_by_id(id, &hub)?;
            (game, text, data_dir().join("server-identity.key"), Some(id))
        }
        (None, None) => return Err("usage: libroxel server <game.lrx> (see libroxel help)".into()),
    };
    let stop_when_empty: u64 = flags.number("--stop-when-empty", 0)?;
    let settings = ServerSettings {
        name: flags.get("--name").map_or_else(|| game.clone(), str::to_owned),
        game,
        text,
        key: flags.get("--key").map_or(key, PathBuf::from),
        port: flags.number("--port", DEFAULT_PORT)?,
        hub,
        official: flags.has("--official"),
        max_players: flags.number("--max-players", 0u32)?,
        game_id: game_id.map(str::to_owned),
        udp_port: flags
            .get("--udp-port")
            .map(|p| p.parse())
            .transpose()
            .map_err(|_| "--udp-port needs a number")?,
        stop_when_empty: (stop_when_empty > 0).then(|| Duration::from_secs(stop_when_empty)),
    };
    if settings.stop_when_empty.is_some() {
        // A server that comes and goes: if the computer runs short of
        // memory, stop one of these before anything else.
        let _ = std::fs::write("/proc/self/oom_score_adj", "500");
    }
    run(settings, &mut |line| println!("{line}"), &AtomicBool::new(false))
}

/// A game's title and saved place by its id: a starter game, or a published
/// one fetched from the Hub.
fn game_by_id(id: &str, hub: &HubUse) -> Result<(String, String), String> {
    if let Some(name) = id.strip_prefix("starter:") {
        let game = crate::starter_games::starter(name).ok_or(format!("there is no starter game {name}"))?;
        let text = place::save(&(game.build)()).map_err(|e| format!("could not save {name}: {e}"))?;
        return Ok((game.name.to_owned(), text));
    }
    // From the Hub even when this server isn't listed on it.
    let address = match hub {
        HubUse::At(address) => address.clone(),
        HubUse::None => libroxel_hub::hub_address(),
    };
    let client = HubClient::new(&address);
    let info = client.game(id)?;
    Ok((info.title, client.download(id)?))
}

/// Whether a server lists itself on the Hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubUse {
    At(String),
    None,
}

impl HubUse {
    /// The Hub: [`libroxel_hub::hub_address`].
    pub fn usual() -> Self {
        HubUse::At(libroxel_hub::hub_address())
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
    /// The UDP port players reach it on; `None` picks any free one.
    pub udp_port: Option<u16>,
    /// Stop once nobody has been in it for this long.
    pub stop_when_empty: Option<Duration>,
}

/// Hosts a game with no window until `stop` is set, telling `say` what happens.
pub fn run(settings: ServerSettings, say: &mut dyn FnMut(String), stop: &AtomicBool) -> Result<(), String> {
    let ServerSettings {
        game,
        name,
        text,
        key,
        port,
        hub,
        official,
        max_players,
        game_id,
        udp_port,
        stop_when_empty,
    } = settings;
    let (tree, report) = place::load(&text).map_err(|e| format!("could not load {game}: {e}"))?;
    for warning in report.warnings {
        say(format!("warning: {warning}"));
    }
    let mut session = PlaySession::host(Rc::new(RefCell::new(tree)), port, None, false)?;
    let identity =
        Identity::load_or_create(&key).map_err(|e| format!("could not load {}: {e}", key.display()))?;
    // Port 0 picks a free one: the tunnel needs the one it got.
    let port = session.host_address().map_or(port, |a| a.port());
    let tunnel = HostTunnel::start_on(&identity, Network::Internet, port, udp_port)?;
    say(format!("Hosting {game} on port {port}. Getting a join code…"));

    let hub = match hub {
        HubUse::None => None,
        HubUse::At(address) => Some(HubClient::new(&address)),
    };
    let kind = if official { HostKind::Official } else { HostKind::Community };
    if official {
        say(format!(
            "This server's id is {}. A Hub owner running `libroxel-hub owner add <that id>` on the Hub's server lets it list as Official.",
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
    let mut empty_since = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        session.step(frame.as_secs_f32(), CharacterInput::default());
        if session.joined_count() > 0 {
            empty_since = Instant::now();
        } else if let Some(wait) = stop_when_empty
            && empty_since.elapsed() >= wait
        {
            say(format!("Nobody has played for {} seconds; stopping.", wait.as_secs()));
            break;
        }
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
