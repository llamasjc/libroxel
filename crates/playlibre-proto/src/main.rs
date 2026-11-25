//! Plays a game in a window, without the editor.
//!
//! ```text
//! cargo run -p playlibre-proto                       # the bundled demo script
//! cargo run -p playlibre-proto -- my_game.lrx        # a place saved from the editor
//! cargo run -p playlibre-proto -- my_script.luau     # one script in an empty world
//! ```
//!
//! WASD to walk, Space to jump, hold right mouse to turn the camera, scroll to zoom.

use bevy::prelude::*;
use playlibre_engine::PlaySession;
use playlibre_engine::view::{EnginePlugin, EngineState, RenderPlugin};
use playlibre_instance::{ClassKind, InstanceTree, Property, PropertyValue, place};

const DEFAULT_SCRIPT: &str = include_str!("../scripts/move_part.luau");

fn world_with_script(name: &str, source: &str) -> InstanceTree {
    let mut tree = InstanceTree::new();
    let script = tree.create(ClassKind::Script, name).expect("scripts are creatable");
    tree.set_parent(script, Some(tree.workspace())).expect("workspace accepts children");
    tree.set(script, Property::Source, PropertyValue::String(source.into())).expect("scripts have Source");
    tree
}

fn load(path: &str) -> InstanceTree {
    let text = std::fs::read_to_string(path).unwrap_or_else(|err| {
        eprintln!("could not read {path}: {err}");
        std::process::exit(1);
    });
    if path.ends_with(".luau") || path.ends_with(".lua") {
        return world_with_script(path, &text);
    }
    match place::load(&text) {
        Ok((tree, report)) => {
            for warning in report.warnings {
                eprintln!("warning: {warning}");
            }
            tree
        }
        Err(err) => {
            eprintln!("could not open {path}: {err}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let tree = match std::env::args().nth(1) {
        Some(path) => load(&path),
        None => world_with_script("move_part", DEFAULT_SCRIPT),
    };
    let mut engine = EngineState::new(tree);
    match PlaySession::start(engine.tree.clone(), true) {
        Ok(session) => engine.session = Some(session),
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "PlayLibre".into(), ..default() }),
            ..default()
        }))
        .insert_non_send(engine)
        .add_plugins((EnginePlugin, RenderPlugin))
        .add_systems(Update, print_log)
        .run();
}

fn print_log(mut engine: NonSendMut<EngineState>, mut printed: Local<usize>) {
    for entry in &engine.log[*printed..] {
        match entry.level {
            playlibre_engine::LogLevel::Info => info!(target: "game", "{}", entry.text),
            playlibre_engine::LogLevel::Warn => warn!(target: "game", "{}", entry.text),
            playlibre_engine::LogLevel::Error => error!(target: "game", "{}", entry.text),
        }
    }
    *printed = engine.log.len();
    // Keep memory bounded in long sessions.
    if engine.log.len() > 10_000 {
        engine.log.drain(..5_000);
        *printed = engine.log.len();
    }
}
