-- The kart race screen: the start menu (pick a track, the computer
-- racers' level and how many laps, then press START), the racing HUD
-- (place, lap, time, item, drift sparks, the map and everyone's places),
-- the results, and the camera that chases your kart.
--
-- The server drives the karts. This script sends it your drift and item
-- buttons, and shows what the server says.

local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local UserInputService = game:GetService("UserInputService")
local storage = game:GetService("ReplicatedStorage")

local folder = storage:WaitForChild("Kart")
local Tracks = require(folder:WaitForChild("Tracks"))
local Control = folder:WaitForChild("Control")
local Race = folder:WaitForChild("Race")
local me = Players.LocalPlayer
local screen = script.Parent
local camera = workspace.CurrentCamera

for _, kind in { Enum.CoreGuiType.PlayerList, Enum.CoreGuiType.Backpack, Enum.CoreGuiType.Health } do
	pcall(function()
		game:GetService("StarterGui"):SetCoreGuiEnabled(kind, false)
	end)
end

------------------------------------------------------------------------
-- Looks: a plain, bold racing-game style.

local WHITE = Color3.fromRGB(255, 255, 255)
local INK = Color3.fromRGB(18, 20, 28)
local PANEL = Color3.fromRGB(24, 27, 38)
local SOFT = Color3.fromRGB(170, 176, 196)
local YELLOW = Color3.fromRGB(255, 205, 40)
local GREEN = Color3.fromRGB(60, 200, 90)
local RED = Color3.fromRGB(230, 70, 60)
local BLUE = Color3.fromRGB(70, 150, 240)
local PLACE_COLOURS = { Color3.fromRGB(255, 205, 40), Color3.fromRGB(210, 215, 225), Color3.fromRGB(220, 140, 70) }
local ITEM_LOOKS = {
	Turbo = { "TURBO", Color3.fromRGB(255, 140, 40) },
	TripleTurbo = { "TURBO x3", Color3.fromRGB(255, 110, 30) },
	Bumper = { "BUMPER", Color3.fromRGB(60, 210, 120) },
	Seeker = { "SEEKER", Color3.fromRGB(240, 60, 80) },
	Slime = { "SLIME", Color3.fromRGB(130, 220, 60) },
	Overdrive = { "OVERDRIVE", Color3.fromRGB(255, 220, 60) },
	Storm = { "STORM", Color3.fromRGB(120, 140, 255) },
}
local ROLL_ORDER = { "Turbo", "Bumper", "Seeker", "Slime", "Overdrive", "Storm" }
local CHARGE_COLOURS = { Color3.fromRGB(80, 180, 255), Color3.fromRGB(255, 160, 40), Color3.fromRGB(240, 90, 255) }

local function make(class, props, parent)
	local object = Instance.new(class)
	for key, value in props do
		object[key] = value
	end
	object.Parent = parent
	return object
end

local function round(object, radius)
	make("UICorner", { CornerRadius = UDim.new(0, radius or 10) }, object)
	return object
end

local function stroke(object, colour, thickness)
	make("UIStroke", { Color = colour or INK, Thickness = thickness or 2 }, object)
	return object
end

local function panel(parent, props)
	local p = make("Frame", { BackgroundColor3 = PANEL, BackgroundTransparency = 0.15, BorderSizePixel = 0 }, parent)
	for key, value in props do
		p[key] = value
	end
	return round(p, 14)
end

local function text(parent, props)
	local base = {
		BackgroundTransparency = 1,
		TextColor3 = WHITE,
		TextSize = 18,
		Text = "",
	}
	for key, value in props do
		base[key] = value
	end
	return make("TextLabel", base, parent)
end

local function button(parent, label, colour, props)
	local b = make("TextButton", {
		Text = label,
		TextColor3 = WHITE,
		TextSize = 20,
		BackgroundColor3 = colour or BLUE,
		BorderSizePixel = 0,
	}, parent)
	for key, value in props or {} do
		b[key] = value
	end
	round(b, 12)
	return b
end

local function clockText(seconds)
	if seconds < 0 then
		return "--:--.--"
	end
	local minutes = math.floor(seconds / 60)
	local rest = seconds - minutes * 60
	return string.format("%d:%05.2f", minutes, rest)
end

local function ordinal(n)
	if n % 100 >= 11 and n % 100 <= 13 then
		return n .. "th"
	end
	local last = n % 10
	return n .. (if last == 1 then "st" elseif last == 2 then "nd" elseif last == 3 then "rd" else "th")
end

------------------------------------------------------------------------
-- Sounds.

local function sound(id, volume)
	return make("Sound", { SoundId = id, Volume = volume or 0.6 }, screen)
end
local sounds = {
	beep = sound("playlibre://click", 0.8),
	go = sound("playlibre://chime", 0.8),
	coin = sound("playlibre://coin", 0.5),
	boom = sound("playlibre://boom", 0.5),
	boost = sound("playlibre://jump", 0.6),
	bonk = sound("playlibre://click", 0.4),
}
local engine = make("Sound", { SoundId = "playlibre://hum", Volume = 0.25, Looped = true }, screen)

------------------------------------------------------------------------
-- A map of the track, drawn with dots.

local function drawMap(parent, index, size, dotSize, colour)
	for _, child in parent:GetChildren() do
		if child.Name == "Dot" then
			child:Destroy()
		end
	end
	local samples = Tracks.samples(index)
	local minX, maxX, minZ, maxZ = math.huge, -math.huge, math.huge, -math.huge
	for _, s in ipairs(samples) do
		minX, maxX = math.min(minX, s.p.X), math.max(maxX, s.p.X)
		minZ, maxZ = math.min(minZ, s.p.Z), math.max(maxZ, s.p.Z)
	end
	local span = math.max(maxX - minX, maxZ - minZ)
	local function toMap(p)
		local u = (p.X - (minX + maxX) / 2) / span
		local v = (p.Z - (minZ + maxZ) / 2) / span
		return UDim2.new(0.5 + u * 0.86, 0, 0.5 + v * 0.86, 0)
	end
	for i, s in ipairs(samples) do
		if i % 2 == 0 then
			local dot = make("Frame", {
				Name = "Dot",
				AnchorPoint = Vector2.new(0.5, 0.5),
				Position = toMap(s.p),
				Size = UDim2.new(0, dotSize, 0, dotSize),
				BackgroundColor3 = if s.p.Y > 2 then YELLOW else colour,
				BorderSizePixel = 0,
			}, parent)
			round(dot, dotSize)
		end
	end
	local start = make("Frame", {
		Name = "Dot",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = toMap(samples[1].p),
		Size = UDim2.new(0, dotSize * 2.2, 0, dotSize * 2.2),
		BackgroundColor3 = WHITE,
		BorderSizePixel = 0,
	}, parent)
	round(start, 3)
	return toMap
end

------------------------------------------------------------------------
-- The start menu.

local menu = panel(screen, {
	Name = "Menu",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.new(0.5, 0, 0.5, 0),
	Size = UDim2.new(0, 760, 0, 470),
})
text(menu, { Name = "Title", Text = "KART", TextSize = 44, TextColor3 = YELLOW, Size = UDim2.new(1, 0, 0, 56), Position = UDim2.new(0, 0, 0, 14) })

local cards = {}
for i, track in Tracks.list do
	local card = make("TextButton", {
		Name = "Track" .. i,
		Text = "",
		BackgroundColor3 = Color3.fromRGB(40, 44, 60),
		BorderSizePixel = 0,
		Position = UDim2.new(0, 24 + (i - 1) * 242, 0, 84),
		Size = UDim2.new(0, 228, 0, 212),
	}, menu)
	round(card, 12)
	local outline = make("UIStroke", { Color = YELLOW, Thickness = 4, Enabled = false }, card)
	local map = make("Frame", {
		Name = "Map",
		BackgroundColor3 = track.ground[1],
		BorderSizePixel = 0,
		Position = UDim2.new(0, 14, 0, 12),
		Size = UDim2.new(1, -28, 0, 130),
	}, card)
	round(map, 8)
	drawMap(map, i, 130, 5, Color3.fromRGB(50, 50, 60))
	text(card, { Text = track.name, TextSize = 22, Position = UDim2.new(0, 0, 0, 150), Size = UDim2.new(1, 0, 0, 28) })
	local votes = text(card, { Name = "Votes", Text = "", TextSize = 14, TextColor3 = YELLOW, Position = UDim2.new(0, 0, 1, -24), Size = UDim2.new(1, 0, 0, 20) })
	card.MouseButton1Click:Connect(function()
		sounds.beep:Play()
		Control:FireServer("track", i)
	end)
	cards[i] = { card = card, outline = outline, votes = votes }
end

local function choiceRow(y, title, options, attribute, send)
	text(menu, { Text = title, TextSize = 18, TextColor3 = SOFT, TextXAlignment = Enum.TextXAlignment.Left, Position = UDim2.new(0, 24, 0, y), Size = UDim2.new(0, 160, 0, 36) })
	local buttons = {}
	for k, option in options do
		local b = button(menu, tostring(option), Color3.fromRGB(50, 56, 76), {
			Name = title .. tostring(option),
			TextSize = 18,
			Position = UDim2.new(0, 160 + (k - 1) * 112, 0, y),
			Size = UDim2.new(0, 104, 0, 36),
		})
		b.MouseButton1Click:Connect(function()
			sounds.beep:Play()
			Control:FireServer(send, option)
		end)
		buttons[option] = b
	end
	local function show()
		local current = folder:GetAttribute(attribute)
		for option, b in buttons do
			b.BackgroundColor3 = if option == current then BLUE else Color3.fromRGB(50, 56, 76)
		end
	end
	folder:GetAttributeChangedSignal(attribute):Connect(show)
	show()
end
choiceRow(320, "CPU", { "Easy", "Normal", "Hard" }, "Level", "level")
choiceRow(364, "Laps", { 1, 3, 5 }, "Laps", "laps")

local startButton = button(menu, "START", GREEN, {
	Name = "Start",
	TextSize = 26,
	AnchorPoint = Vector2.new(1, 0),
	Position = UDim2.new(1, -24, 0, 320),
	Size = UDim2.new(0, 210, 0, 80),
})
startButton.MouseButton1Click:Connect(function()
	sounds.go:Play()
	Control:FireServer("start")
end)
local helpLine = text(menu, {
	Text = "W/S  Drive     A/D  Steer     Space  Drift     E  Item",
	TextSize = 15,
	TextColor3 = SOFT,
	Position = UDim2.new(0, 0, 1, -40),
	Size = UDim2.new(1, 0, 0, 24),
})
if UserInputService.TouchEnabled then
	helpLine.Text = ""
end
local who = text(menu, { Text = "", TextSize = 14, TextColor3 = SOFT, Position = UDim2.new(0, 0, 1, -20), Size = UDim2.new(1, 0, 0, 18) })

local function showMenu()
	local current = folder:GetAttribute("Track")
	for i, c in cards do
		c.outline.Enabled = i == current
		c.card.BackgroundColor3 = if i == current then Color3.fromRGB(92, 80, 28) else Color3.fromRGB(40, 44, 60)
		local n = folder:GetAttribute("Votes" .. i) or 0
		c.votes.Text = if n == 1 then "1 vote" elseif n > 1 then n .. " votes" else ""
	end
	local startsIn = folder:GetAttribute("StartsIn") or -1
	startButton.Text = if startsIn > 0 then "STARTING " .. math.ceil(startsIn) else "START"
	local names = {}
	for _, p in Players:GetPlayers() do
		table.insert(names, p.Name)
	end
	who.Text = if #names > 1 then table.concat(names, "  ") else ""
end

------------------------------------------------------------------------
-- The racing HUD.

local hud = make("Frame", { Name = "Hud", BackgroundTransparency = 1, Size = UDim2.new(1, 0, 1, 0), Visible = false }, screen)

local itemBox = panel(hud, { Name = "ItemBox", Position = UDim2.new(0, 20, 0, 20), Size = UDim2.new(0, 130, 0, 110) })
stroke(itemBox, WHITE, 3)
text(itemBox, { Text = "ITEM", TextSize = 13, TextColor3 = SOFT, Position = UDim2.new(0, 0, 0, 6), Size = UDim2.new(1, 0, 0, 16) })
local itemFill = make("Frame", { Name = "Fill", BackgroundColor3 = PANEL, BorderSizePixel = 0, Position = UDim2.new(0, 10, 0, 26), Size = UDim2.new(1, -20, 1, -36) }, itemBox)
round(itemFill, 8)
local itemName = text(itemFill, { Name = "Name", Text = "", TextSize = 18, Size = UDim2.new(1, 0, 1, 0) })
stroke(itemName, INK, 2)
local itemKey = text(itemBox, { Text = if UserInputService.TouchEnabled then "" else "E", TextSize = 14, TextColor3 = YELLOW, AnchorPoint = Vector2.new(1, 1), Position = UDim2.new(1, -6, 1, -2), Size = UDim2.new(0, 20, 0, 18) })

local lapLabel = text(hud, { Name = "Lap", Text = "LAP 1/3", TextSize = 30, AnchorPoint = Vector2.new(0.5, 0), Position = UDim2.new(0.5, 0, 0, 14), Size = UDim2.new(0, 260, 0, 36) })
stroke(lapLabel, INK, 3)
local timeLabel = text(hud, { Name = "Time", Text = "0:00.00", TextSize = 22, AnchorPoint = Vector2.new(0.5, 0), Position = UDim2.new(0.5, 0, 0, 50), Size = UDim2.new(0, 260, 0, 28) })
stroke(timeLabel, INK, 2)

local placeLabel = text(hud, { Name = "Place", Text = "1st", TextSize = 84, AnchorPoint = Vector2.new(1, 1), Position = UDim2.new(1, -28, 1, -24), Size = UDim2.new(0, 220, 0, 96), TextXAlignment = Enum.TextXAlignment.Right })
stroke(placeLabel, INK, 5)
local speedLabel = text(hud, { Name = "Speed", Text = "0", TextSize = 26, AnchorPoint = Vector2.new(1, 1), Position = UDim2.new(1, -30, 1, -118), Size = UDim2.new(0, 220, 0, 30), TextXAlignment = Enum.TextXAlignment.Right })
stroke(speedLabel, INK, 2)

local chargeBar = panel(hud, { Name = "Charge", AnchorPoint = Vector2.new(0.5, 1), Position = UDim2.new(0.5, 0, 1, -26), Size = UDim2.new(0, 240, 0, 16), BackgroundTransparency = 0.4, Visible = false })
local chargeFill = make("Frame", { BackgroundColor3 = CHARGE_COLOURS[1], BorderSizePixel = 0, Size = UDim2.new(0, 0, 1, 0) }, chargeBar)
round(chargeFill, 8)

local mapPanel = panel(hud, { Name = "Map", AnchorPoint = Vector2.new(1, 0), Position = UDim2.new(1, -20, 0, 20), Size = UDim2.new(0, 190, 0, 190), BackgroundTransparency = 0.35 })
local toMap = nil
local mapDots = {}

local board = make("Frame", { Name = "Standings", BackgroundTransparency = 1, Position = UDim2.new(0, 20, 0, 150), Size = UDim2.new(0, 200, 0, 8 * 30) }, hud)
local rows = {}
for i = 1, 8 do
	local row = panel(board, { Name = "Row" .. i, Position = UDim2.new(0, 0, 0, (i - 1) * 30), Size = UDim2.new(1, 0, 0, 26), BackgroundTransparency = 0.3 })
	local swatch = make("Frame", { BorderSizePixel = 0, Position = UDim2.new(0, 34, 0, 6), Size = UDim2.new(0, 14, 0, 14) }, row)
	round(swatch, 4)
	rows[i] = {
		frame = row,
		place = text(row, { Text = tostring(i), TextSize = 16, TextColor3 = YELLOW, Size = UDim2.new(0, 30, 1, 0) }),
		swatch = swatch,
		name = text(row, { Text = "", TextSize = 16, TextXAlignment = Enum.TextXAlignment.Left, Position = UDim2.new(0, 56, 0, 0), Size = UDim2.new(1, -60, 1, 0) }),
	}
end

local big = text(screen, { Name = "Big", Text = "", TextSize = 96, TextColor3 = YELLOW, AnchorPoint = Vector2.new(0.5, 0.5), Position = UDim2.new(0.5, 0, 0.36, 0), Size = UDim2.new(0, 800, 0, 110), Visible = false })
stroke(big, INK, 6)
local small = text(screen, { Name = "Small", Text = "", TextSize = 30, AnchorPoint = Vector2.new(0.5, 0.5), Position = UDim2.new(0.5, 0, 0.47, 0), Size = UDim2.new(0, 800, 0, 40), Visible = false })
stroke(small, INK, 3)
local flash = make("Frame", { Name = "Flash", BackgroundColor3 = WHITE, BackgroundTransparency = 1, BorderSizePixel = 0, Size = UDim2.new(1, 0, 1, 0), ZIndex = 0 }, screen)

local bigShown, smallShown = 0, 0
local function shout(message, colour, seconds)
	bigShown += 1
	local mine = bigShown
	big.Text = message
	big.TextColor3 = colour or YELLOW
	big.Visible = true
	task.delay(seconds or 1.2, function()
		if bigShown == mine then
			big.Visible = false
		end
	end)
end
local function say(message, seconds)
	smallShown += 1
	local mine = smallShown
	small.Text = message
	small.Visible = true
	task.delay(seconds or 2, function()
		if smallShown == mine then
			small.Visible = false
		end
	end)
end

-- Big touch buttons for drift and items.
local touchButtons = make("Frame", { Name = "Touch", BackgroundTransparency = 1, Size = UDim2.new(1, 0, 1, 0), Visible = UserInputService.TouchEnabled }, hud)
local driftButton = button(touchButtons, "DRIFT", BLUE, { AnchorPoint = Vector2.new(1, 1), Position = UDim2.new(1, -30, 1, -140), Size = UDim2.new(0, 110, 0, 110), TextSize = 22 })
round(driftButton, 55)
local itemButton = button(touchButtons, "ITEM", Color3.fromRGB(240, 140, 40), { AnchorPoint = Vector2.new(1, 1), Position = UDim2.new(1, -160, 1, -40), Size = UDim2.new(0, 96, 0, 96), TextSize = 20 })
round(itemButton, 48)

------------------------------------------------------------------------
-- Results.

local resultsPanel = panel(screen, { Name = "Results", AnchorPoint = Vector2.new(0.5, 0.5), Position = UDim2.new(0.5, 0, 0.5, 0), Size = UDim2.new(0, 560, 0, 470), Visible = false })
text(resultsPanel, { Text = "RESULTS", TextSize = 36, TextColor3 = YELLOW, Position = UDim2.new(0, 0, 0, 10), Size = UDim2.new(1, 0, 0, 44) })
local resultRows = {}
for i = 1, 8 do
	local row = make("Frame", { Name = "Row" .. i, BackgroundColor3 = Color3.fromRGB(40, 44, 60), BorderSizePixel = 0, Position = UDim2.new(0, 20, 0, 60 + (i - 1) * 38), Size = UDim2.new(1, -40, 0, 34) }, resultsPanel)
	round(row, 8)
	resultRows[i] = {
		frame = row,
		place = text(row, { Text = ordinal(i), TextSize = 18, TextColor3 = YELLOW, Size = UDim2.new(0, 56, 1, 0) }),
		name = text(row, { Text = "", TextSize = 18, TextXAlignment = Enum.TextXAlignment.Left, Position = UDim2.new(0, 64, 0, 0), Size = UDim2.new(0, 220, 1, 0) }),
		time = text(row, { Text = "", TextSize = 17, TextColor3 = SOFT, Position = UDim2.new(0, 280, 0, 0), Size = UDim2.new(0, 120, 1, 0) }),
		points = text(row, { Text = "", TextSize = 18, TextColor3 = GREEN, Position = UDim2.new(1, -90, 0, 0), Size = UDim2.new(0, 80, 1, 0) }),
	}
end
local againButton = button(resultsPanel, "AGAIN", GREEN, { Name = "Again", TextSize = 22, Position = UDim2.new(0.5, 10, 1, -66), Size = UDim2.new(0.5, -30, 0, 50) })
local menuButton = button(resultsPanel, "TRACKS", BLUE, { Name = "PickTrack", TextSize = 22, Position = UDim2.new(0, 20, 1, -66), Size = UDim2.new(0.5, -30, 0, 50) })
againButton.MouseButton1Click:Connect(function()
	sounds.go:Play()
	Control:FireServer("start")
end)
menuButton.MouseButton1Click:Connect(function()
	sounds.beep:Play()
	Control:FireServer("menu")
end)

local function showResults(list)
	for i, row in resultRows do
		local entry = list[i]
		row.frame.Visible = entry ~= nil
		if entry then
			row.name.Text = entry.name
			row.name.TextColor3 = if entry.userId == me.UserId then YELLOW else WHITE
			row.time.Text = if entry.time >= 0 then clockText(entry.time) else "DNF"
			row.points.Text = "+" .. entry.points
			row.frame.BackgroundColor3 = if entry.userId == me.UserId then Color3.fromRGB(70, 64, 40) else Color3.fromRGB(40, 44, 60)
		end
	end
end

------------------------------------------------------------------------
-- Controls: drift and items go to the server; the seat already takes
-- the throttle and steering.

local DRIFT_KEYS = { Enum.KeyCode.Space, Enum.KeyCode.LeftShift, Enum.KeyCode.RightShift }
local ITEM_KEYS = { Enum.KeyCode.E, Enum.KeyCode.F, Enum.KeyCode.LeftControl }
local function isOneOf(key, keys)
	return table.find(keys, key) ~= nil
end
local drifting = false
local function setDrift(on)
	if drifting ~= on then
		drifting = on
		Control:FireServer("drift", on)
	end
end

UserInputService.InputBegan:Connect(function(input, used)
	if used then
		return
	end
	if isOneOf(input.KeyCode, DRIFT_KEYS) then
		setDrift(true)
	elseif isOneOf(input.KeyCode, ITEM_KEYS) then
		Control:FireServer("item")
	elseif input.UserInputType == Enum.UserInputType.MouseButton1 and UserInputService:GetLastInputType() == Enum.UserInputType.Gamepad1 then
		-- A controller's right trigger.
		setDrift(true)
	end
end)
UserInputService.InputEnded:Connect(function(input)
	if isOneOf(input.KeyCode, DRIFT_KEYS) or input.UserInputType == Enum.UserInputType.MouseButton1 then
		local still = false
		for _, key in DRIFT_KEYS do
			if UserInputService:IsKeyDown(key) then
				still = true
			end
		end
		if not still then
			setDrift(false)
		end
	end
end)
driftButton.MouseButton1Click:Connect(function()
	setDrift(not drifting)
	driftButton.BackgroundColor3 = if drifting then YELLOW else BLUE
end)
itemButton.MouseButton1Click:Connect(function()
	Control:FireServer("item")
end)

------------------------------------------------------------------------
-- What the server tells us.

local rolling = 0
Race.OnClientEvent:Connect(function(what, a, b)
	if what == "countdown" then
		resultsPanel.Visible = false
	elseif what == "go" then
		shout("GO!", GREEN, 1)
		sounds.go:Play()
	elseif what == "rocket" then
		say("Rocket start!")
		sounds.boost:Play()
	elseif what == "lap" then
		if a == b then
			shout("FINAL LAP", RED, 1.6)
		else
			say("LAP " .. a .. "/" .. b)
		end
		sounds.go:Play()
	elseif what == "finish" then
		shout(ordinal(a) .. "!", PLACE_COLOURS[a] or WHITE, 3)
		say(clockText(b), 3)
		sounds.go:Play()
	elseif what == "roll" then
		sounds.coin:Play()
	elseif what == "item" then
		sounds.coin:Play()
	elseif what == "boost" then
		sounds.boost:Play()
	elseif what == "miniturbo" then
		sounds.boost:Play()
		if a == 3 then
			say("Ultra turbo!", 1)
		elseif a == 2 then
			say("Super turbo!", 1)
		end
	elseif what == "trick" then
		say("Trick!", 0.8)
	elseif what == "hit" then
		sounds.boom:Play()
		say(if b then b .. "'s " .. a else "Spun out", 1.5)
		flash.BackgroundTransparency = 0.6
	elseif what == "landed" then
		say(b, 1.2)
	elseif what == "bonk" then
		sounds.bonk:Play()
	elseif what == "storm" then
		flash.BackgroundTransparency = 0.3
		if a ~= me.Name then
			say(a .. " used Storm", 1.5)
		end
	elseif what == "results" then
		showResults(a)
	end
end)

------------------------------------------------------------------------
-- Every frame: the screens, the HUD and the camera.

local function myKart()
	local karts = workspace:FindFirstChild("Race") and workspace.Race:FindFirstChild("Karts")
	if not karts then
		return nil, nil
	end
	local leader = nil
	for _, kart in karts:GetChildren() do
		if kart:GetAttribute("UserId") == me.UserId then
			return kart, kart
		end
		if kart:GetAttribute("Place") == 1 then
			leader = kart
		end
	end
	return nil, leader
end

local shownTrack = nil
local camYaw = nil
local camPos = nil
local lastCountdown = nil
local orbit = 0

local function updateMap(karts)
	local trackIndex = folder:GetAttribute("Track")
	if shownTrack ~= trackIndex then
		shownTrack = trackIndex
		toMap = drawMap(mapPanel, trackIndex, 190, 4, Color3.fromRGB(200, 204, 220))
		for _, dot in mapDots do
			dot:Destroy()
		end
		mapDots = {}
	end
	if not karts then
		return
	end
	local seen = {}
	for _, kart in karts:GetChildren() do
		seen[kart] = true
		local dot = mapDots[kart]
		if not dot then
			dot = make("Frame", { Name = "Racer", AnchorPoint = Vector2.new(0.5, 0.5), BorderSizePixel = 0, ZIndex = 3 }, mapPanel)
			round(dot, 10)
			mapDots[kart] = dot
		end
		local mine = kart:GetAttribute("UserId") == me.UserId
		local size = if mine then 14 else 9
		dot.Size = UDim2.new(0, size, 0, size)
		dot.BackgroundColor3 = kart:GetAttribute("Colour") or WHITE
		dot.ZIndex = if mine then 4 else 3
		if kart.PrimaryPart then
			dot.Position = toMap(kart.PrimaryPart.Position)
		end
	end
	for kart, dot in mapDots do
		if not seen[kart] then
			dot:Destroy()
			mapDots[kart] = nil
		end
	end
end

local function updateHud(kart, karts)
	local laps = folder:GetAttribute("Laps")
	if kart then
		local lap = kart:GetAttribute("Lap") or 1
		lapLabel.Text = "LAP " .. lap .. "/" .. laps
		local place = kart:GetAttribute("Place") or 8
		placeLabel.Text = ordinal(place)
		placeLabel.TextColor3 = PLACE_COLOURS[place] or WHITE
		speedLabel.Text = tostring(kart:GetAttribute("Speed") or 0) .. " km/h"
		local item = kart:GetAttribute("Item") or ""
		if kart:GetAttribute("Rolling") then
			rolling += 1
			local name = ROLL_ORDER[math.floor(rolling / 4) % #ROLL_ORDER + 1]
			itemName.Text = ITEM_LOOKS[name][1]
			itemFill.BackgroundColor3 = ITEM_LOOKS[name][2]
		elseif item ~= "" then
			local look = ITEM_LOOKS[item]
			local uses = kart:GetAttribute("Uses") or 1
			itemName.Text = if item == "TripleTurbo" then "TURBO x" .. uses else look[1]
			itemFill.BackgroundColor3 = look[2]
		else
			itemName.Text = ""
			itemFill.BackgroundColor3 = PANEL
		end
		itemKey.Visible = item ~= "" and not UserInputService.TouchEnabled
		local charge = kart:GetAttribute("Charge") or 0
		chargeBar.Visible = charge > 0
		if charge > 0 then
			chargeFill.Size = UDim2.new(charge / 3, 0, 1, 0)
			chargeFill.BackgroundColor3 = CHARGE_COLOURS[charge]
		end
		if kart:GetAttribute("WrongWay") then
			small.Text = "WRONG WAY"
			small.TextColor3 = RED
			small.Visible = true
		elseif small.Text == "WRONG WAY" then
			small.Visible = false
			small.TextColor3 = WHITE
		end
	else
		lapLabel.Text = "WATCHING"
		placeLabel.Text = ""
		speedLabel.Text = "Next race"
		itemName.Text = ""
		chargeBar.Visible = false
	end
	local finishIn = folder:GetAttribute("FinishIn") or -1
	if finishIn >= 0 then
		timeLabel.Text = clockText(folder:GetAttribute("Clock") or 0) .. "   (ends in " .. finishIn .. ")"
	else
		timeLabel.Text = clockText(folder:GetAttribute("Clock") or 0)
	end
	if kart and kart:GetAttribute("Finished") then
		timeLabel.Text = "Finished " .. clockText(kart:GetAttribute("Time") or 0)
	end
	-- Everyone's places.
	local list = {}
	if karts then
		for _, k in karts:GetChildren() do
			table.insert(list, k)
		end
	end
	table.sort(list, function(a, b)
		return (a:GetAttribute("Place") or 9) < (b:GetAttribute("Place") or 9)
	end)
	for i, row in rows do
		local k = list[i]
		row.frame.Visible = k ~= nil
		if k then
			row.name.Text = k:GetAttribute("Racer") or k.Name
			row.swatch.BackgroundColor3 = k:GetAttribute("Colour") or WHITE
			local mine = k:GetAttribute("UserId") == me.UserId
			row.frame.BackgroundColor3 = if mine then Color3.fromRGB(90, 78, 30) else PANEL
			row.name.TextColor3 = if mine then YELLOW else WHITE
		end
	end
end

local function chase(kart, dt)
	local body = kart.PrimaryPart
	if not body then
		return
	end
	local at = body.Position
	local heading = kart:GetAttribute("Heading")
	if not heading then
		local look = body.CFrame.LookVector
		heading = math.atan2(-look.X, -look.Z)
	end
	if not camYaw then
		camYaw = heading
	end
	local diff = (heading - camYaw + math.pi) % (math.pi * 2) - math.pi
	camYaw += diff * math.min(1, dt * 5)
	local back = Vector3.new(math.sin(camYaw), 0, math.cos(camYaw))
	local goal = at + back * 15 + Vector3.new(0, 6.5, 0)
	camPos = if camPos then camPos:Lerp(goal, math.min(1, dt * 10)) else goal
	camera.CFrame = CFrame.lookAt(camPos, at - back * 6 + Vector3.new(0, 2.2, 0))
	local fov = if kart:GetAttribute("Boost") then 82 else 70
	camera.FieldOfView += (fov - camera.FieldOfView) * math.min(1, dt * 4)
end

local function overview(dt)
	orbit += dt * 0.08
	local trackIndex = folder:GetAttribute("Track")
	local samples = Tracks.samples(trackIndex)
	local centre = Vector3.zero
	for _, s in ipairs(samples) do
		centre += s.p
	end
	centre /= #samples
	local eye = centre + Vector3.new(math.sin(orbit) * 330, 190, math.cos(orbit) * 330)
	camera.CFrame = CFrame.lookAt(eye, centre)
	camera.FieldOfView = 70
	camPos = nil
	camYaw = nil
end

RunService.RenderStepped:Connect(function(dt)
	camera.CameraType = Enum.CameraType.Scriptable
	local phase = folder:GetAttribute("Phase")
	local karts = workspace:FindFirstChild("Race") and workspace.Race:FindFirstChild("Karts")
	local kart, follow = myKart()
	menu.Visible = phase == "menu"
	hud.Visible = phase == "race" or phase == "countdown"
	resultsPanel.Visible = phase == "results"
	if flash.BackgroundTransparency < 1 then
		flash.BackgroundTransparency = math.min(1, flash.BackgroundTransparency + dt * 1.5)
	end
	if phase == "menu" then
		showMenu()
		overview(dt)
		engine.Playing = false
		return
	end
	if phase == "countdown" then
		local n = folder:GetAttribute("Countdown")
		if n ~= lastCountdown and n and n > 0 and n <= 3 then
			lastCountdown = n
			shout(tostring(n), YELLOW, 0.9)
			sounds.beep:Play()
		end
	else
		lastCountdown = nil
	end
	updateMap(karts)
	updateHud(kart, karts)
	if follow and phase ~= "results" then
		chase(follow, dt)
	else
		overview(dt)
	end
	if kart and phase ~= "results" then
		engine.Playing = true
		engine.PlaybackSpeed = 0.7 + (kart:GetAttribute("Speed") or 0) / 140
	else
		engine.Playing = false
	end
end)
