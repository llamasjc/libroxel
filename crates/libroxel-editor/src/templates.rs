//! Starter worlds: the blank template and a sample obby.

use libroxel_instance::{
    ClassKind, Color3, EnumValue, InstanceId, InstanceTree, Material, PartShape, Property, PropertyValue,
    Vec3,
};

fn add(tree: &mut InstanceTree, parent: InstanceId, class: ClassKind, name: &str) -> InstanceId {
    let id = tree.create(class, name).expect("template classes are creatable");
    tree.set_parent(id, Some(parent)).expect("template parents are valid");
    id
}

fn block(
    tree: &mut InstanceTree,
    parent: InstanceId,
    name: &str,
    pos: [f32; 3],
    size: [f32; 3],
    rgb: [u8; 3],
) -> InstanceId {
    let id = add(tree, parent, ClassKind::Part, name);
    let set = |tree: &mut InstanceTree, p, v| tree.set(id, p, v).expect("template properties are valid");
    set(tree, Property::Position, PropertyValue::Vec3(Vec3::new(pos[0], pos[1], pos[2])));
    set(tree, Property::Size, PropertyValue::Vec3(Vec3::new(size[0], size[1], size[2])));
    set(tree, Property::Color, PropertyValue::Color3(Color3::from_rgb8(rgb[0], rgb[1], rgb[2])));
    set(tree, Property::Anchored, PropertyValue::Bool(true));
    id
}

fn set(tree: &mut InstanceTree, id: InstanceId, property: Property, value: PropertyValue) {
    tree.set(id, property, value).expect("template properties are valid");
}

fn material(tree: &mut InstanceTree, id: InstanceId, material: Material) {
    set(tree, id, Property::Material, PropertyValue::Enum(EnumValue::Material(material)));
}

fn shape(tree: &mut InstanceTree, id: InstanceId, shape: PartShape) {
    set(tree, id, Property::Shape, PropertyValue::Enum(EnumValue::Shape(shape)));
}

fn orient(tree: &mut InstanceTree, id: InstanceId, degrees: [f32; 3]) {
    set(tree, id, Property::Orientation, PropertyValue::Vec3(Vec3::new(degrees[0], degrees[1], degrees[2])));
}

fn script(tree: &mut InstanceTree, parent: InstanceId, name: &str, source: &str) -> InstanceId {
    code(tree, parent, ClassKind::Script, name, source)
}

fn code(
    tree: &mut InstanceTree,
    parent: InstanceId,
    class: ClassKind,
    name: &str,
    source: &str,
) -> InstanceId {
    let id = add(tree, parent, class, name);
    tree.set(id, Property::Source, PropertyValue::String(source.trim_start().to_owned()))
        .expect("scripts have Source");
    id
}

/// A baseplate and a spawn location.
pub fn blank() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    block(&mut tree, ws, "Baseplate", [0.0, -0.5, 0.0], [128.0, 1.0, 128.0], [99, 140, 88]);
    let spawn = add(&mut tree, ws, ClassKind::SpawnLocation, "SpawnLocation");
    tree.set(spawn, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.5, 0.0))).unwrap();
    tree.set(spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0))).unwrap();
    tree.drain_changes();
    tree
}

const KILL_SCRIPT: &str = r#"
-- Anything with a Humanoid that touches this part loses all its health.
local part = script.Parent

part.Touched:Connect(function(hit)
	local humanoid = hit.Parent and hit.Parent:FindFirstChild("Humanoid")
	if humanoid then
		humanoid.Health = 0
	end
end)
"#;

const FADE_SCRIPT: &str = r#"
-- Every platform in this folder fades out and comes back, one after another.
local platforms = script.Parent:GetChildren()

while true do
	for _, platform in platforms do
		if platform:IsA("Part") then
			platform.Transparency = 0.7
			platform.CanCollide = false
			task.wait(0.8)
			platform.Transparency = 0
			platform.CanCollide = true
		end
	end
	task.wait(0.4)
end
"#;

const LIFT_SCRIPT: &str = r#"
-- Moves the lift up and down forever. Stand on it to ride.
local lift = script.Parent
local bottom = lift.Position
local height = 10
local speed = 0.6

game:GetService("RunService").Heartbeat:Connect(function()
	local t = (math.sin(time() * speed * math.pi) + 1) / 2
	lift.Position = bottom + Vector3.new(0, height * t, 0)
end)
"#;

const FINISH_SCRIPT: &str = r#"
-- Celebrate when a player reaches the end.
local pad = script.Parent
local celebrated = false

pad.Touched:Connect(function(hit)
	local humanoid = hit.Parent and hit.Parent:FindFirstChild("Humanoid")
	if not humanoid or celebrated then
		return
	end
	celebrated = true
	print(hit.Parent.Name .. " finished the obby!")

	-- Rain down some confetti blocks.
	for i = 1, 30 do
		local bit = Instance.new("Part")
		bit.Size = Vector3.new(0.6, 0.6, 0.6)
		bit.Position = pad.Position + Vector3.new(math.random(-6, 6), 15 + i * 0.5, math.random(-6, 6))
		bit.Color = Color3.fromRGB(math.random(80, 255), math.random(80, 255), math.random(80, 255))
		bit.CanCollide = false
		bit.Parent = workspace
		task.delay(4, function()
			bit:Destroy()
		end)
	end

	task.wait(5)
	celebrated = false
end)
"#;

const WELCOME_SCRIPT: &str = r#"
print("Welcome to the sample obby! Reach the gold pad. Red blocks are lava.")
print("Grab the spinning coins on the way. Your score is on the leaderboard.")
"#;

const STATS_MODULE: &str = r#"
-- Shared helpers for the leaderboard. Scripts load this with require().
local Stats = {}

-- Adds `amount` to one of a player's leaderboard values.
function Stats.add(player, name, amount)
	local leaderstats = player:FindFirstChild("leaderstats")
	local value = leaderstats and leaderstats:FindFirstChild(name)
	if value then
		value.Value += amount
	end
end

return Stats
"#;

const LEADERBOARD_SCRIPT: &str = r#"
-- Gives every player a leaderboard with Coins and Deaths.
local Players = game:GetService("Players")
local Stats = require(game:GetService("ReplicatedStorage").Stats)

Players.PlayerAdded:Connect(function(player)
	local leaderstats = Instance.new("Folder")
	leaderstats.Name = "leaderstats"
	leaderstats.Parent = player

	local coins = Instance.new("IntValue")
	coins.Name = "Coins"
	coins.Parent = leaderstats

	local deaths = Instance.new("IntValue")
	deaths.Name = "Deaths"
	deaths.Parent = leaderstats

	player.CharacterAdded:Connect(function(character)
		character:WaitForChild("Humanoid").Died:Connect(function()
			Stats.add(player, "Deaths", 1)
		end)
	end)
end)
"#;

const COINS_SCRIPT: &str = r#"
-- Spins every coin in this folder. Touch one to collect it; it comes back later.
local Players = game:GetService("Players")
local Stats = require(game:GetService("ReplicatedStorage").Stats)

local coins = {}
for _, coin in script.Parent:GetChildren() do
	if coin:IsA("Part") then
		table.insert(coins, coin)
		local taken = false
		coin.Touched:Connect(function(hit)
			local player = Players:GetPlayerFromCharacter(hit.Parent)
			if taken or not player then
				return
			end
			taken = true
			Stats.add(player, "Coins", 1)
			coin.Transparency = 1
			task.wait(8)
			coin.Transparency = 0
			taken = false
		end)
	end
end

game:GetService("RunService").Heartbeat:Connect(function(dt)
	for _, coin in coins do
		local o = coin.Orientation
		coin.Orientation = Vector3.new(o.X, (o.Y + 120 * dt) % 360, o.Z)
	end
end)
"#;

/// A short obstacle course that shows off scripts, touch events and task.wait.
pub fn sample_obby() -> InstanceTree {
    let mut tree = InstanceTree::new();
    let ws = tree.workspace();
    script(&mut tree, ws, "Welcome", WELCOME_SCRIPT);
    script(&mut tree, ws, "Leaderboard", LEADERBOARD_SCRIPT);
    let storage = tree.service(ClassKind::ReplicatedStorage).expect("every tree has storage");
    code(&mut tree, storage, ClassKind::ModuleScript, "Stats", STATS_MODULE);

    let start = add(&mut tree, ws, ClassKind::Folder, "Start");
    let floor =
        block(&mut tree, start, "StartPlatform", [0.0, -0.5, 0.0], [16.0, 1.0, 16.0], [176, 160, 150]);
    material(&mut tree, floor, Material::Brick);
    let spawn = add(&mut tree, start, ClassKind::SpawnLocation, "SpawnLocation");
    tree.set(spawn, Property::Position, PropertyValue::Vec3(Vec3::new(0.0, 0.5, 0.0))).unwrap();
    tree.set(spawn, Property::Size, PropertyValue::Vec3(Vec3::new(6.0, 1.0, 6.0))).unwrap();

    // A wooden ramp up to a ledge behind the start, with coins on top.
    let ramp = block(&mut tree, start, "Ramp", [0.0, 2.0, -12.0], [6.0, 4.0, 8.0], [150, 105, 65]);
    shape(&mut tree, ramp, PartShape::Wedge);
    orient(&mut tree, ramp, [0.0, 180.0, 0.0]);
    material(&mut tree, ramp, Material::Wood);
    let ledge = block(&mut tree, start, "Ledge", [0.0, 3.5, -20.0], [10.0, 1.0, 8.0], [150, 105, 65]);
    material(&mut tree, ledge, Material::Wood);
    // A big glass ball to admire on the ledge.
    let ball = block(&mut tree, start, "GlassBall", [3.0, 6.5, -21.0], [5.0, 5.0, 5.0], [150, 210, 255]);
    shape(&mut tree, ball, PartShape::Ball);
    material(&mut tree, ball, Material::Glass);

    let coins = add(&mut tree, ws, ClassKind::Folder, "Coins");
    let coin_spots: [[f32; 3]; 6] = [
        [0.0, 5.5, -18.0],
        [0.0, 5.5, -22.0],
        [14.0, 2.5, 0.0],
        [30.0, 5.5, 0.0],
        [48.0, 7.0, 0.0],
        [84.0, 7.0, 0.0],
    ];
    for (i, spot) in coin_spots.iter().enumerate() {
        let coin = block(&mut tree, coins, &format!("Coin{}", i + 1), *spot, [0.4, 2.0, 2.0], [255, 205, 40]);
        shape(&mut tree, coin, PartShape::Cylinder);
        material(&mut tree, coin, Material::Neon);
        set(&mut tree, coin, Property::CanCollide, PropertyValue::Bool(false));
    }
    script(&mut tree, coins, "SpinAndCollect", COINS_SCRIPT);

    // Simple jumps.
    let jumps = add(&mut tree, ws, ClassKind::Folder, "Jumps");
    for i in 0..4 {
        let x = 14.0 + i as f32 * 8.0;
        let y = -0.5 + i as f32 * 1.5;
        block(&mut tree, jumps, &format!("Jump{}", i + 1), [x, y, 0.0], [4.0, 1.0, 4.0], [70, 150, 230]);
    }

    // A lava strip between two platforms.
    let lava = add(&mut tree, ws, ClassKind::Folder, "Lava");
    block(&mut tree, lava, "SafeA", [48.0, 4.0, 0.0], [6.0, 1.0, 6.0], [180, 180, 190]);
    for i in 0..3 {
        let z = -4.0 + i as f32 * 4.0;
        let brick = block(&mut tree, lava, "LavaBrick", [56.0, 4.0, z], [4.0, 1.0, 2.0], [230, 60, 40]);
        script(&mut tree, brick, "Kill", KILL_SCRIPT);
        block(&mut tree, lava, "Stone", [56.0, 4.0, z + 2.0], [4.0, 1.0, 2.0], [120, 120, 130]);
    }
    block(&mut tree, lava, "SafeB", [64.0, 4.0, 0.0], [6.0, 1.0, 6.0], [180, 180, 190]);

    // Platforms that fade in and out.
    let fading = add(&mut tree, ws, ClassKind::Folder, "Fading");
    for i in 0..4 {
        let x = 72.0 + i as f32 * 6.0;
        block(&mut tree, fading, &format!("Fade{}", i + 1), [x, 4.0, 0.0], [4.0, 1.0, 4.0], [160, 90, 220]);
    }
    script(&mut tree, fading, "Fade", FADE_SCRIPT);

    // A lift up to the finish.
    let lift = block(&mut tree, ws, "Lift", [98.0, 4.0, 0.0], [6.0, 1.0, 6.0], [90, 200, 120]);
    material(&mut tree, lift, Material::Metal);
    script(&mut tree, lift, "LiftMotor", LIFT_SCRIPT);
    let finish = block(&mut tree, ws, "Finish", [106.0, 14.0, 0.0], [8.0, 1.0, 8.0], [250, 200, 40]);
    material(&mut tree, finish, Material::Neon);
    script(&mut tree, finish, "Celebrate", FINISH_SCRIPT);

    tree.drain_changes();
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use libroxel_engine::session::start_on;
    use libroxel_engine::{CharacterInput, LogLevel};
    use libroxel_instance::place;

    #[test]
    fn templates_save_and_load() {
        for tree in [blank(), sample_obby()] {
            let text = place::save(&tree).unwrap();
            let (loaded, report) = place::load(&text).unwrap();
            assert!(report.warnings.is_empty());
            assert_eq!(place::save(&loaded).unwrap(), text);
        }
    }

    #[test]
    fn sample_obby_runs_without_script_errors() {
        let mut session = start_on(sample_obby(), true).unwrap();
        for _ in 0..600 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        let log = session.take_log();
        let errors: Vec<_> = log.iter().filter(|l| l.level == LogLevel::Error).collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(log.iter().any(|l| l.text.contains("Welcome")));
    }

    #[test]
    fn sample_obby_finish_pad_celebrates() {
        let mut session = start_on(sample_obby(), true).unwrap();
        session.step(1.0 / 60.0, CharacterInput::default());
        session
            .runtime()
            .run("teleport", "workspace.Player1.HumanoidRootPart.Position = workspace.Finish.Position + Vector3.new(0, 3, 0)")
            .unwrap();
        for _ in 0..60 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        let log = session.take_log();
        assert!(log.iter().any(|l| l.text == "Player1 finished the obby!"), "{log:?}");
    }

    fn leaderstat(session: &libroxel_engine::PlaySession, name: &str) -> f64 {
        let tree = session.runtime().tree().borrow();
        let player = session.player().unwrap();
        let stats = tree.find_first_child(player, "leaderstats").unwrap().unwrap();
        let value = tree.find_first_child(stats, name).unwrap().unwrap();
        tree.get_number(value, Property::NumberValue).unwrap()
    }

    #[test]
    fn walking_up_the_ramp_collects_coins() {
        let mut session = start_on(sample_obby(), true).unwrap();
        for _ in 0..30 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        assert_eq!(leaderstat(&session, "Coins"), 0.0);
        // Walk straight back (-Z): up the wedge and over the two ledge coins.
        let back = CharacterInput { move_dir: Vec3::new(0.0, 0.0, -1.0), jump: false };
        for _ in 0..90 {
            session.step(1.0 / 60.0, back);
        }
        let character = session.character().unwrap();
        let pos = session.runtime().tree().borrow().get_vec3(character.root, Property::Position).unwrap();
        assert!(pos.y > 5.0, "climbed onto the ledge: {pos:?}");
        let log: Vec<_> = session.take_log();
        assert_eq!(leaderstat(&session, "Coins"), 2.0, "{log:?}");
        let errors: Vec<_> = session.take_log().into_iter().filter(|l| l.level == LogLevel::Error).collect();
        assert!(errors.is_empty(), "{errors:?}");
    }
}
