//! Soak runs: play for a long time and check nothing piles up.
//!
//! In normal test runs these are short (a minute of game time). The nightly
//! workflow sets `SOAK_SECONDS` (game seconds) and `SOAK_ROUNDS` for hours of
//! play. Game time runs as fast as the computer can step it.

use std::cell::RefCell;
use std::rc::Rc;

use libroxel_engine::{CharacterInput, LogLevel, PlaySession};
use libroxel_instance::{ClassKind, Property, PropertyValue};
use libroxel_tests::{DT, baseplate, corpus_dir};

fn env_number(name: &str, default: f32) -> f32 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Whether destroyed instances are known to pile up during play. They did
/// until the tree started freeing them while playing; it stays `false` so
/// the soak keeps that fixed. If it comes back and can't be fixed at once,
/// set it to `true`: the soak then checks the leak is still there (and
/// says how big), and fails once it's gone so this goes back to `false`.
const KNOWN_DESTROYED_INSTANCE_LEAK: bool = false;

struct Sample {
    lua: usize,
    /// Instances in the game.
    live: usize,
    /// Slots the tree holds, destroyed instances included.
    slots: usize,
}

fn sample(session: &PlaySession) -> Sample {
    let tree = session.runtime().tree().borrow();
    Sample {
        lua: session.runtime().used_memory(),
        live: tree.descendants(tree.root()).map_or(0, |d| d.len()),
        slots: tree.len(),
    }
}

/// A hosted server running the churn workload, with players joining and
/// leaving the whole time. Script memory and the number of instances must
/// level off, and every player who left must be gone.
#[test]
fn a_busy_server_stays_level_over_a_long_run() {
    let seconds = env_number("SOAK_SECONDS", 60.0);
    let mut tree = baseplate();
    let script = tree.create(ClassKind::Script, "Churn").unwrap();
    let source = std::fs::read_to_string(corpus_dir().join("soak/Churn.server.luau")).unwrap();
    tree.set(script, Property::Source, PropertyValue::String(source)).unwrap();
    let sss = tree.service(ClassKind::ServerScriptService).unwrap();
    tree.set_parent(script, Some(sss)).unwrap();

    let mut host = PlaySession::host(Rc::new(RefCell::new(tree)), 0, None, false).unwrap();
    let address = format!("127.0.0.1:{}", host.host_address().unwrap().port());
    let steps = (seconds / DT) as u32;
    // A guest joins every 10 seconds and stays for 6.
    let visit = (10.0 / DT) as u32;
    let stay = (6.0 / DT) as u32;
    let mut guest: Option<PlaySession> = None;
    let mut visits = 0;
    let mut warm: Option<Sample> = None;
    let mut peak_lua = 0;
    let mut errors = Vec::new();
    for i in 0..steps {
        if i % visit == 0 && guest.is_none() {
            let tree = Rc::new(RefCell::new(libroxel_instance::InstanceTree::new()));
            guest = Some(PlaySession::join(tree, &address, &format!("Guest{visits}")).unwrap());
            visits += 1;
        }
        if i % visit == stay
            && let Some(mut g) = guest.take()
        {
            g.stop();
        }
        host.step(DT, CharacterInput::default());
        if let Some(g) = guest.as_mut() {
            g.step(DT, CharacterInput::default());
            g.take_log();
        }
        errors.extend(host.take_log().into_iter().filter(|l| l.level == LogLevel::Error).map(|l| l.text));
        // Warmed up once the first fifth is done: caches filled, a few
        // guests have come and gone.
        if i == steps / 5 {
            warm = Some(sample(&host));
        }
        if i > steps / 5 {
            peak_lua = peak_lua.max(host.runtime().used_memory());
        }
    }
    drop(guest);
    // Let the host notice the last guest left.
    for _ in 0..120 {
        host.step(DT, CharacterInput::default());
    }
    assert!(errors.is_empty(), "script errors during the soak: {errors:?}");
    let warm = warm.unwrap();
    let end = sample(&host);
    eprintln!(
        "soak: instances in the game {} -> {}; slots held {} -> {}",
        warm.live, end.live, warm.slots, end.slots
    );
    // A wave holds about 50 instances; a few may be mid-wave.
    assert!(
        end.live <= warm.live + 150,
        "instances piled up: {} after warm-up, {} at the end",
        warm.live,
        end.live
    );
    let slots_grew = end.slots > warm.slots + 500;
    if KNOWN_DESTROYED_INSTANCE_LEAK {
        eprintln!("soak: known leak, {} destroyed instances never freed", end.slots - end.live);
        assert!(
            slots_grew || seconds < 30.0,
            "destroyed instances are freed now: set KNOWN_DESTROYED_INSTANCE_LEAK to false"
        );
    } else {
        assert!(
            !slots_grew,
            "destroyed instances piled up: {} slots after warm-up, {} at the end",
            warm.slots, end.slots
        );
    }
    // Garbage collection runs in steps, so allow some slack, but not growth
    // that scales with how long the game ran.
    assert!(
        end.lua <= warm.lua + warm.lua / 2 + 4 * 1024 * 1024,
        "script memory grew from {} KB to {} KB",
        warm.lua / 1024,
        end.lua / 1024
    );
    assert_eq!(host.joined_count(), 0, "players who left are still counted as joined");
    let tree = host.runtime().tree().borrow();
    let service = tree.service(ClassKind::Players).unwrap();
    let left = tree.children(service).unwrap().len();
    assert_eq!(left, 0, "players who left are still in Players");
}
