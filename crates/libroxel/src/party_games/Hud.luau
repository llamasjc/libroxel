local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local me = Players.LocalPlayer
local round = ReplicatedStorage:WaitForChild("Round")
local status = round:WaitForChild("Status")
local clock = round:WaitForChild("Clock")
local notify = round:WaitForChild("Notify")
local screen = script.Parent

local WHITE = Color3.fromRGB(255, 255, 255)
local INK = Color3.fromRGB(15, 17, 22)
local ROLE_COLOURS = {
	It = Color3.fromRGB(225, 60, 55),
	Runner = Color3.fromRGB(60, 170, 90),
	Seeker = Color3.fromRGB(225, 60, 55),
	Hider = Color3.fromRGB(60, 130, 230),
	Found = Color3.fromRGB(240, 140, 40),
	Fighter = Color3.fromRGB(240, 150, 40),
	Out = Color3.fromRGB(110, 110, 120),
	Winner = Color3.fromRGB(245, 195, 40),
	Riding = Color3.fromRGB(70, 160, 245),
	Gliding = Color3.fromRGB(70, 160, 245),
}

local function make(class, props, parent)
	local object = Instance.new(class)
	for key, value in props do
		object[key] = value
	end
	object.Parent = parent
	return object
end

local function outline(object, thickness)
	make("UIStroke", { Color = INK, Thickness = thickness }, object)
end

local top = make("Frame", {
	Name = "Top",
	AnchorPoint = Vector2.new(0.5, 0),
	Position = UDim2.new(0.5, 0, 0, 12),
	Size = UDim2.new(0, 420, 0, 64),
	BackgroundTransparency = 1,
}, screen)
local clockLabel = make("TextLabel", {
	Name = "Clock",
	AnchorPoint = Vector2.new(0.5, 0),
	Position = UDim2.new(0.5, 0, 0, 0),
	Size = UDim2.new(0, 120, 0, 36),
	BackgroundTransparency = 1,
	TextColor3 = WHITE,
	TextSize = 32,
	Text = "",
}, top)
outline(clockLabel, 3)
local statusLabel = make("TextLabel", {
	Name = "Status",
	Position = UDim2.new(0, 0, 0, 38),
	Size = UDim2.new(1, 0, 0, 24),
	BackgroundTransparency = 1,
	TextColor3 = WHITE,
	TextSize = 20,
	Text = "",
}, top)
outline(statusLabel, 2)

local badge = make("TextLabel", {
	Name = "Role",
	AnchorPoint = Vector2.new(0, 1),
	Position = UDim2.new(0, 16, 1, -16),
	Size = UDim2.new(0, 120, 0, 30),
	BackgroundColor3 = INK,
	TextColor3 = WHITE,
	TextSize = 18,
	Text = "",
	Visible = false,
}, screen)
make("UICorner", { CornerRadius = UDim.new(0, 6) }, badge)

local message = make("TextLabel", {
	Name = "Message",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.new(0.5, 0, 0.28, 0),
	Size = UDim2.new(0, 700, 0, 48),
	BackgroundTransparency = 1,
	TextColor3 = WHITE,
	TextSize = 34,
	Text = "",
	Visible = false,
}, screen)
outline(message, 3)

local blindfold = make("Frame", {
	Name = "Blindfold",
	Size = UDim2.new(1, 0, 1, 0),
	BackgroundColor3 = Color3.fromRGB(8, 8, 12),
	Visible = false,
	ZIndex = 5,
}, screen)
make("TextLabel", {
	Name = "Text",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.new(0.5, 0, 0.5, 0),
	Size = UDim2.new(0, 600, 0, 80),
	BackgroundTransparency = 1,
	TextColor3 = WHITE,
	TextSize = 40,
	Text = "Eyes closed",
	ZIndex = 6,
}, blindfold)

local function showClock()
	local left = clock.Value
	clockLabel.Visible = left >= 0
	clockLabel.Text = string.format("%d:%02d", left // 60, left % 60)
end

local function showRole()
	local role = me:GetAttribute("Role") or ""
	badge.Visible = role ~= ""
	badge.Text = role
	badge.BackgroundColor3 = ROLE_COLOURS[role] or INK
	blindfold.Visible = me:GetAttribute("Blind") == true
end

status:GetPropertyChangedSignal("Value"):Connect(function()
	statusLabel.Text = status.Value
end)
clock:GetPropertyChangedSignal("Value"):Connect(showClock)
me:GetAttributeChangedSignal("Role"):Connect(showRole)
me:GetAttributeChangedSignal("Blind"):Connect(showRole)
statusLabel.Text = status.Value
showClock()
showRole()

local shown = 0
notify.OnClientEvent:Connect(function(text)
	shown += 1
	local mine = shown
	message.Text = text
	message.Visible = true
	task.wait(2.5)
	if shown == mine then
		message.Visible = false
	end
end)
