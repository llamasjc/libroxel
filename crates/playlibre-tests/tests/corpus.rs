//! The golden corpus: every game in `corpus/games` must load, play for its
//! time with no script errors and print everything in its `expect.txt`.
//! A game that stops working is a regression, even if no other test
//! covers what it uses.
//!
//! Run one game with `CORPUS_GAME=<name> cargo test -p playlibre-tests --test corpus`.

use std::path::Path;

use playlibre_instance::place;
use playlibre_tests::{check_game, corpus_games, load_game, play, start};

fn selected() -> Vec<playlibre_tests::CorpusGame> {
    let only = std::env::var("CORPUS_GAME").ok();
    let games: Vec<_> =
        corpus_games().into_iter().filter(|g| only.as_deref().is_none_or(|o| o == g.name)).collect();
    assert!(!games.is_empty(), "no corpus games matched");
    games
}

/// Plays each game and sorts the results: what broke, and which
/// known-broken games now pass (their marker should go).
fn check_all(games: Vec<playlibre_tests::CorpusGame>, what: &str) {
    let mut broke = Vec::new();
    let mut fixed = Vec::new();
    let count = games.len();
    for game in games {
        let name = game.name.clone();
        let known = game.known_broken.clone();
        match (check_game(game), known) {
            (Ok(played), None) => eprintln!("ok      {name} (slowest step {:?})", played.slowest_step),
            (Ok(_), Some(why)) => {
                eprintln!("FIXED   {name}");
                fixed.push(format!("{name} passes now: remove `# known-broken: {why}` from its expect.txt"));
            }
            (Err(_), Some(why)) => eprintln!("known   {name}: {why}"),
            (Err(why), None) => {
                eprintln!("FAIL    {name}");
                broke.push(why);
            }
        }
    }
    assert!(
        broke.is_empty(),
        "{} of {count} corpus games broke{what}:\n\n{}",
        broke.len(),
        broke.join("\n\n")
    );
    assert!(fixed.is_empty(), "{}", fixed.join("\n"));
}

#[test]
fn every_corpus_game_plays_and_prints_what_it_should() {
    check_all(selected(), "");
}

#[test]
fn every_corpus_game_still_plays_after_saving_and_loading() {
    let mut games = Vec::new();
    for mut game in selected() {
        let text = place::save(&game.tree).unwrap_or_else(|e| panic!("{}: could not save: {e}", game.name));
        let (tree, report) =
            place::load(&text).unwrap_or_else(|e| panic!("{}: could not load: {e}", game.name));
        assert!(report.warnings.is_empty(), "{}: warnings on load: {:?}", game.name, report.warnings);
        game.tree = tree;
        games.push(game);
    }
    check_all(games, " after saving and loading");
}

#[test]
fn the_corpus_folder_format_is_read_as_documented() {
    let dir = playlibre_tests::corpus_dir().join("games/round_system");
    let game = load_game(&dir).unwrap();
    assert_eq!(game.seconds, 9.0);
    let tree = &game.tree;
    let rs = tree.service(playlibre_instance::ClassKind::ReplicatedStorage).unwrap();
    let kids: Vec<_> = tree.children(rs).unwrap().iter().map(|id| tree.class(*id).unwrap()).collect();
    assert!(kids.iter().all(|c| *c == playlibre_instance::ClassKind::ModuleScript), "{kids:?}");
    assert!(load_game(Path::new("/no/such/game")).is_err());
}

/// Places saved by other engines' editors (the same sample files the importer is
/// tested with) start and play without script errors.
#[test]
fn imported_sample_places_start_and_play() {
    let files = Path::new(env!("CARGO_MANIFEST_DIR")).join("../playlibre-rbx/tests/files");
    for name in ["baseplate-566-binary.rbxl", "baseplate-566-xml.rbxlx", "all-instances-415-binary.rbxl"] {
        let (tree, _) = playlibre_rbx::open(&files.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut session = start(tree);
        let played = play(&mut session, 2.0);
        assert!(played.errors().is_empty(), "{name}:\n{}", played.dump());
        session.stop();
    }
}
