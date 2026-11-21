-- The chess screen: the board, the players' clocks, the computer's
-- thoughts, the move list and the lobby where you pick who to play.
-- Everything here is made of GUI objects; the server decides what is
-- allowed and this script only shows it and passes on your clicks.

local Players = game:GetService("Players")
local storage = game:GetService("ReplicatedStorage")
local TweenService = game:GetService("TweenService")

local folder = storage:WaitForChild("Chess")
local Rules = require(folder:WaitForChild("Rules"))
local Engine = require(folder:WaitForChild("Engine"))
local Action = folder:WaitForChild("Action")
local State = folder:WaitForChild("State")
local me = Players.LocalPlayer
local screen = script.Parent

-- The panel shows everyone's rating, so the usual player list isn't needed,
-- and nobody carries tools here.
for _, kind in { Enum.CoreGuiType.PlayerList, Enum.CoreGuiType.Backpack, Enum.CoreGuiType.Health } do
	pcall(function()
		game:GetService("StarterGui"):SetCoreGuiEnabled(kind, false)
	end)
end

------------------------------------------------------------------------
-- Looks.

local INK = Color3.fromRGB(20, 45, 75)
local SOFT_INK = Color3.fromRGB(70, 100, 130)
local WHITE = Color3.fromRGB(255, 255, 255)
local LIGHT_SQUARE = Color3.fromRGB(226, 240, 250)
local DARK_SQUARE = Color3.fromRGB(104, 158, 204)
local LAST_LIGHT = Color3.fromRGB(246, 238, 150)
local LAST_DARK = Color3.fromRGB(200, 196, 96)
local PICKED = Color3.fromRGB(130, 220, 150)
local CHECKED = Color3.fromRGB(255, 110, 100)
local HINTED = Color3.fromRGB(120, 200, 255)
local GREEN = Color3.fromRGB(70, 190, 90)
local BLUE = Color3.fromRGB(60, 150, 235)
local RED = Color3.fromRGB(225, 85, 80)
local GREY = Color3.fromRGB(150, 170, 190)

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

local function outline(object, colour, thickness, transparency)
	make("UIStroke", { Color = colour or WHITE, Thickness = thickness or 1, Transparency = transparency or 0.3 }, object)
	return object
end

-- A glassy shine from top to bottom.
local function shine(object, top, bottom)
	make("UIGradient", { Color = ColorSequence.new(top or WHITE, bottom or Color3.fromRGB(200, 215, 230)), Rotation = 90 }, object)
	return object
end

local function label(parent, props)
	local base = {
		BackgroundTransparency = 1,
		TextColor3 = INK,
		TextSize = 16,
		Text = "",
		TextXAlignment = Enum.TextXAlignment.Left,
	}
	for key, value in props do
		base[key] = value
	end
	return make("TextLabel", base, parent)
end

-- A glossy Aero button.
local function button(parent, text, colour, props)
	local b = make("TextButton", {
		Text = text,
		TextColor3 = WHITE,
		TextSize = 17,
		BackgroundColor3 = colour or BLUE,
		Size = UDim2.new(0, 120, 0, 38),
	}, parent)
	for key, value in props or {} do
		b[key] = value
	end
	round(b, 10)
	shine(b, WHITE, Color3.fromRGB(175, 195, 215))
	outline(b, WHITE, 1, 0.4)
	return b
end

-- A frosted glass panel.
local function glass(parent, props)
	local p = make("Frame", {
		BackgroundColor3 = WHITE,
		BackgroundTransparency = 0.12,
	}, parent)
	for key, value in props do
		p[key] = value
	end
	round(p, 16)
	outline(p, WHITE, 1.5, 0.2)
	make("UIGradient", {
		Color = ColorSequence.new(WHITE, Color3.fromRGB(215, 232, 245)),
		Rotation = 90,
	}, p)
	return p
end

local function clockText(ms)
	if ms < 10000 then
		return string.format("%d.%d", ms // 1000, (ms % 1000) // 100)
	end
	local seconds = math.ceil(ms / 1000)
	return string.format("%d:%02d", seconds // 60, seconds % 60)
end

local function controlText(control)
	return if control == "none" then "No clock" else control
end

------------------------------------------------------------------------
-- The sky behind everything, with a few bubbles.

local sky = make("Frame", {
	Name = "Sky",
	Size = UDim2.fromScale(1, 1),
	BackgroundColor3 = WHITE,
	BorderSizePixel = 0,
	ZIndex = 0,
}, screen)
make("UIGradient", {
	Color = ColorSequence.new(Color3.fromRGB(196, 238, 255), Color3.fromRGB(46, 128, 205)),
	Rotation = 90,
}, sky)
for _, bubble in {
	{ 0.66, 0.06, 120 }, { 0.92, 0.78, 160 }, { 0.6, 0.86, 70 }, { 0.04, 0.9, 90 }, { 0.97, 0.2, 60 },
} do
	local b = make("Frame", {
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(bubble[1], bubble[2]),
		Size = UDim2.fromOffset(bubble[3], bubble[3]),
		BackgroundColor3 = WHITE,
		BackgroundTransparency = 0.82,
		ZIndex = 0,
	}, sky)
	round(b, bubble[3] // 2)
	outline(b, WHITE, 2, 0.5)
end

------------------------------------------------------------------------
-- The board column: a player card, the board with its evaluation bar,
-- and another player card.

local column = make("Frame", {
	Name = "BoardColumn",
	AnchorPoint = Vector2.new(0, 0.5),
	Position = UDim2.new(0, 24, 0.5, 22),
	Size = UDim2.new(0.62, 0, 0.9, 0),
	BackgroundTransparency = 1,
}, screen)
make("UIAspectRatioConstraint", { AspectRatio = 0.86 }, column)

local function playerCard(name, y)
	local card = glass(column, {
		Name = name,
		Position = UDim2.new(0.055, 0, y, 0),
		Size = UDim2.new(0.945, 0, 0.082, 0),
	})
	local dot = make("Frame", {
		Name = "Dot",
		AnchorPoint = Vector2.new(0, 0.5),
		Position = UDim2.new(0, 12, 0.5, 0),
		Size = UDim2.fromOffset(34, 34),
		BackgroundColor3 = BLUE,
	}, card)
	round(dot, 17)
	shine(dot)
	outline(dot, WHITE, 2, 0.1)
	label(dot, { Name = "Letter", Size = UDim2.fromScale(1, 1), Text = "", TextColor3 = WHITE, TextSize = 18, TextXAlignment = Enum.TextXAlignment.Center })
	label(card, { Name = "PlayerName", Position = UDim2.new(0, 56, 0, 4), Size = UDim2.new(0.5, 0, 0.55, 0), TextSize = 19, Text = "" })
	label(card, { Name = "Detail", Position = UDim2.new(0, 56, 0.5, 0), Size = UDim2.new(0.6, 0, 0.45, 0), TextSize = 14, TextColor3 = SOFT_INK })
	local clock = make("TextLabel", {
		Name = "Clock",
		AnchorPoint = Vector2.new(1, 0.5),
		Position = UDim2.new(1, -10, 0.5, 0),
		Size = UDim2.new(0, 120, 0.76, 0),
		BackgroundColor3 = Color3.fromRGB(30, 50, 75),
		TextColor3 = WHITE,
		TextSize = 26,
		Text = "",
	}, card)
	round(clock, 8)
	return card
end

local topCard = playerCard("TopPlayer", 0)
local bottomCard = playerCard("BottomPlayer", 0.918)

local evalBar = make("Frame", {
	Name = "EvalBar",
	Position = UDim2.new(0, 0, 0.09, 0),
	Size = UDim2.new(0.034, 0, 0.82, 0),
	BackgroundColor3 = Color3.fromRGB(30, 40, 60),
	ClipsDescendants = true,
}, column)
round(evalBar, 6)
outline(evalBar, WHITE, 1.5, 0.2)
local evalFill = make("Frame", {
	Name = "White",
	AnchorPoint = Vector2.new(0, 1),
	Position = UDim2.fromScale(0, 1),
	Size = UDim2.fromScale(1, 0.5),
	BackgroundColor3 = Color3.fromRGB(240, 248, 255),
}, evalBar)
round(evalFill, 6)
local evalText = label(column, {
	Name = "EvalText",
	Position = UDim2.new(0, 0, 0.912, 0),
	Size = UDim2.new(0.05, 0, 0.03, 0),
	TextSize = 13,
	TextColor3 = WHITE,
	TextXAlignment = Enum.TextXAlignment.Center,
})

local boardFrame = make("Frame", {
	Name = "Board",
	Position = UDim2.new(0.055, 0, 0.09, 0),
	Size = UDim2.new(0.945, 0, 0.82, 0),
	BackgroundColor3 = Color3.fromRGB(40, 70, 110),
}, column)
round(boardFrame, 8)
outline(boardFrame, WHITE, 3, 0.15)

-- Squares by board square number (see Rules), each with its piece picture
-- and a dot for where the picked piece can go.
local squares = {}
for _, sq in Rules.SQUARES do
	local light = (Rules.fileOf(sq) + Rules.rankOf(sq)) % 2 == 1
	local square = make("TextButton", {
		Name = Rules.squareName(sq),
		Text = "",
		Size = UDim2.fromScale(1 / 8, 1 / 8),
		BackgroundColor3 = if light then LIGHT_SQUARE else DARK_SQUARE,
		AutoButtonColor = false,
	}, boardFrame)
	local piece = make("ImageLabel", {
		Name = "Piece",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(0.5, 0.5),
		Size = UDim2.fromScale(0.92, 0.92),
		BackgroundTransparency = 1,
		Image = "",
		ZIndex = 3,
	}, square)
	local dot = make("Frame", {
		Name = "Dot",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(0.5, 0.5),
		Size = UDim2.fromScale(0.3, 0.3),
		BackgroundColor3 = Color3.fromRGB(30, 80, 60),
		BackgroundTransparency = 0.55,
		Visible = false,
		ZIndex = 4,
	}, square)
	round(dot, 100)
	local ring = make("Frame", {
		Name = "Ring",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(0.5, 0.5),
		Size = UDim2.fromScale(0.9, 0.9),
		BackgroundTransparency = 1,
		Visible = false,
		ZIndex = 4,
	}, square)
	round(ring, 100)
	outline(ring, Color3.fromRGB(30, 80, 60), 5, 0.45)
	local fileText = label(square, {
		Name = "File",
		AnchorPoint = Vector2.new(1, 1),
		Position = UDim2.new(1, -3, 1, 0),
		Size = UDim2.fromScale(0.3, 0.3),
		TextSize = 13,
		TextXAlignment = Enum.TextXAlignment.Right,
		TextColor3 = if light then DARK_SQUARE else LIGHT_SQUARE,
		ZIndex = 2,
	})
	local rankText = label(square, {
		Name = "Rank",
		Position = UDim2.new(0, 3, 0, 1),
		Size = UDim2.fromScale(0.3, 0.3),
		TextSize = 13,
		TextYAlignment = Enum.TextYAlignment.Top,
		TextColor3 = if light then DARK_SQUARE else LIGHT_SQUARE,
		ZIndex = 2,
	})
	squares[sq] = { button = square, piece = piece, dot = dot, ring = ring, light = light, file = fileText, rank = rankText }
end

-- The card over the board when a game ends, and the promotion picker.
local resultCard = glass(boardFrame, {
	Name = "Result",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5, 0.5),
	Size = UDim2.fromScale(0.62, 0.36),
	Visible = false,
	ZIndex = 8,
	BackgroundTransparency = 0,
})
local resultTitle = label(resultCard, { Name = "Title", Position = UDim2.new(0, 0, 0.08, 0), Size = UDim2.new(1, 0, 0.28, 0), TextSize = 30, TextXAlignment = Enum.TextXAlignment.Center, ZIndex = 9 })
local resultReason = label(resultCard, { Name = "Reason", Position = UDim2.new(0, 0, 0.36, 0), Size = UDim2.new(1, 0, 0.16, 0), TextSize = 18, TextColor3 = SOFT_INK, TextXAlignment = Enum.TextXAlignment.Center, ZIndex = 9 })
local resultRating = label(resultCard, { Name = "Rating", Position = UDim2.new(0, 0, 0.52, 0), Size = UDim2.new(1, 0, 0.14, 0), TextSize = 16, TextXAlignment = Enum.TextXAlignment.Center, ZIndex = 9 })
local resultRematch = button(resultCard, "Rematch", GREEN, { AnchorPoint = Vector2.new(1, 0), Position = UDim2.new(0.48, 0, 0.72, 0), Size = UDim2.new(0.4, 0, 0.2, 0), ZIndex = 9 })
local resultLobby = button(resultCard, "New game", BLUE, { Position = UDim2.new(0.52, 0, 0.72, 0), Size = UDim2.new(0.4, 0, 0.2, 0), ZIndex = 9 })
local resultClose = button(resultCard, "×", GREY, { AnchorPoint = Vector2.new(1, 0), Position = UDim2.new(1, -8, 0, 8), Size = UDim2.fromOffset(30, 30), TextSize = 20, ZIndex = 9 })

local promotionCard = glass(boardFrame, {
	Name = "Promotion",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5, 0.5),
	Size = UDim2.fromScale(0.56, 0.2),
	Visible = false,
	ZIndex = 8,
	BackgroundTransparency = 0,
})
label(promotionCard, { Text = "Promote to", Position = UDim2.new(0, 0, 0, 4), Size = UDim2.new(1, 0, 0.22, 0), TextXAlignment = Enum.TextXAlignment.Center, ZIndex = 9 })
local promotionChoices = {}
for i, letter in { "q", "r", "b", "n" } do
	local choice = make("ImageButton", {
		Name = letter,
		Position = UDim2.new((i - 1) * 0.25 + 0.02, 0, 0.26, 0),
		Size = UDim2.new(0.21, 0, 0.7, 0),
		BackgroundColor3 = LIGHT_SQUARE,
		Image = "",
		ZIndex = 9,
	}, promotionCard)
	round(choice, 8)
	promotionChoices[letter] = choice
end

------------------------------------------------------------------------
-- The side panel: the lobby, or the game you are in.

local panel = glass(screen, {
	Name = "Panel",
	AnchorPoint = Vector2.new(1, 0.5),
	Position = UDim2.new(1, -24, 0.5, 22),
	Size = UDim2.new(0.34, 0, 0.9, 0),
})
make("UISizeConstraint", { MinSize = Vector2.new(330, 400), MaxSize = Vector2.new(560, 2000) }, panel)

local title = label(panel, { Name = "Title", Position = UDim2.new(0, 20, 0, 12), Size = UDim2.new(1, -40, 0, 34), TextSize = 28, Text = "Libroxel Chess" })
local myLine = label(panel, { Name = "Me", Position = UDim2.new(0, 20, 0, 46), Size = UDim2.new(1, -40, 0, 22), TextSize = 15, TextColor3 = SOFT_INK })

local lobbyPage = make("Frame", { Name = "Lobby", Position = UDim2.new(0, 0, 0, 76), Size = UDim2.new(1, 0, 1, -76), BackgroundTransparency = 1 }, panel)
local gamePage = make("Frame", { Name = "Game", Position = UDim2.new(0, 0, 0, 76), Size = UDim2.new(1, 0, 1, -76), BackgroundTransparency = 1, Visible = false }, panel)

-- Lobby tabs.
local tabs = {}
local tabPages = {}
local currentTab = "computer"
for i, info in { { "computer", "Computer" }, { "people", "People" }, { "watch", "Watch" } } do
	local tab = button(lobbyPage, info[2], GREY, {
		Name = info[1] .. "Tab",
		Position = UDim2.new((i - 1) / 3, if i == 1 then 20 else 6, 0, 0),
		Size = UDim2.new(1 / 3, if i == 2 then -12 else -26, 0, 36),
		TextSize = 16,
	})
	tabs[info[1]] = tab
	tabPages[info[1]] = make("Frame", {
		Name = info[1],
		Position = UDim2.new(0, 20, 0, 50),
		Size = UDim2.new(1, -40, 1, -64),
		BackgroundTransparency = 1,
		Visible = false,
	}, lobbyPage)
end

local function sectionTitle(parent, text, y)
	return label(parent, { Position = UDim2.new(0, 0, 0, y), Size = UDim2.new(1, 0, 0, 22), Text = text, TextSize = 15, TextColor3 = SOFT_INK })
end

-- Time controls, shared by the computer and people tabs.
local CONTROLS = { "none", "1+0", "3+2", "5+0", "10+0", "15+10" }
local chosenControl = "none"
local chosenPeopleControl = "5+0"
local controlButtons = { computer = {}, people = {} }
local function controlRow(parent, which, y)
	sectionTitle(parent, "Clock", y)
	for i, control in CONTROLS do
		local b = button(parent, controlText(control), GREY, {
			Position = UDim2.new((i - 1) / 6, 2, 0, y + 24),
			Size = UDim2.new(1 / 6, -4, 0, 34),
			TextSize = 14,
		})
		controlButtons[which][control] = b
	end
end

-- The computer tab: pick an opponent, a clock and a colour.
local computerPage = tabPages.computer
sectionTitle(computerPage, "Opponent", 0)
local bots = make("Frame", { Position = UDim2.new(0, 0, 0, 24), Size = UDim2.new(1, 0, 0, 4 * 66), BackgroundTransparency = 1 }, computerPage)
local chosenBot = 4
local botButtons = {}
for i, bot in Engine.BOTS do
	local b = make("TextButton", {
		Name = bot.name,
		Text = "",
		Position = UDim2.new(((i - 1) % 2) * 0.5, 2, 0, ((i - 1) // 2) * 66),
		Size = UDim2.new(0.5, -4, 0, 60),
		BackgroundColor3 = WHITE,
		BackgroundTransparency = 0.2,
	}, bots)
	round(b, 10)
	outline(b, WHITE, 1, 0.3)
	local face = make("Frame", { AnchorPoint = Vector2.new(0, 0.5), Position = UDim2.new(0, 8, 0.5, 0), Size = UDim2.fromOffset(38, 38), BackgroundColor3 = bot.colour }, b)
	round(face, 19)
	shine(face)
	outline(face, WHITE, 2, 0.1)
	label(face, { Size = UDim2.fromScale(1, 1), Text = string.sub(bot.name, 1, 1), TextColor3 = WHITE, TextSize = 20, TextXAlignment = Enum.TextXAlignment.Center })
	label(b, { Position = UDim2.new(0, 54, 0, 6), Size = UDim2.new(1, -60, 0, 22), Text = bot.name .. "  <font color=\"#5a7896\">" .. bot.rating .. "</font>", RichText = true, TextSize = 17 })
	label(b, { Position = UDim2.new(0, 54, 0, 28), Size = UDim2.new(1, -60, 0, 28), Text = bot.about, TextSize = 12, TextColor3 = SOFT_INK, TextWrapped = true, TextYAlignment = Enum.TextYAlignment.Top })
	botButtons[i] = b
end
controlRow(computerPage, "computer", 4 * 66 + 34)
local colourY = 4 * 66 + 34 + 70
sectionTitle(computerPage, "Play as", colourY)
local chosenColour = "random"
local colourButtons = {}
for i, colour in { "white", "random", "black" } do
	colourButtons[colour] = button(computerPage, ({ white = "White", random = "Either", black = "Black" })[colour], GREY, {
		Position = UDim2.new((i - 1) / 3, 2, 0, colourY + 24),
		Size = UDim2.new(1 / 3, -4, 0, 34),
		TextSize = 15,
	})
end
local playBotButton = button(computerPage, "Play", GREEN, {
	Position = UDim2.new(0, 0, 0, colourY + 74),
	Size = UDim2.new(1, 0, 0, 50),
	TextSize = 22,
})

-- The people tab: look for a game, challenge someone, answer challenges.
local peoplePage = tabPages.people
controlRow(peoplePage, "people", 0)
local seekButton = button(peoplePage, "Find a game", GREEN, { Position = UDim2.new(0, 0, 0, 68), Size = UDim2.new(1, 0, 0, 46), TextSize = 20 })
sectionTitle(peoplePage, "Challenges", 126)
local challengeList = make("Frame", { Position = UDim2.new(0, 0, 0, 150), Size = UDim2.new(1, 0, 0, 0), BackgroundTransparency = 1 }, peoplePage)
local playersTitle = sectionTitle(peoplePage, "Players here", 160)
local playerList = make("ScrollingFrame", {
	Position = UDim2.new(0, 0, 0, 184),
	Size = UDim2.new(1, 0, 1, -184),
	BackgroundTransparency = 1,
	ScrollBarThickness = 6,
	CanvasSize = UDim2.new(0, 0, 0, 0),
}, peoplePage)

-- The watch tab: games going on in this server.
local watchPage = tabPages.watch
sectionTitle(watchPage, "Games being played now", 0)
local liveList = make("ScrollingFrame", {
	Position = UDim2.new(0, 0, 0, 26),
	Size = UDim2.new(1, 0, 1, -26),
	BackgroundTransparency = 1,
	ScrollBarThickness = 6,
	CanvasSize = UDim2.new(0, 0, 0, 0),
}, watchPage)

-- The game page.
local gameTitle = label(gamePage, { Name = "Status", Position = UDim2.new(0, 20, 0, 0), Size = UDim2.new(1, -40, 0, 28), TextSize = 20 })
local gameSub = label(gamePage, { Name = "Sub", Position = UDim2.new(0, 20, 0, 28), Size = UDim2.new(1, -40, 0, 20), TextSize = 14, TextColor3 = SOFT_INK })

local thoughts = make("Frame", {
	Name = "Thoughts",
	Position = UDim2.new(0, 20, 0, 56),
	Size = UDim2.new(1, -40, 0, 96),
	BackgroundColor3 = WHITE,
}, gamePage)
round(thoughts, 12)
make("UIGradient", { Color = ColorSequence.new(Color3.fromRGB(70, 130, 200), Color3.fromRGB(20, 50, 100)), Rotation = 90 }, thoughts)
outline(thoughts, Color3.fromRGB(150, 210, 255), 1, 0.4)
local thoughtsTitle = label(thoughts, { Position = UDim2.new(0, 12, 0, 6), Size = UDim2.new(1, -24, 0, 20), TextSize = 14, TextColor3 = Color3.fromRGB(170, 220, 255), Text = "Engine" })
local thoughtsScore = label(thoughts, { Position = UDim2.new(0, 12, 0, 26), Size = UDim2.new(0.4, 0, 0, 34), TextSize = 30, TextColor3 = WHITE, Text = "0.00" })
local thoughtsStats = label(thoughts, { Position = UDim2.new(0.38, 0, 0, 28), Size = UDim2.new(0.62, -12, 0, 32), TextSize = 13, TextColor3 = Color3.fromRGB(200, 225, 250), TextWrapped = true, TextYAlignment = Enum.TextYAlignment.Top })
local thoughtsLine = label(thoughts, { Position = UDim2.new(0, 12, 0, 62), Size = UDim2.new(1, -24, 0, 30), TextSize = 14, TextColor3 = WHITE, TextWrapped = true, TextYAlignment = Enum.TextYAlignment.Top })

local movesBox = make("ScrollingFrame", {
	Name = "Moves",
	Position = UDim2.new(0, 20, 0, 162),
	Size = UDim2.new(1, -40, 1, -330),
	BackgroundColor3 = WHITE,
	BackgroundTransparency = 0.1,
	ScrollBarThickness = 6,
	CanvasSize = UDim2.new(0, 0, 0, 0),
}, gamePage)
round(movesBox, 10)

local drawBanner = make("Frame", { Name = "DrawOffer", Position = UDim2.new(0, 20, 1, -164), Size = UDim2.new(1, -40, 0, 40), BackgroundColor3 = Color3.fromRGB(255, 240, 200), Visible = false }, gamePage)
round(drawBanner, 10)
local drawText = label(drawBanner, { Position = UDim2.new(0, 10, 0, 0), Size = UDim2.new(0.5, 0, 1, 0), TextSize = 15, Text = "Draw offered" })
local acceptDraw = button(drawBanner, "Accept", GREEN, { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -96, 0.5, 0), Size = UDim2.fromOffset(84, 30), TextSize = 14 })
local declineDraw = button(drawBanner, "Decline", RED, { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -6, 0.5, 0), Size = UDim2.fromOffset(84, 30), TextSize = 14 })

local actions = make("Frame", { Name = "Actions", Position = UDim2.new(0, 20, 1, -118), Size = UDim2.new(1, -40, 0, 40), BackgroundTransparency = 1 }, gamePage)
make("UIListLayout", { FillDirection = Enum.FillDirection.Horizontal, Padding = UDim.new(0, 6) }, actions)
local function actionButton(text, colour)
	return button(actions, text, colour, { Size = UDim2.new(0.2, -5, 1, 0), TextSize = 14 })
end
local resignButton = actionButton("Resign", RED)
local drawButton = actionButton("Draw", GREY)
local takebackButton = actionButton("Take back", BLUE)
local hintButton = actionButton("Hint", BLUE)
local flipButton = actionButton("Flip", GREY)

local chatRow = make("Frame", { Name = "Chat", Position = UDim2.new(0, 20, 1, -70), Size = UDim2.new(1, -40, 0, 56), BackgroundTransparency = 1 }, gamePage)
local chatLog = label(chatRow, { Position = UDim2.new(0, 0, 0, 0), Size = UDim2.new(1, 0, 0, 20), TextSize = 13, TextColor3 = SOFT_INK })
local PHRASES = { "Hi!", "Good luck!", "Have fun!", "Nice move!", "Oops", "Thanks!", "Good game!", "Rematch?" }
local phraseButtons = {}
for i, phrase in PHRASES do
	phraseButtons[i] = button(chatRow, phrase, Color3.fromRGB(120, 170, 220), {
		Position = UDim2.new((i - 1) % 4 / 4, 1, 0, 22 + ((i - 1) // 4) * 18),
		Size = UDim2.new(0.25, -2, 0, 16),
		TextSize = 11,
	})
end
local backButton = button(gamePage, "Back to lobby", BLUE, { Position = UDim2.new(0, 20, 1, -70), Size = UDim2.new(1, -40, 0, 44), TextSize = 18, Visible = false })

-- Messages at the top of the screen.
local toastBox = make("TextLabel", {
	Name = "Toast",
	AnchorPoint = Vector2.new(0.5, 0),
	Position = UDim2.new(0.5, 0, 0, 14),
	Size = UDim2.fromOffset(520, 40),
	BackgroundColor3 = Color3.fromRGB(25, 45, 75),
	BackgroundTransparency = 0.1,
	TextColor3 = WHITE,
	TextSize = 17,
	Visible = false,
	ZIndex = 20,
}, screen)
round(toastBox, 12)
outline(toastBox, Color3.fromRGB(150, 210, 255), 1, 0.3)

local sounds = {}
for name, id in { move = "libroxel://click", capture = "libroxel://coin", over = "libroxel://chime" } do
	sounds[name] = make("Sound", { Name = name, SoundId = id, Volume = 0.5 }, screen)
end

------------------------------------------------------------------------
-- What the screen shows.

local view = nil -- the game being shown, as the server last described it
local lobby = nil
local flipped = false
local picked = nil -- the square of the piece you picked up
local targets = {} -- where it can go: square -> list of moves
local pending = nil -- a promotion waiting for a choice
local position = nil
local received = os.clock()
local resultHidden = false
local shownPly = -1
local shownId = nil
local toastCount = 0

local function toast(text)
	toastCount += 1
	local mine = toastCount
	toastBox.Text = text
	toastBox.Visible = true
	task.delay(3.5, function()
		if toastCount == mine then
			toastBox.Visible = false
		end
	end)
end

local function send(action, data)
	Action:FireServer(action, data or {})
end

local function mySide()
	if view and view.you == "white" then
		return 0
	elseif view and view.you == "black" then
		return 1
	end
	return nil
end

local function myTurn()
	return view ~= nil and view.status == "playing" and position ~= nil and mySide() == position.side
end

-- Where a square sits on screen, with the board turned for black.
local function place(sq)
	local f, r = Rules.fileOf(sq), Rules.rankOf(sq)
	if flipped then
		f, r = 7 - f, 7 - r
	end
	return UDim2.fromScale(f / 8, (7 - r) / 8)
end

local function drawBoard()
	local last = view and view.last or ""
	local lastFrom = if #last >= 4 then Rules.parseSquare(string.sub(last, 1, 2)) else nil
	local lastTo = if #last >= 4 then Rules.parseSquare(string.sub(last, 3, 4)) else nil
	local hint = view and view.hint or ""
	local hintFrom = if #hint >= 4 then Rules.parseSquare(string.sub(hint, 1, 2)) else nil
	local hintTo = if #hint >= 4 then Rules.parseSquare(string.sub(hint, 3, 4)) else nil
	local checked = nil
	if position and position:inCheck() then
		checked = position.kings[position.side]
	end
	for sq, s in squares do
		s.button.Position = place(sq)
		local colour = if s.light then LIGHT_SQUARE else DARK_SQUARE
		if sq == lastFrom or sq == lastTo then
			colour = if s.light then LAST_LIGHT else LAST_DARK
		end
		if sq == hintFrom or sq == hintTo then
			colour = HINTED
		end
		if sq == picked then
			colour = PICKED
		end
		if sq == checked then
			colour = CHECKED
		end
		s.button.BackgroundColor3 = colour
		local name = position and position:pieceName(sq)
		s.piece.Image = if name then "libroxel://chess-" .. name else ""
		local target = targets[sq]
		s.dot.Visible = target ~= nil and (position == nil or position.board[sq] == 0)
		s.ring.Visible = target ~= nil and position ~= nil and position.board[sq] ~= 0
		local f, r = Rules.fileOf(sq), Rules.rankOf(sq)
		local bottomRank = if flipped then 7 else 0
		local leftFile = if flipped then 7 else 0
		s.file.Text = if r == bottomRank then string.char(97 + f) else ""
		s.rank.Text = if f == leftFile then tostring(r + 1) else ""
	end
end

-- The pieces each side has lost, and who is ahead in material.
local VALUES = { p = 1, n = 3, b = 3, r = 5, q = 9 }
local function capturedText(sideLetter)
	if not position then
		return "", 0
	end
	local start = { p = 8, n = 2, b = 2, r = 2, q = 1 }
	local count = { p = 0, n = 0, b = 0, r = 0, q = 0 }
	local material = { w = 0, b = 0 }
	for _, sq in Rules.SQUARES do
		local name = position:pieceName(sq)
		if name then
			local kind = string.sub(name, 2, 2)
			if VALUES[kind] then
				material[string.sub(name, 1, 1)] += VALUES[kind]
				if string.sub(name, 1, 1) ~= sideLetter then
					count[kind] += 1
				end
			end
		end
	end
	local symbols = { q = "Q", r = "R", b = "B", n = "N", p = "P" }
	local text = ""
	for _, kind in { "q", "r", "b", "n", "p" } do
		local lost = math.max(0, start[kind] - count[kind])
		if lost > 0 then
			text ..= string.rep(symbols[kind], lost) .. " "
		end
	end
	local other = if sideLetter == "w" then "b" else "w"
	return text, material[sideLetter] - material[other]
end

local function fillCard(card, seat, side)
	if not seat then
		card.PlayerName.Text = ""
		card.Detail.Text = ""
		card.Clock.Visible = false
		return
	end
	local letter = if side == 0 then "w" else "b"
	card.PlayerName.Text = seat.name .. "  (" .. seat.rating .. ")" .. (if seat.bot then "  · computer" else "")
	local taken, lead = capturedText(letter)
	card.Detail.Text = (if side == 0 then "White" else "Black") .. (if taken ~= "" then "  ·  took " .. taken else "") .. (if lead > 0 then " +" .. lead else "")
	card.Dot.BackgroundColor3 = if seat.bot and seat.colour then seat.colour else if side == 0 then Color3.fromRGB(240, 246, 252) else Color3.fromRGB(40, 55, 80)
	card.Dot.Letter.Text = string.sub(seat.name, 1, 1)
	card.Dot.Letter.TextColor3 = if side == 0 and not seat.bot then INK else WHITE
	card.Clock.Visible = view.timed
end

local function updateClocks()
	if not view then
		return
	end
	local elapsed = (os.clock() - received) * 1000
	for side = 0, 1 do
		local card = if (side == 0) ~= flipped then bottomCard else topCard
		local left = view.clocks[side + 1]
		local running = view.running == side
		if running then
			left = math.max(0, left - elapsed)
		end
		card.Clock.Text = clockText(math.floor(left))
		card.Clock.BackgroundColor3 = if running
			then (if left < 20000 then RED else Color3.fromRGB(40, 150, 90))
			else Color3.fromRGB(30, 50, 75)
		card.Clock.TextTransparency = if running then 0 else 0.3
	end
	if view.firstMoveLeft and view.firstMoveLeft >= 0 and view.status == "playing" and not view.vsBot then
		local left = math.max(0, view.firstMoveLeft - math.floor(elapsed / 1000))
		gameSub.Text = (if position and position.side == 0 then "White" else "Black") .. " has " .. left .. "s to make a first move"
	end
end

local function drawMoves()
	for _, child in movesBox:GetChildren() do
		if child:IsA("TextLabel") then
			child:Destroy()
		end
	end
	local sans = view.sans
	local rows = math.ceil(#sans / 2)
	for row = 1, rows do
		local white, black = sans[row * 2 - 1], sans[row * 2]
		local y = (row - 1) * 24 + 4
		local shade = row % 2 == 0
		local line = label(movesBox, {
			Position = UDim2.new(0, 4, 0, y),
			Size = UDim2.new(1, -16, 0, 24),
			BackgroundColor3 = Color3.fromRGB(200, 225, 245),
			BackgroundTransparency = if shade then 0.5 else 1,
			Text = "",
		})
		label(line, { Position = UDim2.new(0, 6, 0, 0), Size = UDim2.new(0.18, 0, 1, 0), Text = row .. ".", TextColor3 = SOFT_INK, TextSize = 15 })
		label(line, { Position = UDim2.new(0.2, 0, 0, 0), Size = UDim2.new(0.38, 0, 1, 0), Text = white or "", TextSize = 16, RichText = true })
		label(line, { Position = UDim2.new(0.6, 0, 0, 0), Size = UDim2.new(0.38, 0, 1, 0), Text = black or "", TextSize = 16 })
	end
	local height = rows * 24 + 8
	movesBox.CanvasSize = UDim2.new(0, 0, 0, height)
	movesBox.CanvasPosition = Vector2.new(0, math.max(0, height - 200))
end

local RESULT_WORDS = {
	["1-0"] = "White wins",
	["0-1"] = "Black wins",
	["1/2-1/2"] = "Draw",
	["*"] = "Game called off",
}

local function drawThoughts()
	local info = view.info
	if info then
		thoughtsTitle.Text = (if info.by then info.by .. "'s thoughts" else "Engine") .. (if view.thinking then "  · thinking…" else "")
		thoughtsScore.Text = if info.book then "Book" else Engine.scoreText(info.score)
		local speed = if info.time > 0.01 then math.floor(info.nodes / info.time / 1000) .. "k positions/s" else ""
		thoughtsStats.Text = if info.book then "An opening it knows" else
			"Depth " .. info.depth .. " · " .. info.nodes .. " positions\n" .. speed
		thoughtsLine.Text = "Line: " .. table.concat(info.line, " ")
		local share = Engine.whiteShare(info.score)
		evalFill.Size = UDim2.fromScale(1, if flipped then 1 - share else share)
		evalFill.AnchorPoint = if flipped then Vector2.new(0, 0) else Vector2.new(0, 1)
		evalFill.Position = if flipped then UDim2.fromScale(0, 0) else UDim2.fromScale(0, 1)
		evalText.Text = if info.book then "" else Engine.scoreText(info.score)
	else
		thoughtsTitle.Text = if view.vsBot then "The computer's thoughts" else "Engine"
		thoughtsScore.Text = "0.00"
		thoughtsStats.Text = if view.vsBot then "Waiting for its first move" else "People playing people: the engine stays quiet"
		thoughtsLine.Text = ""
		evalFill.Size = UDim2.fromScale(1, 0.5)
		evalText.Text = ""
	end
end

local function drawGame()
	local playing = view.status == "playing"
	local side = mySide()
	local watching = side == nil
	local turnName = if position.side == 0 then view.white.name else view.black.name
	if playing then
		if watching then
			gameTitle.Text = "Watching " .. view.white.name .. " vs " .. view.black.name
		elseif myTurn() then
			gameTitle.Text = "Your move"
		elseif view.thinking then
			gameTitle.Text = turnName .. " is thinking…"
		else
			gameTitle.Text = "Waiting for " .. turnName
		end
	else
		gameTitle.Text = RESULT_WORDS[view.result] or "Game over"
	end
	local kind = if view.vsBot then "Against the computer" elseif view.rated then "Rated" else "Casual"
	gameSub.Text = kind .. " · " .. controlText(view.control) .. (if position:inCheck() and playing then " · Check!" else "")

	drawThoughts()
	drawMoves()

	local topSide = if flipped then 0 else 1
	fillCard(topCard, if topSide == 0 then view.white else view.black, topSide)
	fillCard(bottomCard, if topSide == 0 then view.black else view.white, 1 - topSide)

	resignButton.Visible = playing and not watching
	resignButton.Text = if #view.sans < 2 then "Abort" else "Resign"
	drawButton.Visible = playing and not watching
	drawButton.Text = if view.drawOffer == side then "Offered" else "Draw"
	takebackButton.Visible = playing and not watching and view.vsBot
	hintButton.Visible = playing and not watching and view.vsBot
	chatRow.Visible = not watching and not view.vsBot and playing
	backButton.Visible = watching or not playing
	if not view.vsBot and #view.chat > 0 then
		chatLog.Text = view.chat[#view.chat]
	else
		chatLog.Text = ""
	end
	drawBanner.Visible = playing and not watching and view.drawOffer >= 0 and view.drawOffer ~= side
	drawText.Text = (if view.drawOffer == 0 then view.white.name else view.black.name) .. " offers a draw"

	resultCard.Visible = not playing and not resultHidden
	if not playing then
		resultTitle.Text = RESULT_WORDS[view.result] or "Game over"
		if side ~= nil then
			local mine = if side == 0 then "1-0" else "0-1"
			if view.result == mine then
				resultTitle.Text = "You win!"
			elseif view.result ~= "1/2-1/2" and view.result ~= "*" then
				resultTitle.Text = "You lose"
			end
		end
		resultReason.Text = if view.reason == "aborted" then "Not enough moves were made" else "By " .. view.reason
		if view.change and side ~= nil then
			local delta = view.change[side + 1]
			local seat = if side == 0 then view.white else view.black
			resultRating.Text = "Rating " .. (seat.rating + delta) .. " (" .. (if delta >= 0 then "+" else "") .. delta .. ")"
		else
			resultRating.Text = ""
		end
		resultRematch.Visible = not watching
		local offered = view.rematch[(side or 0) + 1]
		local theyOffered = view.rematch[2 - (side or 0)]
		resultRematch.Text = if offered then "Asked…" elseif theyOffered then "Accept rematch" else "Rematch"
	end
	drawBoard()
	updateClocks()
end

local function showLobbyTab(name)
	currentTab = name
	for key, page in tabPages do
		page.Visible = key == name
		tabs[key].BackgroundColor3 = if key == name then BLUE else GREY
	end
end

local function rowFrame(parent, y, height)
	local row = make("Frame", {
		Position = UDim2.new(0, 0, 0, y),
		Size = UDim2.new(1, -10, 0, height),
		BackgroundColor3 = WHITE,
		BackgroundTransparency = 0.3,
	}, parent)
	round(row, 10)
	outline(row, WHITE, 1, 0.4)
	return row
end

local function clear(parent)
	for _, child in parent:GetChildren() do
		if child:IsA("Frame") then
			child:Destroy()
		end
	end
end

local function drawLobby()
	if not lobby then
		return
	end
	local my = lobby.me
	myLine.Text = "Rating " .. my.rating .. " · " .. my.wins .. " won, " .. my.draws .. " drawn, " .. my.losses .. " lost · " .. my.botWins .. " wins against the computer"
	for i, b in botButtons do
		b.BackgroundColor3 = if i == chosenBot then Color3.fromRGB(150, 210, 255) else WHITE
	end
	for control, b in controlButtons.computer do
		b.BackgroundColor3 = if control == chosenControl then BLUE else GREY
	end
	for control, b in controlButtons.people do
		b.BackgroundColor3 = if control == chosenPeopleControl then BLUE else GREY
	end
	for colour, b in colourButtons do
		b.BackgroundColor3 = if colour == chosenColour then BLUE else GREY
	end
	if my.seeking ~= "" then
		seekButton.Text = "Looking for a " .. controlText(my.seeking) .. " game… (cancel)"
		seekButton.BackgroundColor3 = Color3.fromRGB(230, 160, 60)
	else
		seekButton.Text = "Find a game"
		seekButton.BackgroundColor3 = GREEN
	end

	-- Challenges to and from you.
	clear(challengeList)
	local y = 0
	for _, c in lobby.incoming do
		local row = rowFrame(challengeList, y, 44)
		label(row, { Position = UDim2.new(0, 10, 0, 0), Size = UDim2.new(0.55, 0, 1, 0), TextSize = 14, TextWrapped = true, Text = c.name .. " (" .. c.rating .. ") · " .. controlText(c.control) })
		local yes = button(row, "Play", GREEN, { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -78, 0.5, 0), Size = UDim2.fromOffset(66, 30), TextSize = 14 })
		local no = button(row, "No", RED, { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -6, 0.5, 0), Size = UDim2.fromOffset(66, 30), TextSize = 14 })
		yes.MouseButton1Click:Connect(function()
			send("answer", { id = c.id, accept = true })
		end)
		no.MouseButton1Click:Connect(function()
			send("answer", { id = c.id, accept = false })
		end)
		y += 50
	end
	for _, c in lobby.outgoing do
		local row = rowFrame(challengeList, y, 44)
		label(row, { Position = UDim2.new(0, 10, 0, 0), Size = UDim2.new(0.7, 0, 1, 0), TextSize = 14, Text = "You challenged " .. c.name .. " · " .. controlText(c.control) })
		local cancel = button(row, "Cancel", GREY, { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -6, 0.5, 0), Size = UDim2.fromOffset(76, 30), TextSize = 14 })
		cancel.MouseButton1Click:Connect(function()
			send("answer", { id = c.id, accept = false })
		end)
		y += 50
	end
	if y == 0 then
		local row = rowFrame(challengeList, 0, 30)
		row.BackgroundTransparency = 1
		label(row, { Position = UDim2.new(0, 2, 0, 0), Size = UDim2.new(1, 0, 1, 0), TextSize = 14, TextColor3 = SOFT_INK, Text = "None yet. Challenge someone below." })
		y = 34
	end
	playersTitle.Position = UDim2.new(0, 0, 0, 156 + y)
	playerList.Position = UDim2.new(0, 0, 0, 180 + y)
	playerList.Size = UDim2.new(1, 0, 1, -(180 + y))

	clear(playerList)
	for i, p in lobby.players do
		local row = rowFrame(playerList, (i - 1) * 52, 46)
		label(row, { Position = UDim2.new(0, 10, 0, 3), Size = UDim2.new(0.6, 0, 0, 22), TextSize = 16, Text = p.name .. "  (" .. p.rating .. ")" })
		label(row, { Position = UDim2.new(0, 10, 0, 23), Size = UDim2.new(0.6, 0, 0, 18), TextSize = 13, TextColor3 = SOFT_INK, Text = p.status })
		if p.free then
			local challenge = button(row, "Challenge", BLUE, { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -6, 0.5, 0), Size = UDim2.fromOffset(100, 32), TextSize = 14 })
			challenge.MouseButton1Click:Connect(function()
				send("challenge", { to = p.id, control = chosenPeopleControl })
			end)
		end
	end
	if #lobby.players == 0 then
		local row = rowFrame(playerList, 0, 60)
		row.BackgroundTransparency = 1
		label(row, { Position = UDim2.new(0, 2, 0, 0), Size = UDim2.new(1, -4, 1, 0), TextSize = 14, TextColor3 = SOFT_INK, TextWrapped = true, Text = "Nobody else is here yet. Host this game so friends can join, or play the computer meanwhile." })
	end
	playerList.CanvasSize = UDim2.new(0, 0, 0, #lobby.players * 52)

	clear(liveList)
	for i, g in lobby.live do
		local row = rowFrame(liveList, (i - 1) * 56, 50)
		label(row, { Position = UDim2.new(0, 10, 0, 4), Size = UDim2.new(0.68, 0, 0, 22), TextSize = 15, Text = g.white .. " (" .. g.whiteRating .. ")  vs  " .. g.black .. " (" .. g.blackRating .. ")" })
		label(row, { Position = UDim2.new(0, 10, 0, 26), Size = UDim2.new(0.68, 0, 0, 18), TextSize = 13, TextColor3 = SOFT_INK, Text = controlText(g.control) .. " · move " .. (g.ply // 2 + 1) })
		local watch = button(row, if g.mine then "Yours" else "Watch", BLUE, { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -6, 0.5, 0), Size = UDim2.fromOffset(86, 32), TextSize = 14 })
		watch.MouseButton1Click:Connect(function()
			if not g.mine then
				send("watch", { game = g.id })
			end
		end)
	end
	if #lobby.live == 0 then
		local row = rowFrame(liveList, 0, 40)
		row.BackgroundTransparency = 1
		label(row, { Position = UDim2.new(0, 2, 0, 0), Size = UDim2.new(1, 0, 1, 0), TextSize = 14, TextColor3 = SOFT_INK, Text = "No games right now." })
	end
	liveList.CanvasSize = UDim2.new(0, 0, 0, #lobby.live * 56)
end

local function showPage()
	local inGame = view ~= nil
	lobbyPage.Visible = not inGame
	gamePage.Visible = inGame
	if not inGame then
		position = Rules.new()
		picked, targets = nil, {}
		for _, card in { topCard, bottomCard } do
			card.Clock.Visible = false
		end
		topCard.PlayerName.Text = "Pick an opponent"
		topCard.Detail.Text = "The computer, or someone in this server"
		topCard.Dot.Letter.Text = "?"
		bottomCard.PlayerName.Text = me.Name
		bottomCard.Detail.Text = if lobby then "Rating " .. lobby.me.rating else ""
		bottomCard.Dot.Letter.Text = string.sub(me.Name, 1, 1)
		resultCard.Visible = false
		promotionCard.Visible = false
		evalFill.Size = UDim2.fromScale(1, 0.5)
		evalText.Text = ""
		flipped = false
		drawBoard()
		drawLobby()
	end
end

-- Slides a piece picture from one square to another.
local function slide(fromSq, toSq)
	local from, to = squares[fromSq], squares[toSq]
	if not from or not to or to.piece.Image == "" then
		return
	end
	local ghost = make("ImageLabel", {
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = place(fromSq) + UDim2.fromScale(1 / 16, 1 / 16),
		Size = UDim2.fromScale(0.92 / 8, 0.92 / 8),
		BackgroundTransparency = 1,
		Image = to.piece.Image,
		ZIndex = 6,
	}, boardFrame)
	to.piece.ImageTransparency = 1
	local tween = TweenService:Create(ghost, TweenInfo.new(0.14, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
		Position = place(toSq) + UDim2.fromScale(1 / 16, 1 / 16),
	})
	tween:Play()
	task.delay(0.15, function()
		ghost:Destroy()
		to.piece.ImageTransparency = 0
	end)
end

local function onGame(newView)
	local fresh = shownId ~= newView.id
	view = newView
	received = os.clock()
	position = Rules.fromFen(view.fen)
	if fresh then
		resultHidden = false
		flipped = view.you == "black"
		picked, targets, pending = nil, {}, nil
		promotionCard.Visible = false
	end
	local moved = not fresh and view.ply ~= shownPly
	if moved then
		picked, targets, pending = nil, {}, nil
		promotionCard.Visible = false
	end
	local wasOver = gamePage.Visible and resultCard.Visible
	showPage()
	drawGame()
	if moved and view.ply > shownPly and #view.last >= 4 then
		slide(Rules.parseSquare(string.sub(view.last, 1, 2)), Rules.parseSquare(string.sub(view.last, 3, 4)))
		local san = view.sans[#view.sans] or ""
		if string.find(san, "x", 1, true) then
			sounds.capture:Play()
		else
			sounds.move:Play()
		end
	end
	if view.status ~= "playing" and not wasOver and (moved or not fresh) and shownPly >= 0 then
		sounds.over:Play()
	end
	shownPly = view.ply
	shownId = view.id
end

------------------------------------------------------------------------
-- Clicks.

local function tryMove(m)
	local uci = Rules.uci(m)
	-- Show it straight away; the server's answer follows.
	if position:make(m) then
		view.last = uci
		drawBoard()
	end
	send("move", { game = view.id, uci = uci })
	picked, targets = nil, {}
end

local function pick(sq)
	picked, targets = sq, {}
	for _, m in position:legalMoves() do
		if Rules.from(m) == sq then
			local to = Rules.to(m)
			targets[to] = targets[to] or {}
			table.insert(targets[to], m)
		end
	end
	drawBoard()
end

for sq, s in squares do
	s.button.MouseButton1Click:Connect(function()
		if not view or not myTurn() or pending then
			return
		end
		local moves = targets[sq]
		if picked and moves then
			if #moves > 1 then
				-- A promotion: choose the piece.
				pending = moves
				local colour = if mySide() == 0 then "w" else "b"
				for letter, choice in promotionChoices do
					choice.Image = "libroxel://chess-" .. colour .. letter
				end
				promotionCard.Visible = true
				return
			end
			tryMove(moves[1])
			return
		end
		local name = position:pieceName(sq)
		local mine = if mySide() == 0 then "w" else "b"
		if name and string.sub(name, 1, 1) == mine and sq ~= picked then
			pick(sq)
		else
			picked, targets = nil, {}
			drawBoard()
		end
	end)
end

local PROMOTION_KIND = { q = Rules.QUEEN, r = Rules.ROOK, b = Rules.BISHOP, n = Rules.KNIGHT }
for letter, choice in promotionChoices do
	choice.MouseButton1Click:Connect(function()
		if not pending then
			return
		end
		for _, m in pending do
			if Rules.promotion(m) == PROMOTION_KIND[letter] then
				pending = nil
				promotionCard.Visible = false
				tryMove(m)
				return
			end
		end
	end)
end

for name, tab in tabs do
	tab.MouseButton1Click:Connect(function()
		showLobbyTab(name)
	end)
end
for i, b in botButtons do
	b.MouseButton1Click:Connect(function()
		chosenBot = i
		drawLobby()
	end)
end
for control, b in controlButtons.computer do
	b.MouseButton1Click:Connect(function()
		chosenControl = control
		drawLobby()
	end)
end
for control, b in controlButtons.people do
	b.MouseButton1Click:Connect(function()
		chosenPeopleControl = control
		drawLobby()
	end)
end
for colour, b in colourButtons do
	b.MouseButton1Click:Connect(function()
		chosenColour = colour
		drawLobby()
	end)
end
playBotButton.MouseButton1Click:Connect(function()
	send("playBot", { level = chosenBot, control = chosenControl, colour = chosenColour })
end)
seekButton.MouseButton1Click:Connect(function()
	if lobby and lobby.me.seeking ~= "" then
		send("cancelSeek")
	else
		send("seek", { control = chosenPeopleControl })
	end
end)
resignButton.MouseButton1Click:Connect(function()
	send("resign")
end)
drawButton.MouseButton1Click:Connect(function()
	send("draw")
end)
acceptDraw.MouseButton1Click:Connect(function()
	send("draw")
end)
declineDraw.MouseButton1Click:Connect(function()
	send("declineDraw")
end)
takebackButton.MouseButton1Click:Connect(function()
	send("takeback")
end)
hintButton.MouseButton1Click:Connect(function()
	toast("Asking the engine…")
	send("hint")
end)
flipButton.MouseButton1Click:Connect(function()
	flipped = not flipped
	if view then
		drawGame()
	else
		drawBoard()
	end
end)
for i, b in phraseButtons do
	b.MouseButton1Click:Connect(function()
		send("chat", { phrase = i })
	end)
end
resultRematch.MouseButton1Click:Connect(function()
	send("rematch")
end)
resultClose.MouseButton1Click:Connect(function()
	resultHidden = true
	resultCard.Visible = false
end)
local function toLobby()
	send("leave")
	view, shownId, shownPly = nil, nil, -1
	showPage()
end
resultLobby.MouseButton1Click:Connect(toLobby)
backButton.MouseButton1Click:Connect(toLobby)

------------------------------------------------------------------------
-- What the server says.

State.OnClientEvent:Connect(function(kind, data)
	if kind == "lobby" then
		lobby = data
		if not view then
			showPage()
		else
			myLine.Text = "Rating " .. data.me.rating .. " · " .. data.me.wins .. " won, " .. data.me.draws .. " drawn, " .. data.me.losses .. " lost"
		end
	elseif kind == "game" then
		onGame(data)
	elseif kind == "left" then
		view, shownId, shownPly = nil, nil, -1
		showPage()
	elseif kind == "toast" then
		toast(data)
	end
end)

showLobbyTab(currentTab)
showPage()
send("hello")

while true do
	task.wait(0.1)
	updateClocks()
end
