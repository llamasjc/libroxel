//! Starter games beyond the sample obby: a kart race and a coin rush.
//! Each one is a small, finished game to play straight away, and a place
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
        name: "Sample obby",
        symbol: "🏁",
        about: "An obby with coins, TNT and a rover",
        build: crate::templates::sample_obby,
    },
    StarterGame {
        name: "Kart race",
        symbol: "🚗",
        about: "Three laps round the track in a kart",
        build: kart_race,
    },
    StarterGame {
        name: "Coin rush",
        symbol: "💰",
        about: "Beat a robot to the coins before time runs out",
        build: coin_rush,
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
    let line = block(&mut tree, track, "FinishLine", [0.0, 0.21, 35.0], [2.0, 0.02, 16.0], [255, 255, 255]);
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

const ROUNDS_SCRIPT: &str = r#"
-- Coin rush: coins keep appearing around the arena. Grab as many as you
-- can before the time runs out; whoever has the most wins the round.
-- Watch out for the spinning lava bar in the middle.
local Players = game:GetService("Players")
local storage = game:GetService("ReplicatedStorage")
local status = storage:WaitForChild("Status")
local notify = storage:WaitForChild("Notify")
local spots = workspace:WaitForChild("CoinSpots"):GetChildren()
local ROUND, BREAK, MOST_COINS = 60, 8, 12

Players.PlayerAdded:Connect(function(player)
	local stats = Instance.new("Folder")
	stats.Name = "leaderstats"
	stats.Parent = player
	local coins = Instance.new("IntValue")
	coins.Name = "Coins"
	coins.Parent = stats
	local wins = Instance.new("IntValue")
	wins.Name = "Wins"
	wins.Parent = stats
end)

local live = {}

local function dropCoin()
	local spot = spots[math.random(1, #spots)]
	local coin = Instance.new("Part")
	coin.Name = "Coin"
	coin.Shape = Enum.PartType.Cylinder
	coin.Size = Vector3.new(0.4, 2.2, 2.2)
	coin.Color = Color3.fromRGB(255, 200, 40)
	coin.Material = Enum.Material.Neon
	coin.Anchored = true
	coin.CanCollide = false
	coin.Position = spot.Position + Vector3.new(math.random(-3, 3), 2, math.random(-3, 3))
	local ding = Instance.new("Sound")
	ding.SoundId = "libroxel://coin"
	ding.Parent = coin
	coin.Parent = workspace
	table.insert(live, coin)
	local taken = false
	coin.Touched:Connect(function(hit)
		local player = Players:GetPlayerFromCharacter(hit.Parent)
		local robot = hit.Parent and hit.Parent.Name == "Robot"
		if taken or not (player or robot) then
			return
		end
		taken = true
		if player then
			ding:Play()
			player.leaderstats.Coins.Value += 1
		end
		coin.Transparency = 1
		task.wait(0.5)
		coin:Destroy()
	end)
end

local function clearCoins()
	for _, coin in live do
		if coin.Parent then
			coin:Destroy()
		end
	end
	live = {}
end

-- Coins spin, and so does the lava bar.
local bar = workspace.Arena.Sweeper
game:GetService("RunService").Heartbeat:Connect(function(dt)
	for _, coin in live do
		if coin.Parent then
			local o = coin.Orientation
			coin.Orientation = Vector3.new(o.X, (o.Y + 120 * dt) % 360, o.Z)
		end
	end
	local o = bar.Orientation
	bar.Orientation = Vector3.new(0, (o.Y + 40 * dt) % 360, 0)
end)
bar.Touched:Connect(function(hit)
	local humanoid = hit.Parent and hit.Parent:FindFirstChild("Humanoid")
	-- The robot is fireproof.
	if humanoid and hit.Parent.Name ~= "Robot" then
		humanoid.Health = 0
	end
end)

while true do
	for _, player in Players:GetPlayers() do
		player.leaderstats.Coins.Value = 0
	end
	for left = ROUND, 1, -1 do
		status.Value = "Grab the coins! " .. left .. " seconds left"
		local count = 0
		for _, coin in live do
			if coin.Parent then
				count += 1
			end
		end
		if count < MOST_COINS then
			dropCoin()
		end
		task.wait(1)
	end
	clearCoins()
	local best, winner = 0, nil
	for _, player in Players:GetPlayers() do
		local coins = player.leaderstats.Coins.Value
		if coins > best then
			best, winner = coins, player
		end
	end
	if winner then
		winner.leaderstats.Wins.Value += 1
		notify:FireAllClients(winner.Name .. " wins with " .. best .. " coins!")
	end
	for left = BREAK, 1, -1 do
		local who = if winner then winner.Name .. " won! " else "Nobody got a coin. "
		status.Value = who .. "Next round in " .. left
		task.wait(1)
	end
end
"#;

const ROBOT_SCRIPT: &str = r#"
-- The robot hunts for coins too. It finds a way to the nearest one with
-- the pathfinding service, round the pillar and up the ramps, and any coin
-- it reaches is gone. Beat it there!
local PathfindingService = game:GetService("PathfindingService")
local robot = script.Parent
local humanoid = robot.Humanoid
local root = robot.HumanoidRootPart
humanoid.WalkSpeed = 9
local path = PathfindingService:CreatePath({ AgentRadius = 1, AgentHeight = 5 })

local function nearestCoin()
	local best, bestDistance = nil, math.huge
	for _, thing in workspace:GetChildren() do
		if thing.Name == "Coin" and thing.Transparency < 1 then
			local distance = (thing.Position - root.Position).Magnitude
			if distance < bestDistance then
				best, bestDistance = thing, distance
			end
		end
	end
	return best
end

while true do
	local coin = nearestCoin()
	if coin then
		path:ComputeAsync(root.Position, coin.Position)
		if path.Status == Enum.PathStatus.Success then
			for _, waypoint in path:GetWaypoints() do
				if not coin.Parent or coin.Transparency >= 1 then
					break
				end
				if waypoint.Action == Enum.PathWaypointAction.Jump then
					humanoid.Jump = true
				end
				humanoid:MoveTo(waypoint.Position)
				humanoid.MoveToFinished:Wait()
			end
		end
	end
	task.wait(0.3)
end
"#;

/// A robot that walks itself (with a script), made like a character.
fn robot(tree: &mut InstanceTree, at: [f32; 3]) -> InstanceId {
    use libroxel_engine::character::{BodyColors, Hat, build_rig, pose_rig, wear_hat};
    let ws = tree.workspace();
    let model = add(tree, ws, ClassKind::Model, "Robot");
    let root = block(tree, model, "HumanoidRootPart", at, [2.0, 4.0, 1.5], [150, 150, 160]);
    set(tree, root, Property::Anchored, PropertyValue::Bool(false));
    set(tree, root, Property::Transparency, PropertyValue::Number(1.0));
    let colors = BodyColors {
        skin: Color3::from_rgb8(170, 175, 185),
        shirt: Color3::from_rgb8(90, 110, 140),
        pants: Color3::from_rgb8(70, 75, 90),
    };
    let (head, limbs) = build_rig(tree, model, colors);
    for part in limbs.parts().into_iter().chain([head]) {
        material(tree, part, Material::Metal);
    }
    pose_rig(tree, root, head, &limbs, Default::default());
    wear_hat(tree, model, Hat::Antenna, Color3::from_rgb8(255, 70, 60));
    add(tree, model, ClassKind::Humanoid, "Humanoid");
    script(tree, model, "Hunt", ROBOT_SCRIPT);
    model
}

/// A walled arena with platforms and ramps, where coins keep appearing for
/// a minute at a time. The most coins wins the round.
pub fn coin_rush() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    let arena = add(&mut tree, ws, ClassKind::Folder, "Arena");
    let floor = block(&mut tree, arena, "Floor", [0.0, -0.5, 0.0], [96.0, 1.0, 96.0], [176, 160, 150]);
    material(&mut tree, floor, Material::Brick);
    let stone = [120, 120, 135];
    for (name, at, size) in [
        ("NorthWall", [0.0, -48.5], [98.0, 1.0]),
        ("SouthWall", [0.0, 48.5], [98.0, 1.0]),
        ("WestWall", [-48.5, 0.0], [1.0, 96.0]),
        ("EastWall", [48.5, 0.0], [1.0, 96.0]),
    ] {
        block(&mut tree, arena, name, [at[0], 3.0, at[1]], [size[0], 6.0, size[1]], stone);
    }
    // Raised platforms in the corners, each with a ramp up from the middle.
    let platforms = add(&mut tree, arena, ClassKind::Folder, "Platforms");
    let spots = add(&mut tree, ws, ClassKind::Folder, "CoinSpots");
    let colours = [[230, 90, 80], [80, 150, 230], [240, 190, 60], [90, 190, 110]];
    for (i, (sx, sz)) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
        let (x, z) = (sx * 34.0, sz * 34.0);
        let top = block(&mut tree, platforms, "Platform", [x, 2.0, z], [16.0, 4.0, 16.0], colours[i]);
        material(&mut tree, top, Material::SmoothPlastic);
        // The ramp runs from the floor (towards the middle) up to the platform.
        let ramp = block(&mut tree, platforms, "Ramp", [x, 2.0, z - sz * 14.0], [8.0, 4.0, 12.0], colours[i]);
        shape(&mut tree, ramp, PartShape::Wedge);
        let facing = if sz > 0.0 { 0.0 } else { 180.0 };
        crate::templates::orient(&mut tree, ramp, [0.0, facing, 0.0]);
        material(&mut tree, ramp, Material::SmoothPlastic);
        let spot = block(&mut tree, spots, "Spot", [x, 4.0, z], [1.0, 1.0, 1.0], [255, 255, 255]);
        set(&mut tree, spot, Property::Transparency, PropertyValue::Number(1.0));
        set(&mut tree, spot, Property::CanCollide, PropertyValue::Bool(false));
    }
    // Coins also appear around the floor.
    for (x, z) in [(0.0, -30.0), (0.0, 30.0), (-30.0, 0.0), (30.0, 0.0), (-16.0, 16.0), (16.0, -16.0)] {
        let spot = block(&mut tree, spots, "Spot", [x, 0.0, z], [1.0, 1.0, 1.0], [255, 255, 255]);
        set(&mut tree, spot, Property::Transparency, PropertyValue::Number(1.0));
        set(&mut tree, spot, Property::CanCollide, PropertyValue::Bool(false));
    }
    // The spinning lava bar in the middle, round a pillar; jump over it.
    let pillar = block(&mut tree, arena, "Pillar", [0.0, 1.5, 0.0], [3.0, 3.0, 3.0], [90, 90, 100]);
    shape(&mut tree, pillar, PartShape::Cylinder);
    crate::templates::orient(&mut tree, pillar, [0.0, 0.0, 90.0]);
    let bar = block(&mut tree, arena, "Sweeper", [0.0, 1.0, 0.0], [44.0, 1.0, 1.0], [255, 80, 30]);
    material(&mut tree, bar, Material::Neon);
    set(&mut tree, bar, Property::CanCollide, PropertyValue::Bool(false));

    spawn_at(&mut tree, ws, [0.0, 0.5, 40.0]);
    status_hud(&mut tree, "Coin rush is starting!");
    let server = tree.service(ClassKind::ServerScriptService).expect("every tree has ServerScriptService");
    script(&mut tree, server, "Rounds", ROUNDS_SCRIPT);
    robot(&mut tree, [-20.0, 2.0, 20.0]);
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

    #[test]
    fn coin_rush_drops_coins_and_counts_them() {
        let mut session = start_on(coin_rush(), true).unwrap();
        for _ in 0..90 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        assert!(value(&session, "game.ReplicatedStorage.Status.Value").contains("seconds left"));
        // Stand on a coin the robot hasn't taken (with the lava bar made
        // harmless, since the coin may have landed where it sweeps).
        session
            .runtime()
            .run(
                "grab",
                "workspace.Arena.Sweeper.CanTouch = false
                for _, coin in workspace:GetChildren() do
                    if coin.Name == 'Coin' and coin.Transparency == 0 then
                        workspace.Player1.HumanoidRootPart.Position = coin.Position
                        break
                    end
                end",
            )
            .unwrap();
        for _ in 0..30 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        // Two coins can land on the same spot, so this may pick up both.
        let coins: i32 = value(&session, "game.Players.Player1.leaderstats.Coins.Value").parse().unwrap();
        assert!(coins >= 1, "{coins}");
        assert!(errors(&mut session).is_empty());
    }

    #[test]
    fn the_coin_rush_robot_goes_after_coins() {
        let mut session = start_on(coin_rush(), true).unwrap();
        for _ in 0..60 * 8 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        assert!(errors(&mut session).is_empty());
        let moved = value(
            &session,
            "(workspace.Robot.HumanoidRootPart.Position - Vector3.new(-20, 2, 20)).Magnitude",
        );
        assert!(moved.parse::<f32>().unwrap() > 5.0, "{moved}");
        assert_eq!(value(&session, "workspace.Robot.Humanoid.Health > 0"), "true");
    }
}
