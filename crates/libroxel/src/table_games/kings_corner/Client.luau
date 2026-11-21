-- The Kings in the Corner screen: a green card table with the draw pile in the
-- middle, four side piles round it and the corners for the Kings. Your
-- hand is along the bottom. Click a card, then the pile to put it on. To
-- move a whole side pile, click it, then the pile it goes on.

local storage = game:GetService("ReplicatedStorage")

local folder = storage:WaitForChild("TableGame")
local Screen = require(folder:WaitForChild("Screen"))
local Rules = require(folder:WaitForChild("Rules"))
local make, round, label = Screen.make, Screen.round, Screen.label

local FELT = Color3.fromRGB(28, 110, 62)
local RED_INK = Color3.fromRGB(200, 30, 35)
local BLACK_INK = Color3.fromRGB(25, 25, 30)
local SEAT_COLOURS = {
	Color3.fromRGB(45, 110, 200),
	Color3.fromRGB(205, 60, 55),
	Color3.fromRGB(240, 170, 40),
	Color3.fromRGB(130, 80, 190),
}

local ui = Screen.new(script.Parent, folder, {
	title = "Kings in the Corner",
	tagline = "Kings in the corners: be the first to play every card",
	levels = Rules.LEVELS,
	level = 2,
	players = { 2, 4 },
	background = Color3.fromRGB(20, 80, 45),
	colourOf = function(seat)
		return SEAT_COLOURS[seat]
	end,
	seatNote = function(v, seat)
		local n = v.game.counts[seat] or 0
		return n .. (if n == 1 then " card left" else " cards left")
	end,
	rules = {
		"Everyone gets 7 cards. Four cards go face up beside the draw pile, one on each side. The four corners start empty.",
		"On your turn you draw a card first (that happens by itself). Then play as many cards as you like: a card goes on a pile if it is one lower and the other colour, like a black 6 on a red 7. Aces are lowest.",
		"Only a King can start a corner. Any card can go in an empty side spot. You can also move a whole side pile onto another pile when its bottom card fits there, which frees that side spot.",
		"Press End turn when you're done. The first to play every card wins. If the draw pile runs out and nobody can play for a whole round, the fewest cards wins.",
		"Click a card in your hand, then the pile to put it on. Piles it can go on light up. To move a pile, click it, then where it goes.",
	},
})

-- The table keeps a 5:4 shape inside the space it is given.
local tableFrame = make("Frame", {
	Name = "Table",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5, 0.5),
	Size = UDim2.fromScale(1, 1),
	BackgroundColor3 = FELT,
}, ui.area)
make("UIAspectRatioConstraint", { AspectRatio = 1.25 }, tableFrame)
round(tableFrame, 24)
make("UIStroke", { Color = Color3.fromRGB(90, 55, 25), Thickness = 10 }, tableFrame)

local CARD_W, CARD_H = 0.095, 0.17
local PEEK = 0.04 -- how far the top card sits below the bottom one

-- A face-up card, or the back of one.
local function newCard(parent, name)
	local c = make("TextButton", {
		Name = name,
		Text = "",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Size = UDim2.fromScale(CARD_W, CARD_H),
		BackgroundColor3 = Screen.WHITE,
		AutoButtonColor = false,
		ZIndex = 3,
	}, parent)
	round(c, 6)
	-- Buttons outline their text with a stroke, so the card's edge is on a frame over it.
	local rim = make("Frame", { Name = "Rim", Size = UDim2.fromScale(1, 1), BackgroundTransparency = 1, ZIndex = 4 }, c)
	round(rim, 6)
	local stroke = make("UIStroke", { Name = "Edge", Color = Color3.fromRGB(0, 0, 0), Thickness = 1, Transparency = 0.6 }, rim)
	label(c, { Name = "Corner", Position = UDim2.new(0, 5, 0, 2), Size = UDim2.new(1, -10, 0.3, 0), TextSize = 18, ZIndex = 4 })
	label(c, { Name = "Suit", Position = UDim2.fromScale(0, 0.28), Size = UDim2.fromScale(1, 0.6), TextSize = 40, TextXAlignment = Enum.TextXAlignment.Center, ZIndex = 4 })
	local back = make("Frame", { Name = "Back", Position = UDim2.fromScale(0.08, 0.06), Size = UDim2.fromScale(0.84, 0.88), BackgroundColor3 = Color3.fromRGB(160, 40, 45), Visible = false, ZIndex = 4 }, c)
	round(back, 4)
	make("UIStroke", { Color = Color3.fromRGB(240, 220, 200), Thickness = 2 }, back)
	return c, stroke
end

local function setFace(c, card, textSize)
	local red = Rules.isRed(card)
	c.Corner.Text = Rules.RANKS[Rules.rankOf(card)] .. Rules.SUITS[Rules.suitOf(card)]
	c.Corner.TextColor3 = if red then RED_INK else BLACK_INK
	c.Corner.TextSize = math.max(10, math.floor(textSize * 0.17))
	c.Suit.Text = if Rules.rankOf(card) >= 11 then Rules.RANKS[Rules.rankOf(card)] .. Rules.SUITS[Rules.suitOf(card)] else Rules.SUITS[Rules.suitOf(card)]
	c.Suit.TextColor3 = c.Corner.TextColor3
	c.Suit.TextSize = math.max(14, math.floor(textSize * (if Rules.rankOf(card) >= 11 then 0.28 else 0.42)))
	c.Back.Visible = false
	c.BackgroundColor3 = Screen.WHITE
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
		Position = UDim2.fromScale(spot[1], spot[2] + PEEK / 2),
		Size = UDim2.fromScale(CARD_W + 0.012, CARD_H + PEEK + 0.012),
		BackgroundColor3 = Color3.fromRGB(255, 255, 255),
		BackgroundTransparency = 0.88,
		AutoButtonColor = false,
		ZIndex = 2,
	}, tableFrame)
	round(slot, 8)
	local rim = make("Frame", { Name = "Rim", Size = UDim2.fromScale(1, 1), BackgroundTransparency = 1, ZIndex = 2 }, slot)
	round(rim, 8)
	local glow = make("UIStroke", { Color = Color3.fromRGB(255, 230, 90), Thickness = 5, Enabled = false }, rim)
	local hint = label(slot, {
		Name = "Hint",
		Size = UDim2.fromScale(1, 1),
		TextSize = 14,
		TextColor3 = Color3.fromRGB(200, 235, 210),
		TextWrapped = true,
		TextXAlignment = Enum.TextXAlignment.Center,
		Text = if Rules.isCorner(i) then "King\ncorner" else "Empty",
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
		TextColor3 = Color3.fromRGB(215, 240, 220),
		TextXAlignment = Enum.TextXAlignment.Center,
	})
	piles[i] = { slot = slot, glow = glow, hint = hint, bottom = bottom, top = top, count = count }
end

-- The draw pile in the middle.
local stock = newCard(tableFrame, "Stock")
stock.Position = UDim2.fromScale(0.5, 0.34 + PEEK / 2)
stock.Back.Visible = true
local stockCount = label(tableFrame, {
	Name = "StockCount",
	AnchorPoint = Vector2.new(0.5, 0),
	Position = UDim2.fromScale(0.5, 0.34 + PEEK / 2 + CARD_H / 2 + 0.005),
	Size = UDim2.fromScale(0.2, 0.03),
	TextSize = 13,
	TextColor3 = Color3.fromRGB(215, 240, 220),
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
local endButton = Screen.button(ui.extra, "End turn", Screen.GREEN, {
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
	TextColor3 = Screen.SOFT,
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
	local textSize = tableFrame.AbsoluteSize.Y * CARD_H
	local targets = targetsFor(v)
	for i, p in piles do
		local cards = g.piles[i]
		p.bottom.Visible = #cards >= 2
		p.top.Visible = #cards >= 1
		p.hint.Visible = #cards == 0
		if #cards >= 1 then
			setFace(p.top, cards[#cards], textSize)
		end
		if #cards >= 2 then
			setFace(p.bottom, cards[1], textSize)
		end
		p.count.Text = if #cards >= 2 then #cards .. " cards" else ""
		p.glow.Enabled = targets[i] == true
		p.top.Rim.Edge.Color = if selected and selected.kind == "pile" and selected.from == i then Color3.fromRGB(255, 200, 40) else Color3.fromRGB(0, 0, 0)
		p.top.Rim.Edge.Thickness = if selected and selected.kind == "pile" and selected.from == i then 4 else 1
		p.top.Rim.Edge.Transparency = if selected and selected.kind == "pile" and selected.from == i then 0 else 0.6
	end
	stock.Visible = g.stock > 0
	stockCount.Text = if g.stock > 0 then g.stock .. " to draw" else "Draw pile empty"

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
			setFace(c, card, textSize)
			local lifted = selected and selected.kind == "card" and selected.card == card
			c.Position = UDim2.fromScale(0.5 - width / 2 + CARD_W / 0.96 / 2 + (i - 1) * step, if lifted then 0.42 else 0.56)
			c.ZIndex = 10 + i
			for _, child in c:GetChildren() do
				if child:IsA("GuiObject") then
					child.ZIndex = 10 + i
				end
			end
			c.Rim.Edge.Color = if lifted then Color3.fromRGB(255, 200, 40) elseif card == g.drew then Color3.fromRGB(60, 140, 255) else Color3.fromRGB(0, 0, 0)
			c.Rim.Edge.Thickness = if lifted or card == g.drew then 3 else 1
			c.Rim.Edge.Transparency = if lifted or card == g.drew then 0 else 0.6
			c.BackgroundColor3 = if ui.myTurn() and not playable[card] then Color3.fromRGB(225, 225, 225) else Screen.WHITE
		end
	end

	endButton.Visible = ui.myTurn()
	if v.status == "playing" then
		if ui.myTurn() and g.drew ~= 0 and not g.played then
			logLine.Text = "You drew the " .. Rules.name(g.drew) .. " (outlined in blue)."
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
	ui.subText = if v.game.stock > 0 then v.game.stock .. " cards to draw" else "the draw pile is empty"
	draw()
	if not fresh then
		ui.sounds.move:Play()
	end
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
			ui.toast("Only a King can start a corner")
		else
			ui.toast("It has to be one lower and the other colour")
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
			ui.toast("The bottom card of that pile doesn't fit there")
		end
		draw()
		return
	end
	if Rules.isCorner(i) or #g.piles[i] == 0 then
		ui.toast("Pick a card from your hand first")
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
		ui.toast("Pick a card from your hand, then click this pile")
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
