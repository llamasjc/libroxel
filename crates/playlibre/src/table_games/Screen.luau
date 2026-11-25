-- The screen shared by the table games: the menu with its big Play
-- button, the side panel during a game (whose turn it is, who is playing,
-- Leave), the card when a game ends and the rules. Each game's own screen
-- script draws its board into `ui.area` and passes on clicks with `ui.send`.

local Players = game:GetService("Players")

local Screen = {}

Screen.WHITE = Color3.fromRGB(255, 255, 255)
Screen.INK = Color3.fromRGB(35, 35, 40)
Screen.SOFT = Color3.fromRGB(105, 105, 115)
Screen.GREEN = Color3.fromRGB(46, 160, 67)
Screen.BLUE = Color3.fromRGB(45, 110, 200)
Screen.RED = Color3.fromRGB(205, 60, 55)
Screen.GREY = Color3.fromRGB(120, 125, 135)
Screen.LIGHT = Color3.fromRGB(236, 236, 240)
Screen.GOLD = Color3.fromRGB(240, 190, 50)

function Screen.make(class, props, parent)
	local object = Instance.new(class)
	for key, value in props do
		object[key] = value
	end
	object.Parent = parent
	return object
end
local make = Screen.make

function Screen.round(object, radius)
	make("UICorner", { CornerRadius = UDim.new(0, radius or 8) }, object)
	return object
end
local round = Screen.round

function Screen.label(parent, props)
	local base = {
		BackgroundTransparency = 1,
		TextColor3 = Screen.INK,
		TextSize = 16,
		Text = "",
		TextXAlignment = Enum.TextXAlignment.Left,
	}
	for key, value in props do
		base[key] = value
	end
	return make("TextLabel", base, parent)
end
local label = Screen.label

-- A plain, flat button.
function Screen.button(parent, text, colour, props)
	local b = make("TextButton", {
		Text = text,
		TextColor3 = Screen.WHITE,
		TextSize = 17,
		BackgroundColor3 = colour or Screen.BLUE,
		Size = UDim2.new(0, 120, 0, 40),
	}, parent)
	for key, value in props or {} do
		b[key] = value
	end
	round(b, 8)
	return b
end
local button = Screen.button

local function card(look, parent, props)
	local c = make("Frame", { BackgroundColor3 = look.page }, parent)
	for key, value in props do
		c[key] = value
	end
	round(c, look.radius)
	make("UIStroke", { Color = look.line, Thickness = 1 }, c)
	return c
end

-- What the screen looks like. A game's `look` overrides any of these.
local LOOK = {
	background = Color3.fromRGB(60, 60, 60),
	backgroundImage = nil,
	page = Screen.WHITE,
	ink = Screen.INK,
	soft = Screen.SOFT,
	line = Color3.fromRGB(215, 215, 220),
	plain = Screen.LIGHT,
	plainInk = Screen.INK,
	accent = Screen.BLUE,
	accentInk = Screen.WHITE,
	go = Screen.GREEN,
	goInk = Screen.WHITE,
	danger = Screen.RED,
	dangerInk = Screen.WHITE,
	turn = Color3.fromRGB(255, 240, 190),
	turnEdge = Screen.GOLD,
	yourTurn = Color3.fromRGB(205, 240, 210),
	seat = Screen.LIGHT,
	resultPage = Color3.fromRGB(255, 246, 214),
	resultEdge = Screen.GOLD,
	waiting = Color3.fromRGB(220, 140, 40),
	toast = Color3.fromRGB(30, 30, 35),
	toastInk = Screen.WHITE,
	radius = 8,
}

local function clear(parent, class)
	for _, child in parent:GetChildren() do
		if child:IsA(class or "Frame") then
			child:Destroy()
		end
	end
end

-- config: title, rules (lines of text), levels (computer player
-- names, easiest first), level (the one picked at first), background,
-- players = { min, max } for games of more than two, colourOf(seat) for
-- the dot by each player's name, and seatNote(view, seat) for a line
-- under it.
function Screen.new(screen, folder, config)
	local Action = folder:WaitForChild("Action")
	local State = folder:WaitForChild("State")
	local me = Players.LocalPlayer
	local ui = { view = nil, lobby = nil }
	local look = {}
	for key, value in LOOK do
		look[key] = value
	end
	for key, value in config.look or {} do
		look[key] = value
	end
	ui.look = look
	local function btn(parent, text, kind, props)
		local b = button(parent, text, look[kind], props)
		b.TextColor3 = look[kind .. "Ink"]
		for _, child in b:GetChildren() do
			if child:IsA("UICorner") then
				child.CornerRadius = UDim.new(0, look.radius)
			end
		end
		return b
	end
	ui.button = btn

	for _, kind in { Enum.CoreGuiType.PlayerList, Enum.CoreGuiType.Backpack, Enum.CoreGuiType.Health } do
		pcall(function()
			game:GetService("StarterGui"):SetCoreGuiEnabled(kind, false)
		end)
	end

	local backdrop = make(if look.backgroundImage then "ImageLabel" else "Frame", {
		Name = "Background",
		Size = UDim2.fromScale(1, 1),
		BackgroundColor3 = look.background,
		BorderSizePixel = 0,
		ZIndex = 0,
	}, screen)
	if look.backgroundImage then
		backdrop.Image = look.backgroundImage
		backdrop.ScaleType = Enum.ScaleType.Crop
	end

	--------------------------------------------------------------------
	-- The menu.

	local menu = card(look, screen, {
		Name = "Menu",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.new(0.5, 0, 0.5, 20),
		Size = UDim2.new(0, 560, 0.86, 0),
	})
	make("UISizeConstraint", { MinSize = Vector2.new(340, 460), MaxSize = Vector2.new(560, 660) }, menu)
	label(menu, { Name = "Title", Position = UDim2.new(0, 28, 0, 20), Size = UDim2.new(1, -56, 0, 44), TextSize = 38, TextColor3 = look.ink, Text = config.title })

	local playButton = btn(menu, "Play", "go", {
		Name = "PlayButton",
		Position = UDim2.new(0, 28, 0, 78),
		Size = UDim2.new(1, -56, 0, 64),
		TextSize = 28,
	})

	local y = 160
	local function optionRow(title, choices, chosen, onPick)
		label(menu, { Position = UDim2.new(0, 28, 0, y), Size = UDim2.new(0, 110, 0, 36), TextSize = 16, TextColor3 = look.ink, Text = title })
		local buttons = {}
		local function paint()
			for i, b in buttons do
				b.BackgroundColor3 = if i == chosen then look.accent else look.plain
				b.TextColor3 = if i == chosen then look.accentInk else look.plainInk
			end
		end
		local holder = make("Frame", { Name = title, Position = UDim2.new(0, 138, 0, y), Size = UDim2.new(1, -166, 0, 36), BackgroundTransparency = 1 }, menu)
		for i, text in choices do
			local b = btn(holder, text, "plain", {
				Name = text,
				Position = UDim2.new((i - 1) / #choices, 3, 0, 0),
				Size = UDim2.new(1 / #choices, -6, 1, 0),
				TextSize = 15,
			})
			b.MouseButton1Click:Connect(function()
				chosen = i
				onPick(i)
				paint()
			end)
			buttons[i] = b
		end
		paint()
		y += 44
	end

	local chosenLevel = config.level or 2
	local chosenPlayers = config.players and 2 or 2
	local chosenFirst = 1
	optionRow("Computer", config.levels, chosenLevel, function(i)
		chosenLevel = i
	end)
	if config.players then
		local counts = {}
		for n = config.players[1], config.players[2] do
			table.insert(counts, tostring(n))
		end
		optionRow("Players", counts, 1, function(i)
			chosenPlayers = config.players[1] + i - 1
		end)
	else
		optionRow("First", { "Me", "Computer" }, 1, function(i)
			chosenFirst = i
		end)
	end

	local rulesButton = btn(menu, "Rules", "plain", {
		Name = "RulesButton",
		Position = UDim2.new(0, 28, 0, y),
		Size = UDim2.new(1, -56, 0, 34),
		TextSize = 15,
	})
	y += 48

	make("Frame", { Position = UDim2.new(0, 28, 0, y), Size = UDim2.new(1, -56, 0, 1), BackgroundColor3 = look.line, BorderSizePixel = 0 }, menu)
	y += 12
	label(menu, { Position = UDim2.new(0, 28, 0, y), Size = UDim2.new(0.6, 0, 0, 26), TextSize = 19, TextColor3 = look.ink, Text = "Online" })
	local stats = label(menu, { Name = "Stats", AnchorPoint = Vector2.new(1, 0), Position = UDim2.new(1, -28, 0, y + 4), Size = UDim2.new(0.4, -28, 0, 20), TextSize = 14, TextColor3 = look.soft, TextXAlignment = Enum.TextXAlignment.Right })
	y += 32
	local seekButton = btn(menu, "Find a game", "accent", {
		Name = "FindButton",
		Position = UDim2.new(0, 28, 0, y),
		Size = UDim2.new(1, -56, 0, 44),
		TextSize = 18,
	})
	y += 54
	local people = make("ScrollingFrame", {
		Name = "People",
		Position = UDim2.new(0, 28, 0, y),
		Size = UDim2.new(1, -56, 1, -(y + 16)),
		BackgroundTransparency = 1,
		ScrollBarThickness = 6,
		CanvasSize = UDim2.new(0, 0, 0, 0),
	}, menu)

	--------------------------------------------------------------------
	-- The game: the area the board goes in, and the side panel.

	local area = make("Frame", {
		Name = "Area",
		Position = UDim2.new(0, 20, 0, 60),
		Size = UDim2.new(1, -360, 1, -80),
		BackgroundTransparency = 1,
		Visible = false,
	}, screen)
	ui.area = area

	local panel = card(look, screen, {
		Name = "Panel",
		AnchorPoint = Vector2.new(1, 0),
		Position = UDim2.new(1, -20, 0, 60),
		Size = UDim2.new(0, 310, 1, -80),
		Visible = false,
	})
	label(panel, { Name = "Title", Position = UDim2.new(0, 18, 0, 12), Size = UDim2.new(1, -36, 0, 30), TextSize = 24, TextColor3 = look.ink, Text = config.title })
	local status = label(panel, {
		Name = "Status",
		Position = UDim2.new(0, 12, 0, 50),
		Size = UDim2.new(1, -24, 0, 48),
		TextSize = 22,
		TextXAlignment = Enum.TextXAlignment.Center,
		BackgroundTransparency = 0,
		BackgroundColor3 = look.seat,
		TextColor3 = look.ink,
		TextWrapped = true,
	})
	round(status, look.radius)
	local sub = label(panel, { Name = "Sub", Position = UDim2.new(0, 18, 0, 102), Size = UDim2.new(1, -36, 0, 40), TextSize = 14, TextColor3 = look.soft, TextWrapped = true, TextYAlignment = Enum.TextYAlignment.Top })
	local seatList = make("Frame", { Name = "Seats", Position = UDim2.new(0, 12, 0, 146), Size = UDim2.new(1, -24, 0, 0), BackgroundTransparency = 1 }, panel)
	ui.extra = make("Frame", { Name = "Extra", Position = UDim2.new(0, 12, 0, 146), Size = UDim2.new(1, -24, 0, 120), BackgroundTransparency = 1 }, panel)
	local panelRules = btn(panel, "Rules", "plain", {
		Name = "RulesButton",
		AnchorPoint = Vector2.new(0, 1),
		Position = UDim2.new(0, 12, 1, -62),
		Size = UDim2.new(1, -24, 0, 36),
		TextSize = 15,
	})
	local leaveButton = btn(panel, "Leave", "danger", {
		Name = "LeaveButton",
		AnchorPoint = Vector2.new(0, 1),
		Position = UDim2.new(0, 12, 1, -14),
		Size = UDim2.new(1, -24, 0, 42),
	})

	-- The card when a game ends.
	-- In the side panel, under the players, so the board stays in view.
	local result = make("Frame", {
		Name = "Result",
		Position = UDim2.new(0, 12, 0, 260),
		Size = UDim2.new(1, -24, 0, 196),
		BackgroundColor3 = look.resultPage,
		Visible = false,
		ZIndex = 10,
	}, panel)
	round(result, look.radius)
	make("UIStroke", { Color = look.resultEdge, Thickness = 2 }, result)
	local resultTitle = label(result, { Name = "Title", Position = UDim2.new(0, 0, 0, 14), Size = UDim2.new(1, 0, 0, 44), TextSize = 28, TextColor3 = look.ink, TextXAlignment = Enum.TextXAlignment.Center, ZIndex = 11 })
	local resultReason = label(result, { Name = "Reason", Position = UDim2.new(0, 14, 0, 60), Size = UDim2.new(1, -28, 0, 44), TextSize = 16, TextColor3 = look.soft, TextWrapped = true, TextXAlignment = Enum.TextXAlignment.Center, ZIndex = 11 })
	local againButton = btn(result, "Play again", "go", { Name = "Again", Position = UDim2.new(0, 14, 1, -66), Size = UDim2.new(0.5, -20, 0, 50), TextSize = 19, ZIndex = 11 })
	local menuButton = btn(result, "Menu", "accent", { Name = "Menu", AnchorPoint = Vector2.new(1, 0), Position = UDim2.new(1, -14, 1, -66), Size = UDim2.new(0.5, -20, 0, 50), TextSize = 19, ZIndex = 11 })

	-- The rules.
	local rules = card(look, screen, {
		Name = "Rules",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.new(0.5, 0, 0.5, 20),
		Size = UDim2.new(0, 600, 0.8, 0),
		Visible = false,
		ZIndex = 20,
	})
	make("UISizeConstraint", { MinSize = Vector2.new(320, 300), MaxSize = Vector2.new(600, 640) }, rules)
	label(rules, { Position = UDim2.new(0, 24, 0, 16), Size = UDim2.new(1, -48, 0, 34), TextSize = 26, TextColor3 = look.ink, Text = "Rules", ZIndex = 21 })
	label(rules, {
		Position = UDim2.new(0, 24, 0, 58),
		Size = UDim2.new(1, -48, 1, -130),
		TextSize = 16,
		TextColor3 = look.ink,
		TextWrapped = true,
		TextYAlignment = Enum.TextYAlignment.Top,
		Text = table.concat(config.rules, "\n\n"),
		ZIndex = 21,
	})
	local rulesClose = btn(rules, "OK", "go", { AnchorPoint = Vector2.new(0.5, 1), Position = UDim2.new(0.5, 0, 1, -16), Size = UDim2.fromOffset(180, 46), ZIndex = 21 })

	-- Messages at the top of the screen.
	local toastBox = make("TextLabel", {
		Name = "Toast",
		AnchorPoint = Vector2.new(0.5, 0),
		Position = UDim2.new(0.5, 0, 0, 12),
		Size = UDim2.fromOffset(520, 40),
		BackgroundColor3 = look.toast,
		BackgroundTransparency = 0.1,
		TextColor3 = look.toastInk,
		TextSize = 17,
		Visible = false,
		ZIndex = 30,
	}, screen)
	round(toastBox, look.radius)
	local toastCount = 0
	function ui.toast(text)
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

	-- config.sounds names a sound for: move, capture, bad, start, win, lose,
	-- draw and challenge. Any left out are quiet.
	local sounds = {}
	for name, id in config.sounds or {} do
		sounds[name] = make("Sound", { Name = name, SoundId = id, Volume = 0.6 }, screen)
	end
	ui.sounds = sounds
	function ui.play(name)
		if sounds[name] then
			sounds[name]:Play()
		end
	end
	local heardChallenges = 0

	--------------------------------------------------------------------
	-- Showing things.

	local function send(action, data)
		Action:FireServer(action, data or {})
	end
	ui.sendAction = send

	function ui.send(move)
		if ui.view then
			send("move", { table = ui.view.id, move = move })
		end
	end

	function ui.myTurn()
		local v = ui.view
		return v ~= nil and v.status == "playing" and v.you ~= 0 and v.turn == v.you
	end

	local resultHidden = false

	local function drawLobby()
		local lobby = ui.lobby
		if not lobby then
			return
		end
		stats.Text = lobby.me.wins .. " / " .. lobby.me.played .. " won"
		if lobby.me.seeking then
			seekButton.Text = "Searching…  Cancel"
			seekButton.BackgroundColor3 = look.waiting
		else
			seekButton.Text = "Find a game"
			seekButton.BackgroundColor3 = look.accent
		end
		clear(people)
		local rowY = 0
		local function row(height)
			local r = make("Frame", { Position = UDim2.new(0, 0, 0, rowY), Size = UDim2.new(1, -8, 0, height), BackgroundColor3 = look.seat }, people)
			round(r, look.radius)
			rowY += height + 6
			return r
		end
		for _, c in lobby.incoming do
			local r = row(46)
			r.BackgroundColor3 = look.turn
			label(r, { Position = UDim2.new(0, 12, 0, 0), Size = UDim2.new(0.55, 0, 1, 0), TextSize = 15, TextColor3 = look.ink, TextWrapped = true, Text = c.name .. " wants to play" })
			local yes = btn(r, "Play", "go", { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -82, 0.5, 0), Size = UDim2.fromOffset(70, 32), TextSize = 15 })
			local no = btn(r, "No", "plain", { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -6, 0.5, 0), Size = UDim2.fromOffset(70, 32), TextSize = 15 })
			yes.MouseButton1Click:Connect(function()
				send("answer", { id = c.id, accept = true })
			end)
			no.MouseButton1Click:Connect(function()
				send("answer", { id = c.id, accept = false })
			end)
		end
		for _, c in lobby.outgoing do
			local r = row(46)
			label(r, { Position = UDim2.new(0, 12, 0, 0), Size = UDim2.new(0.65, 0, 1, 0), TextSize = 15, TextColor3 = look.ink, Text = "Sent to " .. c.name })
			local cancel = btn(r, "Cancel", "plain", { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -6, 0.5, 0), Size = UDim2.fromOffset(80, 32), TextSize = 15 })
			cancel.MouseButton1Click:Connect(function()
				send("answer", { id = c.id, accept = false })
			end)
		end
		for _, p in lobby.players do
			local r = row(46)
			label(r, { Position = UDim2.new(0, 12, 0, 4), Size = UDim2.new(0.6, 0, 0, 22), TextSize = 16, TextColor3 = look.ink, Text = p.name })
			label(r, { Position = UDim2.new(0, 12, 0, 24), Size = UDim2.new(0.6, 0, 0, 18), TextSize = 13, TextColor3 = look.soft, Text = p.status .. " · " .. p.wins .. " wins" })
			if p.free then
				local ask = btn(r, "Play", "accent", { AnchorPoint = Vector2.new(1, 0.5), Position = UDim2.new(1, -6, 0.5, 0), Size = UDim2.fromOffset(80, 32), TextSize = 15 })
				ask.MouseButton1Click:Connect(function()
					send("challenge", { to = p.id })
				end)
			end
		end
		if #lobby.players == 0 and #lobby.incoming == 0 then
			local r = row(54)
			r.BackgroundTransparency = 1
			label(r, { Position = UDim2.new(0, 2, 0, 0), Size = UDim2.new(1, -4, 1, 0), TextSize = 14, TextColor3 = look.soft, TextWrapped = true, Text = "No one else here" })
		end
		people.CanvasSize = UDim2.new(0, 0, 0, rowY)
	end

	local function drawSeats()
		local v = ui.view
		clear(seatList)
		for i, seat in v.seats do
			local r = make("Frame", {
				Name = "Seat" .. i,
				Position = UDim2.new(0, 0, 0, (i - 1) * 54),
				Size = UDim2.new(1, 0, 0, 48),
				BackgroundColor3 = if v.status == "playing" and v.turn == i then look.turn else look.seat,
			}, seatList)
			round(r, look.radius)
			if v.status == "playing" and v.turn == i then
				make("UIStroke", { Color = look.turnEdge, Thickness = 2 }, r)
			end
			local picture = config.pictureOf and config.pictureOf(i)
			local dot = make(if picture then "ImageLabel" else "Frame", { AnchorPoint = Vector2.new(0, 0.5), Position = UDim2.new(0, 8, 0.5, 0), Size = UDim2.fromOffset(30, 30), BackgroundColor3 = config.colourOf and config.colourOf(i) or Screen.GREY }, r)
			if picture then
				dot.Image = picture
				dot.BackgroundTransparency = 1
			else
				round(dot, 15)
				make("UIStroke", { Color = Color3.fromRGB(0, 0, 0), Thickness = 1, Transparency = 0.6 }, dot)
			end
			local name = seat.name .. (if i == v.you then " (you)" else "") .. (if seat.bot then " · computer" else "")
			label(r, { Name = "PlayerName", Position = UDim2.new(0, 46, 0, 4), Size = UDim2.new(1, -52, 0, 22), TextSize = 16, TextColor3 = look.ink, Text = name })
			label(r, { Name = "Note", Position = UDim2.new(0, 46, 0, 25), Size = UDim2.new(1, -52, 0, 18), TextSize = 13, TextColor3 = look.soft, Text = config.seatNote and config.seatNote(v, i) or "" })
		end
		seatList.Size = UDim2.new(1, -24, 0, #v.seats * 54)
		ui.extra.Position = UDim2.new(0, 12, 0, 150 + #v.seats * 54)
		result.Position = UDim2.new(0, 12, 0, 156 + #v.seats * 54)
	end

	local function drawPanel()
		local v = ui.view
		local turnName = v.seats[v.turn] and v.seats[v.turn].name or ""
		ui.extra.Visible = true
		if v.status == "playing" then
			if ui.myTurn() then
				status.Text = "Your turn"
				status.BackgroundColor3 = look.yourTurn
			elseif v.you == 0 then
				status.Text = turnName .. "'s turn"
				status.BackgroundColor3 = look.seat
			else
				status.Text = (if v.seats[v.turn] and v.seats[v.turn].bot then turnName .. " is thinking…" else "Waiting for " .. turnName)
				status.BackgroundColor3 = look.seat
			end
		else
			status.Text = "Game over"
			status.BackgroundColor3 = look.seat
			ui.extra.Visible = false
		end
		sub.Text = (if v.vsBot then "Computer" else "Online") .. (if ui.subText then " · " .. ui.subText else "")
		drawSeats()
		leaveButton.Text = if v.status == "playing" then "Leave" else "Back to menu"
		result.Visible = v.status == "over" and not resultHidden
		if v.status == "over" then
			if v.you ~= 0 and v.winner == v.you then
				resultTitle.Text = "You win!"
			elseif v.winner == 0 then
				resultTitle.Text = "It's a draw"
			elseif v.you ~= 0 then
				local winner = v.seats[v.winner]
				resultTitle.Text = if winner and winner.bot and #v.seats == 2
					then "The computer wins"
					else (winner and winner.name or "Someone") .. " wins"
			end
			resultReason.Text = v.reason
			local asked = v.you ~= 0 and v.rematch[v.you]
			local theyAsked = false
			for i, wants in v.rematch do
				if wants and i ~= v.you then
					theyAsked = true
				end
			end
			againButton.Text = if asked then "Asked…" elseif theyAsked then "Accept" else "Play again"
		end
	end

	local function showPage()
		local inGame = ui.view ~= nil
		menu.Visible = not inGame
		area.Visible = inGame
		panel.Visible = inGame
		if not inGame then
			result.Visible = false
			drawLobby()
		end
	end

	--------------------------------------------------------------------
	-- Clicks.

	playButton.MouseButton1Click:Connect(function()
		send("playBot", {
			level = chosenLevel,
			opponents = chosenPlayers - 1,
			first = if chosenFirst == 2 then "computer" else "me",
		})
	end)
	seekButton.MouseButton1Click:Connect(function()
		if ui.lobby and ui.lobby.me.seeking then
			send("cancelSeek")
		else
			send("seek")
		end
	end)
	for _, b in { rulesButton, panelRules } do
		b.MouseButton1Click:Connect(function()
			rules.Visible = true
		end)
	end
	rulesClose.MouseButton1Click:Connect(function()
		rules.Visible = false
	end)
	local function toMenu()
		send("leave")
		ui.view = nil
		if ui.onLeft then
			ui.onLeft()
		end
		showPage()
	end
	leaveButton.MouseButton1Click:Connect(toMenu)
	menuButton.MouseButton1Click:Connect(toMenu)
	againButton.MouseButton1Click:Connect(function()
		send("rematch")
	end)

	--------------------------------------------------------------------
	-- What the server says.

	function ui.redraw()
		if ui.view then
			drawPanel()
		end
	end

	function ui.start()
		State.OnClientEvent:Connect(function(kind, data)
			if kind == "lobby" then
				ui.lobby = data
				if #data.incoming > heardChallenges then
					ui.play("challenge")
				end
				heardChallenges = #data.incoming
				if not ui.view then
					showPage()
				end
			elseif kind == "table" then
				local fresh = not ui.view or ui.view.id ~= data.id
				local wasOver = ui.view and ui.view.status == "over"
				if fresh then
					resultHidden = false
				end
				ui.view = data
				showPage()
				if ui.onTable then
					ui.onTable(data, fresh)
				end
				drawPanel()
				if fresh and data.status == "playing" then
					ui.play("start")
				end
				if data.status == "over" and not wasOver and not fresh then
					if data.you ~= 0 and data.winner == data.you then
						ui.play("win")
					elseif data.winner == 0 or data.you == 0 then
						ui.play("draw")
					else
						ui.play("lose")
					end
				end
			elseif kind == "left" then
				ui.view = nil
				showPage()
			elseif kind == "toast" then
				ui.toast(data)
			end
		end)
		showPage()
		send("hello")
	end

	return ui
end

return Screen
