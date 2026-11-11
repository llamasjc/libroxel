//! Starter worlds: the blank template and a sample obby.

use libroxel_instance::{
    ClassKind, Color3, EnumValue, InstanceId, InstanceTree, Material, PartShape, Property, PropertyValue,
    UDim, UDim2, Vec2, Vec3,
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
    let help = add(&mut tree, hud, ClassKind::Frame, "Help");
    set(&mut tree, help, Property::GuiPosition, PropertyValue::UDim2(UDim2::new(0.0, 12.0, 0.5, 0.0)));
    set(&mut tree, help, Property::GuiSize, PropertyValue::UDim2(UDim2::new(0.0, 260.0, 0.0, 170.0)));
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
    for id in [label, button, message, help, chat] {
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
        let west = CharacterInput { move_dir: Vec3::new(-1.0, 0.0, 0.0), jump: false };
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
}
