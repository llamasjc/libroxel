//! `libroxel server` and `libroxel hub`: the windowless parts of the app.

use std::cell::RefCell;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
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
      --hub <address>          list the game on that Hub instead of running one
      --no-hub                 do not run or use a Hub
      --official               list it as an official server (the last resort)
      --max-players <n>        players the list shows as the limit (0: none)
  libroxel hub                 run only a Hub
      --port <n>               Hub port (default 7430)
      --data <file>            where the Hub keeps its list
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
    let port = flags.number("--port", libroxel_hub::DEFAULT_PORT)?;
    let data = flags.get("--data").map_or_else(|| data_dir().join("hub.sqlite"), PathBuf::from);
    libroxel_hub::run(SocketAddr::from(([0, 0, 0, 0], port)), &data)
}

fn server(args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(args, &["--port", "--name", "--hub", "--max-players"])?;
    let Some(path) = flags.rest.first() else {
        return Err("usage: libroxel server <game.lrx> (see libroxel help)".into());
    };
    let port = flags.number("--port", DEFAULT_PORT)?;
    let max_players = flags.number("--max-players", 0u32)?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {path}: {e}"))?;
    let (tree, report) = place::load(&text).map_err(|e| format!("could not load {path}: {e}"))?;
    for warning in report.warnings {
        eprintln!("warning: {warning}");
    }
    let game =
        Path::new(path).file_stem().map_or("Untitled game".into(), |s| s.to_string_lossy().into_owned());
    let name = flags.get("--name").map_or_else(|| game.clone(), str::to_owned);

    let mut session = PlaySession::host(Rc::new(RefCell::new(tree)), port, None, false)?;
    let key = Path::new(path).with_file_name("server-identity.key");
    let identity =
        Identity::load_or_create(&key).map_err(|e| format!("could not load {}: {e}", key.display()))?;
    let tunnel = HostTunnel::start(&identity, Network::Internet, port)?;
    println!("Hosting {path} on port {port}. Getting a join code…");

    // A Hub of our own unless told to use another one (or none).
    let mut _own_hub = None;
    let hub = if flags.has("--no-hub") {
        None
    } else if let Some(address) = flags.get("--hub") {
        Some(HubClient::new(address))
    } else {
        let hub_port = libroxel_hub::DEFAULT_PORT;
        match libroxel_hub::start(
            SocketAddr::from(([0, 0, 0, 0], hub_port)),
            Some(&data_dir().join("hub.sqlite")),
        ) {
            Ok(running) => {
                println!("Running a Hub on port {hub_port}: players add this computer's address in Browse.");
                let client = HubClient::new(&running.url());
                _own_hub = Some(running);
                Some(client)
            }
            Err(e) => {
                // Usually another Hub on this computer already has the port.
                println!("Using the Hub already on this computer ({e}).");
                Some(HubClient::new(&format!("127.0.0.1:{hub_port}")))
            }
        }
    };
    let kind = if flags.has("--official") { HostKind::Official } else { HostKind::Community };
    let announcer = hub.map(|hub| {
        Announcer::start(
            hub,
            identity.player_id(),
            Listing { name, game, code: None, players: 0, max_players, kind },
        )
    });

    let frame = Duration::from_secs_f64(1.0 / 60.0);
    let mut next = Instant::now();
    let mut announced = false;
    let mut hub_said = None;
    loop {
        session.step(frame.as_secs_f32(), CharacterInput::default());
        if !announced {
            if let Some(code) = tunnel.code() {
                println!("Join code (paste it into Join, or find this server in Browse):\n\n{code}\n");
                if let Some(announcer) = &announcer {
                    announcer.update(|l| l.code = Some(code));
                }
                announced = true;
            } else if let Some(error) = tunnel.error() {
                println!("No join code, so only this network can join: {error}");
                announced = true;
            }
        }
        if let Some(announcer) = &announcer {
            let players = session.joined_count() as u32;
            announcer.update(|l| l.players = players);
            let said = announcer.status();
            if said != hub_said {
                if let Some(said) = &said {
                    println!("Hub: {said}");
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
            println!("[{:8.2}] {level}{}", line.time, line.text);
        }
        next += frame;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
}
