//! Starter worlds: the blank template and a sample obby.

use libroxel_instance::{
    ClassKind, Color3, EnumValue, InstanceId, InstanceTree, Material, PartShape, Property, PropertyValue,
    UDim, UDim2, Vec2, Vec3,
};

/// Builds a go-kart standing on the ground at `at`, facing `yaw` degrees
/// (0 faces -Z): a chassis with a vehicle seat, wheels and a bumper welded on.
pub fn kart(tree: &mut InstanceTree, parent: InstanceId, at: Vec3, yaw: f32, rgb: [u8; 3]) -> InstanceId {
    let turn = libroxel_instance::Rotation::about_y(yaw.to_radians());
    let model = add(tree, parent, ClassKind::Model, "Kart");
    let place = |tree: &mut InstanceTree, id: InstanceId, offset: [f32; 3]| {
        let p = at + turn.apply(Vec3::new(offset[0], offset[1], offset[2]));
        set(tree, id, Property::Position, PropertyValue::Vec3(p));
        orient(tree, id, [0.0, yaw, 0.0]);
        set(tree, id, Property::Anchored, PropertyValue::Bool(false));
    };
    let chassis = block(tree, model, "Chassis", [0.0; 3], [4.0, 1.0, 5.0], rgb);
    place(tree, chassis, [0.0, 0.5, 0.0]);
    let seat = add(tree, model, ClassKind::VehicleSeat, "VehicleSeat");
    set(tree, seat, Property::Size, PropertyValue::Vec3(Vec3::new(2.0, 1.0, 2.0)));
    set(tree, seat, Property::Color, PropertyValue::Color3(Color3::from_rgb8(40, 40, 45)));
    place(tree, seat, [0.0, 1.5, 0.8]);
    let mut parts = vec![seat];
    for (i, (x, z)) in [(-2.3, -1.7), (2.3, -1.7), (-2.3, 1.7), (2.3, 1.7)].into_iter().enumerate() {
        let wheel = block(tree, model, &format!("Wheel{}", i + 1), [0.0; 3], [0.6, 1.4, 1.4], [30, 30, 30]);
        shape(tree, wheel, PartShape::Cylinder);
        material(tree, wheel, Material::SmoothPlastic);
        set(tree, wheel, Property::CanCollide, PropertyValue::Bool(false));
        place(tree, wheel, [x, 0.5, z]);
        parts.push(wheel);
    }
    let bumper = block(tree, model, "Bumper", [0.0; 3], [4.0, 0.6, 0.4], [230, 230, 235]);
    material(tree, bumper, Material::Metal);
    place(tree, bumper, [0.0, 0.6, -2.7]);
    parts.push(bumper);
    for part in parts {
        let name = format!("{}Weld", tree.name(part).unwrap_or("Part"));
        let weld = add(tree, chassis, ClassKind::WeldConstraint, &name);
        set(tree, weld, Property::Part0, PropertyValue::Ref(Some(chassis)));
        set(tree, weld, Property::Part1, PropertyValue::Ref(Some(part)));
    }
    model
}

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

const LAVA_SCRIPT: &str = r#"
-- Every part tagged "Lava" kills whatever touches it. Tag a part "Lava" in
-- the Properties panel (or with CollectionService) to make more.
local CollectionService = game:GetService("CollectionService")

local function makeDeadly(part)
	part.Touched:Connect(function(hit)
		local humanoid = hit.Parent and hit.Parent:FindFirstChild("Humanoid")
		if humanoid then
			humanoid.Health = 0
		end
	end)
end

for _, part in CollectionService:GetTagged("Lava") do
	makeDeadly(part)
end
CollectionService:GetInstanceAddedSignal("Lava"):Connect(makeDeadly)
"#;

const AIM_SCRIPT: &str = r#"
-- Runs for the player holding the tool: sends where they clicked to the game.
local tool = script.Parent
local mouse = game:GetService("Players").LocalPlayer:GetMouse()

tool.Activated:Connect(function()
	if mouse.Target then
		tool.Place:FireServer(mouse.Hit.Position)
	end
end)
"#;

const BUILD_SCRIPT: &str = r#"
-- The game side of the builder: puts a brick where the player clicked,
-- if it's close enough. Bricks disappear after a while.
local Debris = game:GetService("Debris")
local tool = script.Parent
local REACH, LIFETIME = 40, 20

tool.Place.OnServerEvent:Connect(function(player, position)
	local root = player.Character and player.Character:FindFirstChild("HumanoidRootPart")
	if not root or (position - root.Position).Magnitude > REACH then
		return
	end
	local brick = Instance.new("Part")
	brick.Name = "BuiltBrick"
	brick.Size = Vector3.new(2, 1, 2)
	-- Snap to a 2-stud grid, sitting on what was clicked.
	local snapped = Vector3.new(math.floor(position.X / 2 + 0.5) * 2, position.Y + 0.5, math.floor(position.Z / 2 + 0.5) * 2)
	brick.Position = snapped
	brick.Anchored = true
	brick.Color = Color3.fromRGB(math.random(80, 255), math.random(80, 255), math.random(80, 255))
	brick:SetAttribute("Builder", player.Name)
	brick.Parent = workspace
	Debris:AddItem(brick, LIFETIME)
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
-- Celebrate when a player reaches the end, and tell everyone.
local Debris = game:GetService("Debris")
local notify = game:GetService("ReplicatedStorage"):WaitForChild("Notify")
local pad = script.Parent
local celebrated = false

pad.Touched:Connect(function(hit)
	local humanoid = hit.Parent and hit.Parent:FindFirstChild("Humanoid")
	if not humanoid or celebrated then
		return
	end
	celebrated = true
	print(hit.Parent.Name .. " finished the obby!")
	notify:FireAllClients(hit.Parent.Name .. " finished the obby!")
	-- Finishers move to the Winners team on the leaderboard.
	local player = game:GetService("Players"):GetPlayerFromCharacter(hit.Parent)
	if player then
		player.Team = game:GetService("Teams").Winners
	end

	-- Rain down some confetti blocks.
	for i = 1, 30 do
		local bit = Instance.new("Part")
		bit.Size = Vector3.new(0.6, 0.6, 0.6)
		bit.Position = pad.Position + Vector3.new(math.random(-6, 6), 15 + i * 0.5, math.random(-6, 6))
		bit.Color = Color3.fromRGB(math.random(80, 255), math.random(80, 255), math.random(80, 255))
		bit.CanCollide = false
		bit.Parent = workspace
		Debris:AddItem(bit, 4)
	end

	task.wait(5)
	celebrated = false
end)
"#;

const DOOR_SCRIPT: &str = r#"
-- Click the door to open it. It slides up, waits, then closes again.
local TweenService = game:GetService("TweenService")
local door = script.Parent
local closed = door.CFrame
local open = closed + Vector3.new(0, 5.5, 0)
local info = TweenInfo.new(0.6, Enum.EasingStyle.Quad, Enum.EasingDirection.Out)
local busy = false

door.ClickDetector.MouseClick:Connect(function(player)
	if busy then
		return
	end
	busy = true
	print(player.Name .. " opened the vault")
	local opening = TweenService:Create(door, info, { CFrame = open })
	opening:Play()
	opening.Completed:Wait()
	task.wait(3)
	local closing = TweenService:Create(door, info, { CFrame = closed })
	closing:Play()
	closing.Completed:Wait()
	busy = false
end)
"#;

const SHOP_SCRIPT: &str = r#"
-- Hold E at the stand to spend coins on a short speed boost.
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Stats = require(ReplicatedStorage.Stats)
local notify = ReplicatedStorage:WaitForChild("Notify")
local PRICE, BOOSTED_SPEED, SECONDS = 3, 32, 10

script.Parent.ProximityPrompt.Triggered:Connect(function(player)
	local humanoid = player.Character and player.Character:FindFirstChild("Humanoid")
	if not humanoid then
		return
	end
	if player.leaderstats.Coins.Value < PRICE then
		notify:FireClient(player, "You need " .. PRICE .. " coins for a speed boost")
		return
	end
	Stats.add(player, "Coins", -PRICE)
	humanoid.WalkSpeed = BOOSTED_SPEED
	notify:FireClient(player, "Speed boost for " .. SECONDS .. " seconds!")
	task.delay(SECONDS, function()
		if humanoid.Parent then
			humanoid.WalkSpeed = 16
		end
	end)
end)
"#;

const LAUNCH_SCRIPT: &str = r#"
-- Step on the pad to be thrown into the air.
local pad = script.Parent
local cooling = {}

pad.Touched:Connect(function(hit)
	local character = hit.Parent
	local root = character and character:FindFirstChild("HumanoidRootPart")
	if not root or cooling[character] then
		return
	end
	cooling[character] = true
	root.AssemblyLinearVelocity = Vector3.new(0, 110, 0)
	task.wait(1)
	cooling[character] = nil
end)
"#;

const STREET_LIGHTS_SCRIPT: &str = r#"
-- Lamps tagged "StreetLight" switch on at dusk and off at dawn.
local Lighting = game:GetService("Lighting")
local CollectionService = game:GetService("CollectionService")

local function update()
	local night = Lighting.ClockTime >= 18 or Lighting.ClockTime < 6
	for _, lamp in CollectionService:GetTagged("StreetLight") do
		lamp.PointLight.Enabled = night
		lamp.Material = if night then Enum.Material.Neon else Enum.Material.Glass
	end
end
Lighting:GetPropertyChangedSignal("ClockTime"):Connect(update)
update()
"#;

const TNT_SCRIPT: &str = r#"
-- Click the TNT to blow up the crates. Everything comes back a few seconds later.
local tnt = script.Parent
local detector = tnt.ClickDetector
local crates = workspace.Quarry.Crates
local saved = crates:Clone()

detector.MouseClick:Connect(function(player)
	if tnt.Transparency > 0 then
		return
	end
	local boom = Instance.new("Explosion")
	boom.Position = tnt.Position
	boom.BlastRadius = 8
	local thrown = 0
	boom.Hit:Connect(function(part)
		if part.Parent == crates then
			thrown += 1
		end
	end)
	boom.Parent = workspace
	tnt.Transparency = 1
	tnt.Sign.Enabled = false
	task.wait(0.1)
	print(player.Name .. " blew up " .. thrown .. " crates!")
	task.wait(6)
	crates:Destroy()
	crates = saved:Clone()
	crates.Parent = workspace.Quarry
	tnt.Transparency = 0
	tnt.Sign.Enabled = true
end)
"#;

const CART_SCRIPT: &str = r#"
-- The mine cart's sides are welded to its base, so the whole cart moves
-- together. Click it to push it one way, then the other.
local cart = script.Parent
local base = cart.Base
local away = true

base.ClickDetector.MouseClick:Connect(function(player)
	local push = if away then -30 else 30
	away = not away
	base.AssemblyLinearVelocity = Vector3.new(0, 8, push)
	print(player.Name .. " pushed the mine cart")
end)
"#;

const PLATE_SCRIPT: &str = r#"
-- A pressure plate: it glows while someone stands on it.
local plate = script.Parent
local Players = game:GetService("Players")
local standing = {}

local function count()
	local n = 0
	for _ in standing do
		n += 1
	end
	return n
end

local function update()
	local pressed = count() > 0
	plate.Material = if pressed then Enum.Material.Neon else Enum.Material.Metal
	plate.Color = if pressed then Color3.fromRGB(120, 255, 160) else Color3.fromRGB(110, 110, 120)
end

plate.Touched:Connect(function(hit)
	if Players:GetPlayerFromCharacter(hit.Parent) and not standing[hit] then
		standing[hit] = true
		if count() == 1 then
			print("Pressure plate pressed")
		end
		update()
	end
end)

plate.TouchEnded:Connect(function(hit)
	if standing[hit] then
		standing[hit] = nil
		update()
	end
end)
"#;

const CHAT_SCRIPT: &str = r#"
-- What players type in the chat box floats over their head for a few seconds.
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Debris = game:GetService("Debris")

ReplicatedStorage:WaitForChild("Say").OnServerEvent:Connect(function(player, text)
	if typeof(text) ~= "string" then
		return
	end
	text = string.sub(text, 1, 80)
	local head = player.Character and player.Character:FindFirstChild("Head")
	if text == "" or not head then
		return
	end
	local old = head:FindFirstChild("Bubble")
	if old then
		old:Destroy()
	end
	local bubble = Instance.new("BillboardGui")
	bubble.Name = "Bubble"
	bubble.Size = UDim2.new(0, 240, 0, 36)
	bubble.StudsOffset = Vector3.new(0, 3.6, 0)
	local label = Instance.new("TextLabel", bubble)
	label.Size = UDim2.new(1, 0, 1, 0)
	label.BackgroundColor3 = Color3.new(1, 1, 1)
	label.TextSize = 16
	label.Text = text
	Instance.new("UICorner", label).CornerRadius = UDim.new(0, 12)
	bubble.Parent = head
	Debris:AddItem(bubble, 5)
	print(player.Name .. ": " .. text)
end)
"#;

const ZOMBIE_SCRIPT: &str = r#"
-- The quarry zombie chases players who come into the quarry and hurts them
-- on touch. TNT knocks it down; a new one climbs out a few seconds later.
local Players = game:GetService("Players")
local template = workspace.Zombie:Clone()
local SIGHT, DAMAGE = 20, 20

local function inQuarry(position)
	return position.Z > 8 and position.Z < 21 and math.abs(position.X) < 9
end

local function nearestPlayer(from)
	local best, bestDistance = nil, SIGHT
	for _, player in Players:GetPlayers() do
		local root = player.Character and player.Character:FindFirstChild("HumanoidRootPart")
		if root and inQuarry(root.Position) then
			local distance = (root.Position - from).Magnitude
			if distance < bestDistance then
				best, bestDistance = root, distance
			end
		end
	end
	return best
end

local function run(zombie)
	local humanoid = zombie.Humanoid
	local root = zombie.HumanoidRootPart
	local home = root.Position
	humanoid.WalkSpeed = 10
	local resting = false
	root.Touched:Connect(function(hit)
		local victim = hit.Parent and hit.Parent:FindFirstChild("Humanoid")
		if victim and victim ~= humanoid and not resting and humanoid.Health > 0 then
			resting = true
			victim:TakeDamage(DAMAGE)
			task.wait(1)
			resting = false
		end
	end)
	humanoid.Died:Connect(function()
		print("The zombie is down!")
		task.wait(5)
		zombie:Destroy()
		local fresh = template:Clone()
		fresh.Parent = workspace
		run(fresh)
	end)
	task.spawn(function()
		while humanoid.Health > 0 and zombie.Parent do
			local target = nearestPlayer(root.Position)
			if target then
				humanoid:MoveTo(target.Position)
			elseif (root.Position - home).Magnitude > 2 then
				humanoid:MoveTo(home)
			end
			task.wait(0.4)
		end
	end)
end

run(workspace.Zombie)
"#;

const HUD_SCRIPT: &str = r#"
-- Runs for the player: shows their coins and switches between day and night.
local Players = game:GetService("Players")
local TweenService = game:GetService("TweenService")
local Lighting = game:GetService("Lighting")
local player = Players.LocalPlayer
local hud = script.Parent

local coins = player:WaitForChild("leaderstats"):WaitForChild("Coins")
local label = hud.CoinLabel
local function show()
	label.Text = "Coins: " .. coins.Value
end
coins.Changed:Connect(show)
show()

local button = hud.DayNight
local night = false
button.MouseButton1Click:Connect(function()
	night = not night
	local goal = if night then 21 else 14
	TweenService:Create(Lighting, TweenInfo.new(2, Enum.EasingStyle.Sine), { ClockTime = goal }):Play()
	button.Text = if night then "Make it day" else "Make it night"
end)

-- Messages from the game's scripts pop up for a few seconds.
local message = hud.Message
local shown = 0
game:GetService("ReplicatedStorage"):WaitForChild("Notify").OnClientEvent:Connect(function(text)
	shown += 1
	local mine = shown
	message.Text = text
	message.Visible = true
	task.wait(3)
	if shown == mine then
		message.Visible = false
	end
end)

-- The chat box: press / to type, Enter to send.
local say = game:GetService("ReplicatedStorage"):WaitForChild("Say")
local chat = hud.ChatBox
chat.FocusLost:Connect(function(enterPressed)
	if enterPressed and chat.Text ~= "" then
		say:FireServer(chat.Text)
	end
	chat.Text = ""
end)

-- The health bar follows the character's health.
local bar = hud.HealthBar
local fill = bar.Fill
local function watch(character)
	local humanoid = character:WaitForChild("Humanoid")
	local function show(health)
		local share = math.clamp(health / humanoid.MaxHealth, 0, 1)
		fill.Size = UDim2.new(share, 0, 1, 0)
		fill.BackgroundColor3 = if share > 0.3 then Color3.fromRGB(80, 200, 100) else Color3.fromRGB(230, 70, 60)
		bar.Label.Text = "Health: " .. math.floor(health + 0.5)
	end
	humanoid.HealthChanged:Connect(show)
	show(humanoid.Health)
end
if player.Character then
	watch(player.Character)
end
player.CharacterAdded:Connect(watch)

-- C flies the camera up for a look over the whole course, and back.
local camera = workspace.CurrentCamera
local overhead = false
local function toggleOverhead()
	overhead = not overhead
	if overhead then
		camera.CameraType = Enum.CameraType.Scriptable
		local view = CFrame.lookAt(Vector3.new(30, 45, 50), Vector3.new(40, 0, 0))
		TweenService:Create(camera, TweenInfo.new(1.5, Enum.EasingStyle.Sine), { CFrame = view }):Play()
	else
		camera.CameraType = Enum.CameraType.Custom
	end
end

-- H shows or hides the help panel.
local UserInputService = game:GetService("UserInputService")
UserInputService.InputBegan:Connect(function(input, processed)
	if processed then
		return
	end
	if input.KeyCode == Enum.KeyCode.H then
		hud.Help.Visible = not hud.Help.Visible
	elseif input.KeyCode == Enum.KeyCode.Slash then
		chat:CaptureFocus()
	elseif input.KeyCode == Enum.KeyCode.C then
		toggleOverhead()
	end
end)

-- How high you are: cast a ray straight down, ignoring your own character.
local height = hud.Height
local params = RaycastParams.new()
game:GetService("RunService").Heartbeat:Connect(function()
	local character = player.Character
	local root = character and character:FindFirstChild("HumanoidRootPart")
	if not root then
		return
	end
	params.FilterDescendantsInstances = { character }
	local hit = workspace:Raycast(root.Position, Vector3.new(0, -100, 0), params)
	height.Text = if hit then string.format("Height: %d studs", math.floor(hit.Distance - 2 + 0.5)) else "Height: falling!"
end)
"#;

const WELCOME_SCRIPT: &str = r#"
print("Welcome to the sample obby! Reach the gold pad. Red blocks are lava.")
print("Grab the spinning coins on the way. Your score is on the leaderboard.")
print("There's a coin worth 5 locked in the vault: click its door to open it.")
print("Press 1 to hold the builder, then click to place bricks.")
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
-- Gives every player a leaderboard with Coins and Deaths. Coins are saved
-- between play tests (File > Clear saved data forgets them).
local Players = game:GetService("Players")
local Stats = require(game:GetService("ReplicatedStorage").Stats)
local saves = game:GetService("DataStoreService"):GetDataStore("PlayerData")

Players.PlayerAdded:Connect(function(player)
	local leaderstats = Instance.new("Folder")
	leaderstats.Name = "leaderstats"
	leaderstats.Parent = player

	local coins = Instance.new("IntValue")
	coins.Name = "Coins"
	coins.Value = saves:GetAsync("coins_" .. player.UserId) or 0
	coins.Parent = leaderstats

	local deaths = Instance.new("IntValue")
	deaths.Name = "Deaths"
	deaths.Parent = leaderstats

	player.CharacterAdded:Connect(function(character)
		character:WaitForChild("Humanoid").Died:Connect(function()
			Stats.add(player, "Deaths", 1)
		end)

		-- A name tag floating over the character's head.
		local tag = Instance.new("BillboardGui")
		tag.Size = UDim2.new(0, 160, 0, 30)
		tag.StudsOffset = Vector3.new(0, 1.8, 0)
		local label = Instance.new("TextLabel", tag)
		label.Size = UDim2.new(1, 0, 1, 0)
		label.BackgroundTransparency = 1
		label.TextColor3 = Color3.new(1, 1, 1)
		label.TextSize = 18
		label.Text = player.Name
		tag.Parent = character:WaitForChild("Head")
	end)
end)

Players.PlayerRemoving:Connect(function(player)
	saves:SetAsync("coins_" .. player.UserId, player.leaderstats.Coins.Value)
end)
"#;

const COINS_SCRIPT: &str = r#"
-- Spins every part tagged "Coin". Touch one to collect it; it comes back
-- later. A coin's "Value" attribute says how much it's worth (1 if unset).
local Players = game:GetService("Players")
local CollectionService = game:GetService("CollectionService")
local Stats = require(game:GetService("ReplicatedStorage").Stats)

local coins = {}
for _, coin in CollectionService:GetTagged("Coin") do
	do
		table.insert(coins, coin)
		local taken = false
		coin.Touched:Connect(function(hit)
			local player = Players:GetPlayerFromCharacter(hit.Parent)
			if taken or not player then
				return
			end
			taken = true
			Stats.add(player, "Coins", coin:GetAttribute("Value") or 1)
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

    // A little vault beside the start, with a door that opens when clicked.
    let vault = add(&mut tree, ws, ClassKind::Folder, "Vault");
    let stone = [120, 120, 130];
    let vault_floor = block(&mut tree, vault, "Floor", [-12.0, -0.5, 0.0], [8.0, 1.0, 8.0], stone);
    material(&mut tree, vault_floor, Material::Metal);
    block(&mut tree, vault, "BackWall", [-16.5, 3.0, 0.0], [1.0, 6.0, 10.0], stone);
    block(&mut tree, vault, "LeftWall", [-12.0, 3.0, -4.5], [8.0, 6.0, 1.0], stone);
    block(&mut tree, vault, "RightWall", [-12.0, 3.0, 4.5], [8.0, 6.0, 1.0], stone);
    block(&mut tree, vault, "Roof", [-12.0, 6.5, 0.0], [10.0, 1.0, 10.0], stone);
    let door = block(&mut tree, vault, "Door", [-7.5, 3.0, 0.0], [1.0, 6.0, 8.0], [150, 70, 50]);
    material(&mut tree, door, Material::Wood);
    add(&mut tree, door, ClassKind::ClickDetector, "ClickDetector");
    script(&mut tree, door, "OpenOnClick", DOOR_SCRIPT);

    // An on-screen display: a coin counter and a day/night button.
    let starter_gui = tree.service(ClassKind::StarterGui).expect("every tree has StarterGui");
    let hud = add(&mut tree, starter_gui, ClassKind::ScreenGui, "HUD");
    let dark = Color3::from_rgb8(25, 30, 40);
    let white = Color3::from_rgb8(255, 255, 255);
    let label = add(&mut tree, hud, ClassKind::TextLabel, "CoinLabel");
    set(&mut tree, label, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(0.5, 0.0, 0.0, 10.0)));
    set(&mut tree, label, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 200.0, 0.0, 40.0)));
    set(&mut tree, label, Property::AnchorPoint, PropertyValue::Vec2(Vec2::new(0.5, 0.0)));
    set(&mut tree, label, Property::BackgroundColor, PropertyValue::Color3(dark));
    set(&mut tree, label, Property::BackgroundTransparency, PropertyValue::Number(0.3));
    set(&mut tree, label, Property::TextColor, PropertyValue::Color3(Color3::from_rgb8(255, 215, 60)));
    set(&mut tree, label, Property::TextSize, PropertyValue::Number(22.0));
    set(&mut tree, label, Property::Text, PropertyValue::String("Coins: 0".into()));
    let button = add(&mut tree, hud, ClassKind::TextButton, "DayNight");
    set(&mut tree, button, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(0.0, 12.0, 1.0, -12.0)));
    set(&mut tree, button, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 150.0, 0.0, 40.0)));
    set(&mut tree, button, Property::AnchorPoint, PropertyValue::Vec2(Vec2::new(0.0, 1.0)));
    set(&mut tree, button, Property::BackgroundColor, PropertyValue::Color3(Color3::from_rgb8(60, 110, 200)));
    set(&mut tree, button, Property::TextColor, PropertyValue::Color3(white));
    set(&mut tree, button, Property::TextSize, PropertyValue::Number(18.0));
    set(&mut tree, button, Property::Text, PropertyValue::String("Make it night".into()));
    let message = add(&mut tree, hud, ClassKind::TextLabel, "Message");
    set(&mut tree, message, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(0.5, 0.0, 0.0, 60.0)));
    set(&mut tree, message, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 360.0, 0.0, 36.0)));
    set(&mut tree, message, Property::AnchorPoint, PropertyValue::Vec2(Vec2::new(0.5, 0.0)));
    set(&mut tree, message, Property::BackgroundColor, PropertyValue::Color3(dark));
    set(&mut tree, message, Property::BackgroundTransparency, PropertyValue::Number(0.2));
    set(&mut tree, message, Property::TextColor, PropertyValue::Color3(white));
    set(&mut tree, message, Property::TextSize, PropertyValue::Number(18.0));
    set(&mut tree, message, Property::Text, PropertyValue::String(String::new()));
    set(&mut tree, message, Property::Visible, PropertyValue::Bool(false));
    let height = add(&mut tree, hud, ClassKind::TextLabel, "Height");
    set(&mut tree, height, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(1.0, -12.0, 1.0, -36.0)));
    set(&mut tree, height, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 160.0, 0.0, 28.0)));
    set(&mut tree, height, Property::AnchorPoint, PropertyValue::Vec2(Vec2::new(1.0, 1.0)));
    set(&mut tree, height, Property::BackgroundTransparency, PropertyValue::Number(1.0));
    set(&mut tree, height, Property::TextColor, PropertyValue::Color3(white));
    set(&mut tree, height, Property::TextSize, PropertyValue::Number(16.0));
    set(&mut tree, height, Property::Text, PropertyValue::String("Height: 0 studs".into()));
    // A health bar in the top left: a dark frame with a coloured fill.
    let health = add(&mut tree, hud, ClassKind::Frame, "HealthBar");
    set(&mut tree, health, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(0.0, 12.0, 0.0, 12.0)));
    set(&mut tree, health, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 200.0, 0.0, 24.0)));
    set(&mut tree, health, Property::BackgroundColor, PropertyValue::Color3(dark));
    set(&mut tree, health, Property::BackgroundTransparency, PropertyValue::Number(0.3));
    let fill = add(&mut tree, health, ClassKind::Frame, "Fill");
    set(&mut tree, fill, Property::GuiSize, PropertyValue::UDim2(UDim2::new(1.0, 0.0, 1.0, 0.0)));
    set(&mut tree, fill, Property::BackgroundColor, PropertyValue::Color3(Color3::from_rgb8(80, 200, 100)));
    let fill_corner = add(&mut tree, fill, ClassKind::UICorner, "UICorner");
    set(&mut tree, fill_corner, Property::CornerRadius, PropertyValue::UDim(UDim::new(0.0, 6.0)));
    let health_label = add(&mut tree, health, ClassKind::TextLabel, "Label");
    set(&mut tree, health_label, Property::GuiSize, PropertyValue::UDim2(UDim2::new(1.0, 0.0, 1.0, 0.0)));
    set(&mut tree, health_label, Property::BackgroundTransparency, PropertyValue::Number(1.0));
    set(&mut tree, health_label, Property::TextColor, PropertyValue::Color3(white));
    set(&mut tree, health_label, Property::TextSize, PropertyValue::Number(15.0));
    set(&mut tree, health_label, Property::Text, PropertyValue::String("Health: 100".into()));
    let help = add(&mut tree, hud, ClassKind::Frame, "Help");
    set(&mut tree, help, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(0.0, 12.0, 0.5, 0.0)));
    set(&mut tree, help, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 260.0, 0.0, 258.0)));
    set(&mut tree, help, Property::AnchorPoint, PropertyValue::Vec2(Vec2::new(0.0, 0.5)));
    set(&mut tree, help, Property::BackgroundColor, PropertyValue::Color3(dark));
    set(&mut tree, help, Property::BackgroundTransparency, PropertyValue::Number(0.3));
    // The help lines stack themselves in a column.
    let list = add(&mut tree, help, ClassKind::UIListLayout, "UIListLayout");
    set(&mut tree, list, Property::Padding, PropertyValue::UDim(UDim::new(0.0, 2.0)));
    for (i, line) in [
        "",
        "H: show or hide this help",
        "E: use a prompt (hold it)",
        "1: builder tool, click to build",
        "/: chat    C: overhead camera",
        "Green pad: jump!    Click the TNT",
        "Look out for the quarry zombie",
        "Click the mine cart to push it",
        "Stand on the plate by the shop",
        "Drive the kart past the quarry",
        "Coins are saved",
    ]
    .iter()
    .enumerate()
    {
        let label = add(&mut tree, help, ClassKind::TextLabel, &format!("Line{}", i + 1));
        set(&mut tree, label, Property::LayoutOrder, PropertyValue::Number(i as f64));
        let height = if line.is_empty() { 2.0 } else { 20.0 };
        set(&mut tree, label, Property::GuiSize, PropertyValue::UDim2(UDim2::new(1.0, 0.0, 0.0, height)));
        set(&mut tree, label, Property::BackgroundTransparency, PropertyValue::Number(1.0));
        set(&mut tree, label, Property::TextColor, PropertyValue::Color3(white));
        set(&mut tree, label, Property::TextSize, PropertyValue::Number(15.0));
        set(&mut tree, label, Property::Text, PropertyValue::String((*line).into()));
    }
    let chat = add(&mut tree, hud, ClassKind::TextBox, "ChatBox");
    set(&mut tree, chat, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(0.0, 12.0, 1.0, -62.0)));
    set(&mut tree, chat, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 260.0, 0.0, 34.0)));
    set(&mut tree, chat, Property::AnchorPoint, PropertyValue::Vec2(Vec2::new(0.0, 1.0)));
    set(&mut tree, chat, Property::BackgroundColor, PropertyValue::Color3(dark));
    set(&mut tree, chat, Property::BackgroundTransparency, PropertyValue::Number(0.3));
    set(&mut tree, chat, Property::TextColor, PropertyValue::Color3(white));
    set(&mut tree, chat, Property::TextSize, PropertyValue::Number(16.0));
    set(&mut tree, chat, Property::PlaceholderText, PropertyValue::String("Press / to chat".into()));
    // Rounded corners on every box on the HUD.
    for id in [label, button, message, help, chat, health] {
        let corner = add(&mut tree, id, ClassKind::UICorner, "UICorner");
        set(&mut tree, corner, Property::CornerRadius, PropertyValue::UDim(UDim::new(0.0, 8.0)));
    }
    code(&mut tree, hud, ClassKind::LocalScript, "HudScript", HUD_SCRIPT);

    // Remote events: the game's scripts show messages on the HUD, and the
    // chat box sends what the player types.
    add(&mut tree, storage, ClassKind::RemoteEvent, "Notify");
    add(&mut tree, storage, ClassKind::RemoteEvent, "Say");

    // A stand that sells a speed boost: walk up and hold E.
    let shop = block(&mut tree, ws, "SpeedShop", [6.0, 1.0, -5.0], [3.0, 2.0, 3.0], [60, 200, 120]);
    material(&mut tree, shop, Material::Neon);
    let prompt = add(&mut tree, shop, ClassKind::ProximityPrompt, "ProximityPrompt");
    set(&mut tree, prompt, Property::ActionText, PropertyValue::String("Buy speed".into()));
    set(&mut tree, prompt, Property::ObjectText, PropertyValue::String("Speed boost: 3 coins".into()));
    set(&mut tree, prompt, Property::HoldDuration, PropertyValue::Number(0.5));
    set(&mut tree, prompt, Property::MaxActivationDistance, PropertyValue::Number(8.0));
    script(&mut tree, shop, "Shop", SHOP_SCRIPT);
    let sign = add(&mut tree, shop, ClassKind::BillboardGui, "Sign");
    set(&mut tree, sign, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 170.0, 0.0, 34.0)));
    set(&mut tree, sign, Property::StudsOffset, PropertyValue::Vec3(Vec3::new(0.0, 2.5, 0.0)));
    let sign_label = add(&mut tree, sign, ClassKind::TextLabel, "Label");
    set(&mut tree, sign_label, Property::GuiSize, PropertyValue::UDim2(UDim2::new(1.0, 0.0, 1.0, 0.0)));
    set(
        &mut tree,
        sign_label,
        Property::BackgroundColor,
        PropertyValue::Color3(Color3::from_rgb8(30, 120, 70)),
    );
    set(&mut tree, sign_label, Property::BackgroundTransparency, PropertyValue::Number(0.2));
    set(&mut tree, sign_label, Property::TextColor, PropertyValue::Color3(white));
    set(&mut tree, sign_label, Property::TextSize, PropertyValue::Number(18.0));
    set(&mut tree, sign_label, Property::Text, PropertyValue::String("Speed Shop".into()));

    // One script for every lava brick, found by tag.
    let server = tree.service(ClassKind::ServerScriptService).expect("every tree has ServerScriptService");
    script(&mut tree, server, "Lava", LAVA_SCRIPT);
    script(&mut tree, server, "StreetLights", STREET_LIGHTS_SCRIPT);
    script(&mut tree, server, "Chat", CHAT_SCRIPT);

    // A launch pad on the start platform.
    let pad = block(&mut tree, start, "LaunchPad", [-5.0, 0.1, 5.0], [4.0, 0.2, 4.0], [80, 230, 90]);
    material(&mut tree, pad, Material::Neon);
    script(&mut tree, pad, "Launch", LAUNCH_SCRIPT);

    // Street lamps at the corners of the start, lit at night.
    for (i, (x, z)) in [(7.5, 7.5), (-7.5, 7.5), (7.5, -7.5), (-7.5, -7.5)].into_iter().enumerate() {
        block(&mut tree, start, &format!("LampPost{}", i + 1), [x, 3.0, z], [0.5, 6.0, 0.5], [60, 60, 70]);
        let lamp =
            block(&mut tree, start, &format!("Lamp{}", i + 1), [x, 6.4, z], [1.2, 1.2, 1.2], [255, 220, 150]);
        shape(&mut tree, lamp, PartShape::Ball);
        material(&mut tree, lamp, Material::Glass);
        let light = add(&mut tree, lamp, ClassKind::PointLight, "PointLight");
        set(&mut tree, light, Property::Range, PropertyValue::Number(18.0));
        set(&mut tree, light, Property::Brightness, PropertyValue::Number(2.0));
        set(&mut tree, light, Property::Color, PropertyValue::Color3(Color3::from_rgb8(255, 210, 140)));
        set(&mut tree, light, Property::Enabled, PropertyValue::Bool(false));
        tree.add_tag(lamp, "StreetLight").expect("lamps exist");
    }

    // A quarry in front of the start: a stack of loose crates and some TNT.
    let quarry = add(&mut tree, ws, ClassKind::Folder, "Quarry");
    let ground = block(&mut tree, quarry, "Ground", [0.0, -0.5, 14.0], [16.0, 1.0, 12.0], [150, 130, 100]);
    material(&mut tree, ground, Material::Grass);
    let crates = add(&mut tree, quarry, ClassKind::Folder, "Crates");
    for (i, pos) in [
        [-2.0, 1.0, 16.0],
        [0.0, 1.0, 16.0],
        [2.0, 1.0, 16.0],
        [-1.0, 3.0, 16.0],
        [1.0, 3.0, 16.0],
        [0.0, 5.0, 16.0],
    ]
    .into_iter()
    .enumerate()
    {
        let c = block(&mut tree, crates, &format!("Crate{}", i + 1), pos, [2.0, 2.0, 2.0], [170, 120, 70]);
        material(&mut tree, c, Material::Wood);
        set(&mut tree, c, Property::Anchored, PropertyValue::Bool(false));
    }
    let tnt = block(&mut tree, quarry, "TNT", [5.0, 1.0, 13.0], [2.0, 2.0, 2.0], [220, 40, 40]);
    add(&mut tree, tnt, ClassKind::ClickDetector, "ClickDetector");
    let tnt_sign = add(&mut tree, tnt, ClassKind::BillboardGui, "Sign");
    set(&mut tree, tnt_sign, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 90.0, 0.0, 26.0)));
    set(&mut tree, tnt_sign, Property::StudsOffset, PropertyValue::Vec3(Vec3::new(0.0, 2.2, 0.0)));
    let tnt_label = add(&mut tree, tnt_sign, ClassKind::TextLabel, "Label");
    set(&mut tree, tnt_label, Property::GuiSize, PropertyValue::UDim2(UDim2::new(1.0, 0.0, 1.0, 0.0)));
    set(
        &mut tree,
        tnt_label,
        Property::BackgroundColor,
        PropertyValue::Color3(Color3::from_rgb8(120, 20, 20)),
    );
    set(&mut tree, tnt_label, Property::TextColor, PropertyValue::Color3(white));
    set(&mut tree, tnt_label, Property::TextSize, PropertyValue::Number(16.0));
    set(&mut tree, tnt_label, Property::Text, PropertyValue::String("TNT: click!".into()));
    let tnt_corner = add(&mut tree, tnt_label, ClassKind::UICorner, "UICorner");
    set(&mut tree, tnt_corner, Property::CornerRadius, PropertyValue::UDim(UDim::new(0.5, 0.0)));
    script(&mut tree, tnt, "Blast", TNT_SCRIPT);

    // A zombie that lives in the quarry.
    let zombie = add(&mut tree, ws, ClassKind::Model, "Zombie");
    let zombie_body =
        block(&mut tree, zombie, "HumanoidRootPart", [-5.0, 2.1, 13.0], [2.0, 4.0, 1.5], [90, 140, 80]);
    set(&mut tree, zombie_body, Property::Anchored, PropertyValue::Bool(false));
    let zombie_head = block(&mut tree, zombie, "Head", [-5.0, 4.7, 13.0], [1.2, 1.2, 1.2], [140, 190, 120]);
    set(&mut tree, zombie_head, Property::CanCollide, PropertyValue::Bool(false));
    add(&mut tree, zombie, ClassKind::Humanoid, "Humanoid");
    let zombie_tag = add(&mut tree, zombie_head, ClassKind::BillboardGui, "NameTag");
    set(&mut tree, zombie_tag, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 120.0, 0.0, 26.0)));
    set(&mut tree, zombie_tag, Property::StudsOffset, PropertyValue::Vec3(Vec3::new(0.0, 1.6, 0.0)));
    let zombie_label = add(&mut tree, zombie_tag, ClassKind::TextLabel, "Label");
    set(&mut tree, zombie_label, Property::GuiSize, PropertyValue::UDim2(UDim2::new(1.0, 0.0, 1.0, 0.0)));
    set(&mut tree, zombie_label, Property::BackgroundTransparency, PropertyValue::Number(1.0));
    set(
        &mut tree,
        zombie_label,
        Property::TextColor,
        PropertyValue::Color3(Color3::from_rgb8(170, 255, 140)),
    );
    set(&mut tree, zombie_label, Property::TextSize, PropertyValue::Number(17.0));
    set(&mut tree, zombie_label, Property::Text, PropertyValue::String("Zombie".into()));
    script(&mut tree, server, "ZombieBrain", ZOMBIE_SCRIPT);

    // A mine cart in the quarry: a loose base with its sides welded on.
    let cart = add(&mut tree, ws, ClassKind::Model, "MineCart");
    let iron = [90, 90, 100];
    let base = block(&mut tree, cart, "Base", [5.0, 0.3, 18.0], [2.4, 0.6, 3.0], [110, 80, 55]);
    material(&mut tree, base, Material::Wood);
    add(&mut tree, base, ClassKind::ClickDetector, "ClickDetector");
    for (name, pos, size) in [
        ("Left", [3.95, 1.1, 18.0], [0.3, 1.0, 3.0]),
        ("Right", [6.05, 1.1, 18.0], [0.3, 1.0, 3.0]),
        ("Front", [5.0, 1.1, 16.65], [1.8, 1.0, 0.3]),
        ("Back", [5.0, 1.1, 19.35], [1.8, 1.0, 0.3]),
    ] {
        let side = block(&mut tree, cart, name, pos, size, iron);
        material(&mut tree, side, Material::Metal);
        set(&mut tree, side, Property::Anchored, PropertyValue::Bool(false));
        let weld = add(&mut tree, base, ClassKind::WeldConstraint, &format!("{name}Weld"));
        set(&mut tree, weld, Property::Part0, PropertyValue::Ref(Some(base)));
        set(&mut tree, weld, Property::Part1, PropertyValue::Ref(Some(side)));
    }
    set(&mut tree, base, Property::Anchored, PropertyValue::Bool(false));
    script(&mut tree, cart, "Push", CART_SCRIPT);

    // A pressure plate beside the speed shop.
    let plate = block(&mut tree, start, "PressurePlate", [6.0, 0.05, -1.0], [3.0, 0.1, 3.0], [110, 110, 120]);
    material(&mut tree, plate, Material::Metal);
    script(&mut tree, plate, "Plate", PLATE_SCRIPT);

    // A kart track past the quarry, walled in, with a kart to drive and cones to dodge.
    let track = add(&mut tree, ws, ClassKind::Folder, "KartTrack");
    let asphalt = block(&mut tree, track, "Ground", [0.0, -0.5, 37.0], [40.0, 1.0, 34.0], [70, 72, 78]);
    material(&mut tree, asphalt, Material::SmoothPlastic);
    for (name, pos, size) in [
        ("WallLeft", [-20.5, 0.5, 37.0], [1.0, 1.0, 34.0]),
        ("WallRight", [20.5, 0.5, 37.0], [1.0, 1.0, 34.0]),
        ("WallFar", [0.0, 0.5, 54.5], [42.0, 1.0, 1.0]),
    ] {
        block(&mut tree, track, name, pos, size, [220, 60, 50]);
    }
    for (i, (x, z)) in
        [(-8.0, 34.0), (0.0, 40.0), (8.0, 34.0), (-4.0, 47.0), (6.0, 47.0)].into_iter().enumerate()
    {
        let cone =
            block(&mut tree, track, &format!("Cone{}", i + 1), [x, 0.75, z], [1.0, 1.5, 1.0], [255, 140, 30]);
        material(&mut tree, cone, Material::Neon);
    }
    kart(&mut tree, track, Vec3::new(0.0, 0.0, 26.0), 180.0, [220, 50, 60]);
    // And a bench on the start platform, to sit on.
    let bench = add(&mut tree, start, ClassKind::Seat, "Bench");
    set(&mut tree, bench, Property::Position, PropertyValue::Vec3(Vec3::new(-4.0, 0.5, -6.0)));
    set(&mut tree, bench, Property::Size, PropertyValue::Vec3(Vec3::new(3.0, 1.0, 1.5)));
    set(&mut tree, bench, Property::Color, PropertyValue::Color3(Color3::from_rgb8(150, 105, 65)));
    set(&mut tree, bench, Property::Anchored, PropertyValue::Bool(true));
    material(&mut tree, bench, Material::Wood);

    // Characters come back three seconds after they die.
    let players = tree.service(ClassKind::Players).expect("every tree has Players");
    set(&mut tree, players, Property::RespawnTime, PropertyValue::Number(3.0));

    // Everyone starts as a Runner; finishing the obby makes you a Winner.
    let teams = tree.service(ClassKind::Teams).expect("every tree has Teams");
    let runners = add(&mut tree, teams, ClassKind::Team, "Runners");
    set(&mut tree, runners, Property::TeamColor, PropertyValue::Color3(Color3::from_rgb8(90, 170, 255)));
    let winners = add(&mut tree, teams, ClassKind::Team, "Winners");
    set(&mut tree, winners, Property::TeamColor, PropertyValue::Color3(Color3::from_rgb8(255, 210, 60)));
    set(&mut tree, winners, Property::AutoAssignable, PropertyValue::Bool(false));

    // A builder tool every player gets: press 1 to hold it, click to place bricks.
    let pack = tree.service(ClassKind::StarterPack).expect("every tree has StarterPack");
    let builder = add(&mut tree, pack, ClassKind::Tool, "Builder");
    let handle = block(&mut tree, builder, "Handle", [0.0, 0.0, 0.0], [0.5, 0.5, 2.5], [240, 140, 40]);
    set(&mut tree, handle, Property::Anchored, PropertyValue::Bool(false));
    set(&mut tree, handle, Property::CanCollide, PropertyValue::Bool(false));
    add(&mut tree, builder, ClassKind::RemoteEvent, "Place");
    code(&mut tree, builder, ClassKind::LocalScript, "Aim", AIM_SCRIPT);
    script(&mut tree, builder, "Build", BUILD_SCRIPT);

    let coins = add(&mut tree, ws, ClassKind::Folder, "Coins");
    let coin_spots: [[f32; 3]; 7] = [
        [0.0, 5.5, -18.0],
        [0.0, 5.5, -22.0],
        [14.0, 2.5, 0.0],
        [30.0, 5.5, 0.0],
        [48.0, 7.0, 0.0],
        [84.0, 7.0, 0.0],
        [-12.0, 1.5, 0.0],
    ];
    for (i, spot) in coin_spots.iter().enumerate() {
        let coin = block(&mut tree, coins, &format!("Coin{}", i + 1), *spot, [0.4, 2.0, 2.0], [255, 205, 40]);
        shape(&mut tree, coin, PartShape::Cylinder);
        material(&mut tree, coin, Material::Neon);
        set(&mut tree, coin, Property::CanCollide, PropertyValue::Bool(false));
        tree.add_tag(coin, "Coin").expect("coins exist");
    }
    // The vault coin is worth more.
    let vault_coin = tree.find_first_child(coins, "Coin7").ok().flatten().expect("the vault coin exists");
    tree.set_attribute(vault_coin, "Value", Some(PropertyValue::Number(5.0))).expect("coins take attributes");
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
        tree.add_tag(brick, "Lava").expect("bricks exist");
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
        let back = CharacterInput { move_dir: Vec3::new(0.0, 0.0, -1.0), jump: false, ..Default::default() };
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

    #[test]
    fn vault_door_opens_and_hud_works() {
        let mut session = start_on(sample_obby(), true).unwrap();
        for _ in 0..10 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        let (door, label, button, lighting) = {
            let tree = session.runtime().tree().borrow();
            let vault = tree.find_first_child(tree.workspace(), "Vault").unwrap().unwrap();
            let door = tree.find_first_child(vault, "Door").unwrap().unwrap();
            let gui = session.player_gui().unwrap();
            let hud = tree.find_first_child(gui, "HUD").unwrap().unwrap();
            let label = tree.find_first_child(hud, "CoinLabel").unwrap().unwrap();
            let button = tree.find_first_child(hud, "DayNight").unwrap().unwrap();
            (door, label, button, tree.service(ClassKind::Lighting).unwrap())
        };
        let get = |s: &libroxel_engine::PlaySession, id, p| s.runtime().tree().borrow().get(id, p).unwrap();
        assert_eq!(get(&session, label, Property::Text), PropertyValue::String("Coins: 0".into()));

        session.click_button(button);
        for _ in 0..150 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        assert_eq!(get(&session, lighting, Property::ClockTime), PropertyValue::Number(21.0));
        assert_eq!(get(&session, button, Property::Text), PropertyValue::String("Make it day".into()));

        assert!(session.click_part(door));
        for _ in 0..60 {
            session.step(1.0 / 60.0, CharacterInput::default());
        }
        let PropertyValue::Vec3(open) = get(&session, door, Property::Position) else { panic!() };
        assert!((open.y - 8.5).abs() < 0.01, "door slid up: {open:?}");

        // Walk into the open vault for the coin; the counter follows.
        let west = CharacterInput { move_dir: Vec3::new(-1.0, 0.0, 0.0), jump: false, ..Default::default() };
        for _ in 0..60 {
            session.step(1.0 / 60.0, west);
        }
        let log = session.take_log();
        assert_eq!(leaderstat(&session, "Coins"), 5.0, "{log:?}");
        assert_eq!(get(&session, label, Property::Text), PropertyValue::String("Coins: 5".into()));
        assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
    }

    #[test]
    fn shop_prompt_messages_keys_and_saved_coins() {
        use libroxel_instance::KeyCode;
        use libroxel_script::UserInput;
        let frame = |s: &mut libroxel_engine::PlaySession, n: usize| {
            for _ in 0..n {
                s.step(1.0 / 60.0, CharacterInput::default());
            }
        };
        // Start with 5 saved coins, as if from an earlier play test.
        let shared = std::rc::Rc::new(std::cell::RefCell::new(sample_obby()));
        let mut session =
            libroxel_engine::PlaySession::start_with_data(shared, r#"{"PlayerData": {"coins_1": 5}}"#)
                .unwrap();
        frame(&mut session, 10);
        assert_eq!(leaderstat(&session, "Coins"), 5.0);
        let (hud, shop) = {
            let tree = session.runtime().tree().borrow();
            let hud = tree.find_first_child(session.player_gui().unwrap(), "HUD").unwrap().unwrap();
            (hud, tree.find_first_child(tree.workspace(), "SpeedShop").unwrap().unwrap())
        };
        let child = |s: &libroxel_engine::PlaySession, name: &str| {
            let tree = s.runtime().tree().borrow();
            tree.find_first_child(hud, name).unwrap().unwrap()
        };
        let get = |s: &libroxel_engine::PlaySession, id, p| s.runtime().tree().borrow().get(id, p).unwrap();
        assert_eq!(
            get(&session, child(&session, "Height"), Property::Text),
            PropertyValue::String("Height: 0 studs".into())
        );

        // Out of reach at the spawn, so no prompt. Walk over to the stand.
        assert!(session.active_prompt().is_none());
        session
            .runtime()
            .run("move", "workspace.Player1.HumanoidRootPart.Position = workspace.SpeedShop.Position + Vector3.new(-3, 2, 0)")
            .unwrap();
        frame(&mut session, 5);
        let active = session.active_prompt().expect("the shop prompt shows");
        assert_eq!((active.part, active.key), (shop, KeyCode::E));

        // Holding E for half a second buys the boost.
        session.input_began(UserInput::Key(KeyCode::E), false);
        frame(&mut session, 15);
        assert_eq!(leaderstat(&session, "Coins"), 5.0, "not held long enough yet");
        let progress = session.active_prompt().unwrap().progress;
        assert!((0.4..0.6).contains(&progress), "{progress}");
        frame(&mut session, 20);
        session.input_ended(UserInput::Key(KeyCode::E), false);
        frame(&mut session, 2);
        assert_eq!(leaderstat(&session, "Coins"), 2.0);
        let humanoid = session.character().unwrap().humanoid;
        assert_eq!(get(&session, humanoid, Property::WalkSpeed), PropertyValue::Number(32.0));
        let message = child(&session, "Message");
        assert_eq!(get(&session, message, Property::Visible), PropertyValue::Bool(true));
        assert_eq!(
            get(&session, message, Property::Text),
            PropertyValue::String("Speed boost for 10 seconds!".into())
        );

        // Not enough coins the second time.
        session.input_began(UserInput::Key(KeyCode::E), false);
        frame(&mut session, 40);
        session.input_ended(UserInput::Key(KeyCode::E), false);
        frame(&mut session, 1);
        assert_eq!(leaderstat(&session, "Coins"), 2.0);
        assert_eq!(
            get(&session, message, Property::Text),
            PropertyValue::String("You need 3 coins for a speed boost".into())
        );

        // H toggles the help panel.
        let help = child(&session, "Help");
        session.input_began(UserInput::Key(KeyCode::H), false);
        session.input_ended(UserInput::Key(KeyCode::H), false);
        assert_eq!(get(&session, help, Property::Visible), PropertyValue::Bool(false));

        // Leaving saves the coins.
        session.stop();
        let saved: serde_json::Value = serde_json::from_str(&session.save_data()).unwrap();
        assert_eq!(saved["PlayerData"]["coins_1"], 2.0);
        let errors: Vec<_> = session.take_log().into_iter().filter(|l| l.level == LogLevel::Error).collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn builder_tool_name_tags_and_tagged_lava() {
        use libroxel_instance::KeyCode;
        use libroxel_script::{MouseRay, UserInput};
        let frame = |s: &mut libroxel_engine::PlaySession, n: usize| {
            for _ in 0..n {
                s.step(1.0 / 60.0, CharacterInput::default());
            }
        };
        let mut session = start_on(sample_obby(), true).unwrap();
        frame(&mut session, 5);
        let character = session.character().unwrap();
        let tree = session.runtime().tree().clone();
        // The name tag script put a billboard on the head.
        let tag = tree.borrow().find_first_child_of_class(character.head, ClassKind::BillboardGui).unwrap();
        assert!(tag.is_some(), "the character has a name tag");

        // The builder is in slot 1; pressing 1 holds it.
        let hotbar = session.hotbar();
        assert_eq!(hotbar.len(), 1);
        assert_eq!(tree.borrow().name(hotbar[0].0).unwrap(), "Builder");
        session.input_began(UserInput::Key(KeyCode::One), false);
        session.input_ended(UserInput::Key(KeyCode::One), false);
        frame(&mut session, 2);
        assert!(session.hotbar()[0].1, "held");
        let handle = tree.borrow().find_first_child(hotbar[0].0, "Handle").unwrap().unwrap();
        let hand = tree.borrow().get_vec3(handle, Property::Position).unwrap();
        let body = tree.borrow().get_vec3(character.root, Property::Position).unwrap();
        assert!((hand - body).length() < 3.0, "the handle is in the character's hand: {hand:?} {body:?}");

        // Aim at the start platform and click.
        session.set_mouse(Some(MouseRay {
            origin: Vec3::new(4.0, 20.0, 4.0),
            direction: Vec3::new(0.0, -1.0, 0.0),
            x: 0.0,
            y: 0.0,
        }));
        session.input_began(UserInput::MouseButton1, false);
        session.input_ended(UserInput::MouseButton1, false);
        frame(&mut session, 2);
        let brick = tree.borrow().find_first_child(tree.borrow().workspace(), "BuiltBrick").unwrap();
        let brick = brick.expect("a brick was built");
        assert_eq!(tree.borrow().get_vec3(brick, Property::Position), Some(Vec3::new(4.0, 0.5, 4.0)));
        assert_eq!(
            tree.borrow().get_attribute(brick, "Builder").unwrap(),
            Some(PropertyValue::String("Player1".into()))
        );

        // Pressing 1 again puts it away.
        session.input_began(UserInput::Key(KeyCode::One), false);
        frame(&mut session, 1);
        assert!(!session.hotbar()[0].1);

        // Lava found by its tag still kills.
        session
            .runtime()
            .run("lava", "workspace.Player1.HumanoidRootPart.Position = workspace.Lava.LavaBrick.Position + Vector3.new(0, 2, 0)")
            .unwrap();
        frame(&mut session, 30);
        let log = session.take_log();
        assert!(log.iter().any(|l| l.text == "Your character died"), "{log:?}");
        assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
    }

    #[test]
    fn launch_pad_tnt_chat_and_street_lights() {
        let frame = |s: &mut libroxel_engine::PlaySession, n: usize| {
            for _ in 0..n {
                s.step(1.0 / 60.0, CharacterInput::default());
            }
        };
        let mut session = start_on(sample_obby(), true).unwrap();
        frame(&mut session, 10);
        let tree = session.runtime().tree().clone();
        let character = session.character().unwrap();

        // The pad throws the character high into the air.
        session
            .runtime()
            .run("move", "workspace.Player1.HumanoidRootPart.Position = workspace.Start.LaunchPad.Position + Vector3.new(0, 2.2, 0)")
            .unwrap();
        let mut peak: f32 = 0.0;
        for _ in 0..60 {
            session.step(1.0 / 60.0, CharacterInput::default());
            peak = peak.max(tree.borrow().get_vec3(character.root, Property::Position).unwrap().y);
        }
        assert!(peak > 20.0, "launched: {peak}");
        frame(&mut session, 60);

        // Clicking the TNT throws the crates.
        let (tnt, crate1) = {
            let t = tree.borrow();
            let quarry = t.find_first_child(t.workspace(), "Quarry").unwrap().unwrap();
            let crates = t.find_first_child(quarry, "Crates").unwrap().unwrap();
            (
                t.find_first_child(quarry, "TNT").unwrap().unwrap(),
                t.find_first_child(crates, "Crate1").unwrap().unwrap(),
            )
        };
        let before = tree.borrow().get_vec3(crate1, Property::Position).unwrap();
        assert!(session.click_part(tnt));
        frame(&mut session, 30);
        let after = tree.borrow().get_vec3(crate1, Property::Position).unwrap();
        assert!((after - before).length() > 2.0, "the crate flew: {before:?} -> {after:?}");

        // Chat floats a bubble over the head.
        session.runtime().run("say", "game.ReplicatedStorage.Say:FireServer('hello there')").unwrap();
        frame(&mut session, 2);
        let bubble = tree.borrow().find_first_child(character.head, "Bubble").unwrap();
        assert!(bubble.is_some());

        // At night the street lights come on.
        session.runtime().run("night", "game.Lighting.ClockTime = 20").unwrap();
        frame(&mut session, 2);
        let on = {
            let t = tree.borrow();
            let start = t.find_first_child(t.workspace(), "Start").unwrap().unwrap();
            let lamp = t.find_first_child(start, "Lamp1").unwrap().unwrap();
            let light = t.find_first_child_of_class(lamp, ClassKind::PointLight).unwrap().unwrap();
            t.get_bool(light, Property::Enabled).unwrap()
        };
        assert!(on);

        let log = session.take_log();
        assert!(
            log.iter().any(|l| l.text.starts_with("Player1 blew up ") && !l.text.contains(" 0 ")),
            "{log:?}"
        );
        assert!(log.iter().any(|l| l.text == "Player1: hello there"), "{log:?}");
        assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
    }

    #[test]
    fn zombie_chases_and_teams_change_at_the_finish() {
        let frame = |s: &mut libroxel_engine::PlaySession, n: usize| {
            for _ in 0..n {
                s.step(1.0 / 60.0, CharacterInput::default());
            }
        };
        let mut session = start_on(sample_obby(), true).unwrap();
        frame(&mut session, 10);
        let tree = session.runtime().tree().clone();
        let character = session.character().unwrap();
        let team_name = |s: &libroxel_engine::PlaySession| {
            let t = s.runtime().tree().borrow();
            let team = t.get_ref(s.player().unwrap(), Property::Team).unwrap();
            t.name(team).unwrap().to_owned()
        };
        assert_eq!(team_name(&session), "Runners");

        // Safe on the start platform: the zombie stays home.
        frame(&mut session, 60);
        assert_eq!(tree.borrow().get_number(character.humanoid, Property::Health), Some(100.0));
        // In the quarry it comes for you.
        session
            .runtime()
            .run("move", "workspace.Player1.HumanoidRootPart.Position = Vector3.new(-5, 3, 18.5)")
            .unwrap();
        frame(&mut session, 150);
        let health = tree.borrow().get_number(character.humanoid, Property::Health).unwrap();
        assert!(health < 100.0, "the zombie hit: {health}");

        // Knocked down, then back a few seconds later.
        session.runtime().run("kill", "workspace.Zombie.Humanoid.Health = 0").unwrap();
        frame(&mut session, 60 * 6);
        let zombie = tree.borrow().find_first_child(tree.borrow().workspace(), "Zombie").unwrap().unwrap();
        assert_eq!(
            tree.borrow()
                .find_first_child(zombie, "Humanoid")
                .ok()
                .flatten()
                .and_then(|h| tree.borrow().get_number(h, Property::Health)),
            Some(100.0)
        );

        session
            .runtime()
            .run("finish", "workspace.Player1.HumanoidRootPart.Position = workspace.Finish.Position + Vector3.new(0, 3, 0)")
            .unwrap();
        frame(&mut session, 30);
        assert_eq!(team_name(&session), "Winners");
        let log = session.take_log();
        assert!(log.iter().any(|l| l.text == "The zombie is down!"), "{log:?}");
        assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
    }

    #[test]
    fn mine_cart_rolls_together_and_the_plate_and_health_bar_react() {
        let frame = |s: &mut libroxel_engine::PlaySession, n: usize| {
            for _ in 0..n {
                s.step(1.0 / 60.0, CharacterInput::default());
            }
        };
        let mut session = start_on(sample_obby(), true).unwrap();
        frame(&mut session, 30);
        let tree = session.runtime().tree().clone();
        let part = |path: &[&str]| {
            let t = tree.borrow();
            let mut at = t.workspace();
            for name in path {
                at = t.find_first_child(at, name).unwrap().unwrap();
            }
            at
        };
        let (base, left) = (part(&["MineCart", "Base"]), part(&["MineCart", "Left"]));
        let pos = |id| tree.borrow().get_vec3(id, Property::Position).unwrap();
        let (base_start, left_start) = (pos(base), pos(left));
        assert!((base_start.y - 0.3).abs() < 0.05, "resting on the ground: {base_start:?}");
        assert!((left_start - base_start - Vec3::new(-1.05, 0.8, 0.0)).length() < 0.01);

        // Walk up to the cart and click it.
        session
            .runtime()
            .run("move", "workspace.Player1.HumanoidRootPart.Position = Vector3.new(5, 3, 12)")
            .unwrap();
        frame(&mut session, 5);
        assert!(session.click_part(base));
        frame(&mut session, 90);
        let (base_end, left_end) = (pos(base), pos(left));
        assert!(base_end.z < base_start.z - 2.0, "pushed: {base_start:?} -> {base_end:?}");
        assert!((left_end - base_end - (left_start - base_start)).length() < 0.01, "the side came along");

        // Back on the start, away from the zombie, the health bar shows damage.
        session
            .runtime()
            .run("home", "workspace.Player1.HumanoidRootPart.Position = Vector3.new(0, 3, 0)")
            .unwrap();
        frame(&mut session, 5);
        session.runtime().run("heal", "workspace.Player1.Humanoid.Health = 100").unwrap();
        frame(&mut session, 2);
        session.runtime().run("hurt", "workspace.Player1.Humanoid:TakeDamage(80)").unwrap();
        frame(&mut session, 2);
        let gui = session.player_gui().unwrap();
        let (fill_size, text) = {
            let t = tree.borrow();
            let hud = t.find_first_child(gui, "HUD").unwrap().unwrap();
            let bar = t.find_first_child(hud, "HealthBar").unwrap().unwrap();
            let fill = t.find_first_child(bar, "Fill").unwrap().unwrap();
            let label = t.find_first_child(bar, "Label").unwrap().unwrap();
            (t.get(fill, Property::GuiSize).unwrap(), t.get_string(label, Property::Text).unwrap())
        };
        assert_eq!(fill_size, PropertyValue::UDim2(UDim2::new(0.2, 0.0, 1.0, 0.0)));
        assert_eq!(text, "Health: 20");

        // Standing on the plate lights it; stepping off puts it out.
        let plate = part(&["Start", "PressurePlate"]);
        session
            .runtime()
            .run("plate", "workspace.Player1.HumanoidRootPart.Position = Vector3.new(6, 3, -1)")
            .unwrap();
        frame(&mut session, 30);
        assert_eq!(
            tree.borrow().get(plate, Property::Material).unwrap(),
            PropertyValue::Enum(libroxel_instance::EnumValue::Material(Material::Neon))
        );
        session
            .runtime()
            .run("off", "workspace.Player1.HumanoidRootPart.Position = Vector3.new(0, 3, 0)")
            .unwrap();
        frame(&mut session, 30);
        assert_eq!(
            tree.borrow().get(plate, Property::Material).unwrap(),
            PropertyValue::Enum(libroxel_instance::EnumValue::Material(Material::Metal))
        );
        let log = session.take_log();
        assert!(log.iter().any(|l| l.text == "Player1 pushed the mine cart"), "{log:?}");
        assert!(log.iter().any(|l| l.text == "Pressure plate pressed"), "{log:?}");
        assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
    }

    #[test]
    fn the_kart_drives_and_the_bench_seats() {
        let frame = |s: &mut libroxel_engine::PlaySession, n: usize, input: CharacterInput| {
            for _ in 0..n {
                s.step(1.0 / 60.0, input);
            }
        };
        let idle = CharacterInput::default();
        let mut session = start_on(sample_obby(), true).unwrap();
        frame(&mut session, 30, idle);
        let tree = session.runtime().tree().clone();
        let character = session.character().unwrap();
        let (chassis, seat) = {
            let t = tree.borrow();
            let track = t.find_first_child(t.workspace(), "KartTrack").unwrap().unwrap();
            let kart = t.find_first_child(track, "Kart").unwrap().unwrap();
            (
                t.find_first_child(kart, "Chassis").unwrap().unwrap(),
                t.find_first_child(kart, "VehicleSeat").unwrap().unwrap(),
            )
        };
        let pos = |id| tree.borrow().get_vec3(id, Property::Position).unwrap();
        let start = pos(chassis);
        assert!((start.y - 0.5).abs() < 0.05, "parked on the track: {start:?}");

        // Drop in from above the seat, then drive away from the quarry.
        let above = pos(seat) + Vec3::new(0.0, 3.0, 0.0);
        session
            .runtime()
            .run(
                "hop",
                &format!(
                    "workspace.Player1.HumanoidRootPart.Position = Vector3.new({}, {}, {})",
                    above.x, above.y, above.z
                ),
            )
            .unwrap();
        frame(&mut session, 30, idle);
        assert_eq!(tree.borrow().get_ref(seat, Property::Occupant), Some(character.humanoid));
        frame(&mut session, 45, CharacterInput { throttle: 1.0, ..Default::default() });
        let end = pos(chassis);
        assert!(end.z > start.z + 8.0, "drove toward the far wall: {start:?} -> {end:?}");
        assert!((pos(character.root).z - pos(seat).z).abs() < 0.01, "the driver rides along");

        // Jump out and sit on the bench instead.
        frame(&mut session, 1, CharacterInput { jump: true, ..Default::default() });
        frame(&mut session, 70, idle);
        session.runtime().run("bench", "workspace.Start.Bench:Sit(workspace.Player1.Humanoid)").unwrap();
        frame(&mut session, 2, idle);
        let bench = {
            let t = tree.borrow();
            let start = t.find_first_child(t.workspace(), "Start").unwrap().unwrap();
            t.find_first_child(start, "Bench").unwrap().unwrap()
        };
        assert_eq!(tree.borrow().get_ref(bench, Property::Occupant), Some(character.humanoid));
        assert_eq!(tree.borrow().get_ref(seat, Property::Occupant), None);
        let log = session.take_log();
        assert!(log.iter().all(|l| l.level != LogLevel::Error), "{log:?}");
    }
}
