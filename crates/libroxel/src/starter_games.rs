//! Starter games: small, finished games to play straight away, and places
//! to start from in the editor.

use libroxel_instance::{ClassKind, InstanceId, InstanceTree, Material, Property, PropertyValue, Vec3};

use crate::templates::{add, block, code, material, script, set};

/// A game you can start from: its name, a line about it and how to build it.
pub struct StarterGame {
    pub name: &'static str,
    pub symbol: &'static str,
    pub about: &'static str,
    pub build: fn() -> InstanceTree,
}

/// Every starter game, in the order the home screen shows them.
pub const STARTER_GAMES: &[StarterGame] = &[
    StarterGame {
        name: "Kart race",
        symbol: crate::theme::icon::STEERING_WHEEL,
        about: "Drift, boost and throw items against seven racers on three tracks",
        build: kart_race,
    },
    StarterGame {
        name: "Chess",
        symbol: crate::theme::icon::HORSE,
        about: "Chess against eight computer players or anyone in the server",
        build: chess,
    },
    StarterGame {
        name: "Checkers",
        symbol: crate::theme::icon::CHECKERBOARD,
        about: "Checkers against the computer or anyone in the server",
        build: checkers,
    },
    StarterGame {
        name: "Connect 4",
        symbol: crate::theme::icon::CIRCLES_FOUR,
        about: "Line up four discs before the other player does",
        build: connect_four,
    },
    StarterGame {
        name: "Kings in the Corner",
        symbol: crate::theme::icon::CROWN,
        about: "Be first to play every card. For 2 to 4 players",
        build: kings_in_the_corner,
    },
    StarterGame {
        name: "Tag",
        symbol: crate::theme::icon::PERSON_SIMPLE_RUN,
        about: "Don't be It when the clock runs out",
        build: crate::party_games::tag,
    },
    StarterGame {
        name: "Hide and seek",
        symbol: crate::theme::icon::EYE_CLOSED,
        about: "Hide round the houses, or find everyone",
        build: crate::party_games::hide_and_seek,
    },
    StarterGame {
        name: "Battle royale",
        symbol: crate::theme::icon::SWORD,
        about: "Last one standing as the storm closes in",
        build: crate::party_games::battle_royale,
    },
];

/// Finds a starter game by name.
pub fn starter(name: &str) -> Option<&'static StarterGame> {
    STARTER_GAMES.iter().find(|game| game.name == name)
}

fn spawn_at(tree: &mut InstanceTree, parent: InstanceId, at: [f32; 3]) {
    let spawn = add(tree, parent, ClassKind::SpawnLocation, "SpawnLocation");
    set(tree, spawn, Property::Position, PropertyValue::Vec3(Vec3::new(at[0], at[1], at[2])));
    set(tree, spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0)));
}

/// An arcade kart race, made of scripts like any creator's game:
/// `ReplicatedStorage.Kart` holds the `Tracks` module (three tracks, built
/// when a race starts) and the `Control` and `Race` remote events,
/// `ServerScriptService.KartServer` drives every kart, the computer racers
/// and the items, and `StarterGui.KartScreen` has the start menu, the HUD
/// and the chase camera. The scripts are in `kart_game/`.
pub fn kart_race() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    // Everyone arrives at the garage, off to the side of every track, and
    // is put in a kart when a race starts.
    let garage = add(&mut tree, ws, ClassKind::Folder, "Garage");
    let floor =
        block(&mut tree, garage, "GarageFloor", [0.0, 0.1, 440.0], [40.0, 0.2, 30.0], [176, 160, 150]);
    material(&mut tree, floor, Material::Brick);
    spawn_at(&mut tree, garage, [0.0, 0.5, 440.0]);

    let storage = tree.service(ClassKind::ReplicatedStorage).expect("every tree has storage");
    let shared = add(&mut tree, storage, ClassKind::Folder, "Kart");
    code(&mut tree, shared, ClassKind::ModuleScript, "Tracks", include_str!("kart_game/Tracks.luau"));
    add(&mut tree, shared, ClassKind::RemoteEvent, "Control");
    add(&mut tree, shared, ClassKind::RemoteEvent, "Race");

    let server = tree.service(ClassKind::ServerScriptService).expect("every tree has ServerScriptService");
    script(&mut tree, server, "KartServer", include_str!("kart_game/Server.luau"));

    let starter_gui = tree.service(ClassKind::StarterGui).expect("every tree has StarterGui");
    let screen = add(&mut tree, starter_gui, ClassKind::ScreenGui, "KartScreen");
    tree.set_extra(screen, "IgnoreGuiInset", Some(PropertyValue::Bool(true)))
        .expect("screen GUIs take extras");
    set(&mut tree, screen, Property::ResetOnSpawn, PropertyValue::Bool(false));
    code(&mut tree, screen, ClassKind::LocalScript, "KartClient", include_str!("kart_game/Client.luau"));
    tree.drain_changes();
    tree
}

/// Chess, made only of GUI objects and scripts: a rules module and a
/// computer player shared through ReplicatedStorage, a server script that
/// referees games, pairs players and runs the clocks, and a screen script
/// that draws the board. Nobody gets a character; the screen covers the world.
pub fn chess() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let players = tree.service(ClassKind::Players).expect("every tree has Players");
    set(&mut tree, players, Property::CharacterAutoLoads, PropertyValue::Bool(false));
    let ws = tree.workspace();
    block(&mut tree, ws, "Baseplate", [0.0, -0.5, 0.0], [64.0, 1.0, 64.0], [99, 140, 88]);
    spawn_at(&mut tree, ws, [0.0, 0.5, 0.0]);

    let storage = tree.service(ClassKind::ReplicatedStorage).expect("every tree has storage");
    let shared = add(&mut tree, storage, ClassKind::Folder, "Chess");
    code(&mut tree, shared, ClassKind::ModuleScript, "Rules", include_str!("chess_game/Rules.luau"));
    code(&mut tree, shared, ClassKind::ModuleScript, "Engine", include_str!("chess_game/Engine.luau"));
    add(&mut tree, shared, ClassKind::RemoteEvent, "Action");
    add(&mut tree, shared, ClassKind::RemoteEvent, "State");

    let server = tree.service(ClassKind::ServerScriptService).expect("every tree has ServerScriptService");
    script(&mut tree, server, "ChessServer", include_str!("chess_game/Server.luau"));

    let starter_gui = tree.service(ClassKind::StarterGui).expect("every tree has StarterGui");
    let screen = add(&mut tree, starter_gui, ClassKind::ScreenGui, "ChessScreen");
    // The screen covers the whole window, under the top bar too.
    tree.set_extra(screen, "IgnoreGuiInset", Some(PropertyValue::Bool(true)))
        .expect("screen GUIs take extras");
    code(&mut tree, screen, ClassKind::LocalScript, "ChessClient", include_str!("chess_game/Client.luau"));
    tree.drain_changes();
    tree
}

const TABLE_SERVER: &str = r#"
-- Runs the table: seats, the computer players and the referee.
local folder = game:GetService("ReplicatedStorage"):WaitForChild("TableGame")
require(folder:WaitForChild("Tables")).run(folder, require(folder:WaitForChild("Rules")))
"#;

/// A game played at a table on screen, made only of GUI objects and
/// scripts like Chess: the game's rules, the shared table server and the
/// shared screen in `ReplicatedStorage.TableGame`, a server script that
/// runs the tables, and the game's own screen script that draws the board.
fn table_game(name: &str, rules: &str, client: &str) -> InstanceTree {
    let mut tree = InstanceTree::new();
    let players = tree.service(ClassKind::Players).expect("every tree has Players");
    set(&mut tree, players, Property::CharacterAutoLoads, PropertyValue::Bool(false));
    let ws = tree.workspace();
    block(&mut tree, ws, "Baseplate", [0.0, -0.5, 0.0], [64.0, 1.0, 64.0], [99, 140, 88]);
    spawn_at(&mut tree, ws, [0.0, 0.5, 0.0]);

    let storage = tree.service(ClassKind::ReplicatedStorage).expect("every tree has storage");
    let shared = add(&mut tree, storage, ClassKind::Folder, "TableGame");
    code(&mut tree, shared, ClassKind::ModuleScript, "Rules", rules);
    code(&mut tree, shared, ClassKind::ModuleScript, "Tables", include_str!("table_games/Tables.luau"));
    code(&mut tree, shared, ClassKind::ModuleScript, "Screen", include_str!("table_games/Screen.luau"));
    add(&mut tree, shared, ClassKind::RemoteEvent, "Action");
    add(&mut tree, shared, ClassKind::RemoteEvent, "State");

    let server = tree.service(ClassKind::ServerScriptService).expect("every tree has ServerScriptService");
    script(&mut tree, server, &format!("{name}Server"), TABLE_SERVER);

    let starter_gui = tree.service(ClassKind::StarterGui).expect("every tree has StarterGui");
    let screen = add(&mut tree, starter_gui, ClassKind::ScreenGui, &format!("{name}Screen"));
    tree.set_extra(screen, "IgnoreGuiInset", Some(PropertyValue::Bool(true)))
        .expect("screen GUIs take extras");
    code(&mut tree, screen, ClassKind::LocalScript, &format!("{name}Client"), client);
    tree.drain_changes();
    tree
}

/// Checkers (English draughts) against three computer players or people.
pub fn checkers() -> InstanceTree {
    table_game(
        "Checkers",
        include_str!("table_games/checkers/Rules.luau"),
        include_str!("table_games/checkers/Client.luau"),
    )
}

/// Connect 4 against three computer players or people.
pub fn connect_four() -> InstanceTree {
    table_game(
        "ConnectFour",
        include_str!("table_games/connect4/Rules.luau"),
        include_str!("table_games/connect4/Client.luau"),
    )
}

/// Kings in the Corner, the card game, for two to four players.
pub fn kings_in_the_corner() -> InstanceTree {
    table_game(
        "KingsInTheCorner",
        include_str!("table_games/kings_corner/Rules.luau"),
        include_str!("table_games/kings_corner/Client.luau"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use libroxel_engine::session::start_on;
    use libroxel_engine::{CharacterInput, LogLevel};
    use libroxel_instance::place;

    fn errors(session: &mut libroxel_engine::session::PlaySession) -> Vec<String> {
        session.take_log().into_iter().filter(|l| l.level == LogLevel::Error).map(|l| l.text).collect()
    }

    fn value(session: &libroxel_engine::session::PlaySession, script: &str) -> String {
        session.runtime().run("check", &format!("print({script})")).unwrap();
        session.runtime().take_output().last().map(|l| l.text.clone()).unwrap_or_default()
    }

    #[test]
    fn starter_games_save_and_load() {
        for game in STARTER_GAMES {
            let text = place::save(&(game.build)()).unwrap();
            let (loaded, report) = place::load(&text).unwrap();
            assert!(report.warnings.is_empty(), "{}: {:?}", game.name, report.warnings);
            assert_eq!(place::save(&loaded).unwrap(), text, "{}", game.name);
        }
    }

    #[test]
    fn starter_games_run_without_script_errors() {
        for game in STARTER_GAMES {
            let mut session = start_on((game.build)(), true).unwrap();
            for _ in 0..300 {
                session.step(1.0 / 60.0, CharacterInput::default());
            }
            assert!(errors(&mut session).is_empty(), "{}: {:?}", game.name, errors(&mut session));
        }
    }

    fn steps(session: &mut libroxel_engine::session::PlaySession, frames: usize) {
        for _ in 0..frames {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
    }

    #[test]
    fn chess_rules_count_moves_right() {
        let mut session = start_on(chess(), true).unwrap();
        steps(&mut session, 5);
        let rules = "require(game.ReplicatedStorage.Chess.Rules)";
        assert_eq!(value(&session, &format!("{rules}.new():perft(3)")), "8902");
        let tricky = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
        assert_eq!(value(&session, &format!("{rules}.fromFen('{tricky}'):perft(2)")), "2039");
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
    }

    #[test]
    fn chess_against_the_computer() {
        let mut session = start_on(chess(), true).unwrap();
        steps(&mut session, 10);
        // The screen is built: the board's 64 squares and the lobby.
        let gui = "game.Players.Player1.PlayerGui.ChessScreen";
        // 64 squares, the result card, the promotion picker and a corner.
        assert_eq!(value(&session, &format!("#{gui}.BoardColumn.Board:GetChildren()")), "67");
        assert_eq!(value(&session, &format!("{gui}.Panel.Lobby.Visible")), "true");
        let act = |session: &mut libroxel_engine::session::PlaySession, action: &str| {
            session
                .runtime()
                .run("act", &format!("game.ReplicatedStorage.Chess.Action:FireServer({action})"))
                .unwrap();
        };
        act(&mut session, r#""playBot", { level = 2, control = "none", colour = "white" }"#);
        steps(&mut session, 5);
        assert_eq!(value(&session, &format!("{gui}.Panel.Game.Visible")), "true");
        assert_eq!(
            value(&session, &format!("{gui}.BoardColumn.Board.e2.Piece.Image")),
            "libroxel://chess-wp"
        );
        // An illegal move is refused; a legal one is answered by the computer.
        act(&mut session, r#""move", { game = "1", uci = "e2e5" }"#);
        steps(&mut session, 5);
        assert_eq!(
            value(&session, &format!("{gui}.BoardColumn.Board.e2.Piece.Image")),
            "libroxel://chess-wp"
        );
        act(&mut session, r#""move", { game = "1", uci = "e2e4" }"#);
        steps(&mut session, 150);
        assert_eq!(
            value(&session, &format!("{gui}.BoardColumn.Board.e4.Piece.Image")),
            "libroxel://chess-wp"
        );
        // One row in the move list (beside the list's rounded corner), with both moves.
        let moves = format!("{gui}.Panel.Game.Moves");
        assert_eq!(value(&session, &format!("#{moves}:GetChildren()")), "2");
        let answer = format!("{moves}:FindFirstChildOfClass('TextLabel'):GetChildren()[3].Text");
        assert_ne!(value(&session, &answer), "", "the computer answered");
        assert_eq!(
            value(&session, &format!("{gui}.BoardColumn.BottomPlayer.PlayerName.Text")),
            "Player1  (1200)"
        );
        // Taking back undoes both.
        act(&mut session, r#""takeback""#);
        steps(&mut session, 5);
        assert_eq!(
            value(&session, &format!("{gui}.BoardColumn.Board.e2.Piece.Image")),
            "libroxel://chess-wp"
        );
        // Resigning ends it; back in the lobby the board is cleared for the next game.
        act(&mut session, r#""move", { game = "1", uci = "d2d4" }"#);
        steps(&mut session, 150);
        act(&mut session, r#""resign""#);
        steps(&mut session, 5);
        assert_eq!(value(&session, &format!("{gui}.BoardColumn.Board.Result.Visible")), "true");
        assert_eq!(value(&session, &format!("{gui}.BoardColumn.Board.Result.Title.Text")), "You lose");
        act(&mut session, r#""leave""#);
        steps(&mut session, 5);
        assert_eq!(value(&session, &format!("{gui}.Panel.Lobby.Visible")), "true");
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
    }

    #[test]
    fn chess_between_two_computers() {
        use libroxel_engine::PlaySession;
        use std::{cell::RefCell, rc::Rc};
        let mut host = PlaySession::host(Rc::new(RefCell::new(chess())), 0, None, true).unwrap();
        let port = host.host_address().unwrap().port();
        let mut guest = PlaySession::join(
            Rc::new(RefCell::new(InstanceTree::new())),
            &format!("127.0.0.1:{port}"),
            "Bea",
        )
        .unwrap();
        let gui = "game.Players.LocalPlayer.PlayerGui:FindFirstChild('ChessScreen')";
        let until = |host: &mut PlaySession, guest: &mut PlaySession, check: &str, want: &str| {
            for _ in 0..900 {
                host.step(1.0 / 60.0, CharacterInput::default());
                guest.step(1.0 / 60.0, CharacterInput::default());
                if value(guest, check) == want {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            panic!("gave up waiting for {check} to be {want}: it is {}", value(guest, check));
        };
        // The guest's screen comes up, and lists the host's player in the lobby.
        let lobby =
            format!("{gui} and {gui}.Panel.Lobby.people.Choices:FindFirstChildOfClass('ScrollingFrame')");
        let listed = format!("(function() local list = {lobby} return list and #list:GetChildren() end)()");
        until(&mut host, &mut guest, &listed, "1");
        // Both look for a 5+0 game, and are paired.
        let act = |session: &PlaySession, action: &str| {
            session
                .runtime()
                .run("act", &format!("game.ReplicatedStorage.Chess.Action:FireServer({action})"))
                .unwrap();
        };
        act(&host, r#""seek", { control = "5+0" }"#);
        act(&guest, r#""seek", { control = "5+0" }"#);
        until(&mut host, &mut guest, &format!("{gui}.Panel.Game.Visible"), "true");
        assert_eq!(value(&host, &format!("{gui}.Panel.Game.Visible")), "true");
        assert_eq!(value(&guest, &format!("{gui}.BoardColumn.BottomPlayer.PlayerName.Text")), "Bea  (1200)");
        // White moves, and the guest's board shows it.
        let guest_is_white = value(&guest, &format!("{gui}.Panel.Game.Status.Text")) == "Your move";
        act(if guest_is_white { &guest } else { &host }, r#""move", { game = "1", uci = "e2e4" }"#);
        until(
            &mut host,
            &mut guest,
            &format!("{gui}.BoardColumn.Board.e4.Piece.Image"),
            "libroxel://chess-wp",
        );
        act(if guest_is_white { &host } else { &guest }, r#""move", { game = "1", uci = "e7e5" }"#);
        until(
            &mut host,
            &mut guest,
            &format!("{gui}.BoardColumn.Board.e5.Piece.Image"),
            "libroxel://chess-bp",
        );
        // Black resigns: white wins and the ratings move.
        act(if guest_is_white { &host } else { &guest }, r#""resign""#);
        let title = format!("{gui}.BoardColumn.Board.Result.Title.Text");
        until(&mut host, &mut guest, &title, if guest_is_white { "You win!" } else { "You lose" });
        let rating = "game.Players.LocalPlayer.leaderstats.Rating.Value";
        until(&mut host, &mut guest, rating, if guest_is_white { "1220" } else { "1180" });
        assert!(errors(&mut host).is_empty(), "{:?}", errors(&mut host));
        assert!(errors(&mut guest).is_empty(), "{:?}", errors(&mut guest));
    }

    fn fire(session: &libroxel_engine::session::PlaySession, action: &str) {
        session
            .runtime()
            .run("act", &format!("game.ReplicatedStorage.TableGame.Action:FireServer({action})"))
            .unwrap();
    }

    /// Plays whole games between computer players, straight through the
    /// rules, and returns how each ended.
    fn bots_play(build: fn() -> InstanceTree, seats: u32, levels: &str, games: u32) -> Vec<String> {
        let mut session = start_on(build(), true).unwrap();
        steps(&mut session, 3);
        let mut endings = Vec::new();
        for _ in 0..games {
            let script = format!(
                "local Rules = require(game.ReplicatedStorage.TableGame.Rules)
                local levels = {{ {levels} }}
                local state = Rules.new({seats})
                local result, moves = nil, 0
                while not result and moves < 5000 do
                    local seat = Rules.turn(state)
                    local move = Rules.bot(state, seat, levels[seat])
                    assert(Rules.play(state, seat, move), 'the computer made an illegal move')
                    moves += 1
                    result = Rules.result(state)
                end
                print(if result then (result.winner or 0) .. ':' .. result.reason else 'unfinished')"
            );
            session.runtime().run("bots", &script).unwrap();
            endings.push(session.runtime().take_output().last().map(|l| l.text.clone()).unwrap_or_default());
        }
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
        endings
    }

    #[test]
    fn table_games_finish_between_computers() {
        for ending in bots_play(connect_four, 2, "1, 2", 4) {
            assert!(ending.contains("Four in a row") || ending.contains("full"), "{ending}");
        }
        for ending in bots_play(checkers, 2, "1, 2", 3) {
            assert!(!ending.contains("unfinished"), "{ending}");
        }
        for (seats, levels) in [(2, "1, 3"), (3, "2, 2, 3"), (4, "1, 2, 3, 2")] {
            for ending in bots_play(kings_in_the_corner, seats, levels, 5) {
                assert!(!ending.contains("unfinished"), "{ending}");
            }
        }
    }

    #[test]
    fn connect_four_rules() {
        let mut session = start_on(connect_four(), true).unwrap();
        steps(&mut session, 3);
        let check = |session: &libroxel_engine::session::PlaySession, moves: &str| {
            value(
                session,
                &format!(
                    "(function() local Rules = require(game.ReplicatedStorage.TableGame.Rules)
                    local s = Rules.new(2)
                    for _, c in {{ {moves} }} do assert(Rules.play(s, Rules.turn(s), c)) end
                    local r = Rules.result(s)
                    return if r then tostring(r.winner) .. ' ' .. r.reason else 'none' end)()"
                ),
            )
        };
        assert_eq!(check(&session, "1, 2, 1, 2, 1, 2"), "none");
        assert_eq!(check(&session, "1, 2, 1, 2, 1, 2, 1"), "1 Four in a row");
        assert_eq!(check(&session, "1, 2, 2, 3, 4, 3, 3, 4, 5, 4, 4"), "1 Four in a row");
        // A full column takes no more.
        let full = "(function() local Rules = require(game.ReplicatedStorage.TableGame.Rules)
            local s = Rules.new(2)
            for _ = 1, 6 do Rules.play(s, Rules.turn(s), 4) end
            return tostring(Rules.play(s, Rules.turn(s), 4)) end)()";
        assert_eq!(value(&session, full), "false");
        // The computer takes a win, and blocks one.
        let pick = |session: &libroxel_engine::session::PlaySession, moves: &str| {
            value(
                session,
                &format!(
                    "(function() local Rules = require(game.ReplicatedStorage.TableGame.Rules)
                    local s = Rules.new(2)
                    for _, c in {{ {moves} }} do assert(Rules.play(s, Rules.turn(s), c)) end
                    return Rules.bot(s, Rules.turn(s), 3) end)()"
                ),
            )
        };
        assert_eq!(pick(&session, "7, 1, 7, 1, 7, 2"), "7");
        assert_eq!(pick(&session, "1, 7, 2, 7, 4"), "3");
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
    }

    #[test]
    fn checkers_rules() {
        let mut session = start_on(checkers(), true).unwrap();
        steps(&mut session, 3);
        let rules = "local Rules = require(game.ReplicatedStorage.TableGame.Rules)";
        // Seven opening moves for the first player.
        assert_eq!(
            value(&session, &format!("(function() {rules} return #Rules.moves(Rules.new(2), 1) end)()")),
            "7"
        );
        // A double jump is one move, must be taken, and crowns at the end.
        let setup = format!(
            "(function() {rules}
            local s = Rules.new(2)
            s.board = table.create(64, 0)
            s.board[1] = 1        -- a man at the bottom left (row 0, column 0)
            s.board[10] = 2       -- row 1, column 1
            s.board[28] = 2       -- row 3, column 3
            s.board[64] = 2       -- one more far away
            local moves, must = Rules.moves(s, 1)
            local m = moves[1]
            assert(#moves == 1 and must)
            assert(not Rules.play(s, 1, {{ 1, 19 }}), 'stopping halfway is not allowed')
            assert(Rules.play(s, 1, m))
            return table.concat(m, ',') .. ' ' .. s.board[37] .. ' ' .. s.board[10] .. s.board[28] end)()"
        );
        assert_eq!(value(&session, &setup), "1,19,37 1 00");
        let crown = format!(
            "(function() {rules}
            local s = Rules.new(2)
            s.board = table.create(64, 0)
            s.board[49] = 1       -- row 6, column 0
            s.board[64] = 2       -- row 7, column 7
            assert(Rules.play(s, 1, {{ 49, 58 }}))
            return s.board[58] end)()"
        );
        assert_eq!(value(&session, &crown), "3");
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
    }

    #[test]
    fn kings_in_the_corner_rules() {
        let mut session = start_on(kings_in_the_corner(), true).unwrap();
        steps(&mut session, 3);
        let rules = "local Rules = require(game.ReplicatedStorage.TableGame.Rules)";
        // The deal: 7 each, the first player draws one, four on the sides.
        let deal = format!(
            "(function() {rules} local s = Rules.new(3)
            local sides, corners = 0, 0
            for i = 1, 4 do sides += #s.piles[i] end
            for i = 5, 8 do corners += #s.piles[i] end
            return #s.hands[1] .. ' ' .. #s.hands[2] .. ' ' .. #s.hands[3] .. ' ' .. sides .. ' ' .. #s.stock + corners end)()"
        );
        assert_eq!(value(&session, &deal), "8 7 7 4 26");
        // Cards: 7 of hearts is 20, 6 of spades 6, 6 of diamonds 32, King of clubs 52.
        let fits = format!(
            "(function() {rules}
            local piles = {{ {{ 20 }}, {{}}, {{ 6 }}, {{ 32 }}, {{}}, {{}}, {{}}, {{}} }}
            return tostring(Rules.canPlace(piles, 1, 6)) .. tostring(Rules.canPlace(piles, 1, 32))
                .. tostring(Rules.canPlace(piles, 5, 6)) .. tostring(Rules.canPlace(piles, 5, 52))
                .. tostring(Rules.canPlace(piles, 2, 6)) .. tostring(Rules.canMove(piles, 3, 1))
                .. tostring(Rules.canMove(piles, 4, 1)) end)()"
        );
        assert_eq!(value(&session, &fits), "truefalsefalsetruetruetruefalse");
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
    }

    #[test]
    fn table_games_against_the_computer() {
        for (build, screen) in [
            (checkers as fn() -> InstanceTree, "CheckersScreen"),
            (connect_four, "ConnectFourScreen"),
            (kings_in_the_corner, "KingsInTheCornerScreen"),
        ] {
            let mut session = start_on(build(), true).unwrap();
            steps(&mut session, 10);
            let gui = format!("game.Players.Player1.PlayerGui.{screen}");
            // The menu comes first, with its Play button.
            assert_eq!(value(&session, &format!("{gui}.Menu.Visible")), "true", "{screen}");
            assert_eq!(value(&session, &format!("{gui}.Menu.PlayButton.Text")), "Play", "{screen}");
            fire(&session, r#""playBot", { level = 1, opponents = 1 }"#);
            steps(&mut session, 5);
            assert_eq!(value(&session, &format!("{gui}.Panel.Visible")), "true", "{screen}");
            assert_eq!(value(&session, &format!("{gui}.Menu.Visible")), "false", "{screen}");
            assert_eq!(value(&session, &format!("{gui}.Panel.Status.Text")), "Your turn", "{screen}");
            // Make the first move the rules allow and wait for the computer's answer.
            let first = match screen {
                "CheckersScreen" => "{ 19, 28 }",
                "ConnectFourScreen" => "4",
                _ => r#"{ kind = "end" }"#,
            };
            fire(&session, &format!(r#""move", {{ table = "1", move = {first} }}"#));
            steps(&mut session, 5);
            assert_ne!(value(&session, &format!("{gui}.Panel.Status.Text")), "Your turn", "{screen}");
            let mut frames = 0;
            while value(&session, &format!("{gui}.Panel.Status.Text")) != "Your turn" && frames < 1800 {
                steps(&mut session, 10);
                frames += 10;
            }
            assert_eq!(value(&session, &format!("{gui}.Panel.Status.Text")), "Your turn", "{screen}");
            // Leaving gives the game to the computer and goes back to the menu.
            fire(&session, r#""leave""#);
            steps(&mut session, 5);
            assert_eq!(value(&session, &format!("{gui}.Menu.Visible")), "true", "{screen}");
            assert_eq!(value(&session, "game.Players.Player1.leaderstats.Wins.Value"), "0", "{screen}");
            assert!(errors(&mut session).is_empty(), "{screen}: {:?}", errors(&mut session));
        }
    }

    #[test]
    fn connect_four_won_against_the_computer_shows_the_result() {
        let mut session = start_on(connect_four(), true).unwrap();
        steps(&mut session, 10);
        let gui = "game.Players.Player1.PlayerGui.ConnectFourScreen";
        fire(&session, r#""playBot", { level = 1, opponents = 1 }"#);
        steps(&mut session, 5);
        let mut won = false;
        for _ in 0..21 {
            if value(&session, &format!("{gui}.Panel.Status.Text")) != "Your turn" {
                break;
            }
            // Try every column, left first; a full one is refused.
            for column in 1..=7 {
                fire(&session, &format!(r#""move", {{ table = "1", move = {column} }}"#));
                steps(&mut session, 3);
                if value(&session, &format!("{gui}.Panel.Status.Text")) != "Your turn" {
                    break;
                }
            }
            steps(&mut session, 120);
            if value(&session, &format!("{gui}.Panel.Result.Visible")) == "true" {
                won = true;
                break;
            }
        }
        assert!(won, "the game ended");
        let title = value(&session, &format!("{gui}.Panel.Result.Title.Text"));
        assert!(["You win!", "The computer wins", "It's a draw"].contains(&title.as_str()), "{title}");
        assert_eq!(value(&session, &format!("{gui}.Panel.LeaveButton.Text")), "Back to menu");
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
    }

    #[test]
    fn connect_four_between_two_computers() {
        use libroxel_engine::PlaySession;
        use std::{cell::RefCell, rc::Rc};
        let mut host = PlaySession::host(Rc::new(RefCell::new(connect_four())), 0, None, true).unwrap();
        let port = host.host_address().unwrap().port();
        let mut guest = PlaySession::join(
            Rc::new(RefCell::new(InstanceTree::new())),
            &format!("127.0.0.1:{port}"),
            "Bea",
        )
        .unwrap();
        let gui = "game.Players.LocalPlayer.PlayerGui:FindFirstChild('ConnectFourScreen')";
        let until = |host: &mut PlaySession, guest: &mut PlaySession, check: &str, want: &str| {
            for _ in 0..900 {
                host.step(1.0 / 60.0, CharacterInput::default());
                guest.step(1.0 / 60.0, CharacterInput::default());
                if value(guest, check) == want {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            panic!("gave up waiting for {check} to be {want}: it is {}", value(guest, check));
        };
        // The guest's menu lists the host's player.
        let listed = format!("(function() local s = {gui} return s and #s.Menu.People:GetChildren() end)()");
        until(&mut host, &mut guest, &listed, "1");
        // Both look for a game, and are seated together.
        fire(&host, r#""seek""#);
        fire(&guest, r#""seek""#);
        until(&mut host, &mut guest, &format!("{gui}.Panel.Visible"), "true");
        let guest_first = value(&guest, &format!("{gui}.Panel.Status.Text")) == "Your turn";
        let pump = |host: &mut PlaySession, guest: &mut PlaySession| {
            for _ in 0..20 {
                host.step(1.0 / 60.0, CharacterInput::default());
                guest.step(1.0 / 60.0, CharacterInput::default());
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        // Four down column 1 against column 2: whoever started wins.
        for (by_first, column) in
            [(true, 1), (false, 2), (true, 1), (false, 2), (true, 1), (false, 2), (true, 1)]
        {
            let move_ = format!(r#""move", {{ table = "1", move = {column} }}"#);
            if by_first == guest_first {
                fire(&guest, &move_);
            } else {
                fire(&host, &move_);
            }
            pump(&mut host, &mut guest);
        }
        let title = format!("{gui}.Panel.Result.Title.Text");
        until(&mut host, &mut guest, &title, if guest_first { "You win!" } else { "Player1 wins" });
        let wins = "game.Players.LocalPlayer.leaderstats.Wins.Value";
        until(&mut host, &mut guest, wins, if guest_first { "1" } else { "0" });
        assert!(errors(&mut host).is_empty(), "{:?}", errors(&mut host));
        assert!(errors(&mut guest).is_empty(), "{:?}", errors(&mut guest));
    }

    /// Steps a kart race until `check` prints `want`, holding `input`.
    fn race_until(
        session: &mut libroxel_engine::session::PlaySession,
        check: &str,
        want: &str,
        seconds: f32,
        input: CharacterInput,
    ) {
        for _ in 0..(seconds * 60.0) as usize {
            session.step(1.0 / 60.0, input);
            if value(session, check) == want {
                return;
            }
        }
        panic!("gave up waiting for {check} to be {want}: it is {}", value(session, check));
    }

    fn kart_control(session: &libroxel_engine::session::PlaySession, action: &str) {
        session
            .runtime()
            .run("control", &format!("game.ReplicatedStorage.Kart.Control:FireServer({action})"))
            .unwrap();
    }

    const PHASE: &str = "game.ReplicatedStorage.Kart:GetAttribute('Phase')";
    const MY_KART: &str = "(function() for _, k in workspace.Race.Karts:GetChildren() do \
        if k:GetAttribute('UserId') == game.Players.Player1.UserId then return k end end end)()";

    #[test]
    fn a_kart_race_from_the_menu_to_the_results() {
        let mut session = start_on(kart_race(), true).unwrap();
        steps(&mut session, 10);
        let gui = "game.Players.Player1.PlayerGui.KartScreen";
        assert_eq!(value(&session, &format!("{gui}.Menu.Visible")), "true");
        assert_eq!(value(&session, &format!("{gui}.Menu.Start.Text")), "START RACE");
        // One lap, on your own: it starts straight away.
        kart_control(&session, "'laps', 1");
        kart_control(&session, "'start'");
        steps(&mut session, 5);
        assert_eq!(value(&session, PHASE), "countdown");
        assert_eq!(value(&session, "#workspace.Race.Karts:GetChildren()"), "8");
        // You're in your kart, at the back of the grid, and can't hop out.
        steps(&mut session, 5);
        assert_eq!(value(&session, &format!("{MY_KART}.Seat.Occupant.Parent.Name")), "Player1");
        assert_eq!(value(&session, &format!("{MY_KART}:GetAttribute('Place')")), "8");
        let hop = CharacterInput { jump: true, ..Default::default() };
        steps(&mut session, 5);
        session.step(1.0 / 60.0, hop);
        assert_eq!(value(&session, &format!("{MY_KART}.Seat.Occupant ~= nil")), "true");
        race_until(&mut session, PHASE, "race", 6.0, CharacterInput::default());
        // Full throttle: the kart gets going and moves round the lap.
        let gas = CharacterInput { throttle: 1.0, ..Default::default() };
        let start: f32 = value(&session, &format!("{MY_KART}:GetAttribute('Distance')")).parse().unwrap();
        for _ in 0..180 {
            session.step(1.0 / 60.0, gas);
        }
        let speed: f32 = value(&session, &format!("{MY_KART}:GetAttribute('Speed')")).parse().unwrap();
        assert!(speed > 60.0, "up to speed: {speed}");
        let on: f32 = value(&session, &format!("{MY_KART}:GetAttribute('Distance')")).parse().unwrap();
        assert!(on > 5.0 && on < 40.0, "round the lap: {start} -> {on}");
        assert_eq!(value(&session, &format!("{gui}.Hud.Visible")), "true");
        // The computer racers finish, and the results come up when the
        // time after the first finisher runs out, even if you never finish.
        race_until(&mut session, PHASE, "results", 120.0, CharacterInput::default());
        assert_eq!(value(&session, &format!("{gui}.Results.Visible")), "true");
        let finished = "(function() local n = 0 for _, k in workspace.Race.Karts:GetChildren() do \
            if k:GetAttribute('Finished') then n += 1 end end return n end)()";
        let finished: usize = value(&session, finished).parse().unwrap();
        assert!(finished >= 6, "most of the field finished: {finished}");
        assert_ne!(value(&session, "game.Players.Player1.leaderstats.Points.Value"), "0");
        // And back to the menu for another.
        kart_control(&session, "'menu'");
        steps(&mut session, 3);
        assert_eq!(value(&session, PHASE), "menu");
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
    }

    #[test]
    fn every_kart_track_builds_and_loops() {
        let mut session = start_on(kart_race(), true).unwrap();
        steps(&mut session, 5);
        for track in 1..=3 {
            kart_control(&session, &format!("'track', {track}"));
            steps(&mut session, 3);
            assert_eq!(
                value(&session, "game.ReplicatedStorage.Kart:GetAttribute('Track')"),
                track.to_string()
            );
            let samples = format!("require(game.ReplicatedStorage.Kart.Tracks).samples({track})");
            let length: f32 = value(&session, &format!("{samples}.length")).parse().unwrap();
            assert!((900.0..3000.0).contains(&length), "track {track} is {length} studs");
            // The road joins up: the last sample is one step from the first.
            let gap: f32 = value(
                &session,
                &format!("(function() local s = {samples} return (s[#s].p - s[1].p).Magnitude end)()"),
            )
            .parse()
            .unwrap();
            assert!(gap < 14.0, "track {track} closes: {gap}");
            let boxes: usize = value(&session, "#workspace.Race.Track.Boxes:GetChildren()").parse().unwrap();
            assert!(boxes >= 12, "track {track} has item boxes");
            // Boost pads' arrows (drawn toward the pad's +X) point down the road.
            let worst = format!(
                "(function() local T = require(game.ReplicatedStorage.Kart.Tracks) local s = {samples} \
                local worst = 1 for _, pad in workspace.Race.Track.Pads:GetChildren() do \
                local i = T.locateAnywhere(s, pad.Position) \
                worst = math.min(worst, pad.CFrame.RightVector:Dot(s[i].t)) end return worst end)()"
            );
            let worst: f32 = value(&session, &worst).parse().unwrap();
            assert!(worst > 0.9, "track {track}'s boost arrows point along the road: {worst}");
        }
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
    }

    #[test]
    fn drifting_charges_a_boost() {
        let mut session = start_on(kart_race(), true).unwrap();
        steps(&mut session, 5);
        kart_control(&session, "'start'");
        race_until(&mut session, PHASE, "race", 6.0, CharacterInput::default());
        let gas = CharacterInput { throttle: 1.0, ..Default::default() };
        for _ in 0..90 {
            session.step(1.0 / 60.0, gas);
        }
        // Hold Space while steering right, into the slide, for two seconds.
        let space = libroxel_script::UserInput::Key(libroxel_instance::KeyCode::Space);
        session.input_began(space, false);
        let slide = CharacterInput { throttle: 1.0, steer: 1.0, ..Default::default() };
        for _ in 0..70 {
            session.step(1.0 / 60.0, slide);
        }
        assert_ne!(value(&session, &format!("{MY_KART}:GetAttribute('Charge')")), "0", "drifting");
        for _ in 0..60 {
            session.step(1.0 / 60.0, slide);
        }
        let charge: u32 = value(&session, &format!("{MY_KART}:GetAttribute('Charge')")).parse().unwrap();
        assert!(charge >= 2, "charged: {charge}");
        session.input_ended(space, false);
        steps(&mut session, 2);
        assert_eq!(value(&session, &format!("{MY_KART}:GetAttribute('Boost')")), "true");
        assert!(errors(&mut session).is_empty(), "{:?}", errors(&mut session));
    }

    #[test]
    fn kart_racing_with_someone_who_joined() {
        use libroxel_engine::PlaySession;
        use std::{cell::RefCell, rc::Rc};
        let mut host = PlaySession::host(Rc::new(RefCell::new(kart_race())), 0, None, true).unwrap();
        let port = host.host_address().unwrap().port();
        let mut guest = PlaySession::join(
            Rc::new(RefCell::new(InstanceTree::new())),
            &format!("127.0.0.1:{port}"),
            "Bea",
        )
        .unwrap();
        let idle = CharacterInput::default();
        let until = |host: &mut PlaySession, guest: &mut PlaySession, check: &str, want: &str, input| {
            for _ in 0..1200 {
                host.step(1.0 / 60.0, idle);
                guest.step(1.0 / 60.0, input);
                if value(guest, check) == want {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            panic!("gave up waiting for {check} to be {want}: it is {}", value(guest, check));
        };
        let gui = "game.Players.LocalPlayer.PlayerGui:FindFirstChild('KartScreen')";
        until(&mut host, &mut guest, &format!("{gui} ~= nil and {gui}.Menu.Visible"), "true", idle);
        // The guest votes for the canyon and starts; both race there.
        let control = |session: &PlaySession, action: &str| {
            session
                .runtime()
                .run("control", &format!("game.ReplicatedStorage.Kart.Control:FireServer({action})"))
                .unwrap();
        };
        control(&guest, "'track', 2");
        control(&guest, "'start'");
        let phase = "game.ReplicatedStorage.Kart:GetAttribute('Phase')";
        until(&mut host, &mut guest, phase, "race", idle);
        assert_eq!(value(&host, "game.ReplicatedStorage.Kart:GetAttribute('Track')"), "2");
        let mine = "(function() for _, k in workspace.Race.Karts:GetChildren() do \
            if k:GetAttribute('UserId') == game.Players.LocalPlayer.UserId then return k end end end)()";
        assert_eq!(value(&guest, "#workspace.Race.Karts:GetChildren()"), "8");
        assert_eq!(value(&guest, &format!("{mine}.Seat.Occupant.Parent.Name")), "Bea");
        // The guest's throttle drives the guest's kart, on the host.
        let gas = CharacterInput { throttle: 1.0, ..Default::default() };
        let speed = format!("{mine}:GetAttribute('Speed') > 50");
        until(&mut host, &mut guest, &speed, "true", gas);
        assert_eq!(value(&guest, &format!("{gui}.Hud.Visible")), "true");
        assert!(errors(&mut host).is_empty(), "{:?}", errors(&mut host));
        assert!(errors(&mut guest).is_empty(), "{:?}", errors(&mut guest));
    }
}
