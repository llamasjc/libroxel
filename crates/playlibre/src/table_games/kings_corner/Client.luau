-- The Kings in the Corner screen, after GNOME AisleRiot: its baize, slots
-- and "Anglo" cards, with the draw pile in the middle, four side piles
-- round it and the corners for the Kings. Your hand is along the bottom.
-- Click a card, then the pile to put it on. To move a whole side pile,
-- click it, then the pile it goes on.

local storage = game:GetService("ReplicatedStorage")

local folder = storage:WaitForChild("TableGame")
local Screen = require(folder:WaitForChild("Screen"))
local Rules = require(folder:WaitForChild("Rules"))
local make, round, label = Screen.make, Screen.round, Screen.label

local SLOT = Color3.fromRGB(87, 129, 50)
local SLOT_EDGE = Color3.fromRGB(48, 88, 13)
local ON_BAIZE = Color3.fromRGB(214, 232, 200)
local PICK = Color3.fromRGB(53, 132, 228)
local INK = Color3.fromRGB(46, 52, 54)
local SEAT_COLOURS = {
	Color3.fromRGB(53, 132, 228),
	Color3.fromRGB(224, 27, 36),
	Color3.fromRGB(246, 211, 45),
	Color3.fromRGB(145, 65, 172),
}
local SUIT_LETTERS = { "s", "h", "d", "c" }
local RANK_LETTERS = { "a", "2", "3", "4", "5", "6", "7", "8", "9", "10", "j", "q", "k" }

local ui = Screen.new(script.Parent, folder, {
	title = "Kings in the Corner",
	levels = Rules.LEVELS,
	level = 2,
	players = { 2, 4 },
	look = {
		background = Color3.fromRGB(0, 79, 15),
		backgroundImage = "playlibre://baize",
		page = Color3.fromRGB(246, 245, 244),
		ink = INK,
		soft = Color3.fromRGB(120, 124, 126),
		line = Color3.fromRGB(205, 199, 194),
		plain = Color3.fromRGB(222, 221, 218),
		plainInk = INK,
		accent = PICK,
		go = Color3.fromRGB(46, 194, 126),
		danger = Color3.fromRGB(224, 27, 36),
		seat = Color3.fromRGB(255, 255, 255),
		turn = Color3.fromRGB(255, 255, 255),
		turnEdge = PICK,
		yourTurn = Color3.fromRGB(220, 244, 230),
		resultPage = Color3.fromRGB(255, 255, 255),
		resultEdge = Color3.fromRGB(46, 194, 126),
		waiting = Color3.fromRGB(229, 165, 10),
		toast = Color3.fromRGB(36, 31, 49),
		radius = 6,
	},
	sounds = {
		move = "playlibre://cards/click",
		bad = "playlibre://cards/slide",
		win = "playlibre://cards/win",
		lose = "playlibre://cards/lose",
		draw = "playlibre://cards/lose",
	},
	colourOf = function(seat)
		return SEAT_COLOURS[seat]
	end,
	seatNote = function(v, seat)
		local n = v.game.counts[seat] or 0
		return n .. (if n == 1 then " card left" else " cards left")
	end,
	rules = {
		"7 cards each. Four piles start face up around the deck.",
		"You draw at the start of your turn. Play one lower in the other colour, like a black 6 on a red 7. Aces are low.",
		"Only Kings go in the corners. Any card fills an empty side. Move a whole pile if its bottom card fits.",
		"First to empty their hand wins. If the deck runs out and no one can play, fewest cards wins.",
	},
})

-- The table keeps a 5:4 shape inside the space it is given.
local tableFrame = make("Frame", {
	Name = "Table",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5, 0.5),
	Size = UDim2.fromScale(1, 1),
	BackgroundTransparency = 1,
}, ui.area)
make("UIAspectRatioConstraint", { AspectRatio = 1.25 }, tableFrame)

local CARD_W, CARD_H = 0.088, 0.17
local PEEK = 0.04 -- how far the top card sits below the bottom one

-- A card: its face, or its back.
local function newCard(parent, name)
	local c = make("ImageButton", {
		Name = name,
		Image = "playlibre://card-back",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Size = UDim2.fromScale(CARD_W, CARD_H),
		BackgroundTransparency = 1,
		AutoButtonColor = false,
		ZIndex = 3,
	}, parent)
	local rim = make("Frame", { Name = "Rim", Size = UDim2.fromScale(1, 1), BackgroundTransparency = 1, ZIndex = 4 }, c)
	round(rim, 6)
	make("UIStroke", { Name = "Edge", Color = PICK, Thickness = 3, Enabled = false }, rim)
	return c
end

local function setFace(c, card)
	c.Image = "playlibre://card-" .. RANK_LETTERS[Rules.rankOf(card)] .. SUIT_LETTERS[Rules.suitOf(card)]
	c.ImageColor3 = Color3.fromRGB(255, 255, 255)
end

local function mark(c, on, colour)
	c.Rim.Edge.Enabled = on
	c.Rim.Edge.Color = colour or PICK
end

-- Where each pile sits: the sides round the middle, the corners diagonal.
local SPOTS = {
	{ 0.5, 0.11 }, -- north
	{ 0.64, 0.34 }, -- east
	{ 0.5, 0.57 }, -- south
	{ 0.36, 0.34 }, -- west
	{ 0.76, 0.11 }, -- north-east
	{ 0.76, 0.57 }, -- south-east
	{ 0.24, 0.57 }, -- south-west
	{ 0.24, 0.11 }, -- north-west
}
-- The corners stand out a little more to the sides.

local piles = {}
for i, spot in SPOTS do
	local slot = make("TextButton", {
		Name = "Pile" .. i,
		Text = "",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(spot[1], spot[2]),
		Size = UDim2.fromScale(CARD_W, CARD_H),
		BackgroundColor3 = SLOT,
		AutoButtonColor = false,
		ZIndex = 2,
	}, tableFrame)
	round(slot, 6)
	local rim = make("Frame", { Name = "Rim", Size = UDim2.fromScale(1, 1), BackgroundTransparency = 1, ZIndex = 2 }, slot)
	round(rim, 6)
	make("UIStroke", { Color = SLOT_EDGE, Thickness = 1.5 }, rim)
	local glow = make("UIStroke", { Color = Color3.fromRGB(255, 255, 255), Thickness = 3, Enabled = false }, rim)
	local hint = label(slot, {
		Name = "Hint",
		Size = UDim2.fromScale(1, 1),
		TextSize = 26,
		TextColor3 = SLOT_EDGE,
		TextXAlignment = Enum.TextXAlignment.Center,
		Text = if Rules.isCorner(i) then "K" else "",
		ZIndex = 2,
	})
	local bottom = newCard(tableFrame, "Bottom" .. i)
	bottom.Position = UDim2.fromScale(spot[1], spot[2])
	local top = newCard(tableFrame, "Top" .. i)
	top.Position = UDim2.fromScale(spot[1], spot[2] + PEEK)
	top.ZIndex = 5
	for _, child in top:GetChildren() do
		if child:IsA("GuiObject") then
			child.ZIndex = 6
		end
	end
	local count = label(tableFrame, {
		Name = "Count" .. i,
		AnchorPoint = Vector2.new(0.5, 0),
		Position = UDim2.fromScale(spot[1], spot[2] + PEEK + CARD_H / 2 + 0.005),
		Size = UDim2.fromScale(CARD_W, 0.03),
		TextSize = 13,
		TextColor3 = ON_BAIZE,
		TextXAlignment = Enum.TextXAlignment.Center,
	})
	piles[i] = { spot = spot, slot = slot, glow = glow, hint = hint, bottom = bottom, top = top, count = count }
end

-- The draw pile in the middle.
local stock = newCard(tableFrame, "Stock")
stock.Position = UDim2.fromScale(0.5, 0.34)
local stockCount = label(tableFrame, {
	Name = "StockCount",
	AnchorPoint = Vector2.new(0.5, 0),
	Position = UDim2.fromScale(0.5, 0.34 + CARD_H / 2 + 0.005),
	Size = UDim2.fromScale(0.2, 0.03),
	TextSize = 13,
	TextColor3 = ON_BAIZE,
	TextXAlignment = Enum.TextXAlignment.Center,
})

-- Your hand.
local handFrame = make("Frame", {
	Name = "Hand",
	Position = UDim2.fromScale(0.02, 0.76),
	Size = UDim2.fromScale(0.96, 0.23),
	BackgroundTransparency = 1,
}, tableFrame)
local handCards = {}

-- The End turn button and what just happened, in the side panel.
local endButton = ui.button(ui.extra, "End turn", "go", {
	Name = "EndTurn",
	Position = UDim2.new(0, 0, 0, 0),
	Size = UDim2.new(1, 0, 0, 52),
	TextSize = 22,
})
local logLine = label(ui.extra, {
	Name = "Log",
	Position = UDim2.new(0, 2, 0, 60),
	Size = UDim2.new(1, -4, 0, 44),
	TextSize = 14,
	TextColor3 = ui.look.soft,
	TextWrapped = true,
	TextYAlignment = Enum.TextYAlignment.Top,
})

local shownHand = {}
local selected = nil -- { kind = "card", card = n } or { kind = "pile", from = p }

local function targetsFor(v)
	local out = {}
	if not selected or not ui.myTurn() then
		return out
	end
	for pile = 1, 8 do
		if selected.kind == "card" and Rules.canPlace(v.game.piles, pile, selected.card) then
			out[pile] = true
		elseif selected.kind == "pile" and Rules.canMove(v.game.piles, selected.from, pile) then
			out[pile] = true
		end
	end
	return out
end

local function draw()
	local v = ui.view
	if not v then
		return
	end
	local g = v.game
	local targets = targetsFor(v)
	for i, p in piles do
		local cards = g.piles[i]
		p.bottom.Visible = #cards >= 2
		p.top.Visible = #cards >= 1
		p.hint.Visible = #cards == 0
		if #cards >= 1 then
			setFace(p.top, cards[#cards])
			p.top.Position = UDim2.fromScale(p.spot[1], p.spot[2] + (if #cards >= 2 then PEEK else 0))
		end
		if #cards >= 2 then
			setFace(p.bottom, cards[1])
		end
		p.count.Text = if #cards >= 2 then tostring(#cards) else ""
		p.glow.Enabled = targets[i] == true and #cards == 0
		mark(p.top, (selected and selected.kind == "pile" and selected.from == i) or (targets[i] == true and #cards > 0), if targets[i] then Color3.fromRGB(255, 255, 255) else PICK)
	end
	stock.Visible = g.stock > 0
	stockCount.Text = if g.stock > 0 then tostring(g.stock) else ""

	-- The hand, fanned out to fit.
	local hand = g.hand
	shownHand = hand
	table.sort(hand, function(a, b)
		if Rules.rankOf(a) ~= Rules.rankOf(b) then
			return Rules.rankOf(a) > Rules.rankOf(b)
		end
		return a < b
	end)
	for i = #handCards + 1, #hand do
		local c = newCard(handFrame, "Card" .. i)
		c.Size = UDim2.fromScale(CARD_W / 0.96, CARD_H / 0.23)
		local index = i
		c.MouseButton1Click:Connect(function()
			local card = shownHand[index]
			if not card or not ui.myTurn() then
				return
			end
			if selected and selected.kind == "card" and selected.card == card then
				selected = nil
			else
				selected = { kind = "card", card = card }
			end
			draw()
		end)
		handCards[i] = c
	end
	local n = #hand
	local step = math.min(CARD_W / 0.96 + 0.01, if n > 1 then (1 - CARD_W / 0.96) / (n - 1) else 0)
	local width = (n - 1) * step + CARD_W / 0.96
	local playable = {}
	if ui.myTurn() then
		for _, card in hand do
			for pile = 1, 8 do
				if Rules.canPlace(g.piles, pile, card) then
					playable[card] = true
				end
			end
		end
	end
	for i, c in handCards do
		local card = hand[i]
		c.Visible = card ~= nil
		if card then
			setFace(c, card)
			local lifted = selected and selected.kind == "card" and selected.card == card
			c.Position = UDim2.fromScale(0.5 - width / 2 + CARD_W / 0.96 / 2 + (i - 1) * step, if lifted then 0.42 else 0.56)
			c.ZIndex = 10 + i
			c.Rim.ZIndex = 10 + i
			mark(c, lifted or card == g.drew, if lifted then PICK else Color3.fromRGB(246, 211, 45))
			c.ImageColor3 = if ui.myTurn() and not playable[card] then Color3.fromRGB(205, 205, 205) else Color3.fromRGB(255, 255, 255)
		end
	end

	endButton.Visible = ui.myTurn()
	if v.status == "playing" then
		if ui.myTurn() and g.drew ~= 0 and not g.played then
			logLine.Text = "Drew " .. Rules.name(g.drew)
		elseif g.log ~= "" then
			local who = v.seats[v.turn] and v.seats[v.turn].name or ""
			logLine.Text = (if ui.myTurn() then "You" else who) .. ": " .. g.log
		else
			logLine.Text = ""
		end
	else
		logLine.Text = ""
	end
end

local lastPlayed = nil

ui.onTable = function(v, fresh)
	if fresh or not ui.myTurn() then
		selected = nil
	end
	-- A card that's been played isn't picked any more.
	if selected and selected.kind == "card" and not table.find(v.game.hand, selected.card) then
		selected = nil
	end
	if selected and selected.kind == "pile" and #v.game.piles[selected.from] == 0 then
		selected = nil
	end
	ui.subText = if v.game.stock > 0 then v.game.stock .. " in deck" else "deck empty"
	local played = v.game.counts and table.concat(v.game.counts, ",") .. ":" .. v.game.stock .. ":" .. #v.game.hand
	draw()
	if not fresh and played ~= lastPlayed then
		ui.play("move")
	end
	lastPlayed = played
end

ui.onLeft = function()
	selected = nil
end

local function clickPile(i)
	local v = ui.view
	if not v or not ui.myTurn() then
		return
	end
	local g = v.game
	if selected and selected.kind == "card" then
		if Rules.canPlace(g.piles, i, selected.card) then
			ui.send({ kind = "card", card = selected.card, pile = i })
			selected = nil
		elseif Rules.isCorner(i) and #g.piles[i] == 0 then
			ui.play("bad")
			ui.toast("Only a King can start a corner")
		else
			ui.play("bad")
			ui.toast("One lower, other colour")
		end
		draw()
		return
	end
	if selected and selected.kind == "pile" then
		if selected.from == i then
			selected = nil
		elseif Rules.canMove(g.piles, selected.from, i) then
			ui.send({ kind = "pile", from = selected.from, to = i })
			selected = nil
		else
			ui.play("bad")
			ui.toast("Doesn't fit")
		end
		draw()
		return
	end
	if Rules.isCorner(i) or #g.piles[i] == 0 then
		ui.toast("Pick a card first")
		return
	end
	local anywhere = false
	for to = 1, 8 do
		if Rules.canMove(g.piles, i, to) then
			anywhere = true
		end
	end
	if anywhere then
		selected = { kind = "pile", from = i }
	else
		ui.toast("Pick a card first")
	end
	draw()
end

for i, p in piles do
	for _, b in { p.slot, p.bottom, p.top } do
		b.MouseButton1Click:Connect(function()
			clickPile(i)
		end)
	end
end

endButton.MouseButton1Click:Connect(function()
	if ui.myTurn() then
		selected = nil
		ui.send({ kind = "end" })
	end
end)

pcall(function()
	tableFrame:GetPropertyChangedSignal("AbsoluteSize"):Connect(draw)
end)
ui.start()
