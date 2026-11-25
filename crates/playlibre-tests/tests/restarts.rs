//! Soak: starting and stopping games over and over. In its own test file
//! so nothing else runs in the process while it measures memory.

use playlibre_tests::{check_game, corpus_games};

fn env_number(name: &str, default: f32) -> f32 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Bytes this process holds in memory, where the system says (Linux).
fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

/// Starting and stopping every corpus game over and over must not grow
/// the process: whatever a game used goes away with it.
#[test]
fn restarting_games_gives_memory_back() {
    let rounds = env_number("SOAK_ROUNDS", 2.0).max(2.0) as usize;
    let mut after_first = None;
    for round in 0..rounds {
        for game in corpus_games() {
            let known = game.known_broken.is_some();
            if let Err(why) = check_game(game)
                && !known
            {
                panic!("round {round}: {why}");
            }
        }
        if round == 0 {
            after_first = resident_bytes();
        }
    }
    if let (Some(first), Some(last)) = (after_first, resident_bytes()) {
        let grown = last.saturating_sub(first) / (1024 * 1024);
        eprintln!("soak: {rounds} rounds of the corpus, memory grew {grown} MB after the first");
        // Allocators keep some freed memory; a real leak grows every round.
        let allowed = 48 + 8 * rounds as u64;
        assert!(grown <= allowed, "memory grew {grown} MB over {rounds} rounds (allowed {allowed} MB)");
    }
}
