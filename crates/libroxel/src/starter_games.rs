//! Starter games: small, finished games to play straight away, and places
//! to start from in the editor.

use libroxel_instance::{
    ClassKind, Color3, InstanceId, InstanceTree, Material, NormalId, PartShape, Property, PropertyValue,
    UDim, UDim2, Vec2, Vec3,
};

use crate::templates::{add, block, code, kart, material, picture, script, set, shape};

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
        about: "Three laps round the track in a kart",
        build: kart_race,
    },
    StarterGame {
        name: "Chess",
        symbol: crate::theme::icon::HORSE,
        about: "Chess against eight computer players or anyone in the server",
        build: chess,
    },
];

/// Finds a starter game by name.
pub fn starter(name: &str) -> Option<&'static StarterGame> {
    STARTER_GAMES.iter().find(|game| game.name == name)
}

const HUD_SCRIPT: &str = r#"
-- Shows the game's status at the top of the screen, and short messages
-- for this player under it.
local storage = game:GetService("ReplicatedStorage")
local hud = script.Parent
local status = storage:WaitForChild("Status")

local function show()
	hud.Status.Text = status.Value
end
status:GetPropertyChangedSignal("Value"):Connect(show)
show()

local shown = 0
storage:WaitForChild("Notify").OnClientEvent:Connect(function(text)
	shown += 1
	local mine = shown
	hud.Message.Text = text
	hud.Message.Visible = true
	task.wait(3)
	if shown == mine then
		hud.Message.Visible = false
	end
end)
"#;

/// A status line at the top of the screen and a message box under it, fed
/// by a `Status` value and a `Notify` event in ReplicatedStorage.
fn status_hud(tree: &mut InstanceTree, first: &str) {
    let storage = tree.service(ClassKind::ReplicatedStorage).expect("every tree has storage");
    let status = add(tree, storage, ClassKind::StringValue, "Status");
    set(tree, status, Property::StringValue, PropertyValue::String(first.into()));
    add(tree, storage, ClassKind::RemoteEvent, "Notify");

    let starter_gui = tree.service(ClassKind::StarterGui).expect("every tree has StarterGui");
    let hud = add(tree, starter_gui, ClassKind::ScreenGui, "HUD");
    let dark = Color3::from_rgb8(25, 30, 40);
    let white = Color3::from_rgb8(255, 255, 255);
    for (name, y, height, size) in [("Status", 10.0, 40.0, 20.0), ("Message", 58.0, 34.0, 18.0)] {
        let label = add(tree, hud, ClassKind::TextLabel, name);
        set(tree, label, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(0.5, 0.0, 0.0, y)));
        set(tree, label, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 460.0, 0.0, height)));
        set(tree, label, Property::AnchorPoint, PropertyValue::Vec2(Vec2::new(0.5, 0.0)));
        set(tree, label, Property::BackgroundColor, PropertyValue::Color3(dark));
        set(tree, label, Property::BackgroundTransparency, PropertyValue::Number(0.3));
        set(tree, label, Property::TextColor, PropertyValue::Color3(white));
        set(tree, label, Property::TextSize, PropertyValue::Number(size));
        let corner = add(tree, label, ClassKind::UICorner, "UICorner");
        set(tree, corner, Property::CornerRadius, PropertyValue::UDim(UDim::new(0.0, 8.0)));
        if name == "Message" {
            set(tree, label, Property::Visible, PropertyValue::Bool(false));
        }
    }
    code(tree, hud, ClassKind::LocalScript, "HudScript", HUD_SCRIPT);
}

fn spawn_at(tree: &mut InstanceTree, parent: InstanceId, at: [f32; 3]) {
    let spawn = add(tree, parent, ClassKind::SpawnLocation, "SpawnLocation");
    set(tree, spawn, Property::Position, PropertyValue::Vec3(Vec3::new(at[0], at[1], at[2])));
    set(tree, spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0)));
}

fn tree_at(tree: &mut InstanceTree, parent: InstanceId, x: f32, z: f32) {
    let trunk = block(tree, parent, "Trunk", [x, 2.0, z], [1.0, 4.0, 1.0], [120, 85, 55]);
    material(tree, trunk, Material::Wood);
    let leaves = block(tree, parent, "Leaves", [x, 5.5, z], [5.0, 5.0, 5.0], [70, 150, 70]);
    shape(tree, leaves, PartShape::Ball);
    material(tree, leaves, Material::Grass);
}

const RACE_SCRIPT: &str = r#"
-- The race: drive through the blue gates in order and over the finish
-- line to finish a lap. The first to finish three laps wins, then the
-- race starts again. Your laps and best lap time are on the leaderboard.
local Players = game:GetService("Players")
local storage = game:GetService("ReplicatedStorage")
local status = storage:WaitForChild("Status")
local notify = storage:WaitForChild("Notify")
local gates = workspace:WaitForChild("Gates")
local order = { gates.Gate1, gates.Gate2, gates.Gate3, gates.Finish }
local LAPS = 3

local cheer = Instance.new("Sound")
cheer.SoundId = "libroxel://chime"
cheer.Parent = gates.Finish

-- Where each player is in the race, by their UserId.
local racers = {}

Players.PlayerAdded:Connect(function(player)
	local stats = Instance.new("Folder")
	stats.Name = "leaderstats"
	stats.Parent = player
	local laps = Instance.new("IntValue")
	laps.Name = "Laps"
	laps.Parent = stats
	local best = Instance.new("NumberValue")
	best.Name = "Best"
	best.Parent = stats
	racers[player.UserId] = { next = 1, started = time() }
end)

Players.PlayerRemoving:Connect(function(player)
	racers[player.UserId] = nil
end)

local function inside(gate, position)
	local offset = position - gate.Position
	local half = gate.Size / 2
	return math.abs(offset.X) <= half.X and math.abs(offset.Y) <= half.Y and math.abs(offset.Z) <= half.Z
end

local function restart()
	for _, player in Players:GetPlayers() do
		player.leaderstats.Laps.Value = 0
		racers[player.UserId] = { next = 1, started = time() }
	end
	status.Value = "Drive through the blue gates. First to " .. LAPS .. " laps wins!"
end

restart()
while true do
	task.wait(0.1)
	for _, player in Players:GetPlayers() do
		local racer = racers[player.UserId]
		local root = player.Character and player.Character:FindFirstChild("HumanoidRootPart")
		if racer and root and inside(order[racer.next], root.Position) then
			if racer.next < #order then
				racer.next += 1
			else
				-- Over the finish line after every gate: a lap.
				local lap = math.floor((time() - racer.started) * 10 + 0.5) / 10
				local stats = player.leaderstats
				stats.Laps.Value += 1
				if stats.Best.Value == 0 or lap < stats.Best.Value then
					stats.Best.Value = lap
				end
				racer.next, racer.started = 1, time()
				if stats.Laps.Value >= LAPS then
					cheer:Play()
					status.Value = player.Name .. " wins the race!"
					notify:FireAllClients(player.Name .. " wins the race!")
					task.wait(6)
					restart()
				else
					notify:FireClient(player, "Lap " .. stats.Laps.Value .. " of " .. LAPS .. ": " .. lap .. " seconds")
				end
			end
		end
	end
end
"#;

/// A kart race round a rectangular track: four karts on the grid, three
/// gates and a finish line, with laps and best times on the leaderboard.
pub fn kart_race() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let ground = block(&mut tree, ws, "Baseplate", [0.0, -0.5, 0.0], [240.0, 1.0, 200.0], [99, 140, 88]);
    material(&mut tree, ground, Material::Grass);

    // The track: four straights round a grassy island, with low walls a
    // character can step over but a kart can't drive through.
    let track = add(&mut tree, ws, ClassKind::Folder, "Track");
    let asphalt = [62, 64, 70];
    for (name, at, size) in [
        ("SouthStraight", [0.0, 35.0], [136.0, 16.0]),
        ("NorthStraight", [0.0, -35.0], [136.0, 16.0]),
        ("WestStraight", [-60.0, 0.0], [16.0, 54.0]),
        ("EastStraight", [60.0, 0.0], [16.0, 54.0]),
    ] {
        let road = block(&mut tree, track, name, [at[0], 0.1, at[1]], [size[0], 0.2, size[1]], asphalt);
        material(&mut tree, road, Material::SmoothPlastic);
    }
    let walls = add(&mut tree, track, ClassKind::Folder, "Walls");
    let (red, white) = ([210, 60, 50], [235, 235, 235]);
    for (i, (at, size)) in [
        ([0.0, 43.5], [138.0, 1.0]),
        ([0.0, -43.5], [138.0, 1.0]),
        ([-68.5, 0.0], [1.0, 88.0]),
        ([68.5, 0.0], [1.0, 88.0]),
        ([0.0, 26.5], [105.0, 1.0]),
        ([0.0, -26.5], [105.0, 1.0]),
        ([-52.0, 0.0], [1.0, 52.0]),
        ([52.0, 0.0], [1.0, 52.0]),
    ]
    .into_iter()
    .enumerate()
    {
        let colour = if i % 2 == 0 { red } else { white };
        block(
            &mut tree,
            walls,
            &format!("Wall{}", i + 1),
            [at[0], 0.25, at[1]],
            [size[0], 0.5, size[1]],
            colour,
        );
    }
    // A chequered finish line with an arch over it.
    // It's paint on the road: a kart's flat underside would catch on even
    // this thin a lip and stop dead, so nothing collides with it.
    let line = block(&mut tree, track, "FinishLine", [0.0, 0.21, 35.0], [2.0, 0.02, 16.0], [255, 255, 255]);
    set(&mut tree, line, Property::CanCollide, PropertyValue::Bool(false));
    picture(&mut tree, line, "checker", NormalId::Top, Some(2.0));
    for z in [26.0, 44.0] {
        block(&mut tree, track, "ArchPost", [0.0, 4.0, z], [1.0, 8.0, 1.0], [235, 235, 235]);
    }
    let banner = block(&mut tree, track, "ArchBanner", [0.0, 8.5, 35.0], [1.0, 1.4, 19.0], [255, 255, 255]);
    picture(&mut tree, banner, "checker", NormalId::Right, Some(1.4));
    picture(&mut tree, banner, "checker", NormalId::Left, Some(1.4));

    // The gates the race script checks, in order. You drive through them;
    // they don't stop anything.
    let gates = add(&mut tree, ws, ClassKind::Folder, "Gates");
    for (name, at, size) in [
        ("Gate1", [-60.0, 0.0], [16.0, 3.0]),
        ("Gate2", [0.0, -35.0], [3.0, 16.0]),
        ("Gate3", [60.0, 0.0], [16.0, 3.0]),
        ("Finish", [0.0, 35.0], [3.0, 16.0]),
    ] {
        let gate =
            block(&mut tree, gates, name, [at[0], 3.0, at[1]], [size[0], 6.0, size[1]], [80, 160, 255]);
        material(&mut tree, gate, Material::Neon);
        set(&mut tree, gate, Property::CanCollide, PropertyValue::Bool(false));
        let see_through = if name == "Finish" { 1.0 } else { 0.85 };
        set(&mut tree, gate, Property::Transparency, PropertyValue::Number(see_through));
    }

    // Four karts on the grid, behind the line, facing down the straight.
    let grid = add(&mut tree, ws, ClassKind::Folder, "Karts");
    let colours = [[220, 60, 60], [60, 120, 220], [250, 190, 50], [70, 180, 90]];
    for (i, (x, z)) in [(7.0, 31.0), (7.0, 39.0), (14.0, 31.0), (14.0, 39.0)].into_iter().enumerate() {
        kart(&mut tree, grid, Vec3::new(x, 0.2, z), 90.0, colours[i]);
    }

    // Players start beside the grid, in the pit by the south straight.
    let pit = add(&mut tree, ws, ClassKind::Folder, "Pit");
    let pad = block(&mut tree, pit, "PitFloor", [10.0, 0.1, 52.0], [24.0, 0.2, 14.0], [176, 160, 150]);
    material(&mut tree, pad, Material::Brick);
    spawn_at(&mut tree, pit, [10.0, 0.5, 52.0]);
    for (x, z) in [(-30.0, 0.0), (-10.0, -10.0), (15.0, 8.0), (35.0, -5.0), (0.0, 12.0)] {
        tree_at(&mut tree, ws, x, z);
    }

    status_hud(&mut tree, "Get in a kart and drive through the blue gates!");
    let server = tree.service(ClassKind::ServerScriptService).expect("every tree has ServerScriptService");
    script(&mut tree, server, "Race", RACE_SCRIPT);
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

    #[test]
    fn a_lap_needs_every_gate_in_order() {
        let mut session = start_on(kart_race(), true).unwrap();
        session.step(1.0 / 60.0, CharacterInput::default());
        let visit = |session: &mut libroxel_engine::session::PlaySession, gate: &str| {
            let move_to = format!(
                "workspace.Player1.HumanoidRootPart.Position = workspace.Gates.{gate}.Position + Vector3.new(0, 0, 0)"
            );
            session.runtime().run("drive", &move_to).unwrap();
            for _ in 0..12 {
                session.step(1.0 / 60.0, CharacterInput::default());
            }
        };
        // Straight to the finish doesn't count.
        visit(&mut session, "Finish");
        assert_eq!(value(&session, "game.Players.Player1.leaderstats.Laps.Value"), "0");
        for gate in ["Gate1", "Gate2", "Gate3", "Finish"] {
            visit(&mut session, gate);
        }
        assert_eq!(value(&session, "game.Players.Player1.leaderstats.Laps.Value"), "1");
        assert_ne!(value(&session, "game.Players.Player1.leaderstats.Best.Value"), "0");
        assert!(errors(&mut session).is_empty());
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

    /// Drives the first kart off the grid with the throttle held and
    /// returns how far west it got (the finish line is at x = 0).
    fn drive_off_the_grid(frames: usize) -> (f32, f32) {
        let mut session = start_on(kart_race(), true).unwrap();
        for _ in 0..30 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        session
            .runtime()
            .run(
                "hop",
                "local seat = workspace.Karts:GetChildren()[1].VehicleSeat
                workspace.Player1.HumanoidRootPart.Position = seat.Position + Vector3.new(0, 3, 0)",
            )
            .unwrap();
        for _ in 0..30 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        let start: f32 =
            value(&session, "workspace.Karts:GetChildren()[1].Chassis.Position.X").parse().unwrap();
        for _ in 0..frames {
            session.step(1.0 / 60.0, CharacterInput { throttle: 1.0, ..Default::default() });
        }
        let end: f32 =
            value(&session, "workspace.Karts:GetChildren()[1].Chassis.Position.X").parse().unwrap();
        assert!(errors(&mut session).is_empty());
        (start, end)
    }

    #[test]
    fn karts_drive_through_the_finish_line() {
        let (start, end) = drive_off_the_grid(180);
        assert!(start > 5.0, "on the grid behind the line: {start}");
        assert!(end < -10.0, "drove over the line and on down the straight: {start} -> {end}");
    }

    #[test]
    fn players_walk_through_the_finish_line() {
        let mut session = start_on(kart_race(), true).unwrap();
        for _ in 0..30 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        session
            .runtime()
            .run("put", "workspace.Player1.HumanoidRootPart.Position = Vector3.new(6, 3, 35)")
            .unwrap();
        for _ in 0..30 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        // Walk west (-X) for three seconds.
        let west =
            CharacterInput { move_dir: libroxel_instance::Vec3::new(-1.0, 0.0, 0.0), ..Default::default() };
        for _ in 0..180 {
            session.step(1.0 / 60.0, west);
        }
        let x: f32 = value(&session, "workspace.Player1.HumanoidRootPart.Position.X").parse().unwrap();
        assert!(x < -10.0, "walked over the line: {x}");
    }
}
