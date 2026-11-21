-- The checkers screen: a wooden board. Click one of your pieces (the ones
-- that can move are outlined), then the square to move it to. For a
-- double jump, click each landing square in turn.

local storage = game:GetService("ReplicatedStorage")
local TweenService = game:GetService("TweenService")

local folder = storage:WaitForChild("TableGame")
local Screen = require(folder:WaitForChild("Screen"))
local Rules = require(folder:WaitForChild("Rules"))
local make, round, label = Screen.make, Screen.round, Screen.label

local LIGHT = Color3.fromRGB(240, 217, 181)
local DARK = Color3.fromRGB(150, 98, 60)
local LAST = Color3.fromRGB(190, 150, 70)
local PICKED = Color3.fromRGB(110, 170, 90)
local PIECE = {
	[1] = Color3.fromRGB(40, 40, 45),
	[2] = Color3.fromRGB(205, 45, 45),
}
local EDGE = {
	[1] = Color3.fromRGB(95, 95, 105),
	[2] = Color3.fromRGB(245, 120, 110),
}

local ui = Screen.new(script.Parent, folder, {
	title = "Checkers",
	tagline = "Jump your way across the board and take every piece",
	levels = Rules.LEVELS,
	level = 2,
	background = Color3.fromRGB(70, 110, 75),
	colourOf = function(seat)
		return PIECE[seat]
	end,
	seatNote = function(v, seat)
		local men, kings = 0, 0
		for _, piece in v.game.board do
			if Rules.ownerOf(piece) == seat then
				if Rules.isKing(piece) then
					kings += 1
				else
					men += 1
				end
			end
		end
		return (if seat == 1 then "Black" else "Red") .. " · " .. men + kings .. " pieces" .. (if kings > 0 then ", " .. kings .. " kings" else "")
	end,
	rules = {
		"Black moves first, then you take turns. Pieces move one square forward on a slant, onto the dark squares only.",
		"Jump over an enemy piece next to you, onto the empty square behind it, to take it. If you can jump, you must, and if you land where you can jump again, keep going.",
		"Reach the far side and your piece is crowned a king (it gets a crown). Kings move and jump forwards and backwards.",
		"You win when the other player has no pieces left, or can't move. If 40 moves each go by with nothing taken and no plain piece moved, it's a draw.",
		"Click your piece (the ones that can move have a gold outline), then the square to go to. Green dots show where it can go.",
	},
})

local board = make("Frame", {
	Name = "Board",
	AnchorPoint = Vector2.new(0.5, 0.5),
	Position = UDim2.fromScale(0.5, 0.5),
	Size = UDim2.fromScale(1, 1),
	BackgroundColor3 = Color3.fromRGB(90, 55, 30),
}, ui.area)
make("UIAspectRatioConstraint", { AspectRatio = 1 }, board)
round(board, 6)
make("UIStroke", { Color = Color3.fromRGB(60, 35, 18), Thickness = 8 }, board)

local flipped = false
local squares = {}
for sq = 1, 64 do
	local row, col = Rules.rowOf(sq), Rules.colOf(sq)
	local dark = (row + col) % 2 == 0
	local s = make("TextButton", {
		Name = "s" .. sq,
		Text = "",
		Size = UDim2.fromScale(1 / 8, 1 / 8),
		BackgroundColor3 = if dark then DARK else LIGHT,
		AutoButtonColor = false,
		BorderSizePixel = 0,
	}, board)
	local piece = make("Frame", {
		Name = "Piece",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(0.5, 0.5),
		Size = UDim2.fromScale(0.78, 0.78),
		BackgroundColor3 = PIECE[1],
		Visible = false,
		ZIndex = 3,
	}, s)
	round(piece, 1000)
	local edge = make("UIStroke", { Color = EDGE[1], Thickness = 3 }, piece)
	local inner = make("Frame", {
		Name = "Inner",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(0.5, 0.5),
		Size = UDim2.fromScale(0.62, 0.62),
		BackgroundTransparency = 1,
		ZIndex = 4,
	}, piece)
	round(inner, 1000)
	local innerEdge = make("UIStroke", { Color = EDGE[1], Thickness = 2 }, inner)
	local crown = label(piece, {
		Name = "Crown",
		Size = UDim2.fromScale(1, 1),
		Text = "♛",
		TextSize = 30,
		TextColor3 = Screen.GOLD,
		TextXAlignment = Enum.TextXAlignment.Center,
		Visible = false,
		ZIndex = 5,
	})
	local dot = make("Frame", {
		Name = "Dot",
		AnchorPoint = Vector2.new(0.5, 0.5),
		Position = UDim2.fromScale(0.5, 0.5),
		Size = UDim2.fromScale(0.3, 0.3),
		BackgroundColor3 = Color3.fromRGB(120, 220, 120),
		BackgroundTransparency = 0.15,
		Visible = false,
		ZIndex = 6,
	}, s)
	round(dot, 1000)
	squares[sq] = { button = s, piece = piece, edge = edge, innerEdge = innerEdge, crown = crown, dot = dot, dark = dark }
end

local function place(sq)
	local row, col = Rules.rowOf(sq), Rules.colOf(sq)
	if flipped then
		row, col = 7 - row, 7 - col
	end
	return UDim2.fromScale(col / 8, (7 - row) / 8)
end

local path = {} -- the squares clicked so far for the move being made
local shown = nil -- the board as last drawn

-- Moves that start the way `path` does.
local function matching()
	local out = {}
	local v = ui.view
	if not v or not ui.myTurn() then
		return out
	end
	for _, m in v.game.moves do
		local ok = #m >= #path
		for i = 1, #path do
			if m[i] ~= path[i] then
				ok = false
			end
		end
		if ok then
			table.insert(out, m)
		end
	end
	return out
end

local function draw()
	local v = ui.view
	local cells = shown or Rules.new().board
	local last = {}
	if v then
		for _, sq in v.game.last do
			last[sq] = true
		end
	end
	local movable, targets = {}, {}
	if v and ui.myTurn() then
		for _, m in matching() do
			if #path == 0 then
				movable[m[1]] = true
			elseif m[#path + 1] then
				targets[m[#path + 1]] = true
			end
		end
	end
	local size = board.AbsoluteSize.X
	for sq, s in squares do
		s.button.Position = place(sq)
		local colour = if s.dark then DARK else LIGHT
		if last[sq] then
			colour = LAST
		end
		if path[1] == sq or table.find(path, sq) then
			colour = PICKED
		end
		s.button.BackgroundColor3 = colour
		local piece = cells[sq]
		-- While jumping, the piece is shown where it has got to.
		if #path > 1 then
			if sq == path[1] then
				piece = 0
			elseif sq == path[#path] then
				piece = cells[path[1]]
			end
		end
		s.piece.Visible = piece ~= 0
		if piece ~= 0 then
			local owner = Rules.ownerOf(piece)
			s.piece.BackgroundColor3 = PIECE[owner]
			s.edge.Color = EDGE[owner]
			s.innerEdge.Color = EDGE[owner]
			s.crown.Visible = Rules.isKing(piece)
			s.crown.TextSize = math.max(14, math.floor(size / 8 * 0.5))
		end
		-- Pieces that can move get a gold rim.
		if movable[sq] or (path[1] == sq and #path == 1) then
			s.edge.Color = Screen.GOLD
			s.edge.Thickness = 5
		elseif piece ~= 0 then
			s.edge.Thickness = 3
		end
		s.dot.Visible = targets[sq] == true
	end
	if v and v.status == "playing" then
		ui.subText = if v.game.mustJump and ui.myTurn() then "you must jump" else nil
	end
end

local function slide(from, to)
	local s = squares[to]
	if not s or not s.piece.Visible then
		return
	end
	local ghost = s.piece:Clone()
	ghost.Parent = board
	ghost.Position = place(from) + UDim2.fromScale(1 / 16, 1 / 16)
	ghost.Size = UDim2.fromScale(0.78 / 8, 0.78 / 8)
	ghost.ZIndex = 8
	s.piece.Visible = false
	TweenService:Create(ghost, TweenInfo.new(0.18, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), {
		Position = place(to) + UDim2.fromScale(1 / 16, 1 / 16),
	}):Play()
	task.delay(0.2, function()
		ghost:Destroy()
		draw()
	end)
end

ui.onTable = function(v, fresh)
	local before = shown
	shown = table.clone(v.game.board)
	if fresh then
		flipped = v.you == 2
	end
	path = {}
	draw()
	local last = v.game.last
	if not fresh and before and #last >= 2 and before[last[#last]] == 0 then
		slide(last[1], last[#last])
		ui.sounds.move:Play()
	end
end

ui.onLeft = function()
	path = {}
	shown = nil
end

for sq, s in squares do
	s.button.MouseButton1Click:Connect(function()
		if not ui.myTurn() then
			return
		end
		local options = matching()
		if #path > 0 then
			-- A landing square for the piece already picked.
			local extended = {}
			for _, m in options do
				if m[#path + 1] == sq then
					table.insert(extended, m)
				end
			end
			if #extended > 0 then
				table.insert(path, sq)
				for _, m in extended do
					if #m == #path then
						ui.send(m)
						-- Show it straight away; the server's answer follows.
						shown[m[#m]] = shown[m[1]]
						shown[m[1]] = 0
						path = {}
						draw()
						return
					end
				end
				draw()
				return
			end
			if #path > 1 then
				ui.toast("Keep jumping: click the next green dot")
				return
			end
		end
		-- Picking a piece (or another piece).
		path = {}
		for _, m in ui.view.game.moves do
			if m[1] == sq then
				path = { sq }
				break
			end
		end
		if #path == 0 and Rules.ownerOf(shown[sq]) == ui.view.you then
			ui.toast(if ui.view.game.mustJump then "You have to jump with a piece that can" else "That piece can't move")
		end
		draw()
	end)
end

pcall(function()
	board:GetPropertyChangedSignal("AbsoluteSize"):Connect(draw)
end)
draw()
ui.start()
