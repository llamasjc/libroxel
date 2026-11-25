//! Shared pieces of Libroxel's regression checks.
//!
//! - **The corpus** (`corpus/games/<name>/`): small games written the way
//!   real games are, which must keep loading and playing. Each game is a
//!   folder laid out like a Rojo project, so a game from elsewhere can be
//!   dropped in as it is:
//!
//!   | path in the game's folder            | becomes                         |
//!   |--------------------------------------|---------------------------------|
//!   | `place.lrx` (optional)               | the starting place              |
//!   | `<Service>/<Name>.server.luau`       | a `Script` in that service      |
//!   | `<Service>/<Name>.client.luau`       | a `LocalScript`                 |
//!   | `<Service>/<Name>.luau`              | a `ModuleScript`                |
//!   | `<Service>/<Folder>/...`             | a `Folder` holding the rest     |
//!   | `expect.txt`                         | what the game must print        |
//!
//!   `<Service>` is `Workspace`, `ServerScriptService`, `ReplicatedStorage`,
//!   `ServerStorage`, `StarterGui`, `StarterPack`, `StarterPlayerScripts` or
//!   `StarterCharacterScripts`. With no `place.lrx` the game starts on a
//!   baseplate with a spawn.
//!
//!   `expect.txt` holds one line per message the game must print (a
//!   message passes when it contains the line). Lines starting with `#` are
//!   comments, `# seconds: <n>` sets how long the game plays (default 5),
//!   `# errors: allowed` lets a game print script errors (for games that
//!   test error handling and print their own checks), and
//!   `# known-broken: <why>` marks a game that shows an engine bug nobody
//!   has fixed yet. A known-broken game must still not crash the engine,
//!   and once it passes the test fails until the line is removed, so the
//!   fix stays fixed.
//!
//! - **Soak runs** (`tests/soak.rs`) play the corpus for a long time and
//!   check memory and instance counts stay level.
//!
//! - **Benchmarks** (`src/bin/bench.rs`) time fixed workloads; CI runs the
//!   old and new commit on the same machine and compares.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use libroxel_engine::{CharacterInput, LogEntry, LogLevel, PlaySession};
use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Property, PropertyValue, Vec3, place};

/// One step of game time, as the app runs it.
pub const DT: f32 = 1.0 / 60.0;

/// The folder the corpus lives in (`LIBROXEL_CORPUS` points elsewhere, so
/// a benchmark built from another commit plays the same corpus).
pub fn corpus_dir() -> PathBuf {
    std::env::var_os("LIBROXEL_CORPUS")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus"), PathBuf::from)
}

/// A game from the corpus, ready to play.
pub struct CorpusGame {
    pub name: String,
    pub tree: InstanceTree,
    /// Messages the game must print.
    pub expect: Vec<String>,
    /// Seconds of game time to play.
    pub seconds: f32,
    /// Whether script errors are part of the game (it checks them itself).
    pub errors_allowed: bool,
    /// The engine bug this game shows, while it isn't fixed.
    pub known_broken: Option<String>,
}

/// Every game in `corpus/games`, sorted by name.
pub fn corpus_games() -> Vec<CorpusGame> {
    let dir = corpus_dir().join("games");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    names.sort();
    names.iter().map(|p| load_game(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).collect()
}

/// Reads one corpus game's folder.
pub fn load_game(dir: &Path) -> Result<CorpusGame, String> {
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    let mut tree = match std::fs::read_to_string(dir.join("place.lrx")) {
        Ok(text) => place::load(&text).map_err(|e| format!("place.lrx: {e}"))?.0,
        Err(_) => baseplate(),
    };
    let mut services: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    services.sort();
    for path in services {
        let service = path.file_name().unwrap().to_string_lossy().into_owned();
        let parent = container(&tree, &service).ok_or(format!("{service} is not a place scripts can go"))?;
        add_folder(&mut tree, parent, &path)?;
    }
    let mut game = CorpusGame {
        name,
        tree,
        expect: Vec::new(),
        seconds: 5.0,
        errors_allowed: false,
        known_broken: None,
    };
    let expect = std::fs::read_to_string(dir.join("expect.txt")).map_err(|_| "no expect.txt".to_string())?;
    for line in expect.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(rest) = line.strip_prefix("# seconds:") {
            game.seconds = rest.trim().parse().map_err(|_| format!("bad seconds line: {line}"))?;
        } else if let Some(why) = line.strip_prefix("# known-broken:") {
            game.known_broken = Some(why.trim().to_owned());
        } else if line == "# errors: allowed" {
            game.errors_allowed = true;
        } else if !line.starts_with('#') {
            game.expect.push(line.to_owned());
        }
    }
    if game.expect.is_empty() {
        return Err("expect.txt names nothing the game must print".into());
    }
    Ok(game)
}

/// A flat baseplate with a spawn, like a new place.
pub fn baseplate() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let plate = tree.create(ClassKind::Part, "Baseplate").unwrap();
    tree.set(plate, Property::Size, PropertyValue::Vec3(Vec3::new(512.0, 1.0, 512.0))).unwrap();
    tree.set(plate, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, -0.5, 0.0))).unwrap();
    tree.set(plate, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    tree.set_parent(plate, Some(ws)).unwrap();
    let spawn = tree.create(ClassKind::SpawnLocation, "SpawnLocation").unwrap();
    tree.set(spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0))).unwrap();
    tree.set(spawn, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.5, 0.0))).unwrap();
    tree.set(spawn, Property::Anchored, PropertyValue::Bool(true)).unwrap();
    tree.set_parent(spawn, Some(ws)).unwrap();
    tree
}

fn container(tree: &InstanceTree, name: &str) -> Option<InstanceId> {
    Some(match name {
        "Workspace" => tree.workspace(),
        "ServerScriptService" => tree.service(ClassKind::ServerScriptService)?,
        "ReplicatedStorage" => tree.service(ClassKind::ReplicatedStorage)?,
        "ServerStorage" => tree.service(ClassKind::ServerStorage)?,
        "StarterGui" => tree.service(ClassKind::StarterGui)?,
        "StarterPack" => tree.service(ClassKind::StarterPack)?,
        "StarterPlayerScripts" => tree.starter_scripts(ClassKind::StarterPlayerScripts)?,
        "StarterCharacterScripts" => tree.starter_scripts(ClassKind::StarterCharacterScripts)?,
        _ => return None,
    })
}

fn add_folder(tree: &mut InstanceTree, parent: InstanceId, dir: &Path) -> Result<(), String> {
    let mut entries: Vec<PathBuf> =
        std::fs::read_dir(dir).map_err(|e| e.to_string())?.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            let folder = tree.create(ClassKind::Folder, file).map_err(|e| e.to_string())?;
            tree.set_parent(folder, Some(parent)).map_err(|e| e.to_string())?;
            add_folder(tree, folder, &path)?;
            continue;
        }
        let Some(stem) = file.strip_suffix(".luau").or_else(|| file.strip_suffix(".lua")) else { continue };
        let (class, name) = if let Some(n) = stem.strip_suffix(".server") {
            (ClassKind::Script, n)
        } else if let Some(n) = stem.strip_suffix(".client") {
            (ClassKind::LocalScript, n)
        } else {
            (ClassKind::ModuleScript, stem)
        };
        let source = std::fs::read_to_string(&path).map_err(|e| format!("{file}: {e}"))?;
        let script = tree.create(class, name).map_err(|e| e.to_string())?;
        tree.set(script, Property::Source, PropertyValue::String(source)).map_err(|e| e.to_string())?;
        tree.set_parent(script, Some(parent)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Starts `tree` as a solo game with a character, the way the editor's
/// Play button does, with every waiting thread run each step so the game
/// plays the same on a slow computer.
pub fn start(tree: InstanceTree) -> PlaySession {
    let mut session = PlaySession::start(Rc::new(RefCell::new(tree)), true).expect("the game starts");
    session.load_characters_now();
    session.runtime().set_steady(true);
    session
}

/// What playing a game for a while produced.
pub struct Played {
    pub log: Vec<LogEntry>,
    /// The longest single step.
    pub slowest_step: Duration,
}

impl Played {
    pub fn errors(&self) -> Vec<&str> {
        self.log.iter().filter(|l| l.level == LogLevel::Error).map(|l| l.text.as_str()).collect()
    }

    pub fn printed(&self, text: &str) -> bool {
        self.log.iter().any(|l| l.text.contains(text))
    }

    /// The whole log, one message a line, for failure messages.
    pub fn dump(&self) -> String {
        self.log
            .iter()
            .map(|l| format!("[{:>6.2} {:?}] {}", l.time, l.level, l.text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Plays `session` for `seconds` of game time, walking the character in a
/// slow circle so touch, movement and camera code runs too.
pub fn play(session: &mut PlaySession, seconds: f32) -> Played {
    let mut log = Vec::new();
    let mut slowest_step = Duration::ZERO;
    let steps = (seconds / DT).ceil() as u32;
    for i in 0..steps {
        let angle = i as f32 * DT * 0.5;
        let input = CharacterInput {
            move_dir: Vec3::new(angle.cos(), 0.0, angle.sin()),
            ..CharacterInput::default()
        };
        let began = Instant::now();
        session.step(DT, input);
        slowest_step = slowest_step.max(began.elapsed());
        log.extend(session.take_log());
    }
    Played { log, slowest_step }
}

/// Plays a corpus game and checks it printed what it should, with no
/// script errors. Returns a description of what went wrong.
pub fn check_game(game: CorpusGame) -> Result<Played, String> {
    let CorpusGame { name, tree, expect, seconds, errors_allowed, .. } = game;
    let mut session = start(tree);
    let played = play(&mut session, seconds);
    let mut problems = Vec::new();
    if !errors_allowed {
        for error in played.errors() {
            problems.push(format!("script error: {error}"));
        }
    }
    for line in &expect {
        if !played.printed(line) {
            problems.push(format!("never printed: {line}"));
        }
    }
    session.stop();
    if problems.is_empty() {
        Ok(played)
    } else {
        Err(format!("{name}:\n  {}\n--- log ---\n{}", problems.join("\n  "), played.dump()))
    }
}
