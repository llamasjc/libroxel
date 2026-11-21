-- The round HUD: what's happening and the time left at the top, your
-- role in a coloured badge, messages in the middle, and a blindfold when
-- the game says you mustn't look.

local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local me = Players.LocalPlayer
local round = ReplicatedStorage:WaitForChild("Round")
local status = round:WaitForChild("Status")
local clock = round:WaitForChild("Clock")
local notify = round:WaitForChild("Notify")
local screen = script.Parent

local WHITE = Color3.fromRGB(255, 255, 255)
local DARK = Color3.fromRGB(20, 24, 32)
local ROLE_COLOURS = {
	It = Color3.fromRGB(225, 60, 55),
	Runner = Color3.fromRGB(60, 170, 90),
	Seeker = Color3.fromRGB(225, 60, 55),
	Hider = Color3.fromRGB(60, 130, 230),
	Found = Color3.fromRGB(240, 140, 40),
	Fighter = Color3.fromRGB(240, 150, 40),
	Out = Color3.fromRGB(110, 110, 120),
	Winner = Color3.fromRGB(245, 195, 40),
}

local function make(class, props, parent)
	local object = Instance.new(class)
	for key, value in props do
		object[key] = value
	end
	object.Parent = parent
	return object
end

local function corner(object, radius)
	make("UICorner", { CornerRadius = UDim.new(0, radius) }, object)
end

local top = make("Frame", {
	Name = "Top",
	AnchorPoint = Vector2.new(0.5, 0),
	Position = UDim2.new(0.5, 0, 0, 10),
	Size = UDim2.new(0, 520, 0, 44),
	BackgroundColor3 = DARK,
	BackgroundTransparency = 0.25,
}, screen)
corner(top, 10)
local statusLabel = make("TextLabel", {
	Name = "Status",
	Position = UDim2.new(0, 14, 0, 0),
	Size = UDim2.new(1, -100, 1, 0),
	BackgroundTransparency = 1,
	TextColor3 = WHITE,
	TextSize = 19,
	TextXAlignment = Enum.TextXAlignment.Left,
	Text = "",
}, top)
local clockLabel = make("TextLabel", {
	Name = "Clock",
	AnchorPoint = Vector2.new(1, 0.5),
	Position = UDim2.new(1, -6, 0.5, 0),
	Size = UDim2.new(0, 76, 0, 34),
	BackgroundColor3 = WHITE,
	BackgroundTransparency = 0.1,
	TextColor3 = DARK,
	TextSize = 20,
	Text = "",
}, top)
corner(clockLabel, 8)

local badge = make("TextLabel", {
	Name = "Role",
	AnchorPoint = Vector2.new(0.5, 0),
	Position = UDim2.new(0.5, 0, 0, 60),
	Size = UDim2.new(0, 200, 0, 34),
	BackgroundColor3 = DARK,
	TextColor3 = WHITE,
	TextSize = 18,
	Text = "",
	Visible = false,
}, screen)
corner(badge, 17)

local message = make("TextLabel", {
	Name = "Message",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.new(0.5, 0, 0.32, 0),
	Size = UDim2.new(0, 560, 0, 52),
	BackgroundColor3 = DARK,
	BackgroundTransparency = 0.2,
	TextColor3 = WHITE,
	TextSize = 24,
	Text = "",
	Visible = false,
}, screen)
corner(message, 12)

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
	TextSize = 30,
	Text = "You're the seeker. No peeking!",
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
	badge.BackgroundColor3 = ROLE_COLOURS[role] or DARK
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
	task.wait(3)
	if shown == mine then
		message.Visible = false
	end
end)
