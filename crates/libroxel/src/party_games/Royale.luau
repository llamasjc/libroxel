local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local UserInputService = game:GetService("UserInputService")
local RunService = game:GetService("RunService")

local me = Players.LocalPlayer
local remotes = ReplicatedStorage:WaitForChild("Royale")
local dropEvent = remotes:WaitForChild("Drop")
local buildEvent = remotes:WaitForChild("Build")
local resultEvent = remotes:WaitForChild("Result")

local GLIDE_FALL = 22
local WHITE = Color3.fromRGB(255, 255, 255)
local INK = Color3.fromRGB(15, 17, 22)
local GREEN = Color3.fromRGB(80, 200, 90)
local BLUE = Color3.fromRGB(70, 160, 245)
local WOOD = Color3.fromRGB(190, 145, 95)
local GOLD = Color3.fromRGB(245, 195, 40)

local function make(class, props, parent)
	local object = Instance.new(class)
	for key, value in props do
		object[key] = value
	end
	object.Parent = parent
	return object
end

local function round(object, radius)
	make("UICorner", { CornerRadius = UDim.new(0, radius or 6) }, object)
end

local screen = make("ScreenGui", { Name = "Royale", ResetOnSpawn = false }, me:WaitForChild("PlayerGui"))

local bars = make("Frame", {
	Name = "Bars",
	AnchorPoint = Vector2.new(0.5, 1),
	Position = UDim2.new(0.5, 0, 1, -16),
	Size = UDim2.new(0, 300, 0, 46),
	BackgroundTransparency = 1,
	Visible = false,
}, screen)

local function bar(name, y, colour)
	local back = make("Frame", {
		Name = name,
		Position = UDim2.new(0, 0, 0, y),
		Size = UDim2.new(1, 0, 0, 20),
		BackgroundColor3 = INK,
		BackgroundTransparency = 0.3,
	}, bars)
	round(back, 4)
	local fill = make("Frame", { Name = "Fill", Size = UDim2.new(1, 0, 1, 0), BackgroundColor3 = colour }, back)
	round(fill, 4)
	local label = make("TextLabel", {
		Name = "Value",
		Size = UDim2.new(1, -8, 1, 0),
		Position = UDim2.new(0, 8, 0, 0),
		BackgroundTransparency = 1,
		TextColor3 = WHITE,
		TextSize = 15,
		TextXAlignment = Enum.TextXAlignment.Left,
		Text = "",
	}, back)
	make("UIStroke", { Color = INK, Thickness = 1.5 }, label)
	return fill, label
end

local shieldFill, shieldText = bar("Shield", 0, BLUE)
local healthFill, healthText = bar("Health", 24, GREEN)

local wood = make("TextLabel", {
	Name = "Wood",
	AnchorPoint = Vector2.new(0, 1),
	Position = UDim2.new(0.5, 162, 1, -16),
	Size = UDim2.new(0, 70, 0, 44),
	BackgroundColor3 = WOOD,
	TextColor3 = WHITE,
	TextSize = 22,
	Text = "0",
	Visible = false,
}, screen)
round(wood)
make("UIStroke", { Color = INK, Thickness = 1.5 }, wood)

local buildRow = make("Frame", {
	Name = "Build",
	AnchorPoint = Vector2.new(1, 1),
	Position = UDim2.new(1, -16, 1, -16),
	Size = UDim2.new(0, 246, 0, 44),
	BackgroundTransparency = 1,
	Visible = false,
}, screen)
make("UIListLayout", {
	FillDirection = Enum.FillDirection.Horizontal,
	Padding = UDim.new(0, 6),
	HorizontalAlignment = Enum.HorizontalAlignment.Right,
}, buildRow)

local function look()
	local camera = workspace.CurrentCamera
	return camera and camera.CFrame.LookVector or nil
end

local function place(kind)
	buildEvent:FireServer(kind, look())
end

for i, kind in { "Wall", "Floor", "Ramp" } do
	local button = make("TextButton", {
		Name = kind,
		LayoutOrder = i,
		Size = UDim2.new(0, 78, 1, 0),
		BackgroundColor3 = INK,
		BackgroundTransparency = 0.25,
		TextColor3 = WHITE,
		TextSize = 18,
		Text = kind,
	}, buildRow)
	round(button)
	button.MouseButton1Click:Connect(function()
		place(kind)
	end)
end

local jumpButton = make("TextButton", {
	Name = "Jump",
	AnchorPoint = Vector2.new(0.5, 1),
	Position = UDim2.new(0.5, 0, 1, -80),
	Size = UDim2.new(0, 160, 0, 52),
	BackgroundColor3 = GOLD,
	TextColor3 = INK,
	TextSize = 26,
	Text = "Jump",
	Visible = false,
}, screen)
round(jumpButton, 8)
jumpButton.MouseButton1Click:Connect(function()
	dropEvent:FireServer()
end)

local result = make("Frame", {
	Name = "Result",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.new(0.5, 0, 0.42, 0),
	Size = UDim2.new(0, 420, 0, 170),
	BackgroundColor3 = INK,
	BackgroundTransparency = 0.2,
	Visible = false,
	ZIndex = 4,
}, screen)
round(result, 12)
local placeText = make("TextLabel", {
	Name = "Place",
	Position = UDim2.new(0, 0, 0, 14),
	Size = UDim2.new(1, 0, 0, 90),
	BackgroundTransparency = 1,
	TextColor3 = GOLD,
	TextSize = 84,
	Text = "",
	ZIndex = 5,
}, result)
local lineText = make("TextLabel", {
	Name = "Line",
	Position = UDim2.new(0, 0, 0, 110),
	Size = UDim2.new(1, 0, 0, 40),
	BackgroundTransparency = 1,
	TextColor3 = WHITE,
	TextSize = 26,
	Text = "",
	ZIndex = 5,
}, result)

local shown = 0
resultEvent.OnClientEvent:Connect(function(place, total, by)
	shown += 1
	local mine = shown
	placeText.Text = "#" .. place
	placeText.TextColor3 = place == 1 and GOLD or WHITE
	if place == 1 then
		lineText.Text = "Victory"
	elseif by then
		lineText.Text = "Knocked out by " .. by
	else
		lineText.Text = "Out of " .. total
	end
	result.Visible = true
	task.wait(6)
	if shown == mine then
		result.Visible = false
	end
end)

local KEYS = { { Enum.KeyCode.Z, "Wall" }, { Enum.KeyCode.X, "Floor" }, { Enum.KeyCode.C, "Ramp" } }

UserInputService.InputBegan:Connect(function(input, used)
	if used then
		return
	end
	local role = me:GetAttribute("Role")
	if role == "Riding" and (input.KeyCode == Enum.KeyCode.Space or input.KeyCode == Enum.KeyCode.E) then
		dropEvent:FireServer()
	elseif role == "Fighter" then
		for _, key in KEYS do
			if input.KeyCode == key[1] then
				place(key[2])
			end
		end
	end
end)

RunService.Heartbeat:Connect(function()
	if me:GetAttribute("Role") ~= "Gliding" then
		return
	end
	local root = me.Character and me.Character:FindFirstChild("HumanoidRootPart")
	if root then
		local v = root.AssemblyLinearVelocity
		if v.Y < -GLIDE_FALL then
			root.AssemblyLinearVelocity = Vector3.new(v.X, -GLIDE_FALL, v.Z)
		end
	end
end)

local IN_GAME = { Riding = true, Gliding = true, Fighter = true, Winner = true }

while true do
	local role = me:GetAttribute("Role")
	local playing = IN_GAME[role] == true
	bars.Visible = playing
	wood.Visible = playing
	buildRow.Visible = role == "Fighter"
	jumpButton.Visible = role == "Riding"
	local humanoid = me.Character and me.Character:FindFirstChildOfClass("Humanoid")
	if playing and humanoid then
		local health = math.max(0, math.ceil(humanoid.Health))
		healthFill.Size = UDim2.new(math.clamp(humanoid.Health / humanoid.MaxHealth, 0, 1), 0, 1, 0)
		healthText.Text = tostring(health)
		local shield = me:GetAttribute("Shield") or 0
		shieldFill.Size = UDim2.new(math.clamp(shield / 100, 0, 1), 0, 1, 0)
		shieldFill.Visible = shield > 0
		shieldText.Text = tostring(shield)
		wood.Text = tostring(me:GetAttribute("Wood") or 0)
	end
	task.wait(0.1)
end
